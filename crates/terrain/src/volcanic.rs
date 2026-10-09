//! Dry volcanic plains, broad shields and deformed uplands. All relief is f64 and cell filtered.
use crate::noise::{noise, smoothstep};
use glam::DVec3;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VolcanicOptions {
    pub name: String,
    pub radius_meters: f64,
    pub seed: u32,
    pub max_height_meters: f64,
    pub plains_color: [f64; 3],
    pub upland_color: [f64; 3],
}
impl VolcanicOptions {
    pub fn vesper(radius_meters: f64) -> Self {
        Self {
            name: "Vesper volcanic plains and tessera".into(),
            radius_meters,
            seed: 73,
            max_height_meters: 14000.0,
            // Intrinsic rock reflectance; the yellow illumination belongs to the atmosphere.
            plains_color: [0.12, 0.115, 0.105],
            upland_color: [0.20, 0.19, 0.17],
        }
    }
}
#[derive(Clone, Debug)]
pub struct Volcanic {
    options: VolcanicOptions,
    shields: Vec<DVec3>,
}
impl Volcanic {
    pub fn new(o: &VolcanicOptions) -> Self {
        assert!(o.radius_meters.is_finite() && o.radius_meters > 0.0);
        assert!(o.max_height_meters.is_finite() && o.max_height_meters > 0.0);
        assert!(
            o.plains_color
                .iter()
                .chain(&o.upland_color)
                .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
        );
        let mut state = u64::from(o.seed) + 1;
        let mut random = || {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (state >> 11) as f64 / ((1_u64 << 53) as f64)
        };
        let shields = (0..32)
            .map(|_| {
                let z = 2.0 * random() - 1.0;
                let a = std::f64::consts::TAU * random();
                let r = (1.0 - z * z).sqrt();
                DVec3::new(r * f64::cos(a), r * f64::sin(a), z)
            })
            .collect();
        Self {
            options: o.clone(),
            shields,
        }
    }
    /// A sunlit summit-caldera rim on the smallest suitable shield, for ordinary world fixtures.
    /// Returns a point on existing terrain; it does not alter the field or illumination.
    pub fn sunlit_shield_rim(&self, sun: DVec3) -> DVec3 {
        assert!(sun.is_finite() && (sun.length() - 1.0).abs() < 1e-9);
        let (index, center) = self
            .shields
            .iter()
            .enumerate()
            .filter(|(_, center)| center.dot(sun) > 0.5)
            .min_by(|(a, _), (b, _)| Self::shield_size(*a).total_cmp(&Self::shield_size(*b)))
            .expect("no sunlit shield in volcanic recipe");
        let east = DVec3::Z.cross(*center).normalize();
        (*center + east * Self::shield_size(index) * 0.12).normalize()
    }
    /// Elevated deformed terrain near the subsolar hemisphere, sampled from this exact field.
    pub fn sunlit_upland(&self, sun: DVec3) -> DVec3 {
        assert!(sun.is_finite() && (sun.length() - 1.0).abs() < 1e-9);
        crate::lattice_directions(8192)
            .into_iter()
            .filter(|d| d.dot(sun) > 0.5 && self.upland_strength(*d) > 0.85)
            .max_by(|a, b| {
                self.sample(*a, 100.0)
                    .0
                    .total_cmp(&self.sample(*b, 100.0).0)
            })
            .expect("no sunlit upland")
    }
    fn upland_strength(&self, d: DVec3) -> f64 {
        let offset = f64::from(self.options.seed) * 0.173;
        let n = |p: DVec3, k: f64| noise(p.x + offset + k, p.y, p.z);
        let warp = d + DVec3::new(n(d * 3.0, 0.0), n(d * 3.0, 19.0), n(d * 3.0, 37.0)) * 0.17;
        smoothstep(0.10, 0.38, n(warp * 4.5, 71.0))
    }
    fn shield_size(index: usize) -> f64 {
        0.025 + 0.065 * (index * 37 % 101) as f64 / 100.0
    }
    pub fn sample(&self, d: DVec3, cell: f64) -> (f64, [f64; 3]) {
        assert!(
            d.is_finite() && (d.length() - 1.0).abs() <= 1e-9,
            "volcanic terrain requires unit direction"
        );
        assert!(cell.is_finite() && cell > 0.0, "invalid volcanic cell");
        let o = &self.options;
        let offset = f64::from(o.seed) * 0.173;
        let n = |p: DVec3, k: f64| noise(p.x + offset + k, p.y, p.z);
        let warp = d + DVec3::new(n(d * 3.0, 0.0), n(d * 3.0, 19.0), n(d * 3.0, 37.0)) * 0.17;
        let province = n(warp * 4.5, 71.0);
        let upland = smoothstep(0.10, 0.38, province);
        let mut unit = 0.20 + 0.035 * n(warp * 9.0, 51.0) + 0.28 * upland;
        let mut shield: f64 = 0.0;
        for (i, center) in self.shields.iter().enumerate() {
            let size = Self::shield_size(i);
            let q = (d - center).length() / size;
            if q < 1.0 {
                // Low broad slopes; small summit caldera. Compact C1 edge avoids seams.
                let dome = (1.0 - q * q).powi(3);
                let caldera = (1.0 - smoothstep(0.05, 0.15, q)) * 0.16;
                shield = shield.max((dome - caldera) * 0.25);
            }
        }
        unit += shield;
        let mut wavelength = 80000.0;
        let mut amplitude = 0.012 + 0.025 * upland;
        while wavelength >= 4.0 {
            let fade = smoothstep(2.0 * cell, 4.0 * cell, wavelength);
            if fade == 0.0 {
                break;
            }
            let p = d * (o.radius_meters / wavelength);
            // Cross-cut deformed ridges in tessera; subdued roughness on plains.
            let ridges = (1.0 - n(p, 97.0).abs()) * (1.0 - n(p * 1.31, 127.0).abs()) - 0.55;
            unit += amplitude * fade * ((1.0 - upland) * n(p, 83.0) + upland * ridges);
            amplitude *= 0.58;
            wavelength *= 0.5;
        }
        assert!((0.0..=1.0).contains(&unit), "volcanic height outside bound");
        let tint = 0.25 + 0.5 * upland + 0.15 * n(d * 80.0, 181.0);
        (
            unit * o.max_height_meters,
            std::array::from_fn(|i| {
                o.plains_color[i] + (o.upland_color[i] - o.plains_color[i]) * tint
            }),
        )
    }
}
