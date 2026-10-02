//! The Simulation against the orbit lab's (golden data from `golden/simulation.ts`): the same
//! scripted session of coasting, manual burns in each attitude mode, hold, warp, a planned burn
//! and an impact, compared at every checkpoint.

use glam::DVec3;
use serde_json::Value;
use void_orbit::{
    AttitudeMode, EngineSpec, ManeuverSpec, ReferenceMode, Simulation, SimulationOptions,
    StartPlane, SystemSpec, Tolerances, VesselStartSpec,
};

fn f(v: &Value) -> f64 {
    v.as_f64().unwrap_or_else(|| panic!("not a number: {v}"))
}

fn v3(v: &Value) -> DVec3 {
    DVec3::new(f(&v[0]), f(&v[1]), f(&v[2]))
}

/// The view lab's setup.
pub fn view_lab_simulation() -> Simulation {
    let path = format!("{}/systems/sol.json", env!("CARGO_MANIFEST_DIR"));
    Simulation::new(SimulationOptions {
        system: SystemSpec::from_json(&std::fs::read_to_string(&path).expect(&path)),
        steps_per_orbit: 256.0,
        tolerances: Tolerances {
            position_meters: 1e-4,
            velocity_meters_per_second: 1e-7,
        },
        vessel_start: VesselStartSpec {
            home_body_id: "aurelia".into(),
            altitude_meters: 100e3,
            plane: StartPlane::Equatorial {
                inclination_radians: 0.0,
            },
        },
        engine: EngineSpec {
            thrust_newtons: 250e3,
            specific_impulse_seconds: 350.0,
            dry_mass_kg: 10e3,
            fuel_mass_kg: 30e3,
        },
        retention_seconds: 86_400.0,
        prediction_horizon_seconds: 3.0 * 3600.0,
        plan_coast_seconds: 86_400.0,
    })
}

#[test]
fn matches_the_orbit_lab() {
    let path = format!(
        "{}/tests/golden/simulation.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let golden: Value =
        serde_json::from_str(&std::fs::read_to_string(&path).expect(&path)).expect(&path);
    let checkpoints = golden["checkpoints"].as_array().unwrap();
    let mut sim = view_lab_simulation();
    let mut next = 0;
    let mut worst_position = 0.0_f64;
    let mut worst_velocity = 0.0_f64;
    let mut check = |sim: &mut Simulation, label: &str| {
        let g = &checkpoints[next];
        next += 1;
        assert_eq!(g["label"], label);
        let s = sim.vessel();
        let (dp, dv) = (
            (s.position - v3(&g["position"])).length(),
            (s.velocity - v3(&g["velocity"])).length(),
        );
        let thrust = sim.thrust_direction();
        println!(
            "{label:<36} t {:>8.1} s  position {dp:.1e} m  velocity {dv:.1e} m/s  mass {:.3e} kg  thrust {:.1e}",
            sim.time,
            s.mass_kg - f(&g["massKg"]),
            (thrust - v3(&g["thrust"])).length()
        );
        worst_position = worst_position.max(dp);
        worst_velocity = worst_velocity.max(dv);
        assert!((sim.time - f(&g["time"])).abs() < 1e-9, "{label}: time");
        assert!(dp < 1e-2 && dv < 1e-5, "{label}: state");
        assert!((s.mass_kg - f(&g["massKg"])).abs() < 1e-6, "{label}: mass");
        assert_eq!(
            sim.navigation_reference() as u64,
            g["reference"].as_u64().unwrap()
        );
        assert_eq!(
            sim.effective_throttle(),
            f(&g["effectiveThrottle"]),
            "{label}"
        );
        assert_eq!(sim.attitude_mode().label(), g["attitude"].as_str().unwrap());
        assert!(
            (thrust - v3(&g["thrust"])).length() < 1e-9,
            "{label}: thrust direction"
        );
        assert_eq!(
            sim.prediction_generation,
            g["generation"].as_u64().unwrap(),
            "{label}: prediction restarts"
        );
        match (&g["prediction"], sim.prediction.count()) {
            (Value::Null, n) => assert_eq!(n, 0, "{label}: prediction"),
            (p, n) => {
                assert_eq!(
                    n as u64,
                    p["count"].as_u64().unwrap(),
                    "{label}: prediction samples"
                );
                let last = sim.prediction.last_time();
                assert!(
                    (last - f(&p["lastTime"])).abs() < 1e-6,
                    "{label}: prediction end"
                );
                let d = (sim.prediction.sample(last).0 - v3(&p["last"])).length();
                assert!(d < 1.0, "{label}: prediction end {d} m apart");
            }
        }
        match (&g["impact"], sim.impact) {
            (Value::Null, None) => {}
            (i, Some(impact)) => {
                assert_eq!(impact.body_index as u64, i["body"].as_u64().unwrap());
                let (dt, dp) = (
                    impact.time - f(&i["time"]),
                    (impact.body_fixed_position - v3(&i["at"])).length(),
                );
                println!("{:<36} impact time {dt:.1e} s, site {dp:.1e} m", "");
                assert!(dt.abs() < 1e-3 && dp < 10.0, "{label}: impact");
            }
            (i, None) => panic!("{label}: the lab hit {i}, Rust did not"),
        }
    };
    let frames = |sim: &mut Simulation, count: usize, warp: f64| {
        for _ in 0..count {
            sim.advance(warp / 60.0, 20_000);
            sim.extend_prediction(4_000);
        }
    };
    let burn = |sim: &mut Simulation, mode: AttitudeMode, throttle: f64, count: usize| {
        sim.set_attitude(mode);
        sim.throttle = throttle;
        frames(sim, count, 1.0);
        sim.throttle = 0.0;
    };

    check(&mut sim, "start");
    frames(&mut sim, 120, 100.0);
    check(&mut sim, "coast 200 s at 100x");
    burn(&mut sim, AttitudeMode::Prograde, 1.0, 300);
    check(&mut sim, "prograde 5 s");
    burn(&mut sim, AttitudeMode::Normal, 0.5, 120);
    check(&mut sim, "normal 2 s at half");
    burn(&mut sim, AttitudeMode::RadialOut, 1.0, 60);
    check(&mut sim, "radial-out 1 s");
    sim.set_attitude(AttitudeMode::Hold);
    sim.throttle = 0.3;
    frames(&mut sim, 120, 1.0);
    sim.throttle = 0.0;
    check(&mut sim, "hold 2 s");
    frames(&mut sim, 300, 1000.0);
    check(&mut sim, "coast 5000 s at 1000x");
    let aurelia = sim.body_index("aurelia");
    sim.add_maneuver(ManeuverSpec {
        start_time: sim.time + 600.0,
        reference_body: aurelia,
        reference_mode: ReferenceMode::Auto,
        prograde: 40.0,
        normal: 0.0,
        radial: 5.0,
    });
    for _ in 0..50 {
        sim.extend_plan(4_000);
    }
    check(&mut sim, "planned");
    frames(&mut sim, 120, 10.0);
    check(&mut sim, "before the planned burn");
    burn(&mut sim, AttitudeMode::Retrograde, 1.0, 900);
    check(&mut sim, "retrograde 15 s");
    frames(&mut sim, 600, 10.0);
    check(&mut sim, "falling");
    frames(&mut sim, 300, 100.0);
    check(&mut sim, "planned burn flown, coasting down");
    frames(&mut sim, 300, 1000.0);
    check(&mut sim, "after impact");
    println!("worst over the session: {worst_position:.1e} m, {worst_velocity:.1e} m/s");
}
