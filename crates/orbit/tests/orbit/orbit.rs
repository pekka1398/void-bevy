//! Orbits and the ephemeris against what two-body and conservation laws require.

use glam::DVec3;
use void_frames::{BodyId, BodyStates};
use void_orbit::{
    BuiltSystem, EllipticElements, Ephemeris, EphemerisOptions, SystemSpec, build_system,
    osculating_orbit, solve_kepler_elliptic, state_from_elements, suggested_step_seconds,
};

fn system(id: &str) -> BuiltSystem {
    let path = format!("{}/systems/{id}.json", env!("CARGO_MANIFEST_DIR"));
    build_system(&SystemSpec::from_json(
        &std::fs::read_to_string(&path).expect(&path),
    ))
}

/// A star and one planet: a two-body problem with a known answer.
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
                    "semiMajorAxisMeters": 1.5e11, "eccentricity": 0.2, "inclinationRadians": 0.3,
                    "longitudeOfAscendingNodeRadians": 0.4, "argumentOfPeriapsisRadians": 1.1,
                    "meanAnomalyRadians": 0.5,
                },
                "children": [],
            }],
        },
    });
    build_system(&SystemSpec::from_json(&spec.to_string()))
}

fn relative(a: f64, b: f64) -> f64 {
    (a - b).abs() / a.abs().max(b.abs())
}

#[test]
fn a_two_body_orbit_closes_after_one_period() {
    let system = two_body();
    let gm = system.bodies[0].gm + system.bodies[1].gm;
    let relative_state = |e: &Ephemeris, t: f64| {
        let (star, star_velocity) = e.body_state(BodyId(0), t);
        let (planet, planet_velocity) = e.body_state(BodyId(1), t);
        (planet - star, planet_velocity - star_velocity)
    };
    let mut ephemeris = Ephemeris::new(
        &system,
        EphemerisOptions {
            step_seconds: suggested_step_seconds(&system.bodies, 256.0),
            chunk_steps: 1024,
        },
    );
    let (p0, v0) = (
        system.positions[1] - system.positions[0],
        system.velocities[1] - system.velocities[0],
    );
    let period = osculating_orbit(p0, v0, gm).period_seconds;
    let energy = ephemeris.current_energy();
    let momentum = ephemeris.current_angular_momentum();
    ephemeris.extend_to(period);
    let (p1, v1) = relative_state(&ephemeris, period);
    println!(
        "after one period: {:.2e} m, {:.2e} m/s",
        (p1 - p0).length(),
        (v1 - v0).length()
    );
    assert!((p1 - p0).length() < 1e-7 * p0.length());
    assert!((v1 - v0).length() < 1e-7 * v0.length());
    assert!(relative(ephemeris.current_energy(), energy) < 1e-12);
    assert!((ephemeris.current_angular_momentum() - momentum).length() < 1e-12 * momentum.length());
}

#[test]
fn every_system_keeps_its_energy_and_angular_momentum() {
    for id in ["sol", "binary"] {
        let system = system(id);
        let mut ephemeris = Ephemeris::new(
            &system,
            EphemerisOptions {
                step_seconds: suggested_step_seconds(&system.bodies, 256.0),
                chunk_steps: 1024,
            },
        );
        let energy = ephemeris.current_energy();
        let momentum = ephemeris.current_angular_momentum();
        let end = 365.25 * 86_400.0;
        ephemeris.extend_to(end);
        let drift = relative(ephemeris.current_energy(), energy);
        let turn = (ephemeris.current_angular_momentum() - momentum).length() / momentum.length();
        println!("{id}: one year, energy drift {drift:.1e}, angular momentum {turn:.1e}");
        assert!(drift < 1e-11, "{id}: energy drift {drift:e}");
        assert!(turn < 1e-12, "{id}: angular momentum drift {turn:e}");
        let n = system.bodies.len();
        let (mut p, mut v) = (vec![DVec3::ZERO; n], vec![DVec3::ZERO; n]);
        ephemeris.states_at(end * 0.37, &mut p, Some(&mut v));
        for (i, (position, velocity)) in p.iter().zip(&v).enumerate() {
            assert_eq!(
                ephemeris.body_state(BodyId(i), end * 0.37),
                (*position, *velocity),
                "{id}: body_state against states_at"
            );
        }
    }
}

#[test]
fn elements_and_states_convert_both_ways() {
    let gm = 3.986e14;
    for e in [0.0, 0.3, 0.9] {
        let elements = EllipticElements {
            semi_major_axis_meters: 7.0e6,
            eccentricity: e,
            inclination_radians: 0.7,
            longitude_of_ascending_node_radians: 2.0,
            argument_of_periapsis_radians: 0.4,
            mean_anomaly_radians: 1.3,
        };
        let anomaly = solve_kepler_elliptic(elements.mean_anomaly_radians, e);
        assert!((anomaly - e * anomaly.sin() - elements.mean_anomaly_radians).abs() < 1e-14);
        let (p, v) = state_from_elements(&elements, gm);
        let o = osculating_orbit(p, v, gm);
        assert!(relative(o.semi_major_axis_meters, 7.0e6) < 1e-12);
        assert!((o.eccentricity - e).abs() < 1e-12);
        assert!((o.inclination_radians - 0.7).abs() < 1e-12);
        assert!(relative(o.specific_energy, -gm / (2.0 * 7.0e6)) < 1e-12);
        // One whole period later the state is the same.
        let (p2, v2) = state_from_elements(
            &EllipticElements {
                mean_anomaly_radians: elements.mean_anomaly_radians + std::f64::consts::TAU,
                ..elements
            },
            gm,
        );
        assert!((p2 - p).length() < 1e-6 && (v2 - v).length() < 1e-9);
    }
}

#[test]
#[should_panic(expected = "outside covered")]
fn query_past_the_end_panics() {
    let system = system("sol");
    let mut ephemeris = Ephemeris::new(
        &system,
        EphemerisOptions {
            step_seconds: 600.0,
            chunk_steps: 16,
        },
    );
    ephemeris.extend_to(6_000.0);
    ephemeris.body_state(BodyId(3), 6_000.1);
}

#[test]
#[should_panic(expected = "outside covered")]
fn forgotten_time_panics() {
    let system = system("sol");
    let mut ephemeris = Ephemeris::new(
        &system,
        EphemerisOptions {
            step_seconds: 600.0,
            chunk_steps: 16,
        },
    );
    ephemeris.extend_to(60_000.0);
    ephemeris.forget_before(30_000.0);
    assert!(ephemeris.start_time() > 0.0 && ephemeris.start_time() <= 30_000.0);
    ephemeris.body_state(BodyId(3), 30_000.0);
    ephemeris.body_state(BodyId(3), 100.0);
}

#[test]
#[should_panic(expected = "a non-root body requires an orbit")]
fn body_without_orbit_panics() {
    let mut spec = SystemSpec::from_json(
        &std::fs::read_to_string(format!("{}/systems/sol.json", env!("CARGO_MANIFEST_DIR")))
            .unwrap(),
    );
    spec.root.children[0].orbit = None;
    spec.root.children[0].orbit_plane = None;
    build_system(&spec);
}
