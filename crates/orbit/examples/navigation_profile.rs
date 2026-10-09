//! Actual expanded-system solver including finite-thrust candidate validation.
use glam::DVec3;
use std::time::Instant;
use void_orbit::*;
fn main() {
    let backend = match std::env::args().nth(1).as_deref() {
        Some("scalar") => AccelerationBackend::ScalarReference,
        Some("spin2") => AccelerationBackend::SpinWorkers(2),
        Some("spin4") => AccelerationBackend::SpinWorkers(4),
        Some("spin8") => AccelerationBackend::SpinWorkers(8),
        Some("rows") => AccelerationBackend::Avx2Rows,
        Some("simd") => AccelerationBackend::Avx2,
        Some("local") => AccelerationBackend::ScalarLocal,
        Some("auto") | None => AccelerationBackend::Auto,
        _ => panic!("unknown navigation backend"),
    };
    let system = build_system(&SystemSpec::from_json(include_str!(
        "../systems/sol-expanded.json"
    )));
    let step = suggested_step_seconds(&system.bodies, 256.0);
    let mut ep = Ephemeris::new(
        &system,
        EphemerisOptions {
            step_seconds: step,
            chunk_steps: 1024,
        },
    );
    ep.set_acceleration_backend(backend);
    ep.extend_to(step);
    ep.enable_profiling();
    let reference = system
        .bodies
        .iter()
        .position(|b| b.id == "aurelia")
        .unwrap();
    let target = system.bodies.iter().position(|b| b.id == "selene").unwrap();
    let radius = system.bodies[reference].radius_meters + 400000.0;
    let anchor = PropagationRun::new(VesselState {
        time: 0.0,
        position: system.positions[reference] + DVec3::X * radius,
        velocity: system.velocities[reference]
            + DVec3::Y * (system.bodies[reference].gm / radius).sqrt(),
        mass_kg: 1000.0,
    });
    let request = NavigationRequest {
        operation: NavigationOperation::Departure,
        target_body: target,
        reference_body: reference,
        earliest_departure: 30.0,
        latest_departure: 30.0 + 30.0 * 86400.0,
        min_flight_seconds: 60.0,
        max_flight_seconds: 7.0 * 86400.0,
        periapsis_altitude_m: 100000.0,
    };
    let prediction_mode = std::env::args().skip(2).any(|arg| arg == "--prediction");
    let snapshot_start = Instant::now();
    let mut prediction = prediction_mode.then(|| {
        ep.prediction_snapshot(PredictionBudget::default(), CancellationToken::new())
            .expect("prediction snapshot budget")
            .into_source()
    });
    let snapshot_ms = snapshot_start.elapsed().as_secs_f64() * 1000.0;
    let source: &mut dyn EphemerisSource = match &mut prediction {
        Some(source) => &mut **source,
        None => &mut ep,
    };
    let start = Instant::now();
    let result = solve_navigation(
        source,
        &anchor,
        PlanEngine {
            thrust_newtons: 1e6,
            exhaust_velocity: 10000.0,
            dry_mass_kg: 100.0,
        },
        Tolerances {
            position_meters: 0.02,
            velocity_meters_per_second: 0.001,
        },
        &request,
    );
    let seconds = start.elapsed().as_secs_f64();
    let retained_bytes = source.retained_bytes();
    let end_time = source.end_time();
    let usage = source.prediction_context().map(|context| {
        let used = context.usage();
        serde_json::json!({"reserved_bytes":used.reserved_bytes,"ephemeris_steps":used.ephemeris_steps,"vessel_trials":used.vessel_trials})
    });
    println!(
        "{}",
        serde_json::json!({"backend":backend,"seconds":seconds,"prediction":prediction_mode,"snapshot_ms":snapshot_ms,"prediction_usage":usage,"result":format!("{result:?}"),"retained_bytes":retained_bytes,"end_time":end_time,"ephemeris_profile":ep.profile()})
    );
}
