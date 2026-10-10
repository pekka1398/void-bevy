use glam::DVec3;
use void_frames::{FrameSource, State, SystemId};
use void_orbit::{
    Ephemeris, EphemerisOptions, FrameEvaluator, FrameSpec, NodeKind, SystemSpec, Trajectory,
    build_system, find_nodes,
};
fn eph() -> Ephemeris {
    let system = build_system(&SystemSpec::from_json(include_str!("../systems/sol.json")));
    let mut eph = Ephemeris::new(
        &system,
        EphemerisOptions {
            step_seconds: 1.0,
            chunk_steps: 16,
        },
    );
    eph.extend_to(20.0);
    eph
}
fn path(f: impl Fn(f64) -> (f64, f64)) -> Trajectory {
    let mut t = Trajectory::new();
    for i in 0..=160 {
        let time = i as f64 / 8.0;
        let (z, vz) = f(time);
        t.append(time, &[1000.0, 100.0 * time, z, 0.0, 100.0, vz]);
    }
    t
}
#[test]
fn crossings_multiple_boundary_and_from_time() {
    let eph = eph();
    let trajectory = path(|t| ((t - 0.5).sin() * 100.0, (t - 0.5).cos() * 100.0));
    let nodes = find_nodes(
        &trajectory,
        &eph,
        FrameSpec::Barycentric,
        SystemId(0),
        0.0,
        32,
    );
    assert_eq!(nodes.len(), 7);
    for (i, n) in nodes.iter().enumerate() {
        assert!((n.time - (0.5 + i as f64 * std::f64::consts::PI)).abs() < 1e-5);
        assert_eq!(
            n.kind,
            if i % 2 == 0 {
                NodeKind::Ascending
            } else {
                NodeKind::Descending
            }
        );
        assert!((n.normal_speed_mps.abs() - 100.0).abs() < 0.01);
    }
    let after = find_nodes(
        &trajectory,
        &eph,
        FrameSpec::Barycentric,
        SystemId(0),
        1.0,
        2,
    );
    assert_eq!(after.len(), 2);
    assert!(after[0].time > 1.0);
}
#[test]
fn coplanar_tangent_and_no_crossing_have_no_nodes() {
    let eph = eph();
    for trajectory in [
        path(|_| (0.0, 0.0)),
        path(|t| ((t - 10.0).powi(2), 2.0 * (t - 10.0))),
        path(|_| (1.0, 0.0)),
    ] {
        assert!(
            find_nodes(
                &trajectory,
                &eph,
                FrameSpec::Barycentric,
                SystemId(0),
                0.0,
                32
            )
            .is_empty()
        );
    }
}
#[test]
fn surface_velocity_is_time_derivative_and_equator_is_nonrotating() {
    let eph = eph();
    let body = 3;
    let inertial = FrameEvaluator::new(&eph, FrameSpec::BodyInertial { body });
    let mut orientation = FrameEvaluator::new(&eph, FrameSpec::BodyInertial { body });
    let a = orientation.evaluate(&eph, 1.0);
    let b = orientation.evaluate(&eph, 10.0);
    assert_eq!(a.axes, b.axes);
    assert!((a.axes[2] - eph.bodies()[body].rotation.axis()).length() < 1e-12);
    let surface = FrameEvaluator::new(&eph, FrameSpec::BodySurface { body });
    let p = eph.body_position(body, 10.0) + DVec3::new(1e7, 2e7, 3e7);
    let v = DVec3::new(500.0, 100.0, 200.0);
    for frame in [&surface, &inertial] {
        let s = frame.state_at(
            &eph,
            10.0,
            State {
                position: p,
                velocity: v,
            },
        );
        let dt = 0.001;
        let lo = frame.state_at(
            &eph,
            10.0 - dt,
            State {
                position: p - v * dt,
                velocity: v,
            },
        );
        let hi = frame.state_at(
            &eph,
            10.0 + dt,
            State {
                position: p + v * dt,
                velocity: v,
            },
        );
        assert!(((hi.position - lo.position) / (2.0 * dt) - s.velocity).length() < 0.1);
    }
}

// A tilted plane precesses without changing the two bodies' separation. In-plane-only omega
// therefore cannot supply the time derivative of z; this isolates that missing term.
struct Precessing(Ephemeris, void_frames::SplitPosition);
impl Precessing {
    fn pair(t: f64) -> (DVec3, DVec3) {
        let a = t * 0.1;
        let b = t * 0.2;
        let p = DVec3::new(a.cos() * b.cos(), a.sin() * b.cos(), b.sin()) * 1e7;
        let v = DVec3::new(
            -0.1 * a.sin() * b.cos() - 0.2 * a.cos() * b.sin(),
            0.1 * a.cos() * b.cos() - 0.2 * a.sin() * b.sin(),
            0.2 * b.cos(),
        ) * 1e7;
        (p, v)
    }
}
impl void_frames::BodyStates for Precessing {
    fn body_state(&self, body: void_frames::BodyId, t: f64) -> (DVec3, DVec3) {
        let (p, v) = self.body_in_system(body, t);
        (
            void_frames::SplitPosition::at(p)
                .difference(&self.1)
                .vector(),
            v,
        )
    }
}
impl void_frames::FrameSource for Precessing {
    fn system_state(&self, system: SystemId, t: f64) -> (void_frames::SplitPosition, DVec3) {
        if system.0 == 0 {
            (void_frames::SplitPosition::ORIGIN, DVec3::ZERO)
        } else {
            (
                void_frames::SplitPosition::at(DVec3::new(1e15 + 100.0 * t, 0.0, 0.0)),
                DVec3::new(100.0, 0.0, 0.0),
            )
        }
    }
    fn body_in_system(&self, body: void_frames::BodyId, t: f64) -> (DVec3, DVec3) {
        if body.0 == 0 {
            (DVec3::ZERO, DVec3::ZERO)
        } else if body.0 == 1 {
            Self::pair(t)
        } else {
            void_frames::BodyStates::body_state(&self.0, body, t)
        }
    }
}
impl void_orbit::EphemerisSource for Precessing {
    fn system_count(&self) -> usize {
        2
    }
    fn system_of(&self, _: usize) -> SystemId {
        SystemId(0)
    }
    fn origin_system(&self) -> SystemId {
        SystemId(0)
    }
    fn physics_offset(&self) -> void_frames::SplitPosition {
        self.1
    }
    fn bodies(&self) -> &[void_orbit::CelestialBody] {
        self.0.bodies()
    }
    fn step_seconds(&self) -> f64 {
        1.0
    }
    fn start_time(&self) -> f64 {
        0.0
    }
    fn end_time(&self) -> f64 {
        20.0
    }
    fn retained_bytes(&self) -> usize {
        0
    }
    fn extend_to(&mut self, _: f64) {
        panic!("fixed fixture")
    }
    fn forget_before(&mut self, _: f64) {
        panic!("fixed fixture")
    }
    fn states_at(&self, t: f64, p: &mut [DVec3], v: Option<&mut [DVec3]>) {
        let mut v = v;
        for (i, p) in p.iter_mut().enumerate() {
            let s = void_frames::BodyStates::body_state(self, void_frames::BodyId(i), t);
            *p = s.0;
            if let Some(ref mut v) = v {
                v[i] = s.1;
            }
        }
    }
    fn positions_at(&self, t: f64, p: &mut [DVec3]) {
        self.states_at(t, p, None)
    }
    fn body_position(&self, body: usize, t: f64) -> DVec3 {
        self.body_in_system(void_frames::BodyId(body), t).0
    }
    fn frame_acceleration_at(&self, _: f64) -> DVec3 {
        DVec3::ZERO
    }
}
#[test]
fn precessing_pair_velocity_matches_derivative_and_nodes_follow_moving_plane() {
    use void_frames::FrameSource;
    let eph = Precessing(eph(), void_frames::SplitPosition::ORIGIN);
    let spec = FrameSpec::TwoBodyRotating {
        primary: 0,
        secondary: 1,
    };
    let evaluator = FrameEvaluator::new(&eph, spec);
    let p = DVec3::new(2e7, 1e7, 4e6);
    for time in [0.0, 5.0, 10.0, 20.0] {
        let state = evaluator.state_at(
            &eph,
            time,
            State {
                position: p,
                velocity: DVec3::ZERO,
            },
        );
        let dt = 0.01;
        let lo = (time - dt).max(0.0);
        let hi = (time + dt).min(20.0);
        let before = evaluator
            .state_at(
                &eph,
                lo,
                State {
                    position: p,
                    velocity: DVec3::ZERO,
                },
            )
            .position;
        let after = evaluator
            .state_at(
                &eph,
                hi,
                State {
                    position: p,
                    velocity: DVec3::ZERO,
                },
            )
            .position;
        assert!(((after - before) / (hi - lo) - state.velocity).length() < 2.0);
    }
    let mut trajectory = Trajectory::new();
    for i in 0..=200 {
        trajectory.append(i as f64 / 10.0, &[p.x, p.y, p.z, 0.0, 0.0, 0.0]);
    }
    let nodes = find_nodes(&trajectory, &eph, spec, SystemId(0), 0.0, 16);
    assert!(!nodes.is_empty());
    for node in nodes {
        assert!(node.in_frame.z.abs() < 1.0);
        let dt = 0.01;
        let lo = evaluator.state_at(
            &eph,
            node.time - dt,
            State {
                position: p,
                velocity: DVec3::ZERO,
            },
        );
        let hi = evaluator.state_at(
            &eph,
            node.time + dt,
            State {
                position: p,
                velocity: DVec3::ZERO,
            },
        );
        assert!(((hi.position.z - lo.position.z) / (2.0 * dt) - node.normal_speed_mps).abs() < 1.0);
    }
    let _ = eph.body_in_system(void_frames::BodyId(1), 0.0);
}

#[test]
fn barycentric_plot_uses_selected_system_not_global_or_physics_origin() {
    let eph = Precessing(eph(), void_frames::SplitPosition::ORIGIN);
    let evaluator = FrameEvaluator::new_in_system(&eph, FrameSpec::Barycentric, SystemId(1));
    let t = 10.0;
    let p = DVec3::new(1e15 + 100.0 * t + 42.0, 30.0, 10.0);
    let state = evaluator.state_at(
        &eph,
        t,
        State {
            position: p,
            velocity: DVec3::new(105.0, 0.0, 0.0),
        },
    );
    assert_eq!(state.position, DVec3::new(42.0, 30.0, 10.0));
    assert_eq!(state.velocity, DVec3::new(5.0, 0.0, 0.0));
}

#[test]
fn plotting_source_offset_applied_once_to_position_and_nodes() {
    let raw = Precessing(eph(), void_frames::SplitPosition::ORIGIN);
    let offset = void_frames::SplitPosition::at(DVec3::new(4e6, -3e6, 2e6));
    let shifted = Precessing(eph(), offset);
    let spec = FrameSpec::TwoBodyRotating {
        primary: 0,
        secondary: 1,
    };
    let a = FrameEvaluator::new(&raw, spec);
    let b = FrameEvaluator::new(&shifted, spec);
    let p = DVec3::new(2e7, 1e7, 4e6);
    let at = a.state_at(
        &raw,
        5.0,
        State {
            position: p,
            velocity: DVec3::ZERO,
        },
    );
    let bt = b.state_at(
        &shifted,
        5.0,
        State {
            position: p - offset.vector(),
            velocity: DVec3::ZERO,
        },
    );
    assert!((at.position - bt.position).length() < 1e-7);
    assert!((at.velocity - bt.velocity).length() < 1e-7);
    let mut ta = Trajectory::new();
    let mut tb = Trajectory::new();
    for i in 0..=200 {
        let t = i as f64 / 10.0;
        ta.append(t, &[p.x, p.y, p.z, 0.0, 0.0, 0.0]);
        let q = p - offset.vector();
        tb.append(t, &[q.x, q.y, q.z, 0.0, 0.0, 0.0]);
    }
    let na = find_nodes(&ta, &raw, spec, SystemId(0), 0.0, 16);
    let nb = find_nodes(&tb, &shifted, spec, SystemId(0), 0.0, 16);
    assert!(!na.is_empty());
    assert_eq!(na.len(), nb.len());
    for (a, b) in na.iter().zip(nb) {
        assert_eq!(a.time, b.time);
        assert_eq!(a.kind, b.kind);
        assert!((a.in_frame - b.in_frame).length() < 1e-7);
        assert!((a.normal_speed_mps - b.normal_speed_mps).abs() < 1e-7);
    }
}
