use std::time::Instant;

use glam::DVec3;
use serde_json::Value;
use void_frames::{BodyId, BodyStates};
use void_orbit::{
    BuiltSystem, EllipticElements, Ephemeris, EphemerisOptions, SystemSpec, build_system, osculating_orbit,
    solve_kepler_elliptic, state_from_elements, suggested_step_seconds,
};

fn system(id: &str) -> BuiltSystem {
    let path = format!("{}/systems/{id}.json", env!("CARGO_MANIFEST_DIR"));
    build_system(&SystemSpec::from_json(&std::fs::read_to_string(&path).expect(&path)))
}

fn golden(id: &str) -> Value {
    let path = format!("{}/tests/golden/{id}.json", env!("CARGO_MANIFEST_DIR"));
    serde_json::from_str(&std::fs::read_to_string(&path).expect(&path)).expect(&path)
}

fn f(v: &Value) -> f64 {
    v.as_f64().unwrap_or_else(|| panic!("not a number: {v}"))
}

fn vectors(v: &Value) -> Vec<DVec3> {
    let flat: Vec<f64> = v.as_array().unwrap().iter().map(f).collect();
    flat.chunks(3).map(|c| DVec3::new(c[0], c[1], c[2])).collect()
}

fn relative(a: f64, b: f64) -> f64 {
    if a == b { 0.0 } else { (a - b).abs() / a.abs().max(b.abs()) }
}

/// Largest |a - b| over the bodies.
fn worst(a: &[DVec3], b: &[DVec3]) -> f64 {
    a.iter().zip(b).map(|(x, y)| (*x - *y).length()).fold(0.0, f64::max)
}

fn check_system(id: &str) {
    let system = system(id);
    let golden = golden(id);

    let (mut bodies_error, mut angles_error) = (0.0_f64, 0.0_f64);
    for (body, expected) in system.bodies.iter().zip(golden["bodies"].as_array().unwrap()) {
        assert_eq!(body.id, expected["id"].as_str().unwrap());
        assert_eq!(body.parent_index, expected["parentIndex"].as_u64().map(|i| i as usize), "{} parent", body.id);
        let rotation = &expected["rotation"];
        let mut pairs = vec![
            (body.gm, f(&expected["gm"])),
            (body.rotation.period_seconds, f(&rotation["periodSeconds"])),
        ];
        for (ours, key) in [
            (body.orbit_period_seconds, "orbitPeriodSeconds"),
            (body.periapsis_fraction, "periapsisFraction"),
            (body.sphere_of_influence_meters, "sphereOfInfluenceMeters"),
        ] {
            assert_eq!(ours.is_some(), !expected[key].is_null(), "{} {key}", body.id);
            if let Some(ours) = ours {
                pairs.push((ours, f(&expected[key])));
            }
        }
        for (ours, theirs) in pairs {
            bodies_error = bodies_error.max(relative(ours, theirs));
        }
        // Angles absolutely: a locked moon's small obliquity comes from acos near 1, which turns
        // the lab's hypot-versus-sqrt ulp into 1e-14 rad, large only relative to the tiny angle.
        for (ours, key) in [
            (body.rotation.obliquity_radians, "obliquityRadians"),
            (body.rotation.pole_longitude_radians, "poleLongitudeRadians"),
            (body.rotation.angle_at_epoch_radians, "angleAtEpochRadians"),
        ] {
            angles_error = angles_error.max((ours - f(&rotation[key])).abs());
        }
    }

    let positions = worst(&system.positions, &vectors(&golden["positions"]));
    let velocities = worst(&system.velocities, &vectors(&golden["velocities"]));

    let step_seconds = suggested_step_seconds(&system.bodies, f(&golden["stepsPerOrbit"]));
    let step_error = relative(step_seconds, f(&golden["stepSeconds"]));
    let chunk_steps = golden["chunkSteps"].as_u64().unwrap() as usize;
    // The step itself is taken from the lab, so the integration below compares like with like.
    let mut ephemeris = Ephemeris::new(&system, EphemerisOptions { step_seconds: f(&golden["stepSeconds"]), chunk_steps });
    let energy_start = ephemeris.current_energy();

    println!("{id}: {} bodies, step {step_seconds:.3} s (lab {step_error:.0e}), body values {bodies_error:.1e}, spin angles {angles_error:.1e} rad, initial state {positions:.1e} m, {velocities:.1e} m/s",
        system.bodies.len());
    assert!(bodies_error < 1e-14, "{id}: body values differ by {bodies_error:e}");
    assert!(angles_error < 1e-13, "{id}: spin angles differ by {angles_error:e} rad");
    assert!(step_error < 1e-14, "{id}: step differs by {step_error:e}");
    assert!(positions < 1e-3 && velocities < 1e-9, "{id}: initial state differs by {positions:e} m, {velocities:e} m/s");

    // The integrator alone, from the lab's own initial state: any difference is the port's.
    let options = EphemerisOptions { step_seconds: f(&golden["stepSeconds"]), chunk_steps };
    let lab_start = BuiltSystem { positions: vectors(&golden["positions"]), velocities: vectors(&golden["velocities"]), ..system.clone() };
    let mut alone = Ephemeris::new(&lab_start, options);

    let n = system.bodies.len();
    let (mut p, mut v) = (vec![DVec3::ZERO; n], vec![DVec3::ZERO; n]);
    let started = Instant::now();
    for state in golden["states"].as_array().unwrap() {
        let t = f(&state["t"]);
        let (lab_p, lab_v) = (vectors(&state["positions"]), vectors(&state["velocities"]));
        alone.extend_to(t);
        alone.states_at(t, &mut p, Some(&mut v));
        let (alone_p, alone_v) = (worst(&p, &lab_p), worst(&v, &lab_v));
        ephemeris.extend_to(t);
        ephemeris.states_at(t, &mut p, Some(&mut v));
        let (dp, dv) = (worst(&p, &lab_p), worst(&v, &lab_v));
        println!("  t = {:>6.1} d: integrator alone {alone_p:.1e} m, {alone_v:.1e} m/s; whole port {dp:.1e} m, {dv:.1e} m/s",
            t / 86_400.0);
        assert!(alone_p < 1e-6 && alone_v < 1e-12, "{id} at t = {t}: the integrator alone differs by {alone_p:e} m, {alone_v:e} m/s");
        // The built state differs from the lab's by ulps (hypot, pow), which the orbits amplify;
        // a real port error shows up as kilometres.
        assert!(dp < 1.0 && dv < 1e-4, "{id} at t = {t}: {dp:e} m, {dv:e} m/s");

        for (i, (position, velocity)) in p.iter().zip(&v).enumerate() {
            assert_eq!(ephemeris.body_state(BodyId(i), t), (*position, *velocity), "{id}: body_state against states_at");
        }
    }
    let elapsed = started.elapsed();

    assert_eq!(ephemeris.end_time(), f(&golden["endTime"]), "{id}: end time");
    let energy = &golden["energy"];
    let energy_drift = relative(ephemeris.current_energy(), energy_start);
    let lab_drift = relative(f(&energy["end"]), f(&energy["start"]));
    let momentum = ephemeris.current_angular_momentum();
    let lab_momentum = DVec3::new(f(&golden["angularMomentum"]["x"]), f(&golden["angularMomentum"]["y"]), f(&golden["angularMomentum"]["z"]));
    let momentum_error = (momentum - lab_momentum).length() / lab_momentum.length();
    println!("  {:.1} days in {:.0} ms ({} steps); energy drift {energy_drift:.1e} (lab {lab_drift:.1e}); angular momentum against the lab {momentum_error:.1e}",
        ephemeris.end_time() / 86_400.0, elapsed.as_secs_f64() * 1e3, (ephemeris.end_time() / ephemeris.step_seconds()).round());
    assert!(relative(energy_start, f(&energy["start"])) < 1e-12, "{id}: initial energy");
    assert!(energy_drift < 1e-11, "{id}: energy drift {energy_drift:e}");
    assert!(momentum_error < 1e-12, "{id}: angular momentum differs by {momentum_error:e}");
}

#[test]
fn sol_matches_the_orbit_lab() {
    check_system("sol");
}

#[test]
fn binary_matches_the_orbit_lab() {
    check_system("binary");
}

#[test]
fn kepler_matches_the_orbit_lab() {
    let golden = golden("kepler");
    let gm = f(&golden["gm"]);
    let (mut anomaly, mut state, mut osculating) = (0.0_f64, 0.0_f64, 0.0_f64);
    for case in golden["cases"].as_array().unwrap() {
        let el: EllipticElements = serde_json::from_value(case["elements"].clone()).unwrap();
        anomaly = anomaly.max((solve_kepler_elliptic(el.mean_anomaly_radians, el.eccentricity) - f(&case["eccentricAnomaly"])).abs());
        let (p, v) = state_from_elements(&el, gm);
        let (lab_p, lab_v) = (vectors(&case["position"])[0], vectors(&case["velocity"])[0]);
        state = state.max((p - lab_p).length() / lab_p.length()).max((v - lab_v).length() / lab_v.length());
        let o = osculating_orbit(p, v, gm);
        let lab = &case["osculating"];
        for (ours, key) in [
            (o.semi_major_axis_meters, "semiMajorAxisMeters"),
            (o.eccentricity, "eccentricity"),
            (o.inclination_radians, "inclinationRadians"),
            (o.periapsis_radius_meters, "periapsisRadiusMeters"),
            (o.apoapsis_radius_meters, "apoapsisRadiusMeters"),
            (o.period_seconds, "periodSeconds"),
            (o.specific_energy, "specificEnergy"),
        ] {
            osculating = osculating.max(relative(ours, f(&lab[key])));
        }
    }
    println!("kepler: eccentric anomaly {anomaly:.1e} rad, state {state:.1e}, osculating {osculating:.1e} (relative)");
    assert!(anomaly < 1e-14 && state < 1e-14 && osculating < 1e-12);
}

#[test]
#[should_panic(expected = "outside covered")]
fn query_past_the_end_panics() {
    let system = system("sol");
    let mut ephemeris = Ephemeris::new(&system, EphemerisOptions { step_seconds: 600.0, chunk_steps: 16 });
    ephemeris.extend_to(6_000.0);
    ephemeris.body_state(BodyId(3), 6_000.1);
}

#[test]
#[should_panic(expected = "outside covered")]
fn forgotten_time_panics() {
    let system = system("sol");
    let mut ephemeris = Ephemeris::new(&system, EphemerisOptions { step_seconds: 600.0, chunk_steps: 16 });
    ephemeris.extend_to(60_000.0);
    ephemeris.forget_before(30_000.0);
    assert!(ephemeris.start_time() > 0.0 && ephemeris.start_time() <= 30_000.0);
    ephemeris.body_state(BodyId(3), 30_000.0);
    ephemeris.body_state(BodyId(3), 100.0);
}

#[test]
#[should_panic(expected = "a non-root body requires an orbit")]
fn body_without_orbit_panics() {
    let mut spec = SystemSpec::from_json(&std::fs::read_to_string(format!("{}/systems/sol.json", env!("CARGO_MANIFEST_DIR"))).unwrap());
    spec.root.children[0].orbit = None;
    spec.root.children[0].orbit_plane = None;
    build_system(&spec);
}
