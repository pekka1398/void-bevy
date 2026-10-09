//! Investigation only: declared interstellar starting state, not a completed transfer.
use glam::{DQuat, DVec3};
use std::time::Instant;
use void_fleet_flight::world::stellar_neighborhood;
use void_frames::{SplitPosition, SystemId};
use void_vessels::{Fleet, FleetOptions};

fn main() {
    let seconds: f64 = std::env::args()
        .nth(1)
        .unwrap_or("3600".into())
        .parse()
        .unwrap();
    let mut baseline = None;
    let mode = std::env::args().nth(2);
    let chunks: &[f64] = if mode.as_deref() == Some("default") {
        &[1000.0]
    } else {
        &[1.0, 10.0, 100.0, 1000.0]
    };
    for &chunk in chunks {
        let planet = void_landing::aurelia();
        let craft = void_assembly::demo_craft();
        let built = stellar_neighborhood(&planet).build();
        let ground_bodies: Vec<_> = built.grounds.iter().map(|g| g.body_index).collect();
        let world = built.coupled_world.unwrap();
        if mode.as_deref() == Some("world") {
            let mut world = world.borrow_mut();
            let before = world.steps;
            let start = Instant::now();
            let reached = world.extend_to(seconds, 100_000);
            println!(
                "celestial_only target={seconds}s reached={reached} wall={:.6}s step={} steps={}",
                start.elapsed().as_secs_f64(),
                world.step_seconds,
                world.steps - before
            );
            return;
        }
        let mut fleet = Fleet::new(
            built.ephemeris,
            built.environment,
            0.0,
            built.grounds,
            FleetOptions::default(),
        );
        let id = fleet.launch_at_split(
            &craft,
            SystemId(0),
            SplitPosition::at(DVec3::new(2.0 * void_multiscale::LIGHT_YEAR, 0.0, 0.0)),
            DVec3::new(1_000_000.0, 0.0, 0.0),
            DQuat::IDENTITY,
            DVec3::ZERO,
        );
        if chunk == 1.0 {
            for body in ground_bodies {
                let state = fleet.body_fixed_state(&id, body);
                let b = &fleet.ephemeris.bodies()[body];
                let gap = state.position.length() - b.radius_meters;
                let speed = state.velocity.length();
                let acceleration = 1.2 * b.gm / state.position.length_squared();
                let root = (speed * speed + 2.0 * acceleration * gap).sqrt();
                let naive = (-speed + root) / acceleration;
                let stable = 2.0 * gap / (speed + root);
                println!(
                    "body={} approximate_band_seconds={naive:e} stable_seconds={stable:e}",
                    b.id
                );
            }
        }
        // Explicit caps allow comparison with a short-chunk reference. The
        // "default" mode instead measures the unmodified production policy.
        if mode.as_deref() != Some("default") {
            fleet.options.flight_chunk_seconds = chunk;
            fleet.options.rails_chunk_seconds = chunk;
            fleet.options.distant_coast_chunk_seconds = chunk;
        }
        let start_steps = world.borrow().steps;
        let start_time = fleet.time();
        let start = Instant::now();
        let selected_chunk = fleet.rails_coast_chunk_seconds();
        let reached = fleet.advance_on_rails(seconds);
        let elapsed = start.elapsed().as_secs_f64();
        let state = fleet.precise_snapshot(&id);
        let reference = baseline.get_or_insert((state.position, state.local.velocity));
        let position_error = state.position.relative(&reference.0).length();
        let velocity_error = (state.local.velocity - reference.1).length();
        println!(
            "limit={chunk}s selected_chunk={selected_chunk}s target={seconds}s reached={reached} simulated={} wall={elapsed:.6}s celestial_step={} celestial_steps={} baseline_dp={position_error:e}m baseline_dv={velocity_error:e}m/s",
            fleet.time() - start_time,
            world.borrow().step_seconds,
            world.borrow().steps - start_steps
        );
    }
}
