//! Parallel independent trajectory throughput, NOT parallel steps of one trajectory.
use std::{
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Instant,
};
use void_orbit::{
    AccelerationBackend, Ephemeris, EphemerisOptions, SystemSpec, build_system,
    suggested_step_seconds,
};
fn main() {
    let system = build_system(&SystemSpec::from_json(include_str!(
        "../systems/sol-expanded.json"
    )));
    let step = suggested_step_seconds(&system.bodies, 256.0);
    let mut reference = None;
    for workers in [1, 2, 4, 8, 16] {
        let mut times = vec![];
        for _ in 0..3 {
            let next = AtomicUsize::new(0);
            let results = Mutex::new(vec![0u64; 32]);
            let start = Instant::now();
            std::thread::scope(|scope| {
                for _ in 0..workers {
                    let (next, results, system) = (&next, &results, &system);
                    scope.spawn(move || {
                        loop {
                            let task = next.fetch_add(1, Ordering::Relaxed);
                            if task >= 32 {
                                break;
                            }
                            let mut e = Ephemeris::new(
                                system,
                                EphemerisOptions {
                                    step_seconds: step,
                                    chunk_steps: 1024,
                                },
                            );
                            e.set_acceleration_backend(AccelerationBackend::Avx2);
                            e.extend_to(86400.0);
                            results.lock().unwrap()[task] = e.current_energy().to_bits();
                        }
                    });
                }
            });
            times.push(start.elapsed().as_secs_f64());
            let results = results.into_inner().unwrap();
            if let Some(reference) = &reference {
                assert_eq!(&results, reference);
            } else {
                reference = Some(results);
            }
        }
        times.sort_by(f64::total_cmp);
        println!(
            "{}",
            serde_json::json!({"workers":workers,"independent_jobs":32,"seconds_sorted":times,"median_seconds":times[1],"energy_bits_equal":true})
        );
    }
}
