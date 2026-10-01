//! The rotating-frame rigid-body rotation against lab/rotation (golden data from
//! `golden/rotation.ts`).

use glam::{DQuat, DVec3};
use serde_json::Value;
use void_rotation::{Mat3, fictitious_torque, free_rotation_step, inertia_in, rotation_step};

fn golden() -> Value {
    let path = format!("{}/tests/golden/rotation.json", env!("CARGO_MANIFEST_DIR"));
    serde_json::from_str(&std::fs::read_to_string(&path).expect(&path)).expect(&path)
}

fn f(v: &Value) -> f64 {
    v.as_f64().unwrap_or_else(|| panic!("not a number: {v}"))
}

fn v3(v: &Value) -> DVec3 {
    DVec3::new(f(&v[0]), f(&v[1]), f(&v[2]))
}

fn quat(v: &Value) -> DQuat {
    DQuat::from_xyzw(f(&v[0]), f(&v[1]), f(&v[2]), f(&v[3]))
}

fn mat(v: &Value) -> Mat3 {
    let a: Vec<f64> = v.as_array().unwrap().iter().map(f).collect();
    a.try_into().expect("nine entries")
}

/// Largest component difference of two quaternions, sign included.
fn quat_error(a: DQuat, b: DQuat) -> f64 {
    (a.x - b.x)
        .abs()
        .max((a.y - b.y).abs())
        .max((a.z - b.z).abs())
        .max((a.w - b.w).abs())
}

#[test]
fn steps_match_the_rotation_lab() {
    let g = golden();
    let (mut torque, mut inertia, mut free, mut step) = (0.0_f64, 0.0_f64, 0.0_f64, 0.0_f64);
    for c in g["cases"].as_array().unwrap() {
        let (i_local, q, w, tau, spin, dt) = (
            mat(&c["inertia"]),
            quat(&c["rotation"]),
            v3(&c["angularVelocity"]),
            v3(&c["torque"]),
            v3(&c["spin"]),
            f(&c["dt"]),
        );
        let world = inertia_in(q, &i_local);
        let lab_world = mat(&c["inertiaIn"]);
        inertia = inertia.max(
            world
                .iter()
                .zip(&lab_world)
                .map(|(a, b)| (a - b).abs())
                .fold(0.0, f64::max),
        );
        let t = fictitious_torque(&world, w, spin);
        let lab_t = v3(&c["torqueOut"]);
        torque = torque.max((t - lab_t).length() / lab_t.length().max(1e-300));
        let (fq, fw) = free_rotation_step(q, w, &i_local, spin, dt);
        free = free
            .max(quat_error(fq, quat(&c["free"]["rotation"])))
            .max((fw - v3(&c["free"]["angularVelocity"])).length());
        let (sq, sw) = rotation_step(q, w, &i_local, tau, spin, dt);
        step = step
            .max(quat_error(sq, quat(&c["step"]["rotation"])))
            .max((sw - v3(&c["step"]["angularVelocity"])).length());
    }
    println!(
        "inertia in frame {inertia:.1e}, fictitious torque {torque:.1e} (relative), free step {free:.1e}, torqued step {step:.1e}"
    );
    // Pure arithmetic: bit for bit.
    assert!(
        inertia == 0.0 && torque == 0.0,
        "inertia {inertia:e}, torque {torque:e}"
    );
    // The steps use sin and cos, which V8 rounds its own way: within a few ulps.
    assert!(
        free < 1e-14 && step < 1e-14,
        "free step {free:e}, torqued step {step:e}"
    );
}

#[test]
fn ten_minute_tumble_matches_the_rotation_lab() {
    let g = golden();
    let tumble = &g["tumble"];
    let i_local = mat(&tumble["inertia"]);
    let spin = DVec3::new(0.0, 0.0, 7.292e-5);
    let mut q = DQuat::from_xyzw(0.1, 0.2, 0.3, (1.0_f64 - 0.14).sqrt());
    let mut w = DVec3::new(0.3, -0.7, 1.1);
    let trace = tumble["trace"].as_array().unwrap();
    let mut worst = 0.0_f64;
    let mut k = 0;
    for n in 1..=36_000 {
        let torque = if n <= 3600 {
            DVec3::new(0.4, 0.0, -0.2)
        } else {
            DVec3::ZERO
        };
        (q, w) = rotation_step(q, w, &i_local, torque, spin, 1.0 / 60.0);
        if n % 3600 == 0 {
            let lab = &trace[k];
            let e = quat_error(q, quat(&lab["rotation"]))
                .max((w - v3(&lab["angularVelocity"])).length());
            println!("t = {:>4} s: {e:.1e}", f(&lab["t"]));
            worst = worst.max(e);
            k += 1;
        }
    }
    // A free tumble is chaotic: last-digit sin and cos differences grow, but slowly at this length.
    assert!(worst < 1e-9, "the tumble drifts from the lab by {worst:e}");
}
