use glam::DVec3;
use std::panic::{AssertUnwindSafe, catch_unwind};
use void_orbit::*;
fn ephemeris(id: &str) -> Ephemeris {
    let spec = SystemSpec::from_json(
        &std::fs::read_to_string(format!("{}/systems/{id}.json", env!("CARGO_MANIFEST_DIR")))
            .unwrap(),
    );
    let system = build_system(&spec);
    Ephemeris::new(
        &system,
        EphemerisOptions {
            step_seconds: suggested_step_seconds(&system.bodies, 256.0),
            chunk_steps: 2048,
        },
    )
}
#[test]
fn inertial_frames_sit_on_their_body_and_do_not_turn() {
    let mut eph = ephemeris("sol");
    eph.extend_to(60.0 * 86400.0);
    let earth = eph.bodies().iter().position(|b| b.id == "aurelia").unwrap();
    for t in [0.0, 12.5 * 86400.0, 59.0 * 86400.0] {
        let mut barycentric = FrameEvaluator::new(&eph, FrameSpec::Barycentric);
        let frame = barycentric.evaluate(&eph, t);
        assert_eq!(frame.origin, DVec3::ZERO);
        assert_eq!(frame.axes, [DVec3::X, DVec3::Y, DVec3::Z]);
        assert!(barycentric.rotation_period_seconds(&eph, t).is_infinite());
        let mut inertial = FrameEvaluator::new(&eph, FrameSpec::BodyInertial { body: earth });
        let frame = inertial.evaluate(&eph, t);
        assert!((frame.origin - inertial.position(earth)).length() < 1e-3);
        assert!(to_frame(&frame, inertial.position(earth)).length() < 1e-3);
        assert!((frame.axes[0].cross(frame.axes[1]) - frame.axes[2]).length() < 1e-14);
        assert!(inertial.rotation_period_seconds(&eph, t).is_infinite());
        let direction = DVec3::new(0.2, -0.3, 0.7);
        assert!(
            (direction_to_frame(&frame, direction).length() - direction.length()).abs() < 1e-15
        );
        let mut surface = FrameEvaluator::new(&eph, FrameSpec::BodySurface { body: earth });
        let period = surface.rotation_period_seconds(&eph, t);
        assert!((period / eph.bodies()[earth].rotation.period_seconds - 1.0).abs() < 1e-12);
    }
}
#[test]
fn two_body_frame_keeps_both_bodies_on_its_axis_and_the_surface_holds_still() {
    let mut eph = ephemeris("sol");
    eph.extend_to(60.0 * 86400.0);
    let earth = eph.bodies().iter().position(|b| b.id == "aurelia").unwrap();
    let moon = eph.bodies().iter().position(|b| b.id == "selene").unwrap();
    let mut rotating = FrameEvaluator::new(
        &eph,
        FrameSpec::TwoBodyRotating {
            primary: earth,
            secondary: moon,
        },
    );
    let mut worst: f64 = 0.0;
    let mut t = 0.0;
    while t <= 60.0 * 86400.0 {
        let frame = rotating.evaluate(&eph, t);
        let m = to_frame(&frame, rotating.position(moon));
        let e = to_frame(&frame, rotating.position(earth));
        assert!(m.x > 0.0 && e.x < 0.0);
        worst = worst
            .max(m.y.abs())
            .max(m.z.abs())
            .max(e.y.abs())
            .max(e.z.abs());
        for axis in frame.axes {
            assert!((axis.length() - 1.0).abs() < 1e-14);
        }
        assert!((frame.axes[0].cross(frame.axes[1]) - frame.axes[2]).length() < 1e-14);
        t += 0.37 * 86400.0;
    }
    assert!(worst < 1e-4, "off-axis {worst}");
    let days = rotating.rotation_period_seconds(&eph, 0.0) / 86400.0;
    assert!((25.0..30.0).contains(&days));
    let mut surface = FrameEvaluator::new(&eph, FrameSpec::BodySurface { body: earth });
    let fixed = DVec3::new(1e6, -2e6, 6e6);
    for t in (0..=5 * 86400).step_by(3571) {
        let f = surface.evaluate(&eph, t as f64);
        let world = f.origin + fixed.x * f.axes[0] + fixed.y * f.axes[1] + fixed.z * f.axes[2];
        assert!((to_frame(&f, world) - fixed).length() < 1e-4);
    }
    eprintln!("two-body off-axis {worst:e} m; period {days:.4} d");
}
#[test]
fn invalid_frames_and_uncovered_queries_panic() {
    let eph = ephemeris("sol");
    let n = eph.bodies().len();
    for spec in [
        FrameSpec::BodySurface { body: n },
        FrameSpec::TwoBodyRotating {
            primary: 0,
            secondary: 0,
        },
        FrameSpec::TwoBodyRotating {
            primary: 0,
            secondary: n,
        },
    ] {
        assert!(catch_unwind(AssertUnwindSafe(|| FrameEvaluator::new(&eph, spec))).is_err());
    }
    let mut eval = FrameEvaluator::new(&eph, FrameSpec::Barycentric);
    assert!(catch_unwind(AssertUnwindSafe(|| eval.evaluate(&eph, 1.0))).is_err());
}

#[test]
fn system_frames_agree_with_the_ephemeris() {
    let mut e = ephemeris("sol");
    e.extend_to(1e6);
    let frames = SystemFrames::new(&e);
    assert_eq!(frames.systems, vec![frames.origin]);
    let t = 1e6;
    let snapshot = frames.tree.at(t, &e);
    for i in 0..e.bodies().len() {
        let (p, v) = void_frames::BodyStates::body_state(&e, void_frames::BodyId(i), t);
        let s = snapshot
            .transform(frames.inertial[i], frames.origin)
            .apply_state(void_frames::State {
                position: DVec3::ZERO,
                velocity: DVec3::ZERO,
            });
        assert_eq!((s.position, s.velocity), (p, v), "{i}");
    }
}
