//! Release-mode whole-ephemeris benchmark. No renderer or changed integration step.
use glam::DVec3;
use std::{hint::black_box, time::Instant};
use void_orbit::{
    AccelerationBackend, Ephemeris, EphemerisOptions, SystemSpec, build_system,
    suggested_step_seconds,
};
use void_orbit::{PropagationRun, Tolerances, VesselPropagator, VesselState};
fn main() {
    let repeats: usize = std::env::args()
        .nth(1)
        .unwrap_or("7".into())
        .parse()
        .unwrap();
    let days: f64 = std::env::args()
        .nth(3)
        .unwrap_or("1".into())
        .parse()
        .unwrap();
    assert!(repeats > 0 && days > 0.0 && days <= 30.0);
    let profile = std::env::args().nth(4).as_deref() == Some("profile");
    let backend = match std::env::args().nth(2).as_deref() {
        Some("simd") => AccelerationBackend::Avx2,
        Some("local") => AccelerationBackend::ScalarLocal,
        Some("scalar") | None => AccelerationBackend::ScalarReference,
        _ => panic!("kernel must be scalar, local, or simd"),
    };
    for (name, json) in [
        ("sol15", include_str!("../systems/sol.json")),
        ("sol58", include_str!("../systems/sol-expanded.json")),
    ] {
        let system = build_system(&SystemSpec::from_json(json));
        let step = suggested_step_seconds(&system.bodies, 256.0);
        let steps = (days * 86400.0 / step).ceil() as usize;
        let mut measurements = vec![];
        let mut result = serde_json::Value::Null;
        for run in 0..=repeats {
            let mut e = Ephemeris::new(
                &system,
                EphemerisOptions {
                    step_seconds: step,
                    chunk_steps: 1024,
                },
            );
            e.set_acceleration_backend(backend);
            if profile {
                e.enable_profiling();
            }
            let initial_energy = e.current_energy();
            let start = Instant::now();
            e.extend_to(steps as f64 * step);
            let elapsed = start.elapsed().as_secs_f64();
            let mut positions = vec![DVec3::ZERO; system.bodies.len()];
            let query_start = Instant::now();
            for i in 0..10000 {
                e.positions_at(
                    (i as f64 + 0.5) / 10000.0 * e.end_time(),
                    black_box(&mut positions),
                );
            }
            let query = query_start.elapsed().as_secs_f64();
            let body = system
                .bodies
                .iter()
                .position(|b| b.id == "aurelia")
                .unwrap();
            let radius = system.bodies[body].radius_meters + 400000.0;
            let mut velocities = vec![DVec3::ZERO; system.bodies.len()];
            e.states_at(0.0, &mut positions, Some(&mut velocities));
            let ship = positions[body] + DVec3::X * radius;
            let mut prop = VesselPropagator::new(
                &e,
                Tolerances {
                    position_meters: 0.02,
                    velocity_meters_per_second: 0.001,
                },
            );
            let gravity_start = Instant::now();
            for i in 0..10000 {
                black_box(prop.gravity_at(&e, (i as f64 + 0.5) / 10000.0 * e.end_time(), ship));
            }
            let gravity_seconds = gravity_start.elapsed().as_secs_f64();
            let mut coast = PropagationRun::new(VesselState {
                time: 0.0,
                position: ship,
                velocity: velocities[body] + DVec3::Y * (system.bodies[body].gm / radius).sqrt(),
                mass_kg: 1000.0,
            });
            let coast_start = Instant::now();
            let coast_outcome = prop.advance(&mut e, &mut coast, 3600.0, 100000, None, None);
            let coast_seconds = coast_start.elapsed().as_secs_f64();
            // Restore the same diagnostic sample used for cross-kernel equality.
            e.positions_at((9999.5 / 10000.0) * e.end_time(), &mut positions);
            if run > 0 {
                measurements.push(elapsed);
            }
            result = serde_json::json!({"system":name,"gravity_10000_seconds":gravity_seconds,"coast_hour_seconds":coast_seconds,"coast_outcome":format!("{coast_outcome:?}"),"coast_accepted":prop.accepted_steps,"coast_rejected":prop.rejected_steps,"profile":e.profile(),"days":days,"backend":backend,"bodies":system.bodies.len(),"step_seconds":step,"steps":steps,"pair_evaluations":steps*15*system.bodies.len()*(system.bodies.len()-1)/2,"retained_bytes":e.retained_bytes(),"relative_energy_drift":(e.current_energy()-initial_energy)/initial_energy,"query_10000_seconds":query,"end_positions":positions.iter().map(|p| p.to_array().map(f64::to_bits)).collect::<Vec<_>>()});
        }
        measurements.sort_by(f64::total_cmp);
        result["extend_seconds_sorted"] = serde_json::json!(measurements);
        result["extend_median_seconds"] = serde_json::json!(measurements[measurements.len() / 2]);
        println!("{}", result);
    }
}
