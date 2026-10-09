//! A plan's residual samples and rendering focus must use the same offset ephemeris view.
use glam::DVec3;
use void_frames::{BodyId, BodyStates, FrameSource, SplitPosition, SystemId};
use void_orbit::{CelestialBody, EphemerisSource, FrameSpec, Trajectory};
use void_view::plot::{BodyPlots, PlotPath};
struct OffsetView {
    source: Box<dyn EphemerisSource>,
    offset: SplitPosition,
}
impl BodyStates for OffsetView {
    fn body_state(&self, body: BodyId, t: f64) -> (DVec3, DVec3) {
        let (p, v) = self.source.body_state(body, t);
        (SplitPosition::at(p).difference(&self.offset).vector(), v)
    }
}
impl FrameSource for OffsetView {
    fn system_state(&self, s: SystemId, t: f64) -> (SplitPosition, DVec3) {
        self.source.system_state(s, t)
    }
    fn body_in_system(&self, b: BodyId, t: f64) -> (DVec3, DVec3) {
        self.source.body_in_system(b, t)
    }
}
impl EphemerisSource for OffsetView {
    fn system_count(&self) -> usize {
        self.source.system_count()
    }
    fn system_of(&self, b: usize) -> SystemId {
        self.source.system_of(b)
    }
    fn origin_system(&self) -> SystemId {
        self.source.origin_system()
    }
    fn physics_offset(&self) -> SplitPosition {
        self.offset
    }
    fn bodies(&self) -> &[CelestialBody] {
        self.source.bodies()
    }
    fn step_seconds(&self) -> f64 {
        self.source.step_seconds()
    }
    fn start_time(&self) -> f64 {
        self.source.start_time()
    }
    fn end_time(&self) -> f64 {
        self.source.end_time()
    }
    fn retained_bytes(&self) -> usize {
        self.source.retained_bytes()
    }
    fn extend_to(&mut self, t: f64) {
        self.source.extend_to(t)
    }
    fn forget_before(&mut self, t: f64) {
        self.source.forget_before(t)
    }
    fn states_at(&self, t: f64, p: &mut [DVec3], v: Option<&mut [DVec3]>) {
        let mut v = v;
        for (i, p) in p.iter_mut().enumerate() {
            let (position, velocity) = self.body_state(BodyId(i), t);
            *p = position;
            if let Some(ref mut v) = v {
                v[i] = velocity;
            }
        }
    }
    fn positions_at(&self, t: f64, p: &mut [DVec3]) {
        self.states_at(t, p, None)
    }
    fn body_position(&self, b: usize, t: f64) -> DVec3 {
        self.body_state(BodyId(b), t).0
    }
    fn frame_acceleration_at(&self, t: f64) -> DVec3 {
        self.source.frame_acceleration_at(t)
    }
}
#[test]
fn offset_plan_and_focus_match_unshifted_plot_and_invalidate_cache() {
    let planet = void_landing::earth_size();
    let (mut source, body) = void_landing::planet_ephemeris(&planet);
    source.extend_to(3600.0);
    let mut view = OffsetView {
        source: Box::new(source),
        offset: SplitPosition::ORIGIN,
    };
    let mut trajectory = Trajectory::new();
    let mut shifted = Trajectory::new();
    let offset = SplitPosition::at(DVec3::new(3e6, -7e6, 5e6));
    let r = planet.terrain.radius_meters + 400000.0;
    let axes = view.bodies()[body].rotation.equatorial_basis();
    for i in 0..=512 {
        let t = 3600.0 * i as f64 / 512.0;
        let a = t / 3600.0 * std::f64::consts::TAU * 2.0 - 0.3;
        let p =
            (axes[0] * a.cos() + (axes[1] * 0.4_f64.cos() + axes[2] * 0.4_f64.sin()) * a.sin()) * r;
        let v = (-axes[0] * a.sin()
            + (axes[1] * 0.4_f64.cos() + axes[2] * 0.4_f64.sin()) * a.cos())
            * r
            * std::f64::consts::TAU
            * 2.0
            / 3600.0;
        trajectory.append(t, &[p.x, p.y, p.z, v.x, v.y, v.z]);
        let q = p - offset.vector();
        shifted.append(t, &[q.x, q.y, q.z, v.x, v.y, v.z]);
    }
    let now = 10.0;
    let origin = trajectory.sample(now).0;
    for spec in [
        FrameSpec::Barycentric,
        FrameSpec::BodyInertial { body },
        FrameSpec::BodySurface { body },
    ] {
        view.offset = SplitPosition::ORIGIN;
        let mut path = PlotPath::default();
        path.update(&view, &trajectory, spec, 0, now, origin, body);
        let expected_points = path.points.clone();
        let expected_nodes = path.nodes.clone();
        let mut bodies = BodyPlots::default();
        let expected_bodies = bodies.update(&view, spec, now, origin);
        view.offset = offset;
        // Identical generation, times and frame: source offset alone must invalidate caches.
        path.update(
            &view,
            &shifted,
            spec,
            0,
            now,
            origin - offset.vector(),
            body,
        );
        assert_eq!(path.points.len(), expected_points.len());
        for (a, b) in path.points.iter().zip(expected_points) {
            assert!((*a - b).length() < 1e-6);
        }
        assert_eq!(path.nodes.len(), expected_nodes.len());
        for ((a, pa), (b, pb)) in path.nodes.iter().zip(expected_nodes) {
            assert!((a.time - b.time).abs() < 1e-6);
            assert_eq!(a.kind, b.kind);
            assert!((*pa - pb).length() < 1e-6);
        }
        let actual_bodies = bodies.update(&view, spec, now, origin - offset.vector());
        for (actual, expected) in actual_bodies.iter().zip(expected_bodies) {
            for (a, b) in actual.iter().zip(expected) {
                assert!((*a - b).length() < 1e-6);
            }
        }
    }
}
