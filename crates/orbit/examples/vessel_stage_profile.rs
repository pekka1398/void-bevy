//! Compare stage-local ephemeris reuse; all accepted samples must match the uncached path.
use glam::DVec3;
use std::time::Instant;
use void_orbit::*;
fn main() {
    let system = build_system(&expanded_sol());
    let mut ep = Ephemeris::new(
        &system,
        EphemerisOptions {
            step_seconds: suggested_step_seconds(&system.bodies, 256.0),
            chunk_steps: 1024,
        },
    );
    ep.extend_to(86400.0);
    let body = system
        .bodies
        .iter()
        .position(|b| b.id == "aurelia")
        .unwrap();
    let radius = system.bodies[body].radius_meters + 400000.0;
    let state = VesselState {
        time: 0.0,
        position: system.positions[body] + DVec3::X * radius,
        velocity: system.velocities[body] + DVec3::Y * (system.bodies[body].gm / radius).sqrt(),
        mass_kg: 1000.0,
    };
    let mut oracle = None;
    for cache in [false, true, true, false] {
        let mut times = vec![];
        let mut steps = (0, 0);
        for i in 0..22 {
            let mut prop = VesselPropagator::new(
                &ep,
                Tolerances {
                    position_meters: 0.02,
                    velocity_meters_per_second: 0.001,
                },
            );
            prop.set_stage_cache(cache);
            let mut run = PropagationRun::new(state);
            let mut trajectory = Trajectory::new();
            let start = Instant::now();
            let outcome = prop.advance(
                &mut ep,
                &mut run,
                86400.0,
                100000,
                Some(&mut trajectory),
                None,
            );
            let elapsed = start.elapsed().as_secs_f64();
            assert!(matches!(outcome, AdvanceOutcome::Reached));
            if i > 0 {
                times.push(elapsed);
            }
            let samples = serde_json::to_string(&trajectory).unwrap();
            if let Some(oracle) = &oracle {
                assert_eq!(&samples, oracle);
            } else {
                oracle = Some(samples);
            }
            steps = (prop.accepted_steps, prop.rejected_steps);
        }
        times.sort_by(f64::total_cmp);
        println!(
            "{}",
            serde_json::json!({"stage_cache":cache,"median_seconds":times[times.len()/2],"seconds_sorted":times,"accepted":steps.0,"rejected":steps.1,"all_sample_bits_equal":true})
        );
    }
}
