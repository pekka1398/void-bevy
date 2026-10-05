//! The vessel propagator, apsides, dominance and flight plan against the orbit lab
//! (golden data from `golden/vessel.ts`).

use glam::DVec3;
use serde_json::Value;
use void_orbit::{
    AdvanceOutcome, ApsisKind, AttitudeLaw, Control, DominanceTree, Ephemeris, EphemerisOptions,
    FlightPlan, ForceControl, ManeuverSpec, PlanEngine, PropagationRun, ReferenceMode, SystemSpec,
    ThrustControl, Tolerances, Trajectory, VesselPropagator, VesselState, build_system,
    find_apsides, suggested_step_seconds,
};

fn golden() -> Value {
    let path = format!("{}/tests/golden/vessel.json", env!("CARGO_MANIFEST_DIR"));
    serde_json::from_str(&std::fs::read_to_string(&path).expect(&path)).expect(&path)
}

fn ephemeris() -> Ephemeris {
    let path = format!("{}/systems/sol.json", env!("CARGO_MANIFEST_DIR"));
    let system = build_system(&SystemSpec::from_json(
        &std::fs::read_to_string(&path).expect(&path),
    ));
    let step_seconds = suggested_step_seconds(&system.bodies, 256.0);
    Ephemeris::new(
        &system,
        EphemerisOptions {
            step_seconds,
            chunk_steps: 2048,
        },
    )
}

fn f(v: &Value) -> f64 {
    v.as_f64().unwrap_or_else(|| panic!("not a number: {v}"))
}

fn v3(v: &Value) -> DVec3 {
    let a = v.as_array().unwrap_or_else(|| panic!("not a vector: {v}"));
    DVec3::new(f(&a[0]), f(&a[1]), f(&a[2]))
}

fn xyz(v: &Value) -> DVec3 {
    DVec3::new(f(&v["x"]), f(&v["y"]), f(&v["z"]))
}

fn state(v: &Value) -> VesselState {
    VesselState {
        time: f(&v["time"]),
        position: v3(&v["position"]),
        velocity: v3(&v["velocity"]),
        mass_kg: f(&v["massKg"]),
    }
}

fn control(v: &Value) -> Option<Control> {
    if v.is_null() {
        return None;
    }
    if !v["force"].is_null() {
        return Some(Control::Force(ForceControl {
            force: xyz(&v["force"]),
            mass_flow_kg_per_second: f(&v["massFlowKgPerSecond"]),
            minimum_mass_kg: f(&v["minimumMassKg"]),
        }));
    }
    let a = &v["attitude"];
    let body = |key: &str| a[key].as_u64().unwrap() as usize;
    let attitude = match a["kind"].as_str().unwrap() {
        "inertial" => AttitudeLaw::Inertial {
            direction: xyz(&a["direction"]),
        },
        "frenet" => AttitudeLaw::Frenet {
            reference_body: body("referenceBody"),
            tangent: f(&a["tangent"]),
            normal: f(&a["normal"]),
            radial: f(&a["radial"]),
        },
        "surface" => AttitudeLaw::Surface {
            reference_body: body("referenceBody"),
            up: f(&a["up"]),
            prograde: f(&a["prograde"]),
        },
        other => panic!("attitude {other}"),
    };
    Some(Control::Thrust(ThrustControl {
        thrust_newtons: f(&v["thrustNewtons"]),
        exhaust_velocity: f(&v["exhaustVelocity"]),
        minimum_mass_kg: f(&v["minimumMassKg"]),
        attitude,
    }))
}

fn tolerances(g: &Value) -> Tolerances {
    Tolerances {
        position_meters: f(&g["tolerances"]["positionMeters"]),
        velocity_meters_per_second: f(&g["tolerances"]["velocityMetersPerSecond"]),
    }
}

/// Position, velocity and mass differences of a 7-entry state.
fn state_error(y: &[f64], lab: &[f64]) -> (f64, f64, f64) {
    let p = DVec3::new(y[0] - lab[0], y[1] - lab[1], y[2] - lab[2]).length();
    let v = DVec3::new(y[3] - lab[3], y[4] - lab[4], y[5] - lab[5]).length();
    (p, v, (y[6] - lab[6]).abs())
}

#[test]
fn legs_match_the_orbit_lab() {
    let g = golden();
    let mut ephemeris = ephemeris();
    let home = g["home"].as_u64().unwrap() as usize;
    for leg in g["legs"].as_array().unwrap() {
        let name = leg["name"].as_str().unwrap();
        let from = state(&leg["from"]);
        let mut propagator = VesselPropagator::new(&ephemeris, tolerances(&g));
        let mut run = PropagationRun::new(from);
        let mut trajectory = Trajectory::new();
        trajectory.append(run.time, &run.y);
        let outcome = propagator.advance(
            &mut ephemeris,
            &mut run,
            from.time + f(&leg["duration"]),
            1_000_000,
            Some(&mut trajectory),
            control(&leg["control"]),
        );

        let lab_outcome = match leg["outcome"]["kind"].as_str().unwrap() {
            "reached" => AdvanceOutcome::Reached,
            "budget" => AdvanceOutcome::Budget,
            "impact" => AdvanceOutcome::Impact {
                body: leg["outcome"]["bodyIndex"].as_u64().unwrap() as usize,
            },
            other => panic!("outcome {other}"),
        };
        assert_eq!(outcome, lab_outcome, "{name}: outcome");
        let steps = (
            propagator.accepted_steps,
            propagator.rejected_steps,
            trajectory.count() as u64,
        );
        let lab_steps = (
            leg["accepted"].as_u64().unwrap(),
            leg["rejected"].as_u64().unwrap(),
            leg["samples"].as_u64().unwrap(),
        );
        assert_eq!(steps, lab_steps, "{name}: accepted, rejected, samples");

        let lab_end: Vec<f64> = leg["end"]["y"].as_array().unwrap().iter().map(f).collect();
        let (dp, dv, dm) = state_error(&run.y, &lab_end);
        let dt = (run.time - f(&leg["end"]["time"])).abs();
        let (mid_p, mid_v) = trajectory.sample(f(&leg["mid"]["t"]));
        let mid = (mid_p - v3(&leg["mid"]["position"]))
            .length()
            .max((mid_v - v3(&leg["mid"]["velocity"])).length());

        let apsides = find_apsides(&trajectory, &ephemeris, home, from.time, 8);
        let lab_apsides = leg["apsides"].as_array().unwrap();
        assert_eq!(apsides.len(), lab_apsides.len(), "{name}: apsis count");
        let mut apsis_error = 0.0_f64;
        for (ours, lab) in apsides.iter().zip(lab_apsides) {
            let kind = if lab["kind"] == "periapsis" {
                ApsisKind::Periapsis
            } else {
                ApsisKind::Apoapsis
            };
            assert_eq!(ours.kind, kind, "{name}: apsis kind");
            apsis_error = apsis_error
                .max((ours.time - f(&lab["time"])).abs())
                .max((ours.distance_meters - f(&lab["distanceMeters"])).abs());
        }
        println!(
            "{name:>8}: {outcome:?}, {} steps; end {dp:.1e} m, {dv:.1e} m/s, {dm:.1e} kg, {dt:.1e} s; mid sample {mid:.1e}; {} apsides within {apsis_error:.1e}",
            steps.0,
            apsides.len()
        );
        // Bit-for-bit where only gravity acts (the coast leg is). Thrust directions are normalised
        // with Math.hypot in the lab and sqrt(dot) here; that ulp moves the error estimate and
        // so the step sizes, by far less than the propagator's own per-step tolerance.
        let tol = tolerances(&g);
        assert!(
            dp < tol.position_meters
                && dv < tol.velocity_meters_per_second
                && dm < 1e-9
                && dt < 1e-6
                && mid < tol.position_meters,
            "{name}: differs from the lab beyond the propagator's tolerances"
        );
        assert!(
            apsis_error < 1e-6,
            "{name}: apsides differ by {apsis_error:e}"
        );
    }
}

#[test]
fn flight_plan_matches_the_orbit_lab() {
    let g = golden();
    let mut ephemeris = ephemeris();
    let home = g["home"].as_u64().unwrap() as usize;
    let engine = &g["engine"];
    let engine = PlanEngine {
        thrust_newtons: f(&engine["thrustNewtons"]),
        exhaust_velocity: f(&engine["exhaustVelocity"]),
        dry_mass_kg: f(&engine["dryMassKg"]),
    };
    let lab = &g["plan"];
    let start = state(&g["start"]);
    ephemeris.extend_to(start.time);
    let mut plan = FlightPlan::new(&ephemeris, tolerances(&g), engine, f(&lab["coast"]));
    plan.rebase(&PropagationRun::new(start));
    let burn = |start_time: f64, prograde: f64, normal: f64, radial: f64| ManeuverSpec {
        start_time,
        reference_body: home,
        reference_mode: ReferenceMode::Fixed,
        prograde,
        normal,
        radial,
    };
    let t0 = start.time;
    plan.add(burn(t0 + 1800.0, 800.0, 50.0, -20.0));
    let apoapsis = plan
        .start_at_apsis(&mut ephemeris, 0, ApsisKind::Apoapsis, t0)
        .expect("an apoapsis");
    let periapsis = plan
        .start_at_apsis(&mut ephemeris, 0, ApsisKind::Periapsis, t0)
        .expect("a periapsis");
    plan.add(burn(t0 + 4.0 * 3600.0, -300.0, 0.0, 0.0));
    let second = plan
        .start_at_apsis(&mut ephemeris, 1, ApsisKind::Periapsis, t0)
        .expect("a second periapsis");
    plan.add(burn(t0 + 4.0 * 3600.0 + 10.0, 10.0, 0.0, 0.0));
    while !plan.complete() {
        plan.extend(&mut ephemeris, 5000);
    }
    let at = f(&lab["positionAt"]["t"]);
    let position = plan
        .position_at(&mut ephemeris, at)
        .expect("the plan reaches T+4 h");

    let placement = (apoapsis - f(&lab["apoapsisStart"]["startTime"]))
        .abs()
        .max((periapsis - f(&lab["periapsisStart"]["startTime"])).abs())
        .max((second - f(&lab["secondPeriapsis"]["startTime"])).abs());
    for (i, status) in lab["statuses"].as_array().unwrap().iter().enumerate() {
        match (plan.status(i), status["ok"].as_bool().unwrap()) {
            (Ok(burn), true) => {
                let b = &status["burn"];
                assert!(
                    (burn.end_time - f(&b["endTime"])).abs() < 1e-9,
                    "burn {i} end"
                );
                assert!(
                    (burn.mass_after_kg - f(&b["massAfterKg"])).abs() < 1e-9,
                    "burn {i} mass"
                );
            }
            (Err(reason), false) => assert_eq!(
                reason,
                status["reason"].as_str().unwrap(),
                "burn {i} reason"
            ),
            (ours, _) => panic!("burn {i}: {ours:?} against the lab's {status}"),
        }
    }
    let end = plan.trajectory.count() - 1;
    let end_position = (plan.trajectory.position(end) - v3(&lab["end"]["position"])).length();
    let end_velocity = (plan.trajectory.velocity(end) - v3(&lab["end"]["velocity"])).length();
    let impact_speed = plan.trajectory.velocity(end).length();
    let at_error = (position - v3(&lab["positionAt"]["position"])).length();
    let impact = plan
        .impact()
        .expect("the second burn drops the periapsis 700 km below the surface");
    let impact_error = (impact.time - f(&lab["impact"]["time"])).abs();
    println!(
        "plan: {} samples (lab {}); apsis placement {placement:.1e} s; position at T+4 h {at_error:.1e} m; \
         impact on body {} at {:.6} s, {impact_error:.1e} s from the lab, end state {end_position:.1e} m, {end_velocity:.1e} m/s at {impact_speed:.0} m/s",
        plan.trajectory.count(),
        lab["samples"],
        impact.body,
        impact.time,
    );
    assert_eq!(
        plan.trajectory.count() as u64,
        lab["samples"].as_u64().unwrap(),
        "plan samples"
    );
    assert_eq!(
        impact.body as u64,
        lab["impact"]["bodyIndex"].as_u64().unwrap(),
        "plan impact body"
    );
    // The burns' Frenet directions are normalised with Math.hypot in the lab and sqrt(dot)
    // here; that ulp changes step sizes, and over two burns and 559 steps it grows to millimetres.
    // The impact is bisected only to 1e-4 s, so its state may differ by the distance covered then.
    let resolution = 1e-4;
    assert!(
        placement < 1e-3,
        "apsis placement differs by {placement:e} s"
    );
    assert!(
        at_error < 0.01,
        "position at T+4 h differs by {at_error:e} m"
    );
    assert!(
        impact_error < resolution,
        "impact time differs by {impact_error:e} s"
    );
    assert!(
        end_position < impact_speed * resolution + 0.01 && end_velocity < 0.01,
        "impact state differs by {end_position:e} m, {end_velocity:e} m/s"
    );
}

#[test]
fn partially_computed_plan_restores_and_continues_without_restarting() {
    let g = golden();
    let mut eph = ephemeris();
    let start = state(&g["start"]);
    eph.extend_to(start.time);
    let engine = PlanEngine {
        thrust_newtons: f(&g["engine"]["thrustNewtons"]),
        exhaust_velocity: f(&g["engine"]["exhaustVelocity"]),
        dry_mass_kg: f(&g["engine"]["dryMassKg"]),
    };
    let mut original = FlightPlan::new(&eph, tolerances(&g), engine, 3000.0);
    original.rebase(&PropagationRun::new(start));
    for (after, dv) in [(30.0, 60.0), (200.0, -10.0), (200.1, 1.0)] {
        original.add(ManeuverSpec {
            start_time: start.time + after,
            reference_body: g["home"].as_u64().unwrap() as usize,
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
fn dominance_matches_the_orbit_lab() {
    let g = golden();
    let mut ephemeris = ephemeris();
    let lab = &g["dominance"];
    let t = f(&lab["time"]);
    ephemeris.extend_to(t);
    let mut positions = vec![DVec3::ZERO; ephemeris.bodies().len()];
    ephemeris.positions_at(t, &mut positions);
    let tree = DominanceTree::new(ephemeris.bodies());
    let ours: Vec<u64> = lab["points"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| tree.dominant(&positions, v3(p)) as u64)
        .collect();
    let theirs: Vec<u64> = lab["dominant"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d.as_u64().unwrap())
        .collect();
    assert_eq!(ours, theirs);
}

#[test]
#[should_panic(expected = "fell below dry mass")]
fn burning_past_dry_mass_panics() {
    let g = golden();
    let mut ephemeris = ephemeris();
    let mut propagator = VesselPropagator::new(&ephemeris, tolerances(&g));
    let mut run = PropagationRun::new(state(&g["start"]));
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
    let g = golden();
    let mut ephemeris = ephemeris();
    let leg = g["legs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["name"] == "impact")
        .unwrap();
    let mut propagator = VesselPropagator::new(&ephemeris, tolerances(&g));
    let mut run = PropagationRun::new(state(&leg["from"]));
    let end = run.time + 86_400.0;
    propagator.advance(&mut ephemeris, &mut run, end, 1_000_000, None, None);
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
    let g = golden();
    let mut eph = ephemeris();
    let mut run = PropagationRun::new(state(&g["start"]));
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
