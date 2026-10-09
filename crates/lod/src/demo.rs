//! A demonstration terrain for the LOD example: warped continents, a coastal shelf and
//! ridged mountains, on a preset's parameters (`presets/planets.json`, "seam", "normal", "landing").

use glam::DVec3;
use serde::Deserialize;

use crate::{SurfaceSample, SurfaceSampler};

/// A preset's `terrain` block.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DemoTerrainParams {
    pub warp_frequency: f64,
    pub warp_octaves: u32,
    pub warp_strength: f64,
    pub warp_offsets: [f64; 3],
    pub continent_frequency: f64,
    pub continent_octaves: u32,
    pub coast_start: f64,
    pub coast_end: f64,
    pub ridge_frequencies: [f64; 3],
    pub ridge_powers: [f64; 3],
    pub ridge_weights: [f64; 3],
    pub land_base_height_fraction: f64,
    pub land_mountain_height_fraction: f64,
    pub snow_height_meters: f64,
    pub rock_height_meters: f64,
    pub ocean_color: [f64; 3],
    pub snow_color: [f64; 3],
    pub rock_color: [f64; 3],
}

/// The demonstration surface for one preset. It ignores the tile's cell size.
#[derive(Clone, Debug)]
pub struct DemoTerrain {
    pub name: String,
    pub radius_meters: f64,
    pub max_height_meters: f64,
    pub params: DemoTerrainParams,
}

/// The part of a preset `DemoTerrain` needs.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Preset {
    name: String,
    radius_meters: f64,
    max_surface_height_meters: f64,
    terrain: DemoTerrainParams,
}

/// The presets, by id.
pub const PRESETS_JSON: &str = include_str!("../presets/planets.json");

impl DemoTerrain {
    /// A preset by id from `presets/planets.json`: "seam", "normal" or "landing".
    pub fn preset(id: &str) -> Self {
        let mut presets: std::collections::HashMap<String, Preset> =
            serde_json::from_str(PRESETS_JSON).expect("presets/planets.json");
        let p = presets
            .remove(id)
            .unwrap_or_else(|| panic!("unknown lod preset {id:?}; valid: seam, normal, landing"));
        Self {
            name: p.name,
            radius_meters: p.radius_meters,
            max_height_meters: p.max_surface_height_meters,
            params: p.terrain,
        }
    }

    /// Height above the reference radius and display colour at a unit direction.
    pub fn sample_direction(&self, d: DVec3) -> SurfaceSample {
        let length = d.length();
        assert!(
            length.is_finite() && (length - 1.0).abs() <= 1e-6,
            "DemoTerrain: expected a unit direction; preset={}; direction={d}; length={length}",
            self.name
        );
        let t = &self.params;
        let (x, y, z) = (d.x, d.y, d.z);
        let (wf, wo, ws) = (t.warp_frequency, t.warp_octaves, t.warp_strength);
        let warp_x = fractal(x * wf + t.warp_offsets[0], y * wf, z * wf, wo) * ws;
        let warp_y = fractal(x * wf, y * wf + t.warp_offsets[1], z * wf, wo) * ws;
        let warp_z = fractal(x * wf, y * wf, z * wf + t.warp_offsets[2], wo) * ws;
        let (px, py, pz) = (x + warp_x, y + warp_y, z + warp_z);
        let cf = t.continent_frequency;
        let continent = fractal(px * cf, py * cf, pz * cf, t.continent_octaves);
        let land = smoothstep(t.coast_start, t.coast_end, continent);
        let ridge = |j: usize| {
            let rf = t.ridge_frequencies[j];
            1.0 - perlin(px * rf, py * rf, pz * rf).abs().min(1.0)
        };
        let mountains = t.ridge_weights[0] * ridge(0).powf(t.ridge_powers[0])
            + t.ridge_weights[1] * ridge(1).powf(t.ridge_powers[1])
            + t.ridge_weights[2] * ridge(2).powf(t.ridge_powers[2]);
        let height_meters = self.max_height_meters
            * land
            * (t.land_base_height_fraction + t.land_mountain_height_fraction * mountains);
        let color = if land < 0.02 {
            [
                t.ocean_color[0],
                t.ocean_color[1] + 0.07 * land,
                t.ocean_color[2],
            ]
        } else if height_meters > t.snow_height_meters {
            t.snow_color
        } else if height_meters > t.rock_height_meters {
            t.rock_color
        } else {
            [0.13 + 0.12 * land, 0.25 + 0.1 * land, 0.12]
        };
        SurfaceSample {
            height_meters,
            color: color.map(|c| c as f32),
        }
    }
}

impl SurfaceSampler for DemoTerrain {
    fn sample(&self, direction: DVec3, _cell_meters: f64) -> SurfaceSample {
        self.sample_direction(direction)
    }
}

const GRADIENTS: [[f64; 3]; 12] = [
    [1.0, 1.0, 0.0],
    [-1.0, 1.0, 0.0],
    [1.0, -1.0, 0.0],
    [-1.0, -1.0, 0.0],
    [1.0, 0.0, 1.0],
    [-1.0, 0.0, 1.0],
    [1.0, 0.0, -1.0],
    [-1.0, 0.0, -1.0],
    [0.0, 1.0, 1.0],
    [0.0, -1.0, 1.0],
    [0.0, 1.0, -1.0],
    [0.0, -1.0, -1.0],
];

/// Wrapping-multiply hash on int32.
fn hash(x: i32, y: i32, z: i32) -> u32 {
    let h =
        x.wrapping_mul(374_761_393) ^ y.wrapping_mul(668_265_263) ^ z.wrapping_mul(1_442_695_041);
    let h = (h ^ ((h as u32) >> 13) as i32).wrapping_mul(1_274_126_177);
    (h ^ ((h as u32) >> 16) as i32) as u32
}

fn fade(t: f64) -> f64 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

fn smoothstep(a: f64, b: f64, x: f64) -> f64 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Continuous 3D gradient noise; the same direction gives the same height on every cube face.
fn perlin(x: f64, y: f64, z: f64) -> f64 {
    let (ix, iy, iz) = (x.floor(), y.floor(), z.floor());
    let (fx, fy, fz) = (x - ix, y - iy, z - iz);
    let (wx, wy, wz) = (fade(fx), fade(fy), fade(fz));
    let corner = |dx: f64, dy: f64, dz: f64| {
        let g = GRADIENTS
            [hash((ix + dx) as i32, (iy + dy) as i32, (iz + dz) as i32) as usize % GRADIENTS.len()];
        (g[0] * (fx - dx) + g[1] * (fy - dy) + g[2] * (fz - dz)) * std::f64::consts::FRAC_1_SQRT_2
    };
    let bottom = lerp(
        lerp(corner(0.0, 0.0, 0.0), corner(1.0, 0.0, 0.0), wx),
        lerp(corner(0.0, 1.0, 0.0), corner(1.0, 1.0, 0.0), wx),
        wy,
    );
    let top = lerp(
        lerp(corner(0.0, 0.0, 1.0), corner(1.0, 0.0, 1.0), wx),
        lerp(corner(0.0, 1.0, 1.0), corner(1.0, 1.0, 1.0), wx),
        wy,
    );
    lerp(bottom, top, wz)
}

fn fractal(x: f64, y: f64, z: f64, octaves: u32) -> f64 {
    let (mut sum, mut frequency, mut amplitude, mut weight) = (0.0, 1.0, 1.0, 0.0);
    for _ in 0..octaves {
        sum += perlin(x * frequency, y * frequency, z * frequency) * amplitude;
        weight += amplitude;
        frequency *= 2.0;
        amplitude *= 0.5;
    }
    sum / weight
}
