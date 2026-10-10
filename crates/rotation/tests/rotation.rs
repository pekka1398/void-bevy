//! Physical checks of rigid-body rotation in a turning frame.

use glam::{DQuat, DVec3};
use void_rotation::{Mat3, fictitious_torque, free_rotation_step, inertia_in, rotation_step};

const INERTIA: Mat3 = [3.0, 0.0, 0.0, 0.0, 5.0, 0.0, 0.0, 0.0, 9.0];
const DT: f64 = 1.0 / 60.0;

fn mul(m: &Mat3, v: DVec3) -> DVec3 {
    DVec3::new(
        m[0] * v.x + m[1] * v.y + m[2] * v.z,
        m[3] * v.x + m[4] * v.y + m[5] * v.z,
        m[6] * v.x + m[7] * v.y + m[8] * v.z,
    )
}

/// The inertial angular momentum I (ω + Ω), in the frame's axes.
fn momentum(q: DQuat, w: DVec3, spin: DVec3) -> DVec3 {
    mul(&inertia_in(q, &INERTIA), w + spin)
}

fn start() -> (DQuat, DVec3) {
    (
        DQuat::from_xyzw(0.1, 0.2, 0.3, (1.0_f64 - 0.14).sqrt()),
        DVec3::new(0.3, -0.7, 1.1),
    )
}

#[test]
fn a_free_tumble_keeps_its_angular_momentum_and_energy() {
    let (mut q, mut w) = start();
    let l0 = momentum(q, w, DVec3::ZERO);
    let e0 = w.dot(mul(&inertia_in(q, &INERTIA), w)) / 2.0;
    for _ in 0..36_000 {
        (q, w) = free_rotation_step(q, w, &INERTIA, DVec3::ZERO, DT);
    }
    let l = momentum(q, w, DVec3::ZERO);
    let e = w.dot(mul(&inertia_in(q, &INERTIA), w)) / 2.0;
    assert!((l - l0).length() < 1e-9 * l0.length(), "{l0} -> {l}");
    // Midpoint stepping keeps energy to second order, not exactly.
    assert!((e - e0).abs() < 1e-3 * e0, "energy {e0} -> {e}");
}

#[test]
fn in_a_turning_frame_the_inertial_angular_momentum_is_conserved() {
    let spin = DVec3::new(0.0, 0.3, 0.4);
    let (mut q, mut w) = start();
    let l0 = momentum(q, w, spin);
    let steps = 6000;
    for _ in 0..steps {
        (q, w) = free_rotation_step(q, w, &INERTIA, spin, DT);
    }
    // Seen from the frame, a fixed inertial vector turns by −Ω t; turn it back.
    let t = steps as f64 * DT;
    let back = DQuat::from_axis_angle(spin.normalize(), spin.length() * t) * momentum(q, w, spin);
    assert!((back - l0).length() < 1e-9 * l0.length(), "{l0} -> {back}");
}

#[test]
fn a_body_still_in_space_turns_against_the_frame() {
    let spin = DVec3::new(0.0, 0.0, 7.292e-5);
    let q0 = DQuat::from_rotation_x(0.4);
    let (mut q, mut w) = (q0, -spin);
    let steps = 3600;
    for _ in 0..steps {
        (q, w) = free_rotation_step(q, w, &INERTIA, spin, DT);
    }
    let expected = DQuat::from_axis_angle(DVec3::Z, -spin.z * steps as f64 * DT) * q0;
    assert!((w + spin).length() < 1e-15);
    assert!(q.dot(expected).abs() > 1.0 - 1e-12, "{q} vs {expected}");
    // The frame's own torque is exactly what holds it there against the engine's gyroscopic term.
    let world = inertia_in(q, &INERTIA);
    let gyroscopic = -w.cross(mul(&world, w));
    assert!((fictitious_torque(&world, w, spin) + gyroscopic).length() < 1e-18);
}

#[test]
fn a_torque_impulse_changes_the_momentum_by_its_integral() {
    let (q, w) = (DQuat::IDENTITY, DVec3::ZERO);
    let torque = DVec3::new(0.4, 0.0, -0.2);
    // About a principal axis the body never leaves it, so the body-axes torque stays fixed.
    let (q1, w1) = rotation_step(q, w, &INERTIA, DVec3::X * torque.x, DVec3::ZERO, DT);
    let l = momentum(q1, w1, DVec3::ZERO);
    assert!((l - DVec3::X * torque.x * DT).length() < 1e-15, "{l}");
}
