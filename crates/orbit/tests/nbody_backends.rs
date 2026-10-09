use glam::DVec3;
use void_orbit::{
    AccelerationBackend, Ephemeris, EphemerisOptions, SystemSpec, build_system,
    suggested_step_seconds,
};
#[test]
fn backends_preserve_full_states_and_history_across_vector_tails() {
    let mut backends = vec![AccelerationBackend::ScalarLocal];
    #[cfg(target_arch = "x86_64")]
    if std::is_x86_feature_detected!("avx2") {
        backends.push(AccelerationBackend::Avx2);
    }
    let full = build_system(&SystemSpec::from_json(include_str!(
        "../systems/sol-expanded.json"
    )));
    for backend in backends {
        for n in [1, 2, 3, 4, 5, 7, 8, 15, 57, 58] {
            let mut system = full.clone();
            system.bodies.truncate(n);
            system.positions.truncate(n);
            system.velocities.truncate(n);
            let step = suggested_step_seconds(&full.bodies, 256.0);
            let options = EphemerisOptions {
                step_seconds: step,
                chunk_steps: 16,
            };
            let mut scalar = Ephemeris::new(&system, options);
            scalar.set_acceleration_backend(AccelerationBackend::ScalarReference);
            let mut candidate = Ephemeris::new(&system, options);
            candidate.set_acceleration_backend(backend);
            candidate.enable_profiling();
            let mut a = vec![DVec3::ZERO; n];
            let mut b = a.clone();
            let mut av = a.clone();
            let mut bv = a.clone();
            let count = if n == 58 { 822 } else { 35 };
            for k in 1..=count {
                let t = k as f64 * step;
                scalar.extend_to(t);
                candidate.extend_to(t);
                for query in [t, t - step * 0.37] {
                    scalar.states_at(query, &mut a, Some(&mut av));
                    candidate.states_at(query, &mut b, Some(&mut bv));
                    for (a, b) in a.iter().chain(&av).zip(b.iter().chain(&bv)) {
                        assert_eq!(
                            a.to_array().map(f64::to_bits),
                            b.to_array().map(f64::to_bits),
                            "{backend:?} n={n}, t={query}"
                        );
                    }
                }
                assert_eq!(
                    scalar.current_energy().to_bits(),
                    candidate.current_energy().to_bits()
                );
                assert_eq!(
                    scalar
                        .current_angular_momentum()
                        .to_array()
                        .map(f64::to_bits),
                    candidate
                        .current_angular_momentum()
                        .to_array()
                        .map(f64::to_bits)
                );
                if k % 17 == 0 {
                    scalar.forget_before(t - step * 2.0);
                    candidate.forget_before(t - step * 2.0);
                }
                assert_eq!(scalar.retained_bytes(), candidate.retained_bytes());
                assert_eq!(scalar.start_time(), candidate.start_time());
            }
            let p = candidate.profile().unwrap();
            assert!(p.steps >= count);
            assert_eq!(p.acceleration_calls, p.steps * 15);
        }
    }
}
