use glam::{DMat3, DQuat, DVec3};
use void_rotation::{Mat3, rotation_step_with_rotor};
fn momentum(q: DQuat, w: DVec3, i: &Mat3, rotor: DVec3) -> DVec3 {
    q * (DMat3::from_cols_array(i).transpose() * (q.conjugate() * w) + rotor)
}
#[test]
fn motor_brake_and_steering_exchange_internal_momentum_with_carrier() {
    let inertia = [50.0, 0.0, 0.0, 0.0, 80.0, 0.0, 0.0, 0.0, 100.0];
    let (mut q, mut w, mut rotor) = (DQuat::IDENTITY, DVec3::ZERO, DVec3::ZERO);
    for next in [
        DVec3::X * 20.0,
        DVec3::new(0.8, 0.0, 0.6) * 20.0,
        DVec3::ZERO,
    ] {
        let before = momentum(q, w, &inertia, rotor);
        (q, w) = rotation_step_with_rotor(q, w, &inertia, DVec3::ZERO, rotor, next, 1.0 / 60.0);
        assert!((momentum(q, w, &inertia, next) - before).length() < 1e-11);
        rotor = next;
    }
    assert!(
        w.length() < 1e-11,
        "braking all rotors must return conserved total zero momentum"
    );
}
#[test]
fn spinning_internal_rotor_precesses_without_inventing_total_angular_momentum() {
    let inertia = [50.0, 0.0, 0.0, 0.0, 80.0, 0.0, 0.0, 0.0, 100.0];
    let rotor = DVec3::X * 80.0;
    let (mut q, mut w) = (DQuat::IDENTITY, DVec3::new(0.3, 0.4, 0.5));
    let before = momentum(q, w, &inertia, rotor);
    let original = w;
    let mut peak: f64 = 0.0;
    for _ in 0..600 {
        (q, w) = rotation_step_with_rotor(q, w, &inertia, DVec3::ZERO, rotor, rotor, 1.0 / 120.0);
        assert!((momentum(q, w, &inertia, rotor) - before).length() < 1e-9);
        peak = peak.max((w - original).length());
    }
    assert!(peak > 0.05, "rotor must couple to carrier precession");
}
