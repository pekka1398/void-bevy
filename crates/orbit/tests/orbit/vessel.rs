//! The vessel propagator, apsides, dominance and flight plan against two-body physics.

use glam::DVec3;
use void_frames::{BodyId, BodyStates};
use void_orbit::{
    AdvanceOutcome, ApsisKind, AttitudeLaw, BuiltSystem, Control, DominanceTree, Ephemeris,
    EphemerisOptions, FlightPlan, ForceControl, ManeuverSpec, PlanEngine, PropagationRun,
    ReferenceMode, SystemSpec, ThrustControl, Tolerances, Trajectory, VesselPropagator,
    VesselState, build_system, find_apsides, suggested_step_seconds,
};

/// A star and one planet without oblateness, far enough apart that the star's tide on a low orbit
/// is a few parts in 1e8 of the planet's pull.
fn two_body() -> BuiltSystem {
    let rotation = serde_json::json!({
        "periodSeconds": 86400.0, "obliquityRadians": 0.0, "poleLongitudeRadians": 0.0,
        "angleAtEpochRadians": 0.0,
    });
    let spec = serde_json::json!({
        "name": "two body",
        "root": {
            "id": "star", "name": "Star", "massKg": 2.0e30, "radiusMeters": 7.0e8,
            "color": "#fff", "rotation": rotation,
            "children": [{
                "id": "planet", "name": "Planet", "massKg": 6.0e24, "radiusMeters": 6.4e6,
                "color": "#88f", "rotation": rotation, "orbitPlane": "ecliptic",
                "orbit": {
                    "semiMajorAxisMeters": 1.5e11, "eccentricity": 0.0, "inclinationRadians": 0.0,
                    "longitudeOfAscendingNodeRadians": 0.0, "argumentOfPeriapsisRadians": 0.0,
                    "meanAnomalyRadians": 0.0,
                },
                "children": [],
            }],
        },
    });
    build_system(&SystemSpec::from_json(&spec.to_string()))
}

const HOME: usize = 1;
const START: f64 = 1000.0;
const TOLERANCES: Tolerances = Tolerances {
    position_meters: 1e-4,
    velocity_meters_per_second: 1e-7,
};
const ENGINE: PlanEngine = PlanEngine {
    thrust_newtons: 250e3,
    exhaust_velocity: 3432.3275,
    dry_mass_kg: 10e3,
};

fn ephemeris() -> Ephemeris {
    let system = two_body();
    let mut e = Ephemeris::new(
        &system,
        EphemerisOptions {
            step_seconds: suggested_step_seconds(&system.bodies, 256.0),
            chunk_steps: 2048,
        },
    );
    e.extend_to(START);
    e
}

/// At periapsis of an orbit about the planet: `periapsis` from its centre, `eccentricity`, tilted
/// out of the ecliptic.
fn orbit_start(e: &Ephemeris, periapsis: f64, eccentricity: f64) -> VesselState {
    let (centre, velocity) = e.body_state(BodyId(HOME), START);
    let gm = e.bodies()[HOME].gm;
    let speed = (gm * (1.0 + eccentricity) / periapsis).sqrt();
    VesselState {
        time: START,
        position: centre + DVec3::X * periapsis,
        velocity: velocity + DVec3::new(0.0, 0.8, 0.6) * speed,
        mass_kg: 40_000.0,
    }
}

fn start(e: &Ephemeris) -> VesselState {
    orbit_start(e, 6.4e6 + 200e3, 0.0)
}

fn relative(e: &Ephemeris, t: f64, position: DVec3, velocity: DVec3) -> (DVec3, DVec3) {
    let (centre, centre_velocity) = e.body_state(BodyId(HOME), t);
    (position - centre, velocity - centre_velocity)
}

#[test]
fn a_coasting_vessel_closes_its_orbit_between_the_right_apsides() {
    let mut eph = ephemeris();
    let gm = eph.bodies()[HOME].gm;
    let (periapsis, eccentricity) = (7.0e6, 0.1);
    let a = periapsis / (1.0 - eccentricity);
    let period = std::f64::consts::TAU * (a * a * a / gm).sqrt();
    let from = orbit_start(&eph, periapsis, eccentricity);
    let mut propagator = VesselPropagator::new(&eph, TOLERANCES);
    let mut run = PropagationRun::new(from);
    let mut trajectory = Trajectory::new();
    trajectory.append(run.time, &run.y);
    let outcome = propagator.advance(
        &mut eph,
        &mut run,
        START + period,
        1_000_000,
        Some(&mut trajectory),
        None,
    );
    assert_eq!(outcome, AdvanceOutcome::Reached);
    let (p0, v0) = relative(&eph, START, from.position, from.velocity);
    let end = |i| run.y[i];
    let (p1, v1) = relative(
        &eph,
        run.time,
        DVec3::new(end(0), end(1), end(2)),
        DVec3::new(end(3), end(4), end(5)),
    );
    let energy = |p: DVec3, v: DVec3| v.length_squared() / 2.0 - gm / p.length();
    println!(
        "one orbit: back within {:.2} m, energy {:.1e} relative",
        (p1 - p0).length(),
        (energy(p1, v1) / energy(p0, v0) - 1.0).abs()
    );
    assert!((p1 - p0).length() < 100.0);
    assert!((energy(p1, v1) / energy(p0, v0) - 1.0).abs() < 1e-6);
    let apsides = find_apsides(&trajectory, &eph, HOME, START, 8);
    let apoapsis = apsides
        .iter()
        .find(|x| x.kind == ApsisKind::Apoapsis)
        .expect("an apoapsis");
    assert!((apoapsis.distance_meters / (a * (1.0 + eccentricity)) - 1.0).abs() < 1e-6);
    assert!((apoapsis.time - (START + period / 2.0)).abs() < 1.0);
}

#[test]
fn a_burn_spends_mass_at_the_engine_rate_and_gains_the_rocket_equation_speed() {
    let mut eph = ephemeris();
    let from = start(&eph);
    let direction = DVec3::new(0.0, 0.8, 0.6);
    let burn = Control::Thrust(ThrustControl {
        thrust_newtons: ENGINE.thrust_newtons,
        exhaust_velocity: ENGINE.exhaust_velocity,
        minimum_mass_kg: ENGINE.dry_mass_kg,
        attitude: AttitudeLaw::Inertial { direction },
    });
    let seconds = 10.0;
    let mut propagator = VesselPropagator::new(&eph, TOLERANCES);
    let (mut burned, mut coasted) = (PropagationRun::new(from), PropagationRun::new(from));
    propagator.advance(
        &mut eph,
        &mut burned,
        START + seconds,
        1_000_000,
        None,
        Some(burn),
    );
    propagator.advance(
        &mut eph,
        &mut coasted,
        START + seconds,
        1_000_000,
        None,
        None,
    );
    let mass = from.mass_kg - ENGINE.thrust_newtons / ENGINE.exhaust_velocity * seconds;
    assert!(
        (burned.y[6] - mass).abs() < 1e-6,
        "mass {} vs {mass}",
        burned.y[6]
    );
    let gained = DVec3::new(
        burned.y[3] - coasted.y[3],
        burned.y[4] - coasted.y[4],
        burned.y[5] - coasted.y[5],
    );
    let ideal = ENGINE.exhaust_velocity * (from.mass_kg / mass).ln();
    // Gravity differs slightly along the two paths; ten seconds keeps that tiny.
    assert!(
        (gained.dot(direction) / ideal - 1.0).abs() < 1e-4,
        "{gained} vs {ideal}"
    );
}

#[test]
fn the_dominant_body_is_the_planet_near_it_and_the_star_far_away() {
    let eph = ephemeris();
    let mut positions = vec![DVec3::ZERO; eph.bodies().len()];
    eph.positions_at(START, &mut positions);
    let tree = DominanceTree::new(eph.bodies());
    assert_eq!(
        tree.dominant(&positions, positions[HOME] + DVec3::X * 7e6),
        HOME
    );
    assert_eq!(tree.dominant(&positions, positions[0] + DVec3::Y * 3e10), 0);
}

#[test]
fn partially_computed_plan_restores_and_continues_without_restarting() {
    let mut eph = ephemeris();
    let start = start(&eph);
    let mut original = FlightPlan::new(&eph, TOLERANCES, ENGINE, 3000.0);
    original.rebase(&PropagationRun::new(start));
    for (after, dv) in [(30.0, 60.0), (200.0, -10.0), (200.1, 1.0)] {
        original.add(ManeuverSpec {
            start_time: start.time + after,
            reference_body: HOME,
            reference_mode: ReferenceMode::Fixed,
            prograde: dv,
            normal: 1.0,
            radial: 0.0,
        });
    }
    original.extend(&mut eph, 3);
    assert!(!original.complete());
    let bytes = serde_json::to_vec(&original.checkpoint()).unwrap();
    let saved: void_orbit::FlightPlanCheckpoint = serde_json::from_slice(&bytes).unwrap();
    let mut restored = FlightPlan::from_checkpoint(&eph, saved);
    assert_eq!(
        serde_json::to_value(restored.checkpoint()).unwrap(),
        serde_json::to_value(original.checkpoint()).unwrap()
    );
    for _ in 0..10 {
        original.extend(&mut eph, 13);
        restored.extend(&mut eph, 13);
        assert_eq!(
            serde_json::to_value(restored.checkpoint()).unwrap(),
            serde_json::to_value(original.checkpoint()).unwrap()
        );
    }
    assert!(
        original.status(2).is_err(),
        "the overlapping burn must retain its rejection"
    );
}

#[test]
#[should_panic(expected = "fell below dry mass")]
fn burning_past_dry_mass_panics() {
    let mut ephemeris = ephemeris();
    let mut propagator = VesselPropagator::new(&ephemeris, TOLERANCES);
    let mut run = PropagationRun::new(start(&ephemeris));
    let burn = Control::Force(ForceControl {
        force: DVec3::X * 1e5,
        mass_flow_kg_per_second: 100.0,
        minimum_mass_kg: 39_000.0,
    });
    let end = run.time + 60.0;
    propagator.advance(&mut ephemeris, &mut run, end, 1_000, None, Some(burn));
}

#[test]
#[should_panic(expected = "already ended in an impact")]
fn advancing_after_impact_panics() {
    let mut ephemeris = ephemeris();
    // 100 km up, falling straight down at 1 km/s.
    let from = start(&ephemeris);
    let (centre, velocity) = ephemeris.body_state(BodyId(HOME), START);
    let up = (from.position - centre).normalize();
    let falling = VesselState {
        position: centre + up * (6.4e6 + 100e3),
        velocity: velocity - up * 1000.0,
        ..from
    };
    let mut propagator = VesselPropagator::new(&ephemeris, TOLERANCES);
    let mut run = PropagationRun::new(falling);
    let end = run.time + 86_400.0;
    assert_eq!(
        propagator.advance(&mut ephemeris, &mut run, end, 1_000_000, None, None),
        AdvanceOutcome::Impact { body: HOME }
    );
    propagator.advance(&mut ephemeris, &mut run, end, 1_000_000, None, None);
}

#[test]
fn changing_external_force_invalidates_fsal_at_the_accepted_boundary() {
    use std::sync::Arc;
    struct Field(DVec3);
    impl void_orbit::AirSource for Field {
        fn acceleration(
            &self,
            _: &dyn void_orbit::EphemerisSource,
            _: f64,
            _: DVec3,
            _: DVec3,
            _: f64,
        ) -> DVec3 {
            self.0
        }
    }
    let mut eph = ephemeris();
    let mut run = PropagationRun::new(start(&eph));
    let mut prop = VesselPropagator::new(
        &eph,
        Tolerances {
            position_meters: 1e-6,
            velocity_meters_per_second: 1e-9,
        },
    );
    prop.set_air_source(Some(Arc::new(Field(DVec3::X))));
    let end = run.time + 1.0;
    assert_eq!(
        prop.advance(&mut eph, &mut run, end, 100000, None, None),
        AdvanceOutcome::Reached
    );
    let mut fresh = run.restarted();
    let mut stale = run.clone();
    let hint = run.step_hint;
    prop.set_air_source(None);
    run.invalidate_force_derivative();
    assert_eq!(run.step_hint, hint);
    assert_eq!(
        prop.advance(&mut eph, &mut run, end + 1.0, 100000, None, None),
        AdvanceOutcome::Reached
    );
    assert_eq!(
        prop.advance(&mut eph, &mut fresh, end + 1.0, 100000, None, None),
        AdvanceOutcome::Reached
    );
    assert_eq!(
        run.y, fresh.y,
        "same Control, new field must match a fresh derivative"
    );
    // A deliberately stale run must distinguish this test from a constant-field continuation.
    assert_eq!(
        prop.advance(&mut eph, &mut stale, end + 1.0, 100000, None, None),
        AdvanceOutcome::Reached
    );
    assert_ne!(stale.y, fresh.y);
}
