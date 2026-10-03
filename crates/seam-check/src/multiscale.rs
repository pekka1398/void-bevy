//! The lab check's compact fixture: three small two-body systems a few million kilometres apart.

use glam::DVec3;
use void_multiscale::{SplitPosition, SystemSeed};
use void_orbit::{SystemSpec, build_system};

pub fn small_system(id: &str, mass: f64) -> void_orbit::BuiltSystem {
    let rotation = serde_json::json!({
        "periodSeconds": 86400.0, "obliquityRadians": 0.0, "poleLongitudeRadians": 0.0, "angleAtEpochRadians": 0.0,
    });
    let spec = serde_json::json!({
        "name": id,
        "root": {
            "id": "star", "name": "star", "massKg": mass, "radiusMeters": 1e6, "color": "#fff", "rotation": rotation,
            "children": [{
                "id": "planet", "name": "planet", "massKg": mass * 1e-4, "radiusMeters": 1e5, "color": "#aaf",
                "rotation": rotation, "orbitPlane": "ecliptic",
                "orbit": {
                    "semiMajorAxisMeters": 2e8, "eccentricity": 0.1, "inclinationRadians": 0.2,
                    "longitudeOfAscendingNodeRadians": 0.3, "argumentOfPeriapsisRadians": 0.4, "meanAnomalyRadians": 0.5,
                },
                "children": [],
            }],
        },
    });
    build_system(&SystemSpec::from_json(&spec.to_string()))
}

pub fn compact_seeds(anchor: SplitPosition) -> Vec<SystemSeed> {
    let at = |offset: DVec3| anchor.compose(&SplitPosition::at(offset));
    vec![
        SystemSeed {
            id: "A".into(),
            system: small_system("A", 1e25),
            origin: at(DVec3::ZERO),
            velocity: DVec3::new(20.0, 30.0, 0.0),
        },
        SystemSeed {
            id: "B".into(),
            system: small_system("B", 8e24),
            origin: at(DVec3::new(4e9, 8e8, 0.0)),
            velocity: DVec3::new(-30.0, 10.0, 5.0),
        },
        SystemSeed {
            id: "C".into(),
            system: small_system("C", 1.2e25),
            origin: at(DVec3::new(-5e9, 3e9, 1e9)),
            velocity: DVec3::new(5.0, -10.0, 0.0),
        },
    ]
}

use crate::{Input, maximum};
use serde_json::{Value, json};
use void_multiscale::{CoupledWorld, FramedState, Traveller, absolute, reframe, wide_world};
use void_orbit::{
    AdvanceOutcome, BuiltSystem, CelestialBody, Ephemeris, EphemerisOptions, PropagationRun,
    Tolerances, VesselPropagator, VesselState,
};
fn flat(seeds: &[SystemSeed], anchor: SplitPosition) -> Ephemeris {
    let mut system = BuiltSystem {
        name: "seam independent direct reference".into(),
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
            step_seconds: 10.0,
            chunk_steps: 128,
        },
    )
}
pub fn check(input: &Input) -> Value {
    match input {
        Input::Frame {
            position,
            velocity,
            time,
            cells,
        } => {
            let mut world = wide_world(SplitPosition::new(DVec3::new(123.125, -9.25, 7.0), *cells));
            world.extend_to(100.0, 100_000);
            let start = FramedState {
                frame: "Aster".into(),
                position: SplitPosition::at(*position),
                velocity: *velocity,
            };
            let before = absolute(&world, *time, &start);
            let mut state = start;
            let (mut p, mut v) = (0.0_f64, 0.0_f64);
            for _ in 0..100 {
                state = reframe(
                    &world,
                    *time,
                    &reframe(&world, *time, &state, "Beryl"),
                    "Aster",
                );
                let now = absolute(&world, *time, &state);
                p = maximum(p, before.0.relative(&now.0).length());
                v = maximum(v, (before.1 - now.1).length());
            }
            assert!(p < 2e-6, "multiscale roundtrip position error {p}");
            assert!(v < 1e-9, "multiscale roundtrip velocity error {v}");
            json!({"position_m":p,"velocity_m_s":v,"roundtrips":100})
        }
        Input::Transfer {
            position,
            velocity,
            cells,
        } => {
            let anchor = SplitPosition::new(DVec3::new(123.125, -9.25, 7.0), *cells);
            let seeds = compact_seeds(anchor);
            let mut reference = flat(&seeds, anchor);
            let mut world = CoupledWorld::new(seeds, 10.0, 8192);
            let state = FramedState {
                frame: "A".into(),
                position: SplitPosition::at(*position),
                velocity: *velocity,
            };
            let (p, v) = absolute(&world, 0.0, &state);
            let mut flight = Traveller::new(&world, 0.0, state, 100.0);
            let mut run = PropagationRun::new(VesselState {
                time: 0.0,
                position: p.relative(&anchor),
                velocity: v,
                mass_kg: 1.0,
            });
            let mut prop = VesselPropagator::new(
                &reference,
                Tolerances {
                    position_meters: 1e-5,
                    velocity_meters_per_second: 1e-8,
                },
            );
            let mut error = 0.0_f64;
            for i in 1..=9 {
                let end = i as f64 * 1000.0;
                assert!(flight.advance_to(&mut world, end, 10_000));
                assert_eq!(
                    prop.advance(&mut reference, &mut run, end, 100_000, None, None),
                    AdvanceOutcome::Reached
                );
                error = maximum(
                    error,
                    (flight.position(&world).relative(&anchor) - run.state().position).length(),
                );
            }
            assert!(
                flight.events.iter().any(|e| e.from == "A" && e.to == "B"),
                "transfer did not cross the frame seam"
            );
            assert!(
                error < 0.02,
                "multiscale direct reference position error {error}"
            );
            assert!(
                flight
                    .events
                    .iter()
                    .all(|e| e.position_jump < 2e-6 && e.velocity_jump < 1e-8),
                "frame handoff discontinuity {:?}",
                flight.events
            );
            json!({"position_m":error,"frame_changes":flight.events.len()})
        }
        _ => unreachable!(),
    }
}
