//! The lab's own checks (`lab/aerodynamics/aero-check.ts`), with its thresholds.

use std::panic::{AssertUnwindSafe, catch_unwind};

use glam::{DQuat, DVec3};
use void_aero::*;
use void_landing::FrameState;

fn near(value: f64, expected: f64, tolerance: f64) {
    assert!(
        (value - expected).abs() <= tolerance,
        "{value} != {expected} ± {tolerance}"
    );
}

fn panics(f: impl FnOnce()) -> bool {
    catch_unwind(AssertUnwindSafe(f)).is_err()
}

struct Plane {
    vehicle: Vehicle,
    mass: f64,
    elements: Vec<AeroElement>,
}

fn plane() -> Plane {
    let vehicle = aircraft();
    let props = mass_properties(&vehicle, &resources(&vehicle));
    let elements = aero_elements(&vehicle, props.center);
    Plane {
        vehicle,
        mass: props.mass,
        elements,
    }
}

const FLIGHT_STATE: AeroState = AeroState {
    center: DVec3::ZERO,
    velocity: DVec3::new(0.0, 0.0, 70.0),
    rotation: DQuat::IDENTITY,
    angular_velocity: DVec3::ZERO,
};

fn main_wing(p: &Plane) -> WingAero {
    match &p.vehicle.parts[p.vehicle.part_index("wing-left")]
        .aero
        .as_ref()
        .unwrap()
        .shape
    {
        AeroShape::Wing(w) => w.clone(),
        AeroShape::Body(_) => unreachable!(),
    }
}

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
fn invalid_atmosphere_geometry_and_control_values_reject() {
    let p = plane();
    assert!(panics(|| {
        earth(f64::NAN);
    }));
    let mut negative_mass = p.vehicle.clone();
    negative_mass.parts[0].dry_mass_kg = -1.0;
    assert!(panics(|| validate_vehicle(&negative_mass)));
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
    let mut bad_position = p.vehicle.clone();
    bad_position.parts[0].position.y = f64::NAN;
    assert!(panics(|| validate_vehicle(&bad_position)));
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
        ..main_wing(&plane())
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
        shape: AeroShape::Wing(main_wing(&plane())),
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
fn assembly_craft_keeps_real_mass_and_hides_joined_end_faces() {
    let rocket = demo_rocket();
    let mass = mass_properties(&rocket, &resources(&rocket)).mass;
    assert!(rocket.parts.len() == 6 && mass > 4000.0);
    let exposed: f64 = rocket
        .parts
        .iter()
        .map(|p| match &p.aero.as_ref().unwrap().shape {
            AeroShape::Body(b) => b.front_area + b.rear_area,
            AeroShape::Wing(_) => 0.0,
        })
        .sum();
    near(exposed, 2.0 * std::f64::consts::PI * 0.625 * 0.625, 1e-9);
}

#[test]
fn shield_disk_hides_the_pod_going_forward_and_not_backward() {
    let c = capsule(true, DEFAULT_ABLATOR_KG);
    let r = resources(&c);
    let at = |v: f64| {
        evaluate_vehicle(
            &c,
            &r,
            &AeroState {
                velocity: DVec3::new(0.0, 0.0, v),
                ..FLIGHT_STATE
            },
            &earth(40000.0),
            DVec3::ZERO,
            &NEUTRAL,
            250.0,
        )
    };
    let pod = c.part_index("pod");
    assert!(!at(7000.0).heat[pod].env.exposed);
    assert!(at(-7000.0).heat[pod].env.exposed);
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
    let mut spec = aircraft().parts[0].thermal;
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
    let mut spec = capsule(true, DEFAULT_ABLATOR_KG).parts[0].thermal;
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

fn command(elevator: f64, throttle: f64) -> FlightCommand {
    FlightCommand {
        controls: Controls {
            elevator,
            ..NEUTRAL
        },
        throttle,
        brakes: false,
    }
}

#[test]
fn aircraft_flies_on_real_lift_and_falls_faster_in_vacuum() {
    let mut live = AircraftFlight::new(aircraft(), FlightStart::Cruise, Atmosphere::earth());
    let mut vacuum = AircraftFlight::new(aircraft(), FlightStart::Cruise, Atmosphere::Vacuum);
    for _ in 0..120 * 5 {
        live.step(&command(0.0, 0.28));
        vacuum.step(&command(0.0, 0.28));
    }
    assert!(
        live.altitude() > vacuum.altitude() + 80.0,
        "{} vs {}",
        live.altitude(),
        vacuum.altitude()
    );
    assert_eq!(live.failure, None);
    assert!(live.fuel_used_kg > 0.0);
    assert_eq!(vacuum.fuel_used_kg, 0.0);
    near(live.fuel_kg() + live.fuel_used_kg, 180.0, 1e-8);
}

#[test]
fn runway_rests_on_the_gear_and_thrust_accelerates_it() {
    let mut live = AircraftFlight::new(aircraft(), FlightStart::Runway, Atmosphere::earth());
    for _ in 0..120 * 2 {
        live.step(&command(0.0, 0.0));
    }
    assert!(
        live.altitude() > 0.8 && live.altitude() < 1.3,
        "{}",
        live.altitude()
    );
    for _ in 0..120 * 8 {
        live.step(&command(0.0, 1.0));
    }
    assert!(
        live.velocity().z > 20.0,
        "runway v={} h={} thrust={} failure={:?}",
        live.velocity(),
        live.altitude(),
        live.thrust_n,
        live.failure
    );
    assert_eq!(live.failure, None);
}

#[test]
fn aircraft_sustains_a_minute_of_flight_and_takes_off_with_its_elevator() {
    let mut cruise = AircraftFlight::new(aircraft(), FlightStart::Cruise, Atmosphere::earth());
    let mut runway = AircraftFlight::new(aircraft(), FlightStart::Runway, Atmosphere::earth());
    for _ in 0..120 * 60 {
        cruise.step(&command(0.0, 0.28));
    }
    assert_eq!(cruise.failure, None);
    assert!(cruise.altitude() > 500.0 && cruise.loads.aero.speed > 40.0);
    for _ in 0..120 * 30 {
        let elevator = if runway.loads.aero.speed > 40.0 {
            0.15
        } else {
            0.0
        };
        runway.step(&command(elevator, 1.0));
    }
    assert_eq!(runway.failure, None);
    assert!(runway.altitude() > 100.0);
}

#[test]
fn destructive_contact_stops_the_trial() {
    let mut live = AircraftFlight::new(aircraft(), FlightStart::Runway, Atmosphere::Vacuum);
    let body = &mut live.world.bodies[live.body];
    body.set_translation(rapier3d::math::Vector::new(0.0, 5.0, 0.0), true);
    body.set_linvel(rapier3d::math::Vector::new(0.0, -30.0, 0.0), true);
    for _ in 0..120 {
        if live.failure.is_some() {
            break;
        }
        live.step(&command(0.0, 0.0));
    }
    assert_eq!(live.failure.as_deref(), Some("destructive ground impact"));
}

#[test]
fn a_corotating_surface_point_has_zero_airspeed() {
    let mut entry = EntryFlight::new(
        capsule(true, DEFAULT_ABLATOR_KG),
        DEFAULT_ENTRY,
        Atmosphere::earth(),
    );
    entry.ephemeris.extend_to(100.0);
    let fixed = FrameState {
        position: DVec3::new(entry.frame.body.radius_meters + 1000.0, 0.0, 0.0),
        velocity: DVec3::ZERO,
    };
    let inertial = entry.frame.to_inertial(&entry.ephemeris, 100.0, fixed);
    let back = entry.frame.to_body_fixed(&entry.ephemeris, 100.0, inertial);
    let planet = entry.frame.to_inertial(
        &entry.ephemeris,
        100.0,
        FrameState {
            position: DVec3::ZERO,
            velocity: DVec3::ZERO,
        },
    );
    // The lab's 400 m/s includes the planet's own orbital motion; ours is the same frame.
    assert!(length(inertial.velocity) > 400.0);
    assert!(length(inertial.velocity - planet.velocity) > 400.0);
    near(length(back.velocity), 0.0, 1e-8);
}

#[test]
fn vacuum_entry_conserves_inertial_orbital_energy() {
    let mut entry = EntryFlight::new(
        capsule(true, DEFAULT_ABLATOR_KG),
        DEFAULT_ENTRY,
        Atmosphere::Vacuum,
    );
    let energy = |e: &mut EntryFlight| {
        e.ephemeris.extend_to(e.time);
        let inertial = e.frame.to_inertial(
            &e.ephemeris,
            e.time,
            FrameState {
                position: e.position(),
                velocity: e.velocity(),
            },
        );
        inertial.velocity.dot(inertial.velocity) / 2.0 - e.frame.body.gm / length(e.position())
    };
    let start = energy(&mut entry);
    entry.advance(120.0, 100_000);
    near(energy(&mut entry), start, start.abs() * 1e-8);
}

#[test]
fn reentry_decelerates_heats_and_spends_ablator_and_bare_fails_first() {
    let mut protected = EntryFlight::new(
        capsule(true, DEFAULT_ABLATOR_KG),
        DEFAULT_ENTRY,
        Atmosphere::earth(),
    );
    let mut bare = EntryFlight::new(
        capsule(false, DEFAULT_ABLATOR_KG),
        DEFAULT_ENTRY,
        Atmosphere::earth(),
    );
    protected.advance(400.0, 200_000);
    bare.advance(400.0, 200_000);
    let ablator = protected.resources.thermal[0].ablator_kg;
    eprintln!(
        "entry t={:.1} h={:.0} v={:.0} q={:.0} flux={:.0} ablator={ablator:.1} terminal={:?}; \
         bare t={:.1} {:?}",
        protected.time,
        protected.altitude(),
        length(protected.velocity()),
        protected.max_q_pa,
        protected.max_flux_wm2,
        protected.terminal,
        bare.time,
        bare.terminal
    );
    assert!(protected.max_q_pa > 1000.0 && protected.max_flux_wm2 > 100000.0);
    assert!(ablator < 130.0);
    assert!(length(protected.velocity()) < 7600.0 * 0.5);
    assert!(
        bare.terminal
            .as_deref()
            .is_some_and(|t| t.contains("overheated"))
    );
    assert!(bare.time < protected.time);
    assert_eq!(protected.terminal, None);
}
