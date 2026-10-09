use glam::DVec3;
use void_orbit::*;
#[test]
fn cache_preserves_coast_force_and_relative_thrust_across_advance_boundaries() {
    let system = build_system(&expanded_sol());
    let body = system
        .bodies
        .iter()
        .position(|b| b.id == "aurelia")
        .unwrap();
    let radius = system.bodies[body].radius_meters + 400000.0;
    let controls = [
        None,
        Some(Control::Force(ForceControl {
            force: DVec3::Y,
            mass_flow_kg_per_second: 0.001,
            minimum_mass_kg: 100.0,
        })),
        Some(Control::Thrust(ThrustControl {
            thrust_newtons: 1.0,
            exhaust_velocity: 10000.0,
            minimum_mass_kg: 100.0,
            attitude: AttitudeLaw::Inertial {
                direction: DVec3::Y,
            },
        })),
        Some(Control::Thrust(ThrustControl {
            thrust_newtons: 1.0,
            exhaust_velocity: 10000.0,
            minimum_mass_kg: 100.0,
            attitude: AttitudeLaw::Frenet {
                reference_body: body,
                tangent: 1.0,
                normal: 0.0,
                radial: 0.0,
            },
        })),
        Some(Control::Thrust(ThrustControl {
            thrust_newtons: 1.0,
            exhaust_velocity: 10000.0,
            minimum_mass_kg: 100.0,
            attitude: AttitudeLaw::Surface {
                reference_body: body,
                up: 0.0,
                prograde: 1.0,
            },
        })),
    ];
    for control in controls {
        let mut oracle = None;
        for cache in [false, true] {
            let mut ep = Ephemeris::new(
                &system,
                EphemerisOptions {
                    step_seconds: suggested_step_seconds(&system.bodies, 256.0),
                    chunk_steps: 16,
                },
            );
            let mut prop = VesselPropagator::new(
                &ep,
                Tolerances {
                    position_meters: 0.02,
                    velocity_meters_per_second: 0.001,
                },
            );
            prop.set_stage_cache(cache);
            let mut run = PropagationRun::new(VesselState {
                time: 0.0,
                position: system.positions[body] + DVec3::X * radius,
                velocity: system.velocities[body]
                    + DVec3::Y * (system.bodies[body].gm / radius).sqrt(),
                mass_kg: 1000.0,
            });
            let mut trajectory = Trajectory::new();
            for (end, c) in [(300.0, control), (600.0, None), (900.0, control)] {
                assert!(matches!(
                    prop.advance(&mut ep, &mut run, end, 10000, Some(&mut trajectory), c),
                    AdvanceOutcome::Reached
                ));
            }
            let result = (
                serde_json::to_string(&trajectory).unwrap(),
                serde_json::to_string(&run.state()).unwrap(),
                prop.accepted_steps,
                prop.rejected_steps,
            );
            if let Some(oracle) = &oracle {
                assert_eq!(&result, oracle);
            } else {
                oracle = Some(result);
            }
        }
    }
}
