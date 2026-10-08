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

fn cross_matrix(v: DVec3) -> DMat3 {
    DMat3::from_cols(v.cross(DVec3::X), v.cross(DVec3::Y), v.cross(DVec3::Z))
}

/// Implicit resistance with the speed coefficient frozen at the accepted boundary.
/// B is point mobility, including rotation about COM. A COM-only point reduces
/// exactly to v/(1+dt*c*|v|/mass); every principal mobility component shrinks
/// without reversing, even for strong resistance. Total velocity is never clipped.
fn point_drag_impulse(mass: f64, inverse: DMat3, arm: DVec3, u: DVec3, c: f64, dt: f64) -> DVec3 {
    let cross = cross_matrix(arm);
    let mobility = DMat3::IDENTITY / mass - cross * inverse * cross;
    let alpha = dt * c * u.length();
    if alpha == 0. {
        return DVec3::ZERO;
    }
    let next = (DMat3::IDENTITY + alpha * mobility).inverse() * u;
    -alpha * next
}

/// Accepted-step water resistance, contact-frame axes, moment about vessel COM.
pub struct DragImpulse {
    pub linear: DVec3,
    pub angular: DVec3,
}
pub struct DragStep {
    pub mass_kg: f64,
    /// Inverse inertia in body axes.
    pub inverse_inertia: DMat3,
    pub seconds: f64,
}
/// Fleet-owned cache of derived immutable geometry. Never serialized. Keys are
/// addresses of the PartGraph's immutable static definitions, not mutable poses,
/// vessel IDs or definition names. A cache lives only as long as its owning Fleet.
#[derive(Default)]
pub struct HullCache {
    hulls: std::cell::RefCell<std::collections::HashMap<usize, std::sync::Arc<Hull>>>,
}
impl HullCache {
    fn hull(&self, definition: &'static PartDefinition) -> std::sync::Arc<Hull> {
        let key = std::ptr::from_ref(definition) as usize;
        self.hulls
            .borrow_mut()
            .entry(key)
            .or_insert_with(|| std::sync::Arc::new(Hull::new(definition)))
            .clone()
    }
}

/// Immutable geometry assembled once per accepted leg; trials query the shared environment.
pub struct VesselWater {
    environment: std::sync::Arc<void_environment::Environment>,
    parts: Vec<(&'static PartDefinition, DVec3, DQuat, std::sync::Arc<Hull>)>,
    water_bodies: Vec<usize>,
}
impl VesselWater {
    pub fn new(
        environment: &std::sync::Arc<void_environment::Environment>,
        graph: &void_assembly::PartGraph,
        members: &[String],
        centre: DVec3,
    ) -> Self {
        Self::new_cached(environment, graph, members, centre, &HullCache::default())
    }
    pub fn new_cached(
        environment: &std::sync::Arc<void_environment::Environment>,
        graph: &void_assembly::PartGraph,
        members: &[String],
        centre: DVec3,
        cache: &HullCache,
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
                        p.definition,
                        p.pose.position - centre,
                        p.pose.rotation,
                        cache.hull(p.definition),
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
        self.load_in(at, query, state, rotation, angular_velocity, true)
    }
    pub fn buoyancy_in<S: void_frames::FrameSource + ?Sized>(
        &self,
        at: &void_frames::Snapshot<'_, S>,
        query: void_frames::FrameId,
        state: void_frames::State,
        rotation: DQuat,
        angular_velocity: DVec3,
    ) -> crate::Wrench {
        self.load_in(at, query, state, rotation, angular_velocity, false)
    }
    fn load_in<S: void_frames::FrameSource + ?Sized>(
        &self,
        at: &void_frames::Snapshot<'_, S>,
        query: void_frames::FrameId,
        state: void_frames::State,
        rotation: DQuat,
        angular_velocity: DVec3,
        drag: bool,
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
                    * (g * surroundings.up
                        - if drag {
                            1.2 * water.velocity.length() * water.velocity
                        } else {
                            DVec3::ZERO
                        });
                let size = if part.shape == Shape::Box {
                    void_assembly::part_box_size(part)
                } else {
                    DVec3::new(2. * part.radius, part.height, 2. * part.radius)
                };
                // Positive rotational resistance of displaced hull, authored drag rate 1/s.
                // This term covers rotation about the floating centre, where point drag has zero lever.
                let moment = if drag {
                    angular_drag(
                        size,
                        d.volume,
                        q,
                        angular_velocity - surface.to_motion().angular_velocity,
                    )
                } else {
                    DVec3::ZERO
                };
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
    /// Freeze submerged geometry for this accepted step. Apply each part's point
    /// resistance and spin resistance sequentially to the same rigid-body velocity.
    /// Each update dissipates sea-relative kinetic energy; order is deterministic.
    pub fn drag_impulse_in<S: void_frames::FrameSource + ?Sized>(
        &self,
        at: &void_frames::Snapshot<'_, S>,
        query: void_frames::FrameId,
        state: void_frames::State,
        rotation: DQuat,
        angular_velocity: DVec3,
        step: DragStep,
    ) -> DragImpulse {
        let DragStep {
            mass_kg: mass,
            inverse_inertia: inverse_body,
            seconds: dt,
        } = step;
        assert!(
            mass.is_finite()
                && mass > 0.
                && dt.is_finite()
                && dt > 0.
                && inverse_body.is_finite()
                && inverse_body.determinant() > 0.
        );
        let r = DMat3::from_quat(rotation);
        let inverse = r * inverse_body * r.transpose();
        let mut linear = DVec3::ZERO;
        let mut angular = DVec3::ZERO;
        for (part, offset, pose, hull) in &self.parts {
            let q = rotation * *pose;
            let origin_arm = rotation * *offset;
            for body in self.water_bodies.iter().copied() {
                let sample = self.environment.surroundings(
                    at,
                    self.environment.frames(),
                    query,
                    void_frames::State {
                        position: state.position + origin_arm,
                        velocity: state.velocity + angular_velocity.cross(origin_arm),
                    },
                    body,
                );
                let Some(sea) = sample.sea.filter(|s| s.water_present) else {
                    continue;
                };
                let d = hull.displacement(q.conjugate() * sample.up, sea.depth);
                if d.volume == 0. {
                    continue;
                }
                let arm = origin_arm + q * d.centre;
                let sample = self.environment.surroundings(
                    at,
                    self.environment.frames(),
                    query,
                    void_frames::State {
                        position: state.position + arm,
                        velocity: state.velocity + angular_velocity.cross(arm),
                    },
                    body,
                );
                let Some(sea) = sample.sea.filter(|s| s.water_present) else {
                    continue;
                };
                let u = sea.velocity + linear / mass + (inverse * angular).cross(arm);
                let j = point_drag_impulse(mass, inverse, arm, u, 1200. * d.volume, dt);
                linear += j;
                angular += arm.cross(j);
                let size = if part.shape == Shape::Box {
                    void_assembly::part_box_size(part)
                } else {
                    DVec3::new(2. * part.radius, part.height, 2. * part.radius)
                };
                let tensor = DVec3::new(
                    size.y * size.y + size.z * size.z,
                    size.x * size.x + size.z * size.z,
                    size.x * size.x + size.y * size.y,
                ) * (1000. * d.volume / 12.);
                let qr = DMat3::from_quat(q);
                let resistance = qr * DMat3::from_diagonal(tensor) * qr.transpose();
                let sea_spin = at
                    .transform(self.environment.frames().surface[body], query)
                    .to_motion()
                    .angular_velocity;
                let w = angular_velocity + inverse * angular - sea_spin;
                let next = (DMat3::IDENTITY + dt * inverse * resistance).inverse() * w;
                angular -= dt * resistance * next;
                break;
            }
        }
        assert!(
            linear.is_finite() && angular.is_finite(),
            "non-finite water impulse"
        );
        DragImpulse { linear, angular }
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
    fn central_drag_matches_smooth_update_for_weak_and_extreme_loads() {
        let mass = 100.;
        for speed in [0., 2., 30., 200.] {
            for dt in [0.001, 1. / 60., 0.1] {
                for c in [0., 1., 1e8] {
                    let u = DVec3::Y * speed;
                    let j = point_drag_impulse(mass, DMat3::IDENTITY, DVec3::ZERO, u, c, dt);
                    let next = u + j / mass;
                    let want = u / (1. + dt * c * speed / mass);
                    assert!((next - want).length() < 1e-12);
                    assert!(next.dot(u) >= -1e-12);
                }
            }
        }
    }
    #[test]
    fn offcentre_drag_dissipates_coupled_translation_and_rotation() {
        let mass = 30.;
        let inertia = DMat3::from_diagonal(DVec3::new(2., 8., 5.));
        let inverse = inertia.inverse();
        for dt in [0.001, 1. / 60., 0.5] {
            for c in [1., 1e5, 1e9] {
                let v = DVec3::new(200., -20., 3.);
                let w = DVec3::new(2., -5., 9.);
                let arm = DVec3::new(1., 2., -0.5);
                let u = v + w.cross(arm);
                let j = point_drag_impulse(mass, inverse, arm, u, c, dt);
                let nv = v + j / mass;
                let nw = w + inverse * arm.cross(j);
                let energy = |v: DVec3, w: DVec3| mass * v.length_squared() + w.dot(inertia * w);
                assert!(energy(nv, nw) <= energy(v, w) * (1. + 1e-12));
                assert!((nv + nw.cross(arm)).dot(u) >= -1e-8);
                let q = DQuat::from_rotation_y(0.7);
                let r = DMat3::from_quat(q);
                let rotated =
                    point_drag_impulse(mass, r * inverse * r.transpose(), q * arm, q * u, c, dt);
                assert!((rotated - q * j).length() < 1e-5);
            }
        }
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
    fn fleet_hull_cache_reuses_static_geometry_and_releases_ownership() {
        let cache = HullCache::default();
        let definition = void_assembly::catalog().first().unwrap();
        let a = cache.hull(definition);
        let b = cache.hull(definition);
        assert!(std::sync::Arc::ptr_eq(&a, &b));
        assert_eq!(cache.hulls.borrow().len(), 1);
        let weak = std::sync::Arc::downgrade(&a);
        drop(cache);
        assert!(weak.upgrade().is_some()); // a trial evaluator retains its own hull
        drop(a);
        drop(b);
        assert!(weak.upgrade().is_none()); // no process-global retention
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
