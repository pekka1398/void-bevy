//! Physical checks of the aerodynamics and heating.

use std::panic::{AssertUnwindSafe, catch_unwind};

use glam::{DQuat, DVec3};
use void_aero::*;

fn near(value: f64, expected: f64, tolerance: f64) {
    assert!(
        (value - expected).abs() <= tolerance,
        "{value} != {expected} ± {tolerance}"
    );
}

fn panics(f: impl FnOnce()) -> bool {
    catch_unwind(AssertUnwindSafe(f)).is_err()
}

/// A light aircraft's aerodynamic elements: +z forward, +y up, +x toward the right wing.
struct Plane {
    mass: f64,
    elements: Vec<AeroElement>,
}

fn wing(area: f64, aspect_ratio: f64, control: ControlSurface, sign: f64, incidence: f64) -> WingAero {
    WingAero {
        chord: DVec3::Z,
        normal: if control == ControlSurface::Rudder {
            DVec3::X
        } else {
            DVec3::Y
        },
        area,
        aspect_ratio,
        chord_meters: 1.45,
        sweep_radians: 5.0 * DEG,
        incidence_radians: incidence * DEG,
        zero_lift_radians: 0.0,
        stall_radians: 15.0 * DEG,
        cd0: 0.018,
        efficiency: 0.82,
        pitching_moment: 0.0,
        control,
        control_sign: sign,
        max_deflection_radians: 18.0 * DEG,
    }
}

fn main_wing() -> WingAero {
    wing(7.92, 6.2, ControlSurface::Aileron, 1.0, 3.0)
}

fn plane() -> Plane {
    let fuselage = AeroShape::Body(BodyAero {
        axis: DVec3::Z,
        front_area: 0.72,
        rear_area: 0.72,
        side_area: 5.8,
        wet_area: 19.0,
        length_meters: 6.8,
        front_cd: 0.18,
        rear_cd: 0.4,
        side_cd: 1.05,
    });
    // (id, part position, mass, shape); the fuselage's air acts 0.35 m behind its position.
    let parts = [
        ("fuselage", DVec3::ZERO, 750.0, fuselage),
        (
            "wing-left",
            DVec3::new(-2.6, 0.1, 0.0),
            65.0,
            AeroShape::Wing(main_wing()),
        ),
        (
            "wing-right",
            DVec3::new(2.6, 0.1, 0.0),
            65.0,
            AeroShape::Wing(wing(7.92, 6.2, ControlSurface::Aileron, -1.0, 3.0)),
        ),
        (
            "tail",
            DVec3::new(0.0, 0.28, -2.8),
            35.0,
            AeroShape::Wing(wing(2.88, 3.6, ControlSurface::Elevator, -1.0, 1.3)),
        ),
        (
            "fin",
            DVec3::new(0.0, 0.9, -2.6),
            25.0,
            AeroShape::Wing(wing(1.68, 1.5, ControlSurface::Rudder, -1.0, 0.0)),
        ),
    ];
    let mass: f64 = parts.iter().map(|p| p.2).sum();
    let center = parts.iter().map(|p| p.1 * p.2).sum::<DVec3>() / mass;
    Plane {
        mass,
        elements: parts
            .into_iter()
            .map(|(id, position, _, shape)| AeroElement {
                id: id.into(),
                point: position - center
                    + if id == "fuselage" {
                        DVec3::new(0.0, 0.0, -0.35)
                    } else {
                        DVec3::ZERO
                    },
                shape,
            })
            .collect(),
    }
}

fn skin() -> ThermalSpec {
    ThermalSpec {
        skin_capacity_jk: 570.0 * 120.0,
        core_capacity_jk: 570.0 * 780.0,
        conductance_wk: 12.0,
        radiating_area: 19.0,
        heating_area: 3.0,
        convection_area: 12.0,
        emissivity: 0.8,
        nose_radius: 0.2,
        heating_factor: 0.15,
        max_skin_k: 700.0,
        max_core_k: 500.0,
        ablator: None,
    }
}

const FLIGHT_STATE: AeroState = AeroState {
    center: DVec3::ZERO,
    velocity: DVec3::new(0.0, 0.0, 70.0),
    rotation: DQuat::IDENTITY,
    angular_velocity: DVec3::ZERO,
};

fn earth(h: f64) -> Air {
    Atmosphere::earth().sample(h)
}

#[test]
fn standard_atmosphere_sea_level_eleven_km_and_layer_continuity() {
    let sea = earth(0.0);
    near(sea.pressure_pa, 101325.0, 1e-8);
    near(sea.density, 1.225, 0.001);
    near(sea.sound_speed, 340.294, 0.02);
    let eleven = earth(11000.0);
    near(eleven.pressure_pa, 22700.0, 120.0);
    near(eleven.temperature_k, 216.77, 0.1);
    for h in [
        11000.0, 20000.0, 32000.0, 47000.0, 51000.0, 71000.0, 86000.0, 105000.0, 120000.0,
    ] {
        let (a, b) = (earth(h - 0.01), earth(h + 0.01));
        assert!((a.pressure_pa - b.pressure_pa).abs() < 0.1_f64.max(a.pressure_pa * 1e-4));
    }
    let mut density = f64::INFINITY;
    for h in (0..=120_000).step_by(100) {
        let air = earth(h as f64);
        assert!(air.density <= density);
        density = air.density;
    }
    assert_eq!(earth(120000.0).density, 0.0);
}

#[test]
fn invalid_atmosphere_and_control_values_reject() {
    let p = plane();
    assert!(panics(|| {
        earth(f64::NAN);
    }));
    assert!(panics(|| {
        aerodynamic_forces(
            &p.elements,
            &FLIGHT_STATE,
            &earth(0.0),
            DVec3::ZERO,
            &Controls {
                elevator: f64::NAN,
                ..NEUTRAL
            },
        );
    }));
    assert!(panics(|| {
        aerodynamic_forces(
            &p.elements,
            &AeroState {
                velocity: DVec3::new(f64::NAN, 0.0, 0.0),
                ..FLIGHT_STATE
            },
            &Atmosphere::Vacuum.sample(0.0),
            DVec3::ZERO,
            &NEUTRAL,
        );
    }));
}

#[test]
fn vacuum_and_zero_airspeed_give_exact_zero_force() {
    let p = plane();
    let vacuum = Atmosphere::Vacuum.sample(0.0);
    let f = aerodynamic_forces(&p.elements, &FLIGHT_STATE, &vacuum, DVec3::ZERO, &NEUTRAL);
    assert_eq!(length(f.force), 0.0);
    let still = aerodynamic_forces(
        &p.elements,
        &FLIGHT_STATE,
        &earth(0.0),
        FLIGHT_STATE.velocity,
        &NEUTRAL,
    );
    assert_eq!(length(still.force), 0.0);
}

#[test]
fn dynamic_pressure_drag_dissipation_and_lift_perpendicular_to_local_flow() {
    let p = plane();
    let r = aerodynamic_forces(
        &p.elements,
        &FLIGHT_STATE,
        &earth(0.0),
        DVec3::ZERO,
        &NEUTRAL,
    );
    near(r.q_pa, 0.5 * earth(0.0).density * 70.0 * 70.0, 1e-10);
    assert!(r.force.dot(FLIGHT_STATE.velocity) < 0.0);
    for e in &r.elements {
        near(e.lift.dot(FLIGHT_STATE.velocity), 0.0, 1e-7);
    }
    assert!(r.force.y > p.mass * 9.80665 * 0.8);
}

#[test]
fn a_common_wind_and_vehicle_velocity_preserve_loads() {
    let p = plane();
    let air = earth(700.0);
    let a = aerodynamic_forces(&p.elements, &FLIGHT_STATE, &air, DVec3::ZERO, &NEUTRAL);
    let offset = DVec3::new(12000.0, -8000.0, 30000.0);
    let b = aerodynamic_forces(
        &p.elements,
        &AeroState {
            velocity: FLIGHT_STATE.velocity + offset,
            ..FLIGHT_STATE
        },
        &air,
        offset,
        &NEUTRAL,
    );
    near(length(a.force - b.force), 0.0, 1e-8);
    near(length(a.torque - b.torque), 0.0, 1e-8);
}

#[test]
fn wing_polar_is_odd_stalls_and_drags_more_at_high_incidence() {
    let wing = WingAero {
        zero_lift_radians: 0.0,
        ..main_wing()
    };
    let attached = wing_polar(&wing, 12.0 * DEG, 0.2);
    let stalled = wing_polar(&wing, 30.0 * DEG, 0.2);
    near(wing_polar(&wing, -12.0 * DEG, 0.2).cl, -attached.cl, 1e-12);
    assert!(stalled.stall > 0.99 && stalled.cd > attached.cd * 2.0);
    assert!(wing_polar(&wing, 60.0 * DEG, 0.2).cl < attached.cl);
    assert_eq!(wing_polar(&wing, 0.0, 0.0).cl, 0.0);
}

#[test]
fn pure_spanwise_flow_keeps_skin_drag_without_lift() {
    let wing = AeroElement {
        id: "wing".into(),
        point: DVec3::ZERO,
        shape: AeroShape::Wing(main_wing()),
    };
    let f = aerodynamic_forces(
        &[wing],
        &AeroState {
            velocity: DVec3::new(70.0, 0.0, 0.0),
            ..FLIGHT_STATE
        },
        &earth(0.0),
        DVec3::ZERO,
        &NEUTRAL,
    );
    assert!(f.force.x < 0.0);
    assert_eq!(f.force.y, 0.0);
    assert_eq!(f.force.z, 0.0);
}

#[test]
fn elevator_aileron_and_rudder_give_nose_up_right_roll_and_right_yaw() {
    let p = plane();
    let air = earth(700.0);
    let at = |c: Controls| aerodynamic_forces(&p.elements, &FLIGHT_STATE, &air, DVec3::ZERO, &c);
    let neutral = at(NEUTRAL);
    assert!(
        at(Controls {
            elevator: 0.2,
            ..NEUTRAL
        })
        .torque
        .x < neutral.torque.x
    );
    assert!(
        at(Controls {
            aileron: 0.2,
            ..NEUTRAL
        })
        .torque
        .z < neutral.torque.z
    );
    assert!(
        at(Controls {
            rudder: 0.2,
            ..NEUTRAL
        })
        .torque
        .y > neutral.torque.y
    );
}

#[test]
fn aircraft_has_restoring_pitch_stiffness_and_roll_damping() {
    let p = plane();
    let air = earth(700.0);
    let at = |alpha: f64| {
        aerodynamic_forces(
            &p.elements,
            &AeroState {
                velocity: DVec3::new(0.0, -70.0 * alpha.sin(), 70.0 * alpha.cos()),
                ..FLIGHT_STATE
            },
            &air,
            DVec3::ZERO,
            &NEUTRAL,
        )
    };
    let nose_up_derivative = -(at(DEG).torque.x - at(-DEG).torque.x) / (2.0 * DEG);
    assert!(
        nose_up_derivative < 0.0,
        "pitch derivative {nose_up_derivative}"
    );
    let spin = aerodynamic_forces(
        &p.elements,
        &AeroState {
            angular_velocity: DVec3::new(0.0, 0.0, 0.1),
            ..FLIGHT_STATE
        },
        &air,
        DVec3::ZERO,
        &NEUTRAL,
    );
    assert!(spin.torque.z < 0.0);
}

#[test]
fn shield_disk_hides_the_pod_going_forward_and_not_backward() {
    let shield = DiskShield {
        id: "shield".into(),
        point: DVec3::new(0.0, 0.0, 1.0),
        normal: DVec3::Z,
        radius_meters: 1.25,
    };
    let pod = DVec3::ZERO;
    // The air comes from ahead (+z) when flying forward, from behind when flying backward.
    assert!(shielded(pod, DVec3::Z, std::slice::from_ref(&shield), "pod"));
    assert!(!shielded(pod, -DVec3::Z, std::slice::from_ref(&shield), "pod"));
    assert!(!shielded(pod, DVec3::Z, std::slice::from_ref(&shield), "shield"));
    assert!(!shielded(DVec3::ZERO, DVec3::Z, &[], "pod"));
}

#[test]
fn stagnation_flux_follows_v_cubed_and_inverse_root_nose_radius() {
    near(
        stagnation_flux(0.001, 6000.0, 1.0) / stagnation_flux(0.001, 3000.0, 1.0),
        8.0,
        1e-12,
    );
    near(
        stagnation_flux(0.001, 6000.0, 4.0) / stagnation_flux(0.001, 6000.0, 1.0),
        0.5,
        1e-12,
    );
}

#[test]
fn skin_core_conduction_conserves_energy_in_an_isolated_part() {
    let mut spec = skin();
    spec.radiating_area = 0.0;
    spec.heating_area = 0.0;
    spec.convection_area = 0.0;
    let mut t = thermal_state(&spec, 288.15);
    t.skin_k = 600.0;
    t.core_k = 300.0;
    let before = t.skin_k * spec.skin_capacity_jk + t.core_k * spec.core_capacity_jk;
    let env = HeatEnvironment {
        air: Atmosphere::Vacuum.sample(0.0),
        speed: 0.0,
        exposed: false,
        exposure: 0.0,
        background_k: 3.0,
    };
    advance_thermal(&spec, &mut t, &env, 5.0, 0.0);
    near(
        t.skin_k * spec.skin_capacity_jk + t.core_k * spec.core_capacity_jk,
        before,
        1e-5,
    );
    assert!(t.skin_k < 600.0 && t.core_k > 300.0);
}

#[test]
fn finite_ablator_is_spent_then_the_skin_heats() {
    let mut spec = ThermalSpec {
        skin_capacity_jk: 30_000.0,
        core_capacity_jk: 150_000.0,
        conductance_wk: 1.5,
        radiating_area: 4.9,
        heating_area: 4.9,
        convection_area: 4.9,
        nose_radius: 1.25,
        heating_factor: 1.0,
        max_skin_k: 2400.0,
        max_core_k: 550.0,
        ..skin()
    };
    spec.ablator = Some(Ablator {
        mass_kg: 0.001,
        activation_k: 1100.0,
        latent_j_kg: 12e6,
    });
    let mut t = thermal_state(&spec, 1100.0);
    let env = HeatEnvironment {
        air: earth(30000.0),
        speed: 7000.0,
        exposed: true,
        exposure: 1.0,
        background_k: 180.0,
    };
    let b = advance_thermal(&spec, &mut t, &env, 0.2, 0.0);
    assert!(t.ablator_kg < 1e-10 && t.skin_k > 1100.0);
    near(
        b.stored_j + b.ablation_j,
        b.incoming_j - b.radiation_j + b.core_external_j,
        1e-5,
    );
}
