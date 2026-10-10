//! The coupled world, frames and traveller: precision across light-years, conservation and
//! agreement with a flat N-body reference. The moving-origin adapter is checked in ephemeris.rs.

use std::panic::{AssertUnwindSafe, catch_unwind};
use void_testkit::*;

use glam::DVec3;
use void_frames::{CELL_METERS, SplitPosition};
use void_multiscale::*;
use void_orbit::{
    AdvanceOutcome, BuiltSystem, CelestialBody, Ephemeris, EphemerisOptions, PropagationRun,
    Tolerances, VesselPropagator, VesselState,
};

mod common;
use common::{compact_seeds, huge, small_system};

fn near(a: f64, b: f64, tolerance: f64) {
    assert!((a - b).abs() <= tolerance, "{a} != {b} ± {tolerance}");
}

fn panics(f: impl FnOnce()) -> bool {
    catch_unwind(AssertUnwindSafe(f)).is_err()
}

#[test]
fn centimetres_survive_at_30000_light_years_and_any_origin() {
    for origin in [wide_seeds(default_galaxy())[0].origin, huge()] {
        let a = origin.translate(DVec3::new(1.25, 2.0, 3.0));
        let b = a.translate(DVec3::new(0.01, -0.02, 0.005));
        let d = b.relative(&a);
        near(d.x, 0.01, 5e-7);
        near(d.y, -0.02, 5e-7);
        near(d.z, 0.005, 5e-7);
    }
}

#[test]
fn cell_boundaries_composition_and_json_round_trip() {
    let a = SplitPosition::at(DVec3::new(
        CELL_METERS / 2.0 - 0.125,
        -CELL_METERS / 2.0 + 0.125,
        0.0,
    ));
    let b = a.translate(DVec3::new(0.25, -0.25, 0.0));
    assert_eq!(b.cell[0], 1);
    assert_eq!(b.cell[1], -1);
    assert_eq!(b.relative(&a), DVec3::new(0.25, -0.25, 0.0));
    let far = huge().compose(&b);
    assert_eq!(SplitPosition::deserialize(&far.serialize()), Ok(far));
    assert_eq!(far.difference(&huge()), b);
}

#[test]
fn invalid_positions_and_unsafe_conversion_fail_explicitly() {
    assert!(panics(|| {
        SplitPosition::at(DVec3::new(f64::NAN, 0.0, 0.0));
    }));
    assert!(panics(|| {
        SplitPosition::at(DVec3::new(2f64.powi(90), 0.0, 0.0));
    }));
    assert!(panics(|| {
        huge().vector();
    }));
    assert!(SplitPosition::deserialize(r#"{"cell":["a","0","0"],"offset":[0,0,0]}"#).is_err());
}

/// The same bodies in one ungrouped orbit ephemeris, relative to `anchor`.
fn flat_reference(seeds: &[SystemSeed], anchor: SplitPosition, step: f64) -> Ephemeris {
    let mut system = BuiltSystem {
        name: "direct reference".into(),
        bodies: vec![],
        positions: vec![],
        velocities: vec![],
    };
    for seed in seeds {
        let first = system.bodies.len();
        let origin = seed.origin.relative(&anchor);
        for (j, body) in seed.system.bodies.iter().enumerate() {
            system.bodies.push(CelestialBody {
                id: format!("{}/{}", seed.id, body.id),
                index: system.bodies.len(),
                parent_index: body.parent_index.map(|p| first + p),
                ..body.clone()
            });
            system.positions.push(origin + seed.system.positions[j]);
            system
                .velocities
                .push(seed.velocity + seed.system.velocities[j]);
        }
    }
    Ephemeris::new(
        &system,
        EphemerisOptions {
            step_seconds: step,
            chunk_steps: 128,
        },
    )
}

#[test]
fn coupled_hierarchy_matches_an_ungrouped_n_body_reference() {
    let seeds = compact_seeds(SplitPosition::ORIGIN);
    let mut reference = flat_reference(&seeds, SplitPosition::ORIGIN, 10.0);
    let mut world = CoupledWorld::new(seeds, 10.0, 8192);
    world.extend_to(2000.0, 100_000);
    reference.extend_to(2000.0);
    let mut worst: f64 = 0.0;
    for t in [0.0, 123.25, 999.9, 2000.0] {
        let state = world.at(t);
        for i in 0..world.bodies.len() {
            let p = world
                .body_position(i, &state)
                .relative(&SplitPosition::ORIGIN);
            worst = worst.max((p - reference.body_position(i, t)).length());
        }
    }
    eprintln!("direct N-body maximum error {worst:.2e} m");
    assert!(worst < 2e-4, "full N-body position error {worst} m");
}

#[test]
fn external_tides_act_and_the_barycentre_counts_each_mass_once() {
    let seeds = compact_seeds(SplitPosition::ORIGIN);
    let mut isolated = Ephemeris::new(
        &seeds[0].system,
        EphemerisOptions {
            step_seconds: 10.0,
            chunk_steps: 128,
        },
    );
    let bodies = seeds[0].system.bodies.clone();
    let mut world = CoupledWorld::new(seeds, 10.0, 8192);
    world.extend_to(2000.0, 100_000);
    isolated.extend_to(2000.0);
    let g = &world.at(2000.0)[0];
    let tide = (g.body_position(1) - isolated.body_position(1, 2000.0)).length();
    eprintln!("tide displacement {tide:.3} m");
    assert!(
        tide > 0.01,
        "external tides must be observable in this compact fixture"
    );
    let mass: f64 = bodies.iter().map(|b| b.mass_kg).sum();
    let centre = bodies
        .iter()
        .enumerate()
        .fold(DVec3::ZERO, |c, (i, b)| c + g.body_position(i) * b.mass_kg);
    assert!(centre.length() / mass < 1e-6);
}

#[test]
fn galactic_translation_changes_no_local_dynamics() {
    let mut a = CoupledWorld::new(compact_seeds(SplitPosition::ORIGIN), 10.0, 8192);
    let mut b = CoupledWorld::new(compact_seeds(huge()), 10.0, 8192);
    a.extend_to(1000.0, 100_000);
    b.extend_to(1000.0, 100_000);
    for (g, other) in a.at(1000.0).iter().zip(&b.at(1000.0)) {
        assert_eq!(g.positions, other.positions);
        assert_eq!(g.velocities, other.velocities);
        assert_eq!(g.velocity, other.velocity);
        assert!(
            other
                .origin
                .difference(&huge())
                .relative(&g.origin)
                .length()
                < 5e-7
        );
    }
}

#[test]
fn frame_round_trips_across_light_years_keep_centimetres() {
    let mut world = wide_world(default_galaxy());
    world.extend_to(100.0, 100_000);
    let state = FramedState {
        frame: world.system_frame("Aster"),
        position: SplitPosition::at(DVec3::new(1.125, 0.01, -2.25)),
        velocity: DVec3::new(1.5, -0.25, 0.125),
    };
    let before = absolute(&world, 50.0, &state);
    let mut next = state;
    for _ in 0..100 {
        next = reframe(
            &world,
            50.0,
            &reframe(&world, 50.0, &next, world.system_frame("Beryl")),
            world.system_frame("Aster"),
        );
    }
    let after = absolute(&world, 50.0, &next);
    assert!(before.0.relative(&after.0).length() < 2e-6);
    assert!((before.1 - after.1).length() < 1e-9);
}

#[test]
fn frame_changes_change_neither_gravity_nor_axes() {
    let mut world = CoupledWorld::new(compact_seeds(huge()), 10.0, 8192);
    world.extend_to(100.0, 100_000);
    let a = FramedState {
        frame: world.system_frame("A"),
        position: SplitPosition::at(DVec3::new(5e8, 2e9, 0.0)),
        velocity: DVec3::new(20.0, 0.0, 0.0),
    };
    let b = reframe(&world, 50.0, &a, world.system_frame("B"));
    let (pa, pb) = (absolute(&world, 50.0, &a).0, absolute(&world, 50.0, &b).0);
    assert!(pa.relative(&pb).length() < 2e-6);
    assert!((world.gravity_at(50.0, &pa) - world.gravity_at(50.0, &pb)).length() < 1e-15);
}

/// The world's gravity is orbit's one law.
#[test]
fn gravity_is_the_shared_law() {
    let mut world = CoupledWorld::new(compact_seeds(huge()), 10.0, 8192);
    world.extend_to(100.0, 100_000);
    let state = world.at(50.0);
    for offset in [
        DVec3::new(8e8, 1e9, 2e8),
        DVec3::new(-3e9, 4e8, -1e8),
        DVec3::new(2e7, -6e6, 1e6),
    ] {
        let p = state[0].origin.translate(offset);
        let (mut law, mut scale) = (DVec3::ZERO, 0.0);
        for (i, body) in world.bodies.iter().enumerate() {
            let r = -world.body_position(i, &state).relative(&p);
            let a = void_orbit::gravity::body_pull(body, r);
            law += a;
            scale += a.length();
        }
        // Rounding only: a few ulps of each body's pull, which partly cancel in the sum.
        let g = world.gravity_in(&state, &p);
        assert!(
            (g - law).length() <= 4.0 * f64::EPSILON * scale,
            "{g} vs {law}"
        );
    }
}

#[test]
fn flight_from_a_to_b_matches_direct_n_body_through_the_hand_off() {
    let seeds = compact_seeds(SplitPosition::ORIGIN);
    let mut reference = flat_reference(&seeds, SplitPosition::ORIGIN, 10.0);
    let mut world = CoupledWorld::new(seeds, 10.0, 8192);
    let state = FramedState {
        frame: world.system_frame("A"),
        position: SplitPosition::at(DVec3::new(8e8, 2e9, 0.0)),
        velocity: DVec3::new(500_000.0, 0.0, 0.0),
    };
    let (initial_position, initial_velocity) = absolute(&world, 0.0, &state);
    let mut flight = Traveller::new(&world, 0.0, state, 100.0);
    let mut propagator = VesselPropagator::new(
        &reference,
        Tolerances {
            position_meters: 1e-5,
            velocity_meters_per_second: 1e-8,
        },
    );
    let mut run = PropagationRun::new(VesselState {
        time: 0.0,
        position: initial_position.vector(),
        velocity: initial_velocity,
        mass_kg: 1.0,
    });
    assert!(flight.advance_to(&mut world, 9000.0, 10_000));
    assert_eq!(
        propagator.advance(&mut reference, &mut run, 9000.0, 100_000, None, None),
        AdvanceOutcome::Reached
    );
    assert!(
        flight
            .events
            .iter()
            .any(|e| e.from == world.system_frame("A") && e.to == world.system_frame("B"))
    );
    let error = (flight.position(&world).vector() - run.state().position).length();
    eprintln!("transfer position error {error:.2e} m");
    assert!(error < 0.02, "transfer error {error}");
    assert!(
        flight
            .events
            .iter()
            .all(|e| e.position_jump < 2e-6 && e.velocity_jump < 1e-8)
    );
}

#[test]
fn budgets_keep_progress_and_pruning_invalidates_old_queries() {
    let mut world = CoupledWorld::new(compact_seeds(SplitPosition::ORIGIN), 10.0, 8);
    assert!(!world.extend_to(100.0, 1));
    assert_eq!(world.time(), 10.0);
    assert!(world.extend_to(100.0, 20));
    assert!(world.sample_count() <= 8);
    assert!(panics(|| {
        world.at(0.0);
    }));
    assert!(panics(|| {
        world.system_index("missing");
    }));
    let mut wide = wide_world(default_galaxy());
    let mut flight = transfer(&wide, 0.02);
    assert!(!flight.advance_to(&mut wide, 10_000.0, 0));
    assert_eq!(flight.time, 0.0);
    assert!(!flight.advance_to(&mut wide, 10_000.0, 1));
    assert!(flight.time > 0.0 && flight.time < 10_000.0);
    assert!(panics(|| {
        flight.advance_to(&mut wide, -1.0, 200);
    }));
}

#[test]
fn the_one_day_step_converges_against_a_half_day_over_ten_years() {
    let mut a = wide_world(default_galaxy());
    let mut b = CoupledWorld::new(wide_seeds(default_galaxy()), 43_200.0, 16_384);
    a.extend_to(10.0 * YEAR, 100_000);
    b.extend_to(10.0 * YEAR, 100_000);
    let mut worst: f64 = 0.0;
    for t in [YEAR, 5.0 * YEAR, 10.0 * YEAR] {
        for (g, q) in a.at(t).iter().zip(&b.at(t)) {
            for j in (0..g.positions.len()).step_by(3) {
                let d = DVec3::from_slice(&g.positions[j..j + 3])
                    - DVec3::from_slice(&q.positions[j..j + 3]);
                worst = worst.max(d.length());
            }
        }
    }
    eprintln!("one-day vs half-day maximum local position difference {worst:.3} m over ten years");
    assert!(
        worst < 1.0,
        "ten-year local body step convergence {worst} m"
    );
}

#[test]
fn the_four_light_year_flight_reaches_beryl_continuously() {
    let mut world = wide_world(default_galaxy());
    let mut flight = transfer(&world, 0.02);
    assert!(flight.advance_to(&mut world, 210.0 * YEAR, 100_000));
    assert_eq!(flight.terminal, None);
    assert_eq!(flight.state.frame, world.system_frame("Beryl"));
    let handoff = flight
        .events
        .iter()
        .find(|e| e.from == world.system_frame("Aster") && e.to == world.system_frame("Beryl"))
        .expect("hand-off");
    assert!(handoff.position_jump < 2e-6 && handoff.velocity_jump < 1e-8);
    assert!(absolute(&world, flight.time, &flight.state).1.x.is_finite());
    assert!(world.sample_count() <= world.sample_limit);
    eprintln!(
        "{} celestial steps, {} probe steps; hand-off {:.3} yr",
        world.steps,
        flight.steps,
        handoff.time / YEAR
    );
}

#[test]
fn a_small_system_builds_as_described() {
    // Two bodies, barycentric, the planet 2e8 m out.
    let s = small_system("A", 1e25);
    assert_eq!(s.bodies.len(), 2);
    let r = (s.positions[1] - s.positions[0]).length();
    assert!(r > 1.7e8 && r < 2.3e8, "{r}");
}
