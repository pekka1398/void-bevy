//! PlanetFrame against lab/landing (golden data from `golden/planet_frame.ts`).

use glam::DVec3;
use serde_json::Value;
use void_landing::{ContactFrame, FrameState, PlanetFrame};
use void_orbit::{Ephemeris, EphemerisOptions, SystemSpec, build_system};

fn f(v: &Value) -> f64 {
    v.as_f64().unwrap_or_else(|| panic!("not a number: {v}"))
}

fn v3(v: &Value) -> DVec3 {
    DVec3::new(f(&v[0]), f(&v[1]), f(&v[2]))
}

#[test]
fn planet_frame_matches_the_landing_lab() {
    let path = format!(
        "{}/tests/golden/planet_frame.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let golden: Value =
        serde_json::from_str(&std::fs::read_to_string(&path).expect(&path)).expect(&path);
    for planet in golden.as_array().unwrap() {
        let id = planet["id"].as_str().unwrap();
        let spec: SystemSpec = SystemSpec::from_json(&planet["system"].to_string());
        let system = build_system(&spec);
        let index = system
            .bodies
            .iter()
            .position(|b| b.id == planet["bodyId"].as_str().unwrap())
            .expect("the planet");
        let mut ephemeris = Ephemeris::new(
            &system,
            EphemerisOptions {
                step_seconds: f(&planet["stepSeconds"]),
                chunk_steps: 1024,
            },
        );
        let frame = PlanetFrame::new(&ephemeris, index);
        assert_eq!(frame.omega, f(&planet["omega"]), "{id}: spin rate");
        let cases = planet["cases"].as_array().unwrap();
        ephemeris.extend_to(
            cases
                .iter()
                .map(|c| f(&c["t"]))
                .fold(0.0, f64::max)
                .max(f(&planet["stepSeconds"])),
        );
        let (mut transform, mut round_trip, mut acceleration) = (0.0_f64, 0.0_f64, 0.0_f64);
        for c in cases {
            let t = f(&c["t"]);
            let local = FrameState {
                position: v3(&c["position"]),
                velocity: v3(&c["velocity"]),
            };
            let inertial = frame.to_inertial(&ephemeris, t, local);
            let lab = &c["inertial"];
            transform = transform
                .max(
                    (inertial.position - v3(&lab["position"])).length()
                        / inertial.position.length(),
                )
                .max(
                    (inertial.velocity - v3(&lab["velocity"])).length()
                        / inertial.velocity.length(),
                );
            let back = frame.to_body_fixed(&ephemeris, t, inertial);
            round_trip = round_trip
                .max((back.position - v3(&c["back"]["position"])).length())
                .max((back.velocity - v3(&c["back"]["velocity"])).length());
            let a = frame.acceleration(&ephemeris, t, local.position, local.velocity);
            let lab_a = v3(&c["acceleration"]);
            acceleration = acceleration.max((a - lab_a).length() / lab_a.length());
        }
        println!(
            "{id}: to inertial {transform:.1e} (relative), back to body-fixed {round_trip:.1e}, acceleration {acceleration:.1e} (relative)"
        );
        // The spin angle's sin and cos are V8's own: an ulp, which the planet-sized positions keep small.
        assert!(
            transform < 1e-15 && acceleration < 1e-14,
            "{id}: transform {transform:e}, acceleration {acceleration:e}"
        );
        assert!(round_trip < 1e-6, "{id}: round trip {round_trip:e}");
    }
}
