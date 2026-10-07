//! Closed external hull displacement. Pure clipping preserves partial immersion and buoyancy moments.
use glam::{DQuat, DVec3};
use void_assembly::{PartDefinition, Shape};

#[derive(Clone, Copy, Debug, Default)]
pub struct Displacement {
    pub volume: f64,
    pub centre: DVec3,
}
/// Clip a convex hull against n·x <= depth, in part-local metres.
pub fn displacement(part: &PartDefinition, normal: DVec3, depth: f64) -> Displacement {
    assert!(
        normal.is_finite() && (normal.length_squared() - 1.0).abs() < 1e-9 && depth.is_finite()
    );
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
    let mut clipped = vec![];
    let mut cap: Vec<DVec3> = vec![];
    let mut has_inside = false;
    let mut has_outside = false;
    for face in faces {
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

/// Immutable geometry assembled once per accepted leg; every trial queries the shared environment.
pub struct StepParameters {
    pub mass_kg: f64,
    /// Conservative operator norm of inverse body inertia, 1/(kg m²).
    pub inverse_inertia_norm: f64,
    pub maximum_seconds: f64,
}
pub struct VesselWater {
    environment: std::sync::Arc<void_environment::Environment>,
    parts: Vec<(PartDefinition, DVec3, DQuat)>,
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
            parts: members
                .iter()
                .map(|id| {
                    let p = graph.part(id);
                    (
                        p.definition.clone(),
                        p.pose.position - centre,
                        p.pose.rotation,
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
        for (part, offset, pose) in &self.parts {
            let arm = rotation * *offset;
            let q = rotation * *pose;
            for body in 0..self.environment.bodies().len() {
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
                let d = displacement(part, q.conjugate() * surroundings.up, sea.depth);
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
            inverse_inertia_norm,
            maximum_seconds: maximum_dt,
        } = step;
        assert!(
            mass.is_finite()
                && mass > 0.
                && inverse_inertia_norm.is_finite()
                && inverse_inertia_norm > 0.
                && maximum_dt.is_finite()
                && maximum_dt > 0.
        );
        let mut rate = 0.;
        for (part, offset, _) in &self.parts {
            let arm = q * *offset;
            let reach = void_assembly::part_bound_radius(part);
            for body in 0..self.environment.bodies().len() {
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
                let volume = match part.shape {
                    Shape::Box => void_assembly::part_box_size(part).element_product(),
                    Shape::Cylinder => std::f64::consts::PI * part.radius.powi(2) * part.height,
                    Shape::Cone => std::f64::consts::PI * part.radius.powi(2) * part.height / 3.,
                };
                let size = if part.shape == Shape::Box {
                    void_assembly::part_box_size(part)
                } else {
                    DVec3::new(2. * part.radius, part.height, 2. * part.radius)
                };
                let lever = offset.length() + reach;
                let linear = 2. * 1.2 * 1000. * volume * speed;
                let angular = 1000. * volume * size.length_squared() / 12.;
                rate += linear * (mass.recip() + lever * lever * inverse_inertia_norm)
                    + angular * inverse_inertia_norm;
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
            .map(|(part, offset, _)| offset.length() + void_assembly::part_bound_radius(part))
            .fold(0., f64::max);
        (0..self.environment.bodies().len()).any(|body| {
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
