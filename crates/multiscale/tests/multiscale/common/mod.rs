//! A compact fixture: three small two-body systems a few million kilometres apart.
#![allow(dead_code)]

use glam::DVec3;
use void_frames::SplitPosition;
use void_multiscale::SystemSeed;
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

pub fn huge() -> SplitPosition {
    SplitPosition::new(
        DVec3::new(123.125, -9.25, 7.0),
        [10_i128.pow(24), -(10_i128.pow(23)), 10_i128.pow(22)],
    )
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
