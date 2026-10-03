//! PlanetFrame against lab/landing (golden data from `golden/planet_frame.ts`).

use glam::DVec3;
use serde_json::Value;
use void_frames::{BodyId, BodyStates, Spin};
use void_landing::{ContactFrame, FrameState, PlanetFrame};
use void_orbit::{Ephemeris, EphemerisOptions, SystemSpec, build_system};

fn f(v: &Value) -> f64 {
    v.as_f64().unwrap_or_else(|| panic!("not a number: {v}"))
}

fn v3(v: &Value) -> DVec3 {
    DVec3::new(f(&v[0]), f(&v[1]), f(&v[2]))
}

/// The lab's body-fixed to barycentric conversion with the lab's own spin angle,
/// `angle_at_epoch + 2π t / period`, which rounds once t passes a turn.
fn lab_to_inertial(
    spin: &Spin,
    omega: f64,
    centre: (DVec3, DVec3),
    t: f64,
    local: FrameState,
) -> FrameState {
    let [node, quadrature, pole] = spin.equatorial_basis();
    let angle = spin.angle_at_epoch_radians + 2.0 * std::f64::consts::PI * t / spin.period_seconds;
    let (s, c) = (angle.sin(), angle.cos());
    let x = c * node + s * quadrature;
    let axes = [x, pole.cross(x), pole];
    let turn = |a: DVec3| {
        DVec3::new(
            a.x * axes[0].x + a.y * axes[1].x + a.z * axes[2].x,
            a.x * axes[0].y + a.y * axes[1].y + a.z * axes[2].y,
            a.x * axes[0].z + a.y * axes[1].z + a.z * axes[2].z,
        )
    };
    let r = local.position;
    let v = DVec3::new(
        local.velocity.x - omega * r.y,
        local.velocity.y + omega * r.x,
        local.velocity.z,
    );
    FrameState {
        position: centre.0 + turn(r),
        velocity: centre.1 + turn(v),
    }
}

fn relative_error(a: FrameState, b: FrameState) -> f64 {
    ((a.position - b.position).length() / a.position.length())
        .max((a.velocity - b.velocity).length() / a.velocity.length())
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
        let (mut later, mut lab_angle) = (0.0_f64, 0.0_f64);
        let spin = frame.body.rotation;
        for c in cases {
            let t = f(&c["t"]);
            let local = FrameState {
                position: v3(&c["position"]),
                velocity: v3(&c["velocity"]),
            };
            let inertial = frame.to_inertial(&ephemeris, t, local);
            let lab = &c["inertial"];
            let lab = FrameState {
                position: v3(&lab["position"]),
                velocity: v3(&lab["velocity"]),
            };
            if t < spin.period_seconds {
                // Within the first turn the project's spin angle is the lab's to the bit.
                transform = transform.max(relative_error(inertial, lab));
            } else {
                // Past it the lab's 2π t / period rounds; `Spin::angle` takes the exact remainder
                // first. The lab's own angle must reproduce its result at the same threshold, so
                // the whole difference is that rounding and nothing else in the transform.
                let centre = BodyStates::body_state(&ephemeris, BodyId(index), t);
                let reproduced = lab_to_inertial(&spin, frame.omega, centre, t, local);
                lab_angle = lab_angle.max(relative_error(reproduced, lab));
                later = later.max(relative_error(inertial, lab));
            }
            let back = frame.to_body_fixed(&ephemeris, t, inertial);
            round_trip = round_trip
                .max((back.position - v3(&c["back"]["position"])).length())
                .max((back.velocity - v3(&c["back"]["velocity"])).length());
            let a = frame.acceleration(&ephemeris, t, local.position, local.velocity);
            let lab_a = v3(&c["acceleration"]);
            acceleration = acceleration.max((a - lab_a).length() / lab_a.length());
        }
        println!(
            "{id}: to inertial {transform:.1e} (relative), back to body-fixed {round_trip:.1e}, acceleration {acceleration:.1e} (relative); past the first turn the lab's angle reproduces it to {lab_angle:.1e} and the exact remainder differs by {later:.1e}"
        );
        assert!(
            lab_angle < 1e-15,
            "{id}: the lab's own spin angle does not reproduce it: {lab_angle:e}"
        );
        // The spin angle's sin and cos are V8's own: an ulp, which the planet-sized positions keep small.
        assert!(
            transform < 1e-15 && acceleration < 1e-14,
            "{id}: transform {transform:e}, acceleration {acceleration:e}"
        );
        assert!(round_trip < 1e-6, "{id}: round trip {round_trip:e}");
    }
}
