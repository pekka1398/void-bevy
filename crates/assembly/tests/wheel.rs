use glam::DVec3;
use void_assembly::{VehicleControl, WheelContact, WheelDefinition, step_wheel};
fn wheel() -> WheelDefinition {
    WheelDefinition {
        suspension_origin: DVec3::ZERO,
        suspension_direction: -DVec3::Y,
        forward: DVec3::Z,
        radius_meters: 0.4,
        rest_length_meters: 0.35,
        travel_meters: 0.2,
        spring_newtons_per_meter: 20000.0,
        damping_newton_seconds_per_meter: 1800.0,
        wheel_inertia_kg_m2: 1.0,
        drive_torque_nm: 150.0,
        brake_torque_nm: 300.0,
        friction_coefficient: 0.9,
        max_steer_radians: 0.5,
    }
}
fn contact(velocity: DVec3) -> WheelContact {
    WheelContact {
        suspension_length_meters: 0.25,
        normal: DVec3::Y,
        forward: DVec3::Z,
        relative_point_velocity: velocity,
        inverse_mass_normal: 0.004,
        inverse_mass_forward: 0.004,
        inverse_mass_side: 0.004,
    }
}
#[test]
fn airborne_motor_has_spin_but_no_traction() {
    let d = wheel();
    d.validate().unwrap();
    let (s, f, _) = step_wheel(
        &d,
        d.initial(),
        VehicleControl {
            drive: 1.0,
            steer: 0.0,
            brake: 0.0,
        },
        None,
        1.0 / 60.0,
    );
    assert_eq!(f, DVec3::ZERO);
    assert!(!s.grounded);
    assert!(s.spin_radians_per_second > 0.0);
}
#[test]
fn tire_impulse_conserves_longitudinal_momentum_and_dissipates_slip() {
    let d = wheel();
    let dt = 1.0 / 60.0;
    let mut s = d.initial();
    s.spin_radians_per_second = 3.0;
    let mass = 250.0;
    let v = 0.5;
    let before =
        0.5 * mass * v * v + 0.5 * d.wheel_inertia_kg_m2 * s.spin_radians_per_second.powi(2);
    let (next, f, _) = step_wheel(
        &d,
        s,
        VehicleControl {
            drive: 0.0,
            steer: 0.0,
            brake: 0.0,
        },
        Some(contact(DVec3::Z * v)),
        dt,
    );
    let next_v = v + f.z * dt / mass;
    let after = 0.5 * mass * next_v * next_v
        + 0.5 * d.wheel_inertia_kg_m2 * next.spin_radians_per_second.powi(2);
    assert!(after <= before + 1e-12);
    assert!(
        (d.wheel_inertia_kg_m2 * (next.spin_radians_per_second - s.spin_radians_per_second)
            + f.z * d.radius_meters * dt)
            .abs()
            < 1e-12
    );
    assert!((next.spin_radians_per_second * d.radius_meters - next_v).abs() < 1e-12);
}
#[test]
fn lateral_slip_and_drive_share_friction_circle() {
    let d = wheel();
    let mut s = d.initial();
    s.spin_radians_per_second = 100.0;
    let (_, f, _) = step_wheel(
        &d,
        s,
        VehicleControl {
            drive: 1.0,
            steer: 0.0,
            brake: 0.0,
        },
        Some(contact(DVec3::X * 50.0)),
        1.0 / 60.0,
    );
    assert!(f.x < 0.0 && f.z > 0.0);
    assert!((f.x.hypot(f.z) - d.friction_coefficient * f.y).abs() < 1e-10);
}
#[test]
fn suspension_unloads_in_extension_and_brake_never_reverses_free_wheel() {
    let d = wheel();
    let mut c = contact(DVec3::ZERO);
    c.suspension_length_meters = 0.5;
    let mut s = d.initial();
    s.spin_radians_per_second = 0.1;
    let (next, f, _) = step_wheel(&d, s, VehicleControl::default(), Some(c), 1.0 / 60.0);
    assert_eq!(f, DVec3::ZERO);
    assert_eq!(next.spin_radians_per_second, 0.0);
}

#[test]
fn parked_brake_couples_road_impulse_and_spin_reaction() {
    let d = wheel();
    let dt = 1.0 / 60.0;
    let (s, f, reaction) = step_wheel(
        &d,
        d.initial(),
        VehicleControl::default(),
        Some(contact(DVec3::Z * 0.01)),
        dt,
    );
    assert!(s.spin_radians_per_second.abs() < 1e-12);
    assert!((0.01 + f.z * dt * 0.004).abs() < 1e-12);
    assert!(reaction.abs() < 1e-12);
}
#[test]
fn motor_reaction_accounts_virtual_rotor_angular_momentum() {
    let d = wheel();
    let dt = 1.0 / 60.0;
    let s = d.initial();
    let (next, _, reaction) = step_wheel(
        &d,
        s,
        VehicleControl {
            drive: 1.0,
            steer: 0.0,
            brake: 0.0,
        },
        None,
        dt,
    );
    assert!(
        (d.wheel_inertia_kg_m2 * (next.spin_radians_per_second - s.spin_radians_per_second)
            + reaction * dt)
            .abs()
            < 1e-12
    );
    assert!((reaction + d.drive_torque_nm).abs() < 1e-12);
}
