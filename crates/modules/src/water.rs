//! Closed external hull displacement. Pure clipping preserves partial immersion and buoyancy moments.
use glam::{DMat3, DQuat, DVec3};
use void_assembly::{PartDefinition, Shape};

#[derive(Clone, Copy, Debug, Default)]
pub struct Displacement {
    pub volume: f64,
    pub centre: DVec3,
}
/// Clip a convex hull against n·x <= depth, in part-local metres.
pub fn displacement(part: &PartDefinition, normal: DVec3, depth: f64) -> Displacement {
    Hull::new(part).displacement(normal, depth)
}
/// Immutable authored polygon hull. Cached per vessel leg, shared by all trial evaluations.
struct Hull {
    faces: Vec<Vec<DVec3>>,
    vertices: Vec<DVec3>,
    reach: f64,
    volume: f64,
    centre: DVec3,
}
impl Hull {
    fn new(part: &PartDefinition) -> Self {
        let mut faces: Vec<Vec<DVec3>> = vec![];
        if part.shape == Shape::Box {
            let h = void_assembly::part_box_size(part) * 0.5;
            for axis in 0..3 {
                for sign in [-1.0, 1.0] {
                    let a = (axis + 1) % 3;
                    let b = (axis + 2) % 3;
                    let mut face = vec![];
                    for (u, v) in [(-1., -1.), (1., -1.), (1., 1.), (-1., 1.)] {
                        let mut p = DVec3::ZERO;
                        p[axis] = sign * h[axis];
                        p[a] = u * h[a];
                        p[b] = v * h[b];
                        face.push(p);
                    }
                    if sign < 0. {
                        face.reverse();
                    }
                    faces.push(face);
                }
            }
        } else {
            // Circumscribed polygon's radius is corrected so its exact area equals authored pi*r².
            let count = 32;
            let angle = std::f64::consts::TAU / count as f64;
            let radius =
                part.radius * (std::f64::consts::PI / (count as f64 * 0.5 * angle.sin())).sqrt();
            let ring: Vec<_> = (0..count)
                .map(|i| {
                    let t = i as f64 * angle;
                    DVec3::new(radius * t.cos(), -part.height / 2., radius * t.sin())
                })
                .collect();
            faces.push(ring.clone());
            for i in 0..count {
                let j = (i + 1) % count;
                let top = |p: DVec3| {
                    if part.shape == Shape::Cone {
                        DVec3::Y * part.height / 2.
                    } else {
                        p + DVec3::Y * part.height
                    }
                };
                faces.push(vec![ring[j], ring[i], top(ring[i]), top(ring[j])]);
            }
            if part.shape != Shape::Cone {
                faces.push(
                    ring.iter()
                        .rev()
                        .map(|p| *p + DVec3::Y * part.height)
                        .collect(),
                );
            }
        }
        let volume = match part.shape {
            Shape::Box => void_assembly::part_box_size(part).element_product(),
            Shape::Cylinder => std::f64::consts::PI * part.radius.powi(2) * part.height,
            Shape::Cone => std::f64::consts::PI * part.radius.powi(2) * part.height / 3.,
        };
        let centre = if part.shape == Shape::Cone {
            -DVec3::Y * part.height / 4.
        } else {
            DVec3::ZERO
        };
        let mut vertices: Vec<DVec3> = vec![];
        for vertex in faces.iter().flatten() {
            if !vertices
                .iter()
                .any(|v| (*v - *vertex).length_squared() < 1e-20)
            {
                vertices.push(*vertex);
            }
        }
        let reach = vertices.iter().map(|v| v.length()).fold(0., f64::max);
        Self {
            faces,
            vertices,
            reach,
            volume,
            centre,
        }
    }
    fn displacement(&self, normal: DVec3, depth: f64) -> Displacement {
        assert!(
            normal.is_finite() && (normal.length_squared() - 1.0).abs() < 1e-9 && depth.is_finite()
        );
        let mut lower = f64::INFINITY;
        let mut upper = f64::NEG_INFINITY;
        for vertex in &self.vertices {
            let projection = normal.dot(*vertex);
            lower = lower.min(projection);
            upper = upper.max(projection);
        }
        if depth <= lower {
            return Displacement::default();
        }
        if depth >= upper {
            return Displacement {
                volume: self.volume,
                centre: self.centre,
            };
        }
        let mut clipped = vec![];
        let mut cap: Vec<DVec3> = vec![];
        let mut has_inside = false;
        let mut has_outside = false;
        for face in &self.faces {
            let mut result = vec![];
            for i in 0..face.len() {
                let a = face[i];
                let b = face[(i + 1) % face.len()];
                let da = normal.dot(a) - depth;
                let db = normal.dot(b) - depth;
                has_inside |= da < 0.;
                has_outside |= da > 0.;
                if da.abs() < 1e-12 && !cap.iter().any(|v| (*v - a).length_squared() < 1e-20) {
                    cap.push(a);
                }
                if da <= 0. {
                    result.push(a);
                }
                if (da < 0. && db > 0.) || (da > 0. && db < 0.) {
                    let p = a + (b - a) * (da / (da - db));
                    result.push(p);
                    if !cap.iter().any(|v| (*v - p).length_squared() < 1e-20) {
                        cap.push(p);
                    }
                }
            }
            if result.len() >= 3 {
                clipped.push(result);
            }
        }
        if cap.len() >= 3 && has_inside && has_outside {
            let centre = cap.iter().copied().sum::<DVec3>() / cap.len() as f64;
            let u = normal.any_orthonormal_vector();
            let v = normal.cross(u);
            cap.sort_by(|a, b| {
                let a = *a - centre;
                let b = *b - centre;
                a.dot(v)
                    .atan2(a.dot(u))
                    .total_cmp(&b.dot(v).atan2(b.dot(u)))
            });
            clipped.push(cap);
        }
        let mut volume = 0.;
        let mut moment = DVec3::ZERO;
        for face in clipped {
            for i in 1..face.len() - 1 {
                let a = face[0];
                let b = face[i];
                let c = face[i + 1];
                let dv = a.dot(b.cross(c)) / 6.;
                volume += dv;
                moment += (a + b + c) * (dv / 4.);
            }
        }
        if volume.abs() < 1e-14 {
            return Displacement::default();
        }
        assert!(volume > 0., "invalid water hull winding {volume}");
        Displacement {
            volume,
            centre: moment / volume,
        }
    }
}
/// Positive angular resistance about the displaced centre: units kg m²/s.
/// A one-second resistance time multiplies displaced-mass cuboid inertia.
pub fn angular_drag(size: DVec3, volume: f64, rotation: DQuat, spin: DVec3) -> DVec3 {
    let inertia = DVec3::new(
        size.y * size.y + size.z * size.z,
        size.x * size.x + size.z * size.z,
        size.x * size.x + size.y * size.y,
    ) * (1000. * volume / 12.);
    -(rotation * (inertia * (rotation.conjugate() * spin)))
}

fn trace(matrix: DMat3) -> f64 {
    matrix.x_axis.x + matrix.y_axis.y + matrix.z_axis.z
}
fn cross_matrix(v: DVec3) -> DMat3 {
    DMat3::from_cols(v.cross(DVec3::X), v.cross(DVec3::Y), v.cross(DVec3::Z))
}
/// Bound the largest mass-whitened point-drag eigenvalue. The non-zero eigenvalues
/// are those of B^(1/2) K B^(1/2), B = I/m - [r]x I_body^-1 [r]x. Its trace
/// bounds its largest eigenvalue. For fixed positive K this trace is convex in r,
/// so taking its maximum over hull vertices also bounds every displaced centroid.
/// K(u) = |u| I + uu^T/|u| has operator Lipschitz constant at most 3.
/// K(v)+3*margin*I is therefore a positive matrix upper bound.
fn drag_relaxation_bound(
    mass: f64,
    inverse_world: DMat3,
    arm: DVec3,
    velocity: DVec3,
    margin: f64,
) -> f64 {
    let cross = cross_matrix(arm);
    let mobility = DMat3::IDENTITY / mass - cross * inverse_world * cross;
    let speed = velocity.length();
    speed * trace(mobility)
        + if speed > 0. {
            velocity.dot(mobility * velocity) / speed
        } else {
            0.
        }
        + 3. * margin * trace(mobility)
}

/// Immutable geometry assembled once per accepted leg; every trial queries the shared environment.
pub struct StepParameters {
    pub mass_kg: f64,
    /// Positive-definite inverse body inertia, in body axes, 1/(kg m²).
    pub inverse_inertia: DMat3,
    pub maximum_seconds: f64,
}
pub struct VesselWater {
    environment: std::sync::Arc<void_environment::Environment>,
    parts: Vec<(PartDefinition, DVec3, DQuat, Hull)>,
    water_bodies: Vec<usize>,
}
impl VesselWater {
    pub fn new(
        environment: &std::sync::Arc<void_environment::Environment>,
        graph: &void_assembly::PartGraph,
        members: &[String],
        centre: DVec3,
    ) -> Self {
        Self {
            environment: environment.clone(),
            water_bodies: (0..environment.bodies().len())
                .filter(|body| {
                    environment
                        .body(*body)
                        .is_some_and(|config| config.sea_level_meters.is_some())
                })
                .collect(),
            parts: members
                .iter()
                .map(|id| {
                    let p = graph.part(id);
                    (
                        p.definition.clone(),
                        p.pose.position - centre,
                        p.pose.rotation,
                        Hull::new(p.definition),
                    )
                })
                .collect(),
        }
    }
    pub fn wrench_in<S: void_frames::FrameSource + ?Sized>(
        &self,
        at: &void_frames::Snapshot<'_, S>,
        query: void_frames::FrameId,
        state: void_frames::State,
        rotation: DQuat,
        angular_velocity: DVec3,
    ) -> crate::Wrench {
        let mut result = crate::Wrench::zero(query, state.position);
        for (part, offset, pose, hull) in &self.parts {
            let arm = rotation * *offset;
            let q = rotation * *pose;
            for body in self.water_bodies.iter().copied() {
                let point = state.position + arm;
                let surroundings = self.environment.surroundings(
                    at,
                    self.environment.frames(),
                    query,
                    void_frames::State {
                        position: point,
                        velocity: state.velocity + angular_velocity.cross(arm),
                    },
                    body,
                );
                let Some(sea) = surroundings.sea.filter(|s| s.water_present) else {
                    continue;
                };
                let d = hull.displacement(q.conjugate() * surroundings.up, sea.depth);
                if d.volume == 0. {
                    continue;
                }
                // Surface water is stationary in the body's rotating frame. Include centrifugal reduction.
                let celestial = &self.environment.bodies()[body];
                let surface = at.transform(self.environment.frames().surface[body], query);
                let pole = surface.rotation() * DVec3::Z;
                let radial_centrifugal = celestial.rotation.rate().powi(2)
                    * surroundings.radius
                    * (1. - pole.dot(surroundings.up).powi(2));
                let body_gravity = void_orbit::gravity::pull(
                    celestial.gm,
                    void_orbit::gravity::oblateness(celestial),
                    pole,
                    surroundings.up * surroundings.radius,
                );
                let g = -body_gravity.dot(surroundings.up) - radial_centrifugal;
                assert!(g > 0., "water sea requires positive effective gravity");
                let centre_arm = q * d.centre;
                let sample = self.environment.surroundings(
                    at,
                    self.environment.frames(),
                    query,
                    void_frames::State {
                        position: point + centre_arm,
                        velocity: state.velocity + angular_velocity.cross(arm + centre_arm),
                    },
                    body,
                );
                let Some(water) = sample.sea.filter(|s| s.water_present) else {
                    continue;
                };
                let force = 1000.
                    * d.volume
                    * (g * surroundings.up - 1.2 * water.velocity.length() * water.velocity);
                let size = if part.shape == Shape::Box {
                    void_assembly::part_box_size(part)
                } else {
                    DVec3::new(2. * part.radius, part.height, 2. * part.radius)
                };
                // Positive rotational resistance of displaced hull, authored drag rate 1/s.
                // This term covers rotation about the floating centre, where point drag has zero lever.
                let moment = angular_drag(
                    size,
                    d.volume,
                    q,
                    angular_velocity - surface.to_motion().angular_velocity,
                );
                result.add(crate::Wrench::at_offset(
                    query,
                    state.position,
                    arm + centre_arm,
                    force,
                    moment,
                ));
                break;
            }
        }
        result
    }
    /// Accepted-step relaxation bound for distributed quadratic point drag and angular drag.
    /// Uses full hull volume whenever the trial can cross the surface, never reduces a force.
    pub fn stable_step_in<S: void_frames::FrameSource + ?Sized>(
        &self,
        at: &void_frames::Snapshot<'_, S>,
        query: void_frames::FrameId,
        state: void_frames::State,
        q: DQuat,
        w: DVec3,
        step: StepParameters,
    ) -> f64 {
        let StepParameters {
            mass_kg: mass,
            inverse_inertia,
            maximum_seconds: maximum_dt,
        } = step;
        assert!(
            mass.is_finite()
                && mass > 0.
                && inverse_inertia.is_finite()
                && inverse_inertia.determinant() > 0.
                && maximum_dt.is_finite()
                && maximum_dt > 0.
        );
        let mut rate = 0.;
        let body_rotation = DMat3::from_quat(q);
        let inverse_world = body_rotation * inverse_inertia * body_rotation.transpose();
        for (part, offset, pose, hull) in &self.parts {
            let arm = q * *offset;
            let reach = hull.reach;
            for body in self.water_bodies.iter().copied() {
                let sample = self.environment.surroundings(
                    at,
                    self.environment.frames(),
                    query,
                    void_frames::State {
                        position: state.position + arm,
                        velocity: state.velocity + w.cross(arm),
                    },
                    body,
                );
                let Some(sea) = sample.sea else { continue };
                let celestial = &self.environment.bodies()[body];
                let surface = at.transform(self.environment.frames().surface[body], query);
                let spin = w - surface.to_motion().angular_velocity;
                let surface_gravity_bound = celestial.gm / celestial.radius_meters.powi(2)
                    * (1.
                        + 3. * celestial.j2.abs()
                            * (celestial.j2_reference_radius_meters / celestial.radius_meters)
                                .powi(2));
                let speed = sea.velocity.length()
                    + spin.length() * reach
                    + maximum_dt * surface_gravity_bound;
                if sea.depth < -(reach + speed * maximum_dt) {
                    continue;
                }
                let volume = hull.volume;
                let size = if part.shape == Shape::Box {
                    void_assembly::part_box_size(part)
                } else {
                    DVec3::new(2. * part.radius, part.height, 2. * part.radius)
                };
                let velocity = sea.velocity;
                let margin = spin.length() * reach + maximum_dt * surface_gravity_bound;
                let part_rotation = DMat3::from_quat(q * *pose);
                let point_rate = hull
                    .vertices
                    .iter()
                    .map(|vertex| {
                        drag_relaxation_bound(
                            mass,
                            inverse_world,
                            arm + part_rotation * *vertex,
                            velocity,
                            margin,
                        )
                    })
                    .fold(0., f64::max);
                let angular_inertia = DVec3::new(
                    size.y * size.y + size.z * size.z,
                    size.x * size.x + size.z * size.z,
                    size.x * size.x + size.y * size.y,
                ) * (1000. * volume / 12.);
                let angular_matrix = part_rotation
                    * DMat3::from_diagonal(angular_inertia)
                    * part_rotation.transpose();
                rate += 1.2 * 1000. * volume * point_rate + trace(inverse_world * angular_matrix);
                break;
            }
        }
        if rate == 0. {
            maximum_dt
        } else {
            maximum_dt.min(0.2 / rate)
        }
    }
    /// Conservative entry envelope for one bounded translation leg. Terrain band guards take
    /// ownership first where configured; this also protects explicit sea worlds without terrain.
    pub fn near_surface(
        &self,
        e: &dyn void_orbit::EphemerisSource,
        t: f64,
        state: void_frames::State,
        dt: f64,
        thrust_acceleration: f64,
    ) -> bool {
        let mut owned;
        let (frames, query) = if e.physics_offset() == void_frames::SplitPosition::ORIGIN {
            let frames = self.environment.frames();
            (frames, frames.systems[e.origin_system().0])
        } else {
            owned = self.environment.frames().clone();
            let query = owned
                .tree
                .add_split_fixed(owned.systems[e.origin_system().0], e.physics_offset());
            (&owned, query)
        };
        let at = frames.tree.at(t, e);
        let reach = self
            .parts
            .iter()
            .map(|(_, offset, _, hull)| offset.length() + hull.reach)
            .fold(0., f64::max);
        self.water_bodies.iter().copied().any(|body| {
            let sample = self
                .environment
                .surroundings(&at, frames, query, state, body);
            let Some(sea) = sample.sea else { return false };
            let b = &self.environment.bodies()[body];
            let a = b.gm / b.radius_meters.powi(2) + thrust_acceleration;
            sea.depth >= -(reach + sea.velocity.length() * dt + 0.5 * a * dt * dt)
        })
    }
    pub fn wrench(
        &self,
        e: &dyn void_orbit::EphemerisSource,
        time: f64,
        state: void_frames::State,
        q: DQuat,
        w: DVec3,
    ) -> crate::Wrench {
        let mut owned;
        let (frames, query) = if e.physics_offset() == void_frames::SplitPosition::ORIGIN {
            let frames = self.environment.frames();
            (frames, frames.systems[e.origin_system().0])
        } else {
            owned = self.environment.frames().clone();
            let query = owned
                .tree
                .add_split_fixed(owned.systems[e.origin_system().0], e.physics_offset());
            (&owned, query)
        };
        let mut wrench = self.wrench_in(&frames.tree.at(time, e), query, state, q, w);
        wrench.frame = if e.physics_offset() == void_frames::SplitPosition::ORIGIN {
            e.physics_query_frame().unwrap_or(query)
        } else {
            e.physics_query_frame()
                .expect("translated water load requires registered query frame")
        };
        wrench
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn part(shape: Shape) -> PartDefinition {
        let mut p = void_assembly::catalog().first().unwrap().clone();
        p.shape = shape;
        p.radius = 1.;
        p.height = 2.;
        p.box_size_meters = Some(DVec3::splat(2.));
        p
    }
    #[test]
    fn box_half_and_tilt() {
        let p = part(Shape::Box);
        let d = displacement(&p, DVec3::Y, 0.);
        assert!((d.volume - 4.).abs() < 1e-10);
        assert!((d.centre.y + 0.5).abs() < 1e-10);
        for n in [DVec3::new(1., 2., 3.).normalize(), DVec3::X] {
            assert!((displacement(&p, n, 0.).volume - 4.).abs() < 1e-10);
        }
    }
    #[test]
    fn angular_resistance_dissipates_and_rotates() {
        let size = DVec3::new(1., 2., 3.);
        let spin = DVec3::new(2., -1., 3.);
        let q = DQuat::from_rotation_y(0.8);
        let a = angular_drag(size, 2., DQuat::IDENTITY, spin);
        assert!(a.dot(spin) < 0.);
        let b = angular_drag(size, 2., q, q * spin);
        assert!((b - q * a).length() < 1e-10);
    }
    #[test]
    fn partial_volume_continuity_and_wide_hull_righting_arm() {
        let mut p = part(Shape::Box);
        p.box_size_meters = Some(DVec3::new(4., 1., 2.));
        let q = DQuat::from_rotation_z(0.2);
        let normal = q.conjugate() * DVec3::Y;
        let a = displacement(&p, normal, 0.);
        let b = displacement(&p, normal, 1e-6);
        assert!((b.volume - a.volume).abs() < 1e-4);
        let arm = q * a.centre;
        assert!(arm.cross(DVec3::Y).z < 0.);
        assert_eq!(displacement(&p, normal, -10.).volume, 0.);
    }
    #[test]
    fn dense_engine_displacement_cannot_balance_its_weight() {
        let part = void_assembly::definition("flight-booster-engine").unwrap();
        let wet = displacement(part, DVec3::Y, part.height);
        assert!(1000. * wet.volume < part.dry_mass_kg);
    }
    #[test]
    fn clipping_plane_through_vertices_closes_cap_once() {
        let p = part(Shape::Box);
        let normal = DVec3::ONE.normalize();
        let volume = displacement(&p, normal, 1. / 3.0_f64.sqrt()).volume;
        assert!((volume - 20. / 3.).abs() < 1e-10, "{volume}");
        assert!((displacement(&p, DVec3::Y, 1.).volume - 8.).abs() < 1e-10);
    }
    #[test]
    fn tensor_drag_bound_covers_centroids_and_velocity_envelope() {
        let mut p = part(Shape::Box);
        p.box_size_meters = Some(DVec3::new(0.1, 4., 0.1));
        let hull = Hull::new(&p);
        let inverse = DMat3::from_diagonal(DVec3::new(0.01, 100., 0.01));
        let velocity = DVec3::Y * 50.;
        let margin = 2.;
        let bound = hull
            .faces
            .iter()
            .flatten()
            .map(|v| drag_relaxation_bound(100., inverse, *v, velocity, margin))
            .fold(0., f64::max);
        for i in 0..101 {
            let t = i as f64 / 100.;
            let arm = DVec3::new(
                0.05 * (2. * t - 1.),
                2. * (1. - 2. * t),
                0.05 * (3. * t).sin(),
            );
            let delta = DVec3::new(t.cos(), t.sin(), 0.) * margin;
            let u = velocity + delta;
            let cross = cross_matrix(arm);
            let mobility = DMat3::IDENTITY / 100. - cross * inverse * cross;
            let actual = u.length() * trace(mobility) + u.dot(mobility * u) / u.length();
            assert!(actual <= bound + 1e-10, "{actual} > {bound}");
        }
        let reach = void_assembly::part_bound_radius(&p);
        let old = 2. * (velocity.length() + margin) * (0.01 + reach * reach * 100.02);
        assert!(bound * 100. < old, "tensor {bound}, sphere/norm {old}");
    }
    #[test]
    fn cached_hull_full_immersion_centroid_matches_partial_limit() {
        let normal = DVec3::new(0.2, 1., -0.3).normalize();
        for shape in [Shape::Box, Shape::Cylinder, Shape::Cone] {
            let hull = Hull::new(&part(shape));
            let upper = hull
                .faces
                .iter()
                .flatten()
                .map(|v| normal.dot(*v))
                .fold(f64::NEG_INFINITY, f64::max);
            let full = hull.displacement(normal, upper);
            let partial = hull.displacement(normal, upper - 1e-8);
            assert!((full.volume - partial.volume).abs() < 1e-6);
            assert!((full.centre - partial.centre).length() < 1e-6);
            assert_eq!(hull.displacement(normal, -10.).volume, 0.);
        }
    }
    #[test]
    fn cylinder_and_cone_volumes() {
        for (shape, volume) in [
            (Shape::Cylinder, 2. * std::f64::consts::PI),
            (Shape::Cone, 2. * std::f64::consts::PI / 3.),
        ] {
            let d = displacement(&part(shape), DVec3::Y, 2.);
            assert!((d.volume - volume).abs() < 1e-10);
        }
    }
}
