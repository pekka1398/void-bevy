//! void-multiscale against lab/multiscale on the lab's own outputs (`golden/multiscale.ts`). Split
//! arithmetic, the coupled world and the probe are plain float64 arithmetic plus V8's `hypot`
//! (matched by `void_math`), and the built systems match orbit's golden data, so everything is
//! compared bit for bit.

use glam::DVec3;
use serde_json::Value;
use void_multiscale::*;

mod common;
use common::{compact_seeds, huge};

fn golden() -> Value {
    let path = format!(
        "{}/tests/golden/multiscale.json",
        env!("CARGO_MANIFEST_DIR")
    );
    serde_json::from_str(&std::fs::read_to_string(&path).expect(&path)).expect(&path)
}

fn f(v: &Value) -> f64 {
    v.as_f64().unwrap_or_else(|| panic!("not a number: {v}"))
}

fn v3(v: &Value) -> DVec3 {
    DVec3::new(f(&v[0]), f(&v[1]), f(&v[2]))
}

fn split(v: &Value) -> SplitPosition {
    SplitPosition {
        cell: [0, 1, 2].map(|k| v["cell"][k].as_str().unwrap().parse().unwrap()),
        offset: v3(&v["offset"]),
    }
}

fn floats(v: &Value) -> Vec<f64> {
    v.as_array().unwrap().iter().map(f).collect()
}

#[test]
fn split_arithmetic_matches() {
    let g = golden();
    for (i, c) in g["splits"].as_array().unwrap().iter().enumerate() {
        let (a, b, d) = (split(&c["a"]), split(&c["b"]), v3(&c["delta"]));
        assert_eq!(a.translate(d), split(&c["translate"]), "case {i} translate");
        assert_eq!(a.compose(&b), split(&c["compose"]), "case {i} compose");
        assert_eq!(
            a.difference(&b),
            split(&c["difference"]),
            "case {i} difference"
        );
        assert_eq!(
            split(&c["near"]).relative(&a),
            v3(&c["relative"]),
            "case {i} relative"
        );
        let text = c["text"].as_str().unwrap();
        assert_eq!(SplitPosition::deserialize(text), Ok(a), "case {i} lab JSON");
        assert_eq!(
            SplitPosition::deserialize(&a.serialize()),
            Ok(a),
            "case {i} round trip"
        );
        let raw = v3(&c["raw"]);
        assert_eq!(
            SplitPosition::at(raw),
            split(&g["raw"][i]),
            "case {i} carry"
        );
    }
}

#[test]
fn drift_matches() {
    let g = golden();
    for (i, c) in g["splits"].as_array().unwrap().iter().enumerate() {
        let (a, d) = (split(&c["a"]), v3(&c["delta"]));
        let mut correction = v3(&c["before"]);
        assert_eq!(a.drift(d, &mut correction), split(&c["drift"]), "case {i}");
        assert_eq!(correction, v3(&c["correction"]), "case {i} correction");
    }
}

fn compare_states(label: &str, got: &[SystemState], want: &Value) {
    for (k, (g, w)) in got.iter().zip(want.as_array().unwrap()).enumerate() {
        let at = format!("{label} system {k}");
        assert_eq!(g.origin, split(&w["origin"]), "{at} origin");
        assert_eq!(g.velocity, v3(&w["velocity"]), "{at} velocity");
        assert_eq!(g.acceleration, v3(&w["acceleration"]), "{at} acceleration");
        assert_eq!(g.positions, floats(&w["positions"]), "{at} positions");
        assert_eq!(g.velocities, floats(&w["velocities"]), "{at} velocities");
        assert_eq!(
            g.accelerations,
            floats(&w["accelerations"]),
            "{at} accelerations"
        );
    }
}

#[test]
fn compact_world_matches() {
    let g = golden();
    for (anchor, run) in [SplitPosition::ORIGIN, huge()]
        .into_iter()
        .zip(g["compact"].as_array().unwrap())
    {
        let mut world = CoupledWorld::new(compact_seeds(anchor), 10.0, 8192);
        world.extend_to(2000.0, 100_000);
        for s in run["samples"].as_array().unwrap() {
            let t = f(&s["t"]);
            let state = world.at(t);
            compare_states(&format!("t {t}"), &state, &s["systems"]);
            let p = state[0].origin.translate(DVec3::new(8e8, 1e9, 2e8));
            assert_eq!(world.gravity_at(t, &p), v3(&s["gravity"]), "t {t} gravity");
        }
    }
}

/// The wide fixture started from the lab's own initial body states (the t = 0 sample): orbit's
/// `build_system` solves Kepler with fdlibm's sin and cos, V8 with its own, an ulp apart, which
/// 210 years of flight would carry. This isolates the world and the probe.
fn lab_wide_world(g: &Value) -> CoupledWorld {
    let mut seeds = wide_seeds(default_galaxy());
    for (seed, state) in seeds
        .iter_mut()
        .zip(g["wide"][0]["systems"].as_array().unwrap())
    {
        let triples = |v: &Value| {
            floats(v)
                .chunks(3)
                .map(|c| DVec3::new(c[0], c[1], c[2]))
                .collect::<Vec<_>>()
        };
        let (p, v) = (triples(&state["positions"]), triples(&state["velocities"]));
        // Only the last bits may differ.
        for (a, b) in seed.system.positions.iter().zip(&p) {
            assert!((*a - *b).length() <= 1e-4, "built position {a} vs lab {b}");
        }
        seed.system.positions = p;
        seed.system.velocities = v;
    }
    CoupledWorld::new(seeds, 86400.0, 8192)
}

#[test]
fn wide_world_matches_over_ten_years() {
    let g = golden();
    let mut world = lab_wide_world(&g);
    world.extend_to(10.0 * YEAR, 100_000);
    for s in g["wide"].as_array().unwrap() {
        let t = f(&s["t"]);
        compare_states(&format!("t {t}"), &world.at(t), &s["systems"]);
    }
    let base = FramedState {
        frame: "Aster".into(),
        position: SplitPosition::at(DVec3::new(1.125, 0.01, -2.25)),
        velocity: DVec3::new(1.5, -0.25, 0.125),
    };
    let mut next = base;
    for (i, want) in g["reframed"].as_array().unwrap().iter().enumerate() {
        next = reframe(
            &world,
            50.0,
            &next,
            if i % 2 == 1 { "Aster" } else { "Beryl" },
        );
        assert_eq!(next.frame, want["frame"].as_str().unwrap());
        assert_eq!(next.position, split(&want["position"]), "reframe {i}");
        assert_eq!(next.velocity, v3(&want["velocity"]), "reframe {i}");
        assert_eq!(absolute(&world, 50.0, &next).0, split(&want["absolute"]));
    }
}

fn compare_flight(label: &str, flight: &Traveller, want: &Value) {
    assert_eq!(flight.time, f(&want["time"]), "{label} time");
    assert_eq!(
        flight.steps,
        want["steps"].as_u64().unwrap(),
        "{label} steps"
    );
    assert_eq!(
        flight.state.frame,
        want["frame"].as_str().unwrap(),
        "{label} frame"
    );
    assert_eq!(
        flight.state.position,
        split(&want["position"]),
        "{label} position"
    );
    assert_eq!(
        flight.state.velocity,
        v3(&want["velocity"]),
        "{label} velocity"
    );
    assert_eq!(
        flight.terminal.is_some(),
        !want["terminal"].is_null(),
        "{label} terminal"
    );
    let events = want["events"].as_array().unwrap();
    assert_eq!(flight.events.len(), events.len(), "{label} events");
    for (e, w) in flight.events.iter().zip(events) {
        assert_eq!(e.time, f(&w["time"]), "{label} event time");
        assert_eq!(
            (e.from.as_str(), e.to.as_str()),
            (w["from"].as_str().unwrap(), w["to"].as_str().unwrap())
        );
        assert_eq!(
            e.position_jump,
            f(&w["positionJump"]),
            "{label} position jump"
        );
        assert_eq!(
            e.velocity_jump,
            f(&w["velocityJump"]),
            "{label} velocity jump"
        );
    }
}

#[test]
fn compact_flight_matches_through_the_hand_off() {
    let g = golden();
    let mut world = CoupledWorld::new(compact_seeds(SplitPosition::ORIGIN), 10.0, 8192);
    let mut flight = Traveller::new(
        &world,
        0.0,
        FramedState {
            frame: "A".into(),
            position: SplitPosition::at(DVec3::new(8e8, 2e9, 0.0)),
            velocity: DVec3::new(500_000.0, 0.0, 0.0),
        },
        100.0,
    );
    for (t, want) in [1000.0, 4500.0, 9000.0]
        .into_iter()
        .zip(g["compactFlight"].as_array().unwrap())
    {
        flight.advance_to(&mut world, t, 10_000);
        compare_flight(&format!("compact {t} s"), &flight, want);
    }
}

#[test]
fn light_year_flight_matches_for_210_years() {
    let g = golden();
    let mut world = lab_wide_world(&g);
    let mut flight = transfer(&world, 0.02);
    for (years, want) in [1.0, 102.0, 210.0]
        .into_iter()
        .zip(g["longFlight"]["legs"].as_array().unwrap())
    {
        flight.advance_to(&mut world, years * YEAR, 100_000);
        compare_flight(&format!("{years} yr"), &flight, want);
    }
    assert_eq!(world.steps, g["longFlight"]["worldSteps"].as_u64().unwrap());
    let handoff = flight
        .events
        .iter()
        .find(|e| e.from == "Aster" && e.to == "Beryl")
        .expect("Aster to Beryl hand-off");
    eprintln!(
        "{} world steps, {} probe steps; hand-off at {:.3} yr, jumps {:.1e} m, {:.1e} m/s",
        world.steps,
        flight.steps,
        handoff.time / YEAR,
        handoff.position_jump,
        handoff.velocity_jump
    );
}
