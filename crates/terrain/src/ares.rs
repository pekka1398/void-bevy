//! Dry volcanic world: authored provinces around the existing deterministic impact field.
//! Every relief band is sampled here for both collision and rendering. No global image map.
use crate::{
    Basin, ImpactOptions, ImpactTerrain,
    noise::{bump, smoothstep, sphere_noise},
};
use glam::DVec3;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ShieldVolcano {
    pub direction: [f64; 3],
    pub radius_meters: f64,
    pub height_meters: f64,
    pub caldera_radius_meters: f64,
    pub caldera_depth_meters: f64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AresOptions {
    pub impact: ImpactOptions,
    pub max_height_meters: f64,
    pub volcanoes: Vec<ShieldVolcano>,
    pub rise_direction: [f64; 3],
    pub rise_width: f64,
    pub canyon_direction: [f64; 3],
}
impl AresOptions {
    /// Smooth rift erosion footprint, mirrored by the unresolved-normal shader.
    pub fn canyon_mask(&self, d: DVec3) -> f64 {
        let r = self.impact.radius_meters;
        let (center, along, across) = self.canyon_frame();
        let delta = (d - center) * r;
        let x = delta.dot(along) / 1_250_000.0;
        let y = delta.dot(across);
        let mut canyon = 0.0_f64;
        if d.dot(center) > 0.85 && x.abs() < 1.0 {
            let curve = 70_000.0 * (x * x - 0.3) + 16_000.0 * (x * 6.0).sin();
            let width = 45_000.0 * (0.5 + 0.5 * bump(x));
            let edge = sphere_noise(d, r / 35_000.0, 37.0) * 6500.0
                + sphere_noise(d, r / 9000.0, 11.0) * 1800.0;
            for (offset, scale) in [(0.0, 1.0), (80_000.0 * (x + 0.45), 0.60)] {
                let q = (y - curve - offset + edge).abs() / width;
                let floor = 1.0 - smoothstep(0.55, 0.85, q);
                let bench = 1.0 - smoothstep(0.85, 1.35, q);
                canyon = canyon.max((0.78 * floor + 0.22 * bench) * bump(x) * scale);
            }
        }
        canyon
    }
    /// Body-fixed canyon chart, defined even when its centre is a spin pole.
    pub fn canyon_frame(&self) -> (DVec3, DVec3, DVec3) {
        let center = DVec3::from_array(self.canyon_direction);
        assert!(center.is_finite() && (center.length() - 1.0).abs() < 1e-9);
        let reference = if center.z.abs() < 0.9 {
            DVec3::Z
        } else {
            DVec3::X
        };
        let along = center.cross(reference).normalize();
        (center, along, center.cross(along))
    }
    pub fn ares(radius_meters: f64) -> Self {
        let impact = ImpactOptions {
            name: "Ares dry volcanic provinces".into(),
            radius_meters,
            seed: 47,
            datum_meters: 10_000.0,
            max_height_meters: 20_000.0,
            crater_density: 0.65,
            plains_fraction: 0.25,
            scarp_height_meters: 0.0,
            rayed_impacts: Vec::new(),
            mature_color: [0.27, 0.11, 0.055],
            plains_color: [0.40, 0.21, 0.12],
            fresh_color: [0.22, 0.12, 0.08],
            basins: [
                ([0.2, -0.7, -0.68], 950_000.0, 3500.0, 900.0, 0.85),
                ([-0.8, 0.1, -0.59], 610_000.0, 2300.0, 650.0, 0.55),
            ]
            .into_iter()
            .map(|(d, r, depth, rim, fill)| Basin {
                direction: DVec3::from_array(d).normalize().to_array(),
                radius_meters: r,
                depth_meters: depth,
                rim_meters: rim,
                fill,
            })
            .collect(),
        };
        Self {
            impact,
            max_height_meters: 40_000.0,
            rise_direction: DVec3::new(0.12, 0.98, 0.15).normalize().to_array(),
            rise_width: 0.55,
            canyon_direction: DVec3::new(-0.58, 0.79, -0.19).normalize().to_array(),
            volcanoes: [
                ([-0.12, 0.94, 0.32], 340_000.0, 19_000.0, 38_000.0, 2300.0),
                ([0.15, 0.98, 0.08], 220_000.0, 10_000.0, 25_000.0, 1700.0),
                ([0.39, 0.91, -0.10], 185_000.0, 8500.0, 20_000.0, 1300.0),
            ]
            .into_iter()
            .map(|(d, r, h, c, depth)| ShieldVolcano {
                direction: DVec3::from_array(d).normalize().to_array(),
                radius_meters: r,
                height_meters: h,
                caldera_radius_meters: c,
                caldera_depth_meters: depth,
            })
            .collect(),
        }
    }
}
#[derive(Clone, Debug)]
pub struct AresTerrain {
    options: AresOptions,
    impact: ImpactTerrain,
}
/// Shared province mask. North is younger, smoother and lower; the boundary is a broken scarp.
pub fn ares_highlands(d: DVec3) -> f64 {
    1.0 - smoothstep(
        -0.12,
        0.24,
        d.z + 0.16 * sphere_noise(d, 4.2, 19.0) + 0.06 * sphere_noise(d, 13.0, 31.0),
    )
}
impl AresTerrain {
    pub fn new(o: &AresOptions) -> Self {
        assert!(
            o.max_height_meters.is_finite() && o.max_height_meters > o.impact.max_height_meters
        );
        for direction in [o.rise_direction, o.canyon_direction] {
            let d = DVec3::from_array(direction);
            assert!(d.is_finite() && (d.length() - 1.0).abs() < 1e-9);
        }
        assert!(o.rise_width.is_finite() && (0.1..=1.0).contains(&o.rise_width));
        assert!(!o.volcanoes.is_empty() && o.volcanoes.len() <= 16);
        for v in &o.volcanoes {
            let d = DVec3::from_array(v.direction);
            assert!(d.is_finite() && (d.length() - 1.0).abs() < 1e-9);
            assert!(
                v.radius_meters.is_finite() && (10_000.0..1_000_000.0).contains(&v.radius_meters)
            );
            assert!(v.height_meters.is_finite() && (0.0..25_000.0).contains(&v.height_meters));
            assert!(
                v.caldera_radius_meters.is_finite()
                    && (1000.0..v.radius_meters * 0.3).contains(&v.caldera_radius_meters)
            );
            assert!(
                v.caldera_depth_meters.is_finite()
                    && (0.0..v.height_meters * 0.5).contains(&v.caldera_depth_meters)
            );
        }
        Self {
            options: o.clone(),
            impact: ImpactTerrain::new(&o.impact),
        }
    }
    pub fn sample(&self, d: DVec3, cell: f64) -> (f64, [f64; 3]) {
        let o = &self.options;
        let r = o.impact.radius_meters;
        let (impact, _) = self.impact.sample(d, cell);
        let high = ares_highlands(d);
        let rise = bump((d - DVec3::from_array(o.rise_direction)).length() / o.rise_width);
        let resurfaced = (1.0 - high * 0.92).max(rise * 0.94);
        let mut h = 8000.0
            + high * 3600.0
            + rise * 4200.0
            + (impact - o.impact.datum_meters) * (1.0 - resurfaced * 0.94);
        // Large ancient basins cut below the northern lowland datum. The impact field supplies
        // broken rings and younger craters; this long wavelength subsidence survives resurfacing.
        for basin in &o.impact.basins {
            let x = (d - DVec3::from_array(basin.direction)).length() * r / basin.radius_meters;
            h -= basin.depth_meters * (1.0 - smoothstep(0.60, 1.02, x));
        }
        let mut volcanic = 0.0_f64;
        let mut collapsed = 0.0_f64;
        for v in &o.volcanoes {
            let center = DVec3::from_array(v.direction);
            let distance = (d - center).length() * r;
            let x = distance / v.radius_meters;
            if x < 1.2 {
                // A broad, low-angle shield with a broken basal scarp and nested summit caldera.
                let shield = (1.0 - smoothstep(0.0, 1.1, x)).powf(1.7);
                let apron = bump((x - 0.86) / 0.30) * 0.035;
                let channels = sphere_noise(d, r / 35_000.0, 61.0) * 0.025 * x.min(1.0);
                h += v.height_meters * (shield + apron + channels * bump(x));
                let caldera = 1.0 - smoothstep(0.80, 1.05, distance / v.caldera_radius_meters);
                h -= v.caldera_depth_meters * caldera;
                collapsed = collapsed.max(caldera);
                volcanic = volcanic.max(shield);
            }
        }
        // Finite, curved rift complex east of the volcanic rise. Two unequal branches with
        // rounded closed ends, recessed floor and terraced walls; not an infinite latitude stripe.
        let canyon = o.canyon_mask(d);
        // Sediment fill erases older impact relief inside the rift, across every geometry band.
        h -= 5800.0 * canyon
            + (impact - o.impact.datum_meters) * (1.0 - resurfaced * 0.94) * canyon * 0.94;
        // Exposed layered ground and wind worked plains, down to metre relief. Band limited
        // with the same tile cell size as impact terrain; no shader-only dunes or obstacles.
        let mut wavelength = 24_000.0;
        let mut amplitude =
            120.0 * (0.2 + 0.8 * high) * (1.0 - volcanic * 0.6) * (1.0 - canyon * 0.85);
        while wavelength >= 2.0 {
            let fade = smoothstep(1.5 * cell, 3.0 * cell, wavelength);
            if fade == 0.0 {
                break;
            }
            h += sphere_noise(d, r / wavelength, 83.0) * amplitude * fade;
            wavelength /= 3.0;
            amplitude *= 0.40;
        }
        let cap = smoothstep(
            0.972,
            0.993,
            d.z.abs() + sphere_noise(d, 29.0, 73.0) * 0.007,
        );
        h += cap * 500.0;
        let color = ares_color(d, high, volcanic, canyon, cap, collapsed);
        assert!(
            h.is_finite() && (0.0..=o.max_height_meters).contains(&h),
            "Ares height outside envelope: {h} at {d}"
        );
        (h, color)
    }
}
fn ares_color(
    d: DVec3,
    high: f64,
    volcanic: f64,
    canyon: f64,
    cap: f64,
    collapsed: f64,
) -> [f64; 3] {
    // Dust mantles are coherent, domain-warped provinces; the underlying basalt is charcoal
    // brown, not blue (many published Mars views enhance blue or use false colour).
    let warp = d + DVec3::new(
        sphere_noise(d, 4.0, 21.0),
        sphere_noise(d, 4.0, 43.0),
        sphere_noise(d, 4.0, 71.0),
    ) * 0.13;
    let dark = smoothstep(
        -0.04,
        0.24,
        sphere_noise(warp, 2.2, 113.0)
            + high * 0.18
            + sphere_noise(warp, 9.3, 31.0) * 0.23
            + sphere_noise(warp, 31.0, 51.0) * 0.10,
    );
    let dust = [0.40, 0.205, 0.115];
    let basalt = [0.115, 0.078, 0.054];
    let dark = (dark * 0.86 + volcanic * 0.12 + canyon * 0.70 + collapsed * 0.25).min(1.0);
    let mottle = 1.0 + sphere_noise(d, 47.0, 83.0) * 0.11 + sphere_noise(d, 157.0, 33.0) * 0.04;
    std::array::from_fn(|i| {
        ((dust[i] * (1.0 - dark) + basalt[i] * dark) * mottle) * (1.0 - cap)
            + [0.70, 0.73, 0.73][i] * cap
    })
}
