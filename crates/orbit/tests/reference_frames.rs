use glam::DVec3;
use serde_json::Value;
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
fn vector(v: &Value) -> DVec3 {
    DVec3::new(
        v[0].as_f64().unwrap(),
        v[1].as_f64().unwrap(),
        v[2].as_f64().unwrap(),
    )
}
fn spec(v: &Value) -> FrameSpec {
    match v["kind"].as_str().unwrap() {
        "barycentric" => FrameSpec::Barycentric,
        "body-inertial" => FrameSpec::BodyInertial {
            body: v["body"].as_u64().unwrap() as usize,
        },
        "body-surface" => FrameSpec::BodySurface {
            body: v["body"].as_u64().unwrap() as usize,
        },
        "two-body-rotating" => FrameSpec::TwoBodyRotating {
            primary: v["primary"].as_u64().unwrap() as usize,
            secondary: v["secondary"].as_u64().unwrap() as usize,
        },
        _ => panic!("unknown frame"),
    }
}
#[test]
fn four_frames_match_ts() {
    let data: Value = serde_json::from_str(include_str!("golden/reference_frames.json")).unwrap();
    for system in data["systems"].as_array().unwrap() {
        let mut eph = ephemeris(system["id"].as_str().unwrap());
        eph.extend_to(60.0 * 86400.0);
        for case in system["cases"].as_array().unwrap() {
            let t = case["t"].as_f64().unwrap();
            let mut eval = FrameEvaluator::new(&eph, spec(&case["frame"]));
            let frame = eval.evaluate(&eph, t);
            // Existing native initial-state ulps accumulate below a metre in 60 days.
            assert!((frame.origin - vector(&case["origin"])).length() < 1.0);
            for (i, axis) in frame.axes.iter().enumerate() {
                assert!((*axis - vector(&case["axes"][i])).length() < 1e-9);
            }
            for (i, p) in case["bodies"].as_array().unwrap().iter().enumerate() {
                assert!(
                    (to_frame(&frame, eval.position(i)) - vector(p)).length()
                        < 1.0 + vector(p).length() * 1e-9
                );
            }
            assert!(
                (direction_to_frame(&frame, DVec3::new(0.2, -0.3, 0.7))
                    - vector(&case["direction"]))
                .length()
                    < 1e-9
            );
            let period = eval.rotation_period_seconds(&eph, t);
            if case["period"].is_null() {
                assert!(period.is_infinite());
            } else {
                assert!((period / case["period"].as_f64().unwrap() - 1.0).abs() < 1e-9);
            }
        }
    }
}
#[test]
fn original_lab_two_body_axis_and_surface_drift() {
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
