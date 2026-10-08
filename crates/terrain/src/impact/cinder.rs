//! Cinder art direction, distinct from reusable impact mechanics. Locations are authored fiction.
use super::{Basin, ImpactOptions, RayedImpact};
use glam::DVec3;
impl ImpactOptions {
    pub fn cinder(radius_meters: f64) -> Self {
        let basin = |direction: [f64; 3], radius, depth, rim, fill| Basin {
            direction: DVec3::from_array(direction).normalize().to_array(),
            radius_meters: radius,
            depth_meters: depth,
            rim_meters: rim,
            fill,
        };
        Self {
            name: "Cinder impact provinces".into(),
            radius_meters,
            max_height_meters: 16_000.0,
            datum_meters: 8_000.0,
            seed: 31,
            basins: vec![
                basin([0.84, 0.37, 0.40], 720_000.0, 2100.0, 1900.0, 0.95),
                basin([-0.64, 0.68, -0.35], 470_000.0, 1400.0, 1250.0, 0.65),
                basin([0.24, -0.94, 0.24], 360_000.0, 1650.0, 1350.0, 0.28),
                basin([-0.29, -0.23, 0.93], 570_000.0, 1000.0, 550.0, 1.0),
                basin([-0.68, -0.54, -0.48], 260_000.0, 1250.0, 1000.0, 0.40),
            ],
            rayed_impacts: [
                ([0.72, -0.55, -0.42], 43_000.0, 1.0, 107),
                ([0.43, 0.85, -0.29], 31_000.0, 0.92, 293),
                ([-0.78, 0.18, 0.60], 52_000.0, 0.84, 419),
                ([0.15, -0.78, 0.61], 24_000.0, 0.72, 557),
                ([-0.31, -0.86, -0.40], 36_000.0, 0.96, 691),
                ([0.89, 0.25, 0.38], 17_000.0, 0.63, 827),
            ]
            .into_iter()
            .map(
                |(direction, radius_meters, freshness, ray_seed)| RayedImpact {
                    direction: DVec3::from_array(direction).normalize().to_array(),
                    radius_meters,
                    freshness,
                    ray_seed,
                },
            )
            .collect(),
            crater_density: 0.77,
            plains_fraction: 0.39,
            scarp_height_meters: 620.0,
            mature_color: [0.145, 0.137, 0.126],
            plains_color: [0.172, 0.159, 0.140],
            fresh_color: [0.38, 0.373, 0.357],
        }
    }
}
