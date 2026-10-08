//! Multiscale impact geology. The deterministic spherical feature field is evaluated directly,
//! without a global height map. Cube faces own independent crater populations; overlapping face
//! margins are evaluated on both sides, so neither height nor material has a cube-edge seam.
mod cinder;
use crate::noise::{perlin, smoothstep};
use glam::DVec3;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Basin {
    pub direction: [f64; 3],
    pub radius_meters: f64,
    pub depth_meters: f64,
    pub rim_meters: f64,
    /// Later lava flooding: erases old relief, but not the younger impact population.
    pub fill: f64,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RayedImpact {
    pub direction: [f64; 3],
    pub radius_meters: f64,
    pub freshness: f64,
    pub ray_seed: u32,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImpactOptions {
    pub name: String,
    pub radius_meters: f64,
    pub max_height_meters: f64,
    pub datum_meters: f64,
    pub seed: u32,
    pub basins: Vec<Basin>,
    pub rayed_impacts: Vec<RayedImpact>,
    pub crater_density: f64,
    pub plains_fraction: f64,
    pub scarp_height_meters: f64,
    pub mature_color: [f64; 3],
    pub plains_color: [f64; 3],
    pub fresh_color: [f64; 3],
}

#[derive(Clone, Debug)]
pub struct ImpactTerrain {
    options: ImpactOptions,
    scarps: Vec<(DVec3, DVec3, f64, f64)>,
}

/// Integer-only feature identity, also used by the regolith shader. No floating point sine hash.
fn hash(mut n: u32) -> u32 {
    n ^= n >> 16;
    n = n.wrapping_mul(0x7feb352d);
    n ^= n >> 15;
    n = n.wrapping_mul(0x846ca68b);
    n ^ (n >> 16)
}
fn random(n: u32) -> f64 {
    f64::from(hash(n) >> 8) / 16_777_216.0
}
fn bump(x: f64) -> f64 {
    let t = (1.0 - x * x).max(0.0);
    t * t
}
fn noise(d: DVec3, frequency: f64, seed: f64) -> f64 {
    perlin(
        d.x * frequency + seed,
        d.y * frequency + 3.71,
        d.z * frequency - 5.13,
    )
}
fn basis(axis: usize) -> (DVec3, DVec3, DVec3) {
    match axis {
        0 => (DVec3::X, DVec3::Y, DVec3::Z),
        1 => (DVec3::NEG_X, DVec3::Y, DVec3::Z),
        2 => (DVec3::Y, DVec3::X, DVec3::Z),
        3 => (DVec3::NEG_Y, DVec3::X, DVec3::Z),
        4 => (DVec3::Z, DVec3::X, DVec3::Y),
        5 => (DVec3::NEG_Z, DVec3::X, DVec3::Y),
        _ => unreachable!(),
    }
}

impl ImpactTerrain {
    pub fn new(options: &ImpactOptions) -> Self {
        let o = options;
        assert!(o.radius_meters.is_finite() && o.radius_meters > 100_000.0);
        assert!(o.max_height_meters.is_finite() && o.max_height_meters > 0.0);
        assert!(o.datum_meters.is_finite() && (0.0..o.max_height_meters).contains(&o.datum_meters));
        assert!(o.crater_density.is_finite() && (0.0..=1.0).contains(&o.crater_density));
        assert!(o.plains_fraction.is_finite() && (0.0..=0.8).contains(&o.plains_fraction));
        assert!(
            o.scarp_height_meters.is_finite() && (0.0..=1500.0).contains(&o.scarp_height_meters)
        );
        for color in [o.mature_color, o.plains_color, o.fresh_color] {
            assert!(
                color
                    .iter()
                    .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
            );
        }
        assert!(o.basins.len() <= 32);
        for b in &o.basins {
            let d = DVec3::from_array(b.direction);
            assert!(d.is_finite() && (d.length() - 1.0).abs() < 1e-9);
            assert!(
                b.radius_meters.is_finite()
                    && (1000.0..o.radius_meters * 0.6).contains(&b.radius_meters)
            );
            assert!(b.depth_meters.is_finite() && (0.0..=4000.0).contains(&b.depth_meters));
            assert!(b.rim_meters.is_finite() && (0.0..=3000.0).contains(&b.rim_meters));
            assert!(b.fill.is_finite() && (0.0..=1.0).contains(&b.fill));
        }
        assert!(o.rayed_impacts.len() <= 32);
        for c in &o.rayed_impacts {
            let d = DVec3::from_array(c.direction);
            assert!(d.is_finite() && (d.length() - 1.0).abs() < 1e-9);
            assert!(c.radius_meters.is_finite() && (1000.0..=100_000.0).contains(&c.radius_meters));
            assert!(c.freshness.is_finite() && (0.0..=1.0).contains(&c.freshness));
        }
        let scarps = (0..64_u32)
            .map(|i| {
                let s = o.seed.wrapping_add(i * 13);
                let z = 2.0 * random(s) - 1.0;
                let a = random(s.wrapping_add(1)) * std::f64::consts::TAU;
                let center = DVec3::new(
                    (1.0 - z * z).sqrt() * a.cos(),
                    (1.0 - z * z).sqrt() * a.sin(),
                    z,
                );
                let tangent = center
                    .cross(if z.abs() < 0.9 { DVec3::Z } else { DVec3::X })
                    .normalize();
                let angle = random(s.wrapping_add(2)) * std::f64::consts::TAU;
                let along = tangent * angle.cos() + center.cross(tangent) * angle.sin();
                (
                    center,
                    along,
                    (40_000.0 + random(s.wrapping_add(3)) * 260_000.0).min(o.radius_meters * 0.15),
                    0.3 + 0.7 * random(s.wrapping_add(4)),
                )
            })
            .collect();
        Self {
            options: o.clone(),
            scarps,
        }
    }

    pub fn sample(&self, d: DVec3, cell_meters: f64) -> (f64, [f64; 3]) {
        assert!(
            d.is_finite() && (d.length() - 1.0).abs() <= 1e-9,
            "impact terrain requires unit direction"
        );
        assert!(
            cell_meters.is_finite() && cell_meters > 0.0,
            "impact terrain requires positive finite cell size"
        );
        let o = &self.options;
        let r = o.radius_meters;
        let seed = f64::from(o.seed % 10007) * 0.173;
        let resolved = |w: f64| smoothstep(1.5 * cell_meters, 3.0 * cell_meters, w);
        let region = noise(d, 3.1, seed);
        let warp = d + DVec3::new(
            noise(d, 4.0, seed + 71.0),
            noise(d, 4.0, seed + 19.0),
            noise(d, 4.0, seed + 39.0),
        ) * 0.07;
        let plains = smoothstep(
            0.5 - o.plains_fraction,
            0.68 - o.plains_fraction,
            noise(warp, 5.3, seed + 31.0),
        );
        let mut plain = plains;
        let mut h = o.datum_meters + 950.0 * region + 270.0 * noise(d, 11.0, seed + 10.0);
        // Broad ancient basins: broken concentric massifs, low floors, later flooded interiors.
        for b in &o.basins {
            let center = DVec3::from_array(b.direction);
            let x = (d - center).length() * r / b.radius_meters;
            if x > 1.8 {
                continue;
            }
            let ragged = noise(d, r / b.radius_meters * 11.0, seed + 59.0);
            let q = x * (1.0 + 0.045 * ragged);
            let interior = 1.0 - smoothstep(0.68, 1.01, q);
            let wall = bump((q - 1.02) / 0.18);
            let ring = bump((q - 1.29) / 0.16) * 0.34;
            h -= b.depth_meters * interior;
            h += b.rim_meters * (wall + ring) * (0.72 + 0.55 * ragged);
            let fill = interior * b.fill;
            plain = plain.max(fill);
            h = h * (1.0 - fill * 0.65) + (o.datum_meters - b.depth_meters * 0.67) * fill * 0.65;
        }
        // Meter-scale regolith roughness is physical geometry when a collision/render tile can
        // resolve it. Plains are gently rolling; cratered uplands retain much more relief.
        let mut wavelength = 90_000.0;
        let mut amplitude = 230.0 * (1.0 - plain * 0.91);
        while wavelength >= 3.0 {
            let fade = resolved(wavelength);
            if fade == 0.0 {
                break;
            }
            h += noise(d, r / wavelength, seed + 93.0) * amplitude * fade;
            wavelength /= 3.0;
            amplitude *= 0.36;
        }
        let mut fresh: f64 = 0.0;
        let mut dark: f64 = 0.0;
        // Geometric series spans ~300 km to ~15 m diameter. Density and preservation depend on
        // province and feature age. Centres are jittered, radii power-distributed, never a lattice
        // of identically sized bowls. Six overlapping face populations remain continuous at seams.
        for octave in 0..9_u32 {
            let frequency = 6.0 * 3.0_f64.powi(octave as i32);
            let nominal = r / frequency;
            if resolved(nominal * 0.8) == 0.0 {
                break;
            }
            for face in 0..6 {
                let (n, u, v) = basis(face);
                let dn = d.dot(n);
                // Includes ejecta outside the padded face corner at the coarsest octave:
                // its centre can reach dn≈0.468 and support another 0.026 toward the limb.
                if dn < 0.4 {
                    continue;
                }
                let px = d.dot(u) / dn * frequency;
                let py = d.dot(v) / dn * frequency;
                let ix = px.floor() as i32;
                let iy = py.floor() as i32;
                for oy in -1..=1 {
                    for ox in -1..=1 {
                        let cx = ix + ox;
                        let cy = iy + oy;
                        // A fixed overlap margin beyond cube edges; querying a different face never
                        // changes ownership. Support is < one lattice cell even at the face corners.
                        if (cx as f64).abs() > frequency + 1.0
                            || (cy as f64).abs() > frequency + 1.0
                        {
                            continue;
                        }
                        let id = hash(
                            (cx as u32).wrapping_mul(374761393)
                                ^ (cy as u32).wrapping_mul(668265263)
                                ^ (face as u32).wrapping_mul(2246822519)
                                ^ octave.wrapping_mul(3266489917)
                                ^ o.seed,
                        );
                        let chance = random(id);
                        if chance > o.crater_density {
                            continue;
                        }
                        let center = (n
                            + u * ((f64::from(cx) + random(id.wrapping_add(1))) / frequency)
                            + v * ((f64::from(cy) + random(id.wrapping_add(2))) / frequency))
                            .normalize();
                        let radius = nominal
                            * (0.07 + 0.26 * random(id.wrapping_add(3)).powf(1.7))
                            * center.dot(n).powi(2);
                        let x = (d - center).length() * r / radius;
                        if x >= 2.1 {
                            continue;
                        }
                        let age = random(id.wrapping_add(4));
                        let young = smoothstep(0.91, 0.99, age);
                        let survival = 1.0 - plain * (1.0 - young) * 0.94;
                        let fade = resolved(radius * 1.3);
                        if fade == 0.0 {
                            continue;
                        }
                        let rough = noise(d, r / radius * 5.0, f64::from(id % 101));
                        let q = x * (1.0 + (0.025 + 0.10 * (1.0 - age)) * rough);
                        let rim_preservation = 1.0 - (1.0 - age) * 0.65 * (0.5 + 0.5 * rough);
                        let rim_width = 0.11 + 0.17 * (1.0 - age);
                        let rim = bump((q - 1.0) / rim_width);
                        let complex = smoothstep(6000.0, 14000.0, radius);
                        let bowl_start = 0.2 + complex * (0.42 + 0.10 * age);
                        let bowl = 1.0 - smoothstep(bowl_start, 1.0, q);
                        let depth = (radius * 0.21).min(1800.0) * (0.28 + 0.72 * age) * survival;
                        let peak = bump(q / 0.22) * complex * 0.48;
                        let terraces = bump((q - 0.76) / 0.065) * complex * age * 0.13;
                        // Impact depth is relative to the existing local surface. Small impacts must not
                        // reset kilometres of older relief to a global datum. Old rims retain ghost
                        // relief through lava; compact support prevents discontinuous cutoffs.
                        h += depth
                            * (-bowl
                                + rim * (0.23 + 0.17 * age) * rim_preservation
                                + peak
                                + terraces)
                            * fade;
                        let ejecta = bump((q - 1.25) / 0.85)
                            * (0.25 + 0.75 * young)
                            * (1.0 - smoothstep(1.8, 2.1, x));
                        h += depth * 0.055 * ejecta * rough * fade;
                        fresh = fresh.max((rim * 0.35 + ejecta * 0.65) * young * survival * fade);
                        dark = dark.max(bowl * (1.0 - young) * 0.17 * survival * fade);
                    }
                }
            }
        }
        // Sparse young complex impacts have discontinuous, azimuthally asymmetric rays. These
        // deposits brighten material independently of elevation and extend far beyond the rim.
        for c in &o.rayed_impacts {
            let center = DVec3::from_array(c.direction);
            let distance = (d - center).length() * r;
            let x = distance / c.radius_meters;
            if x >= 15.0 {
                continue;
            }
            let seed = f64::from(c.ray_seed % 10007);
            let q = x * (1.0 + 0.025 * noise(d, r / c.radius_meters * 7.0, seed));
            let fade = resolved(c.radius_meters * 1.3);
            let bowl = 1.0 - smoothstep(0.68, 1.0, q);
            let rim = bump((q - 1.0) / 0.12);
            let peak = bump(q / 0.20) * 0.45;
            let depth = (c.radius_meters * 0.16).min(2200.0);
            h += depth * (-bowl + rim * 0.37 + peak) * fade;
            let t = center
                .cross(if center.z.abs() < 0.9 {
                    DVec3::Z
                } else {
                    DVec3::X
                })
                .normalize();
            let b = center.cross(t);
            let azimuth = d.dot(b).atan2(d.dot(t));
            // Different non-harmonic lobes, with noisy edges and gaps; no tidy starburst spokes.
            let angle =
                azimuth + (0.07 + x * 0.005) * noise(d, 93.0, seed + 11.0) + 0.035 * x.ln_1p();
            let lobes = (angle * 11.0 + seed).sin() * 0.48
                + (angle * 19.0 - seed * 0.31).sin() * 0.32
                + (angle * 31.0 + 1.7).sin() * 0.20;
            let streak = smoothstep(0.23, 0.67, lobes);
            let breakup = 0.58 + 0.42 * noise(d, 137.0, seed + 3.0);
            let ray = streak * breakup * (1.0 - smoothstep(2.0, 15.0, x)) * smoothstep(0.9, 1.6, x)
                / (1.0 + x * 0.12);
            let apron = bump((x - 1.18) / 1.3) * 0.37;
            fresh = fresh.max((ray * 1.6 + apron + rim * 0.30) * c.freshness);
        }
        // Lobate scarps: finite, curved asymmetric thrust ramps; no endless great-circle ridges.
        for &(center, along, length, strength) in &self.scarps {
            if d.dot(center) < 0.98 {
                continue;
            }
            let delta = (d - center) * r;
            let x = delta.dot(along) / length;
            if x.abs() >= 1.0 {
                continue;
            }
            let across = delta.dot(center.cross(along)) - length * 0.10 * (x * x - 0.3);
            let width = 2200.0 + length * 0.013;
            let step = smoothstep(-width * 0.16, width * 0.16, across)
                * (1.0 - smoothstep(width * 0.25, width * 3.0, across));
            h += o.scarp_height_meters * strength * bump(x) * step * resolved(width);
        }
        let dark_unit = smoothstep(-0.18, 0.22, noise(warp, 7.2, seed + 177.0));
        let mottling =
            (0.78 + 0.26 * dark_unit) * (1.0 + 0.16 * region + 0.13 * noise(d, 37.0, seed + 117.0));
        let color = std::array::from_fn(|i| {
            let base = o.mature_color[i] * (1.0 - plain * 0.65) + o.plains_color[i] * plain * 0.65;
            (base * (1.0 - fresh * 0.7) + o.fresh_color[i] * fresh * 0.7) * mottling * (1.0 - dark)
        });
        assert!(
            h.is_finite() && (0.0..=o.max_height_meters).contains(&h),
            "impact height {h} outside configured envelope at {d}"
        );
        assert!(
            color
                .iter()
                .all(|v| v.is_finite() && (0.0..=1.0).contains(v)),
            "impact albedo outside linear [0,1] at {d}: {color:?}"
        );
        (h, color)
    }
}
