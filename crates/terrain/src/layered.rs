//! The layered planet: continents, mountain belts, and eroded hills
//! down to metres, so it reads as a planet from 20,000 km and as ground from 2 m.
//!
//! 1. Continents (thousands of km): domain-warped fBm whose sign is land or sea. Sea falls to a
//!    150 m shelf, then to 4.5 km deep basins; land rises slowly inland.
//! 2. Mountain belts (hundreds of km): narrow bands along the zero lines of a low-frequency
//!    noise, on land and at coasts, filled with ridged multifractal noise.
//! 3. Hills down to metres: fBm whose octaves are damped where the ground already slopes, so
//!    slopes grow gullies and ridges and flat ground stays flat.
//!
//! Heights are measured from the ocean floor's reference sphere (never negative, as lod
//! requires); sea level is `SEA_LEVEL` above it. Octaves finer than about 2–4 tile cells fade out,
//! as a mipmap would.

use glam::DVec3;
use serde::{Deserialize, Serialize};

use crate::noise::{fbm, noise, noise_with_gradient, smoothstep};

pub const SEA_LEVEL: f64 = 5000.0;
pub const MAX_HEIGHT: f64 = 16_000.0;

/// Octave wavelengths, metres.
const HILL_LONGEST: f64 = 60_000.0;
const HILL_SHORTEST: f64 = 8.0;
const MOUNTAIN_LONGEST: f64 = 220_000.0;
const MOUNTAIN_SHORTEST: f64 = 1_500.0;

#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LayeredOptions {
    pub radius_meters: f64,
    pub seed: i64,
}

/// scenery's `DEFAULT_LAYERED`.
pub const DEFAULT_LAYERED: LayeredOptions = LayeredOptions {
    radius_meters: 6_371_000.0,
    seed: 7,
};

#[derive(Clone, Copy, Debug)]
pub struct Layered {
    radius: f64,
    seed: f64,
}

const SEA_FLOOR: [f64; 3] = [0.12, 0.11, 0.08];
const DESERT: [f64; 3] = [0.36, 0.27, 0.16];
const STEPPE: [f64; 3] = [0.19, 0.17, 0.09];
const GRASS: [f64; 3] = [0.075, 0.12, 0.04];
const FOREST: [f64; 3] = [0.03, 0.06, 0.025];
const TUNDRA: [f64; 3] = [0.14, 0.13, 0.1];

fn mix(a: [f64; 3], b: [f64; 3], t: f64) -> [f64; 3] {
    [0, 1, 2].map(|i| a[i] + (b[i] - a[i]) * t)
}

impl Layered {
    pub fn new(options: LayeredOptions) -> Self {
        assert!(
            options.radius_meters > 0.0,
            "layered terrain: radius {}",
            options.radius_meters
        );
        Self {
            radius: options.radius_meters,
            seed: options.seed as f64,
        }
    }

    fn offset(&self, k: f64) -> f64 {
        (self.seed * 7919.0 + k * 104_729.0) % 1000.0 + 0.37 * k
    }

    /// Height above the reference sphere and the ground cover's linear albedo, at a unit
    /// body-fixed direction, leaving out detail finer than about two `cell_meters`.
    pub fn sample(&self, d: DVec3, cell_meters: f64) -> (f64, [f64; 3]) {
        let length = d.length();
        assert!(
            (length - 1.0).abs() < 1e-6,
            "layered terrain: not a unit direction {d}"
        );
        assert!(cell_meters > 0.0, "layered terrain: cell {cell_meters}");
        let r = self.radius;
        // 1 for wavelengths well above the cell, fading to 0 at two cells.
        let resolved =
            |wavelength: f64| smoothstep(2.0 * cell_meters, 4.0 * cell_meters, wavelength);
        let o = |k: f64| self.offset(k);

        // 1. Continents, on a warped sphere.
        let wx = fbm(d.x * 2.0 + o(1.0), d.y * 2.0, d.z * 2.0, 3);
        let wy = fbm(d.x * 2.0, d.y * 2.0 + o(2.0), d.z * 2.0, 3);
        let wz = fbm(d.x * 2.0, d.y * 2.0, d.z * 2.0 + o(3.0), 3);
        let (qx, qy, qz) = (d.x + 0.3 * wx, d.y + 0.3 * wy, d.z + 0.3 * wz);
        let continent = fbm(qx * 1.4 + o(4.0), qy * 1.4, qz * 1.4, 6) - 0.06;
        let land = smoothstep(0.0, 0.02, continent);
        let mut elevation = if continent >= 0.0 {
            60.0 * smoothstep(0.0, 0.003, continent) + 500.0 * smoothstep(0.02, 0.35, continent)
        } else {
            -150.0 * smoothstep(0.0, -0.02, continent)
                - 4300.0 * smoothstep(-0.02, -0.15, continent)
        };

        // 2. Mountain belts: near the zero lines of a low-frequency noise.
        let belt_noise = fbm(qx * 3.0 + o(5.0), qy * 3.0, qz * 3.0, 3);
        let belt = smoothstep(0.1, 0.02, belt_noise.abs()) * smoothstep(-0.03, 0.08, continent);
        let belt_strength =
            0.45 + 0.55 * smoothstep(-0.3, 0.3, noise(qx * 9.0 + o(6.0), qy * 9.0, qz * 9.0));
        if belt > 0.0 {
            let (mut sum, mut weight, mut amplitude, mut norm) = (0.0, 1.0, 1.0, 0.0);
            let mut wavelength = MOUNTAIN_LONGEST;
            while wavelength >= MOUNTAIN_SHORTEST {
                let f = r / wavelength;
                let ridge = 1.0 - noise(d.x * f + o(7.0), d.y * f, d.z * f).abs();
                let signal = ridge * ridge * weight;
                weight = (signal * 2.0).min(1.0);
                sum += signal * amplitude * resolved(wavelength);
                norm += amplitude;
                amplitude *= 0.6;
                wavelength /= 2.0;
            }
            // Squared: valleys between the ridges stay low and the peaks sharpen.
            let ridges = (sum / norm) * 1.6;
            elevation += 4200.0 * belt * belt_strength * ridges * ridges;
        }

        // 3. Hills: eroded fBm, rougher in the belts and in rough regions, gentle on plains and the
        // sea floor.
        let region = smoothstep(-0.25, 0.35, noise(qx * 7.0 + o(8.0), qy * 7.0, qz * 7.0));
        let hill_amplitude = (160.0 + 380.0 * region + 3200.0 * belt) * (0.25 + 0.75 * land);
        let (mut slope_x, mut slope_y, mut slope_z) = (0.0, 0.0, 0.0);
        let mut hills = 0.0;
        let mut amplitude = hill_amplitude;
        let mut wavelength = HILL_LONGEST;
        while wavelength >= HILL_SHORTEST {
            let fade = resolved(wavelength);
            if fade == 0.0 {
                break;
            }
            let f = r / wavelength;
            let (n, g) = noise_with_gradient(d.x * f + o(9.0), d.y * f, d.z * f);
            // Slope this octave adds, metres per metre, along the surface (radial part dropped).
            let along = g[0] * d.x + g[1] * d.y + g[2] * d.z;
            let scale = (amplitude * f) / r;
            slope_x += (g[0] - along * d.x) * scale;
            slope_y += (g[1] - along * d.y) * scale;
            slope_z += (g[2] - along * d.z) * scale;
            let damping =
                1.0 / (1.0 + 1.5 * (slope_x * slope_x + slope_y * slope_y + slope_z * slope_z));
            hills += amplitude * n * damping * fade;
            // Rough regions keep more of their small-scale relief.
            amplitude *= 0.5 + 0.04 * region + 0.04 * belt;
            wavelength /= 2.0;
        }
        elevation += hills;

        let height = (SEA_LEVEL + elevation).clamp(0.0, MAX_HEIGHT);
        let color = if elevation < 0.0 {
            SEA_FLOOR
        } else {
            ground_cover(d, qx, qy, qz, elevation, belt, &resolved)
        };
        (height, color)
    }
}

/// What covers the land, as a linear albedo (the shader adds beaches, rock and snow on top): a
/// wetness from continental-scale noise, dried in the subtropical belts and in the lee of
/// mountain belts, picks desert, steppe, grass or forest; the far north and south turn to
/// tundra. Patches of a few kilometres down to tens of metres break it up, band-limited like
/// the heights.
fn ground_cover(
    d: DVec3,
    qx: f64,
    qy: f64,
    qz: f64,
    elevation: f64,
    belt: f64,
    resolved: &impl Fn(f64) -> f64,
) -> [f64; 3] {
    let latitude = f64::asin(d.z.clamp(-1.0, 1.0));
    let tropics = (latitude.abs() - 0.44) / 0.14;
    let subtropics = f64::exp(-(tropics * tropics));
    let wetness = 0.55 + 0.9 * fbm(qx * 2.5 + 311.0, qy * 2.5, qz * 2.5, 4)
        - 0.55 * subtropics
        - 0.2 * belt
        - elevation / 12_000.0;
    let (mut patches, mut weight) = (0.0, 0.0);
    // Wavelengths 6.4 km down to 25 m, in the same radius units as the heights (6,371 km).
    let (mut wavelength, mut a) = (6400.0, 1.0);
    while wavelength >= 25.0 {
        let k = 6_371_000.0 / wavelength;
        patches += a * resolved(wavelength) * noise(d.x * k + 71.0, d.y * k, d.z * k);
        weight += a;
        wavelength /= 2.0;
        a *= 0.8;
    }
    let w = (wetness + 0.5 * (patches / weight) * 2.0).clamp(0.0, 1.0);
    let mut c = if w < 0.25 {
        mix(DESERT, STEPPE, w / 0.25)
    } else if w < 0.5 {
        mix(STEPPE, GRASS, (w - 0.25) / 0.25)
    } else {
        mix(GRASS, FOREST, ((w - 0.5) / 0.3).min(1.0))
    };
    c = mix(c, TUNDRA, smoothstep(0.95, 1.2, latitude.abs()));
    let brightness = 1.0 + 0.4 * (patches / weight) * 2.0;
    c.map(|v| v * brightness)
}
