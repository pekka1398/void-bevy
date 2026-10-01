//! The cloud field's CPU side: the global weather atlas, the repeating 3D noise volumes, and CPU
//! references for the shader's density and shell intervals.

use glam::DVec3;
use void_math::{asin, cos, hypot, pow, sin};
use void_terrain::noise::noise;

/// Heights above the live sea level, never above the ocean-floor reference sphere.
pub const CLOUD_BOTTOM: f64 = 1500.0;
pub const CLOUD_TOP: f64 = 8000.0;
/// m⁻¹ at unit density.
pub const CLOUD_EXTINCTION: f64 = 0.0011;
pub const SHAPE_PERIOD: f64 = 65536.0;
pub const DETAIL_PERIOD: f64 = 2048.0;
pub const WEATHER_WIDTH: usize = 2048;
pub const WEATHER_HEIGHT: usize = 1024;
pub const SHAPE_SIZE: usize = 64;
pub const DETAIL_SIZE: usize = 32;
pub const DEFAULT_CLOUD_COVERAGE: f64 = 0.62;

fn clamp01(v: f64) -> f64 {
    v.clamp(0.0, 1.0)
}

pub fn cloud_smooth(a: f64, b: f64, v: f64) -> f64 {
    let t = clamp01((v - a) / (b - a));
    t * t * (3.0 - 2.0 * t)
}

/// Global weather on body-fixed directions: (humidity, vertical type). No cube-face coordinates or
/// tile state.
pub fn cloud_weather(d: DVec3) -> (f64, f64) {
    let latitude = asin(d.z.clamp(-1.0, 1.0));
    // Warp broad weather systems before adding weaker regional structure.
    let x = d.x + 0.18 * noise(d.x * 3.0 + 71.0, d.y * 3.0, d.z * 3.0);
    let y = d.y + 0.18 * noise(d.x * 3.0, d.y * 3.0 + 29.0, d.z * 3.0);
    let z = d.z + 0.18 * noise(d.x * 3.0, d.y * 3.0, d.z * 3.0 + 13.0);
    let humidity = clamp01(
        0.55 + 1.1 * noise(x * 7.0 + 41.0, y * 7.0, z * 7.0)
            + 0.6 * noise(x * 23.0, y * 23.0 + 17.0, z * 23.0)
            + 0.18 * noise(x * 47.0 + 7.0, y * 47.0, z * 47.0)
            + 0.06 * noise(x * 89.0, y * 89.0 + 53.0, z * 89.0)
            + 0.12 * cos(latitude * 4.0),
    );
    let kind = clamp01(
        0.45 + 0.5 * noise(d.x * 11.0, d.y * 11.0, d.z * 11.0 + 31.0) + 0.25 * cos(latitude * 2.0),
    );
    (humidity, kind)
}

pub fn weather_coverage(humidity: f64, amount: f64) -> f64 {
    cloud_smooth(
        0.3,
        0.65,
        humidity + (amount - DEFAULT_CLOUD_COVERAGE) * 1.5,
    ) * 0.9
}

/// JavaScript's Math.round on a non-negative value.
fn round_byte(v: f64) -> u8 {
    (v + 0.5).floor() as u8
}

/// The atlas's direction for texel (x, y): longitude wraps, the pole rows are constant.
pub fn weather_direction(x: usize, y: usize) -> DVec3 {
    let latitude = std::f64::consts::PI * (y as f64 / (WEATHER_HEIGHT - 1) as f64 - 0.5);
    let longitude = std::f64::consts::PI * (2.0 * x as f64 / WEATHER_WIDTH as f64 - 1.0);
    DVec3::new(
        cos(latitude) * cos(longitude),
        cos(latitude) * sin(longitude),
        sin(latitude),
    )
}

/// RGBA8: R = humidity, G = vertical type (thin stratiform → deep cumulus), A = 255. Rows build in
/// parallel on `threads` threads.
pub fn build_cloud_weather(threads: usize) -> Vec<u8> {
    let mut data = vec![0u8; WEATHER_WIDTH * WEATHER_HEIGHT * 4];
    let rows_per = WEATHER_HEIGHT.div_ceil(threads.max(1));
    std::thread::scope(|scope| {
        for (chunk, rows) in data.chunks_mut(rows_per * WEATHER_WIDTH * 4).enumerate() {
            scope.spawn(move || {
                for (row, texels) in rows.chunks_mut(WEATHER_WIDTH * 4).enumerate() {
                    let y = chunk * rows_per + row;
                    for x in 0..WEATHER_WIDTH {
                        let (humidity, kind) = cloud_weather(weather_direction(x, y));
                        texels[x * 4..x * 4 + 4].copy_from_slice(&[
                            round_byte(humidity * 255.0),
                            round_byte(kind * 255.0),
                            0,
                            255,
                        ]);
                    }
                }
            });
        }
    });
    data
}

fn hash(x: i64, y: i64, z: i64, period: i64, seed: i32) -> f64 {
    let wrap = |v: i64| v.rem_euclid(period) as i32;
    let mut h = wrap(x).wrapping_mul(374761393)
        ^ wrap(y).wrapping_mul(668265263)
        ^ wrap(z).wrapping_mul(1442695041)
        ^ seed.wrapping_mul(1597334677);
    h = (h ^ ((h as u32) >> 13) as i32).wrapping_mul(1274126177);
    ((h ^ ((h as u32) >> 16) as i32) as u32) as f64 / 4294967296.0
}

fn corner_weight(d: i64, t: f64) -> f64 {
    if d == 1 { t } else { 1.0 - t }
}

fn value_noise(x: f64, y: f64, z: f64, period: i64) -> f64 {
    let (ix, iy, iz) = (x.floor() as i64, y.floor() as i64, z.floor() as i64);
    let f = |v: f64| {
        let t = v - v.floor();
        t * t * (3.0 - 2.0 * t)
    };
    let (u, v, w) = (f(x), f(y), f(z));
    let mut sum = 0.0;
    for dz in 0..=1 {
        for dy in 0..=1 {
            for dx in 0..=1 {
                sum += hash(ix + dx, iy + dy, iz + dz, period, 1)
                    * corner_weight(dx, u)
                    * corner_weight(dy, v)
                    * corner_weight(dz, w);
            }
        }
    }
    sum
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

/// Periodic gradient Perlin, remapped to 0–1. Feature gradients wrap across the volume boundary.
fn perlin_noise(x: f64, y: f64, z: f64, period: i64) -> f64 {
    let (ix, iy, iz) = (x.floor() as i64, y.floor() as i64, z.floor() as i64);
    let (fx, fy, fz) = (x - ix as f64, y - iy as f64, z - iz as f64);
    let fade = |t: f64| t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
    let (u, v, w) = (fade(fx), fade(fy), fade(fz));
    let mut sum = 0.0;
    for dz in 0..=1 {
        for dy in 0..=1 {
            for dx in 0..=1 {
                let g =
                    GRADIENTS[(hash(ix + dx, iy + dy, iz + dz, period, 5) * 12.0).floor() as usize];
                sum +=
                    (g[0] * (fx - dx as f64) + g[1] * (fy - dy as f64) + g[2] * (fz - dz as f64))
                        * std::f64::consts::FRAC_1_SQRT_2
                        * corner_weight(dx, u)
                        * corner_weight(dy, v)
                        * corner_weight(dz, w);
            }
        }
    }
    clamp01(0.5 + sum)
}

/// Inverted periodic Worley F1, wrapping feature cells as well as texture coordinates.
fn worley(x: f64, y: f64, z: f64, period: i64) -> f64 {
    let (ix, iy, iz) = (x.floor() as i64, y.floor() as i64, z.floor() as i64);
    let mut nearest = f64::INFINITY;
    for dz in -1..=1 {
        for dy in -1..=1 {
            for dx in -1..=1 {
                let (cx, cy, cz) = (ix + dx, iy + dy, iz + dz);
                let a = cx as f64 + hash(cx, cy, cz, period, 2) - x;
                let b = cy as f64 + hash(cx, cy, cz, period, 3) - y;
                let c = cz as f64 + hash(cx, cy, cz, period, 4) - z;
                nearest = nearest.min(a * a + b * b + c * c);
            }
        }
    }
    1.0 - clamp01(nearest.sqrt())
}

/// A repeating RGBA8 noise volume, `size`³, samples at texel centres (linear repeat filtering
/// therefore agrees at the seam). Shape (`detail` false): R Perlin–Worley, G fine value noise, B a
/// smooth low-frequency field for regional banks. Detail: R Worley.
pub fn build_cloud_noise(size: usize, detail: bool) -> Vec<u8> {
    let mut data = vec![0u8; size * size * size * 4];
    let cells: i64 = if detail { 4 } else { 8 };
    let c = cells as f64;
    std::thread::scope(|scope| {
        for (z, slab) in data.chunks_mut(size * size * 4).enumerate() {
            scope.spawn(move || {
                for y in 0..size {
                    for x in 0..size {
                        let s = size as f64;
                        let (qx, qy, qz) = (
                            (x as f64 + 0.5) / s,
                            (y as f64 + 0.5) / s,
                            (z as f64 + 0.5) / s,
                        );
                        let w = worley(qx * c, qy * c, qz * c, cells);
                        let perlin = perlin_noise(qx * c, qy * c, qz * c, cells);
                        let fine = value_noise(qx * c * 2.0, qy * c * 2.0, qz * c * 2.0, cells * 2);
                        let i = (y * size + x) * 4;
                        slab[i] = round_byte(
                            clamp01(if detail {
                                w
                            } else {
                                (0.65 * perlin + 0.35 * w - 0.25) / 0.5
                            }) * 255.0,
                        );
                        slab[i + 1] = round_byte(fine * 255.0);
                        // A separate smooth, low-frequency field organizes regional cloud banks.
                        slab[i + 2] =
                            round_byte(perlin_noise(qx * 2.0, qy * 2.0, qz * 2.0, 2) * 255.0);
                        slab[i + 3] = 255;
                    }
                }
            });
        }
    });
    data
}

/// CPU reference for the GPU's repeating, trilinear 3D lookup.
pub fn sample_cloud_noise(data: &[u8], size: usize, p: DVec3, period: f64, channel: usize) -> f64 {
    let coordinate = |v: f64| v / period * size as f64 - 0.5;
    let q = [coordinate(p.x), coordinate(p.y), coordinate(p.z)];
    let wrap = |v: i64| v.rem_euclid(size as i64) as usize;
    let f = q.map(|v| v - v.floor());
    let mut sum = 0.0;
    for z in 0..=1i64 {
        for y in 0..=1i64 {
            for x in 0..=1i64 {
                let i = [
                    wrap(q[0].floor() as i64 + x),
                    wrap(q[1].floor() as i64 + y),
                    wrap(q[2].floor() as i64 + z),
                ];
                sum += f64::from(data[((i[2] * size + i[1]) * size + i[0]) * 4 + channel]) / 255.0
                    * corner_weight(x, f[0])
                    * corner_weight(y, f[1])
                    * corner_weight(z, f[2]);
            }
        }
    }
    sum
}

/// Inputs to `cloud_density` beyond the height and the three samples.
#[derive(Clone, Copy, Debug)]
pub struct DensityOptions {
    pub amount: f64,
    pub detail_weight: f64,
    pub footprint: f64,
    pub macro_shape: f64,
}

impl Default for DensityOptions {
    fn default() -> Self {
        Self {
            amount: DEFAULT_CLOUD_COVERAGE,
            detail_weight: 1.0,
            footprint: 0.0,
            macro_shape: 1.0,
        }
    }
}

/// Same remap and profile as the shader's density, for independent density and optical-depth checks.
pub fn cloud_density(
    height_asl: f64,
    humidity: f64,
    kind: f64,
    shape: f64,
    detail: f64,
    o: DensityOptions,
) -> f64 {
    let bank = cloud_smooth(0.15, 0.7, o.macro_shape);
    let unresolved = cloud_smooth(2000.0, 16000.0, o.footprint);
    let top_shape = shape * (1.0 - unresolved) + 0.5 * unresolved;
    let top =
        CLOUD_BOTTOM + (2000.0 + 4500.0 * kind) * (0.2 + 0.8 * bank) * (0.45 + 0.55 * top_shape);
    let h = (height_asl - CLOUD_BOTTOM) / (top - CLOUD_BOTTOM);
    let profile = cloud_smooth(0.0, 0.08, h) * (1.0 - cloud_smooth(0.35, 1.0, h));
    let coverage = weather_coverage(humidity, o.amount);
    let cells = clamp01((shape - h * h * 0.25 - (1.0 - coverage)) / coverage.max(0.001));
    // Moist systems join into sheets; dry margins retain separate cumulus cells.
    let sheet = cloud_smooth(0.55, 0.85, coverage) * (1.0 - 0.6 * kind);
    let base = cells * (1.0 - sheet) + 0.32 * coverage * sheet;
    let filtered_base = base * (1.0 - unresolved)
        + (pow(coverage, 3.0) * 0.45 * (1.0 - sheet) + 0.32 * coverage * sheet)
            * clamp01(1.0 - h * h * 0.6)
            * unresolved;
    clamp01(filtered_base * profile - (1.0 - detail) * 0.16 * o.detail_weight)
        * (0.45 + 0.55 * kind)
        * cloud_smooth(0.05, 0.5, bank)
}

/// Exact shell intervals; includes the far segment when a ray passes through the hollow interior.
pub fn cloud_shell_intervals(
    origin: DVec3,
    direction: DVec3,
    inner: f64,
    outer: f64,
    scene_distance: f64,
) -> Vec<(f64, f64)> {
    let r = hypot([origin.x, origin.y, origin.z]);
    let mu = (origin.x * direction.x + origin.y * direction.y + origin.z * direction.z) / r;
    let roots = |radius: f64| {
        let altitude = r - radius;
        let d = r * r * mu * mu - altitude * (2.0 * radius + altitude);
        (d >= 0.0).then(|| (-r * mu - d.sqrt(), -r * mu + d.sqrt()))
    };
    let Some(outside) = roots(outer) else {
        return Vec::new();
    };
    let (start, end) = (outside.0.max(0.0), scene_distance.min(outside.1));
    // Written as the TS does, so a NaN end also gives no interval.
    #[allow(clippy::neg_cmp_op_on_partial_ord)]
    if !(end > start) {
        return Vec::new();
    }
    let Some(inside) = roots(inner) else {
        return vec![(start, end)];
    };
    [(start, end.min(inside.0)), (start.max(inside.1), end)]
        .into_iter()
        .filter(|(a, b)| b > a)
        .collect()
}
