//! Deterministic impact basins and ejecta rims, shared by rendering and collision.
use crate::noise::perlin;
use glam::DVec3;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CrateredOptions {
    pub name: String,
    pub radius_meters: f64,
    pub max_height_meters: f64,
    pub crater_count: usize,
    pub crater_radius_radians: f64,
    pub roughness: f64,
    pub seed: u32,
    pub low_color: [f64; 3],
    pub high_color: [f64; 3],
}
#[derive(Clone, Debug)]
pub struct Cratered {
    options: CrateredOptions,
    centers: Vec<DVec3>,
}
impl Cratered {
    pub fn new(options: &CrateredOptions) -> Self {
        assert!(options.radius_meters.is_finite() && options.radius_meters > 0.0);
        assert!(options.max_height_meters.is_finite() && options.max_height_meters > 0.0);
        assert!((1..=512).contains(&options.crater_count));
        assert!(
            options.crater_radius_radians.is_finite()
                && (0.001..=0.5).contains(&options.crater_radius_radians)
        );
        assert!(options.roughness.is_finite() && (0.0..=1.0).contains(&options.roughness));
        assert!(
            options
                .low_color
                .iter()
                .chain(&options.high_color)
                .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
        );
        let mut state = u64::from(options.seed) + 1;
        let mut random = || {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (state >> 11) as f64 / ((1_u64 << 53) as f64)
        };
        let centers = (0..options.crater_count)
            .map(|_| {
                let z = 2.0 * random() - 1.0;
                let angle = std::f64::consts::TAU * random();
                let r = (1.0 - z * z).sqrt();
                DVec3::new(r * angle.cos(), r * angle.sin(), z)
            })
            .collect();
        Self {
            options: options.clone(),
            centers,
        }
    }
    pub fn sample(&self, d: DVec3) -> (f64, [f64; 3]) {
        assert!(
            d.is_finite() && (d.length() - 1.0).abs() <= 1e-9,
            "cratered terrain requires unit direction"
        );
        let o = &self.options;
        let shift = f64::from(o.seed) * 0.17;
        let mut unit =
            0.5 + o.roughness * 0.12 * perlin(d.x * 40.0 + shift, d.y * 40.0, d.z * 40.0);
        // Max envelope avoids additive overlaps violating the height contract. Each compact
        // basin/rim is zero with zero derivative at its outer boundary.
        let mut depression: f64 = 0.0;
        let mut rim: f64 = 0.0;
        for (i, center) in self.centers.iter().enumerate() {
            let size = o.crater_radius_radians * (0.45 + 0.55 * ((i * 37 % 101) as f64 / 100.0));
            let x2 = (d - center).length_squared() / (size * size);
            if x2 < 1.0 {
                depression = depression.max(0.32 * (1.0 - x2).powi(2));
            }
            if x2 < 2.25 {
                let x = x2.sqrt();
                let t = ((x - 1.0) / 0.5).abs();
                if t < 1.0 {
                    rim = rim.max(0.16 * (1.0 - t * t).powi(2));
                }
            }
        }
        unit += rim - depression;
        assert!((0.0..=1.0).contains(&unit));
        (
            unit * o.max_height_meters,
            std::array::from_fn(|i| o.low_color[i] + (o.high_color[i] - o.low_color[i]) * unit),
        )
    }
}
