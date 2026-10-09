//! The tables built on the CPU from the transmittance table, after Hillaire 2020: multiple
//! scattering, and the sky's irradiance on the ground. Both are per unit sun illuminance and take
//! (height, sun zenith cosine).
//!
//! Layout of both: x = (mu_s + 1) / 2, y = sqrt(height / air depth), texel centres at 0 and 1 (the
//! square root packs rows toward the ground, where both change fastest). RGBA, alpha 1.

use glam::DVec3;

use crate::atmosphere::{
    AtmosphereParams, Rgb, TRANSMITTANCE_HEIGHT, TRANSMITTANCE_WIDTH, densities_at, extinction_at,
    mie_phase, ray_hits_ground, rayleigh_phase, transmittance_coords,
};

pub const MULTIPLE_SCATTERING_SIZE: usize = 32;
pub const IRRADIANCE_WIDTH: usize = 32;
pub const IRRADIANCE_HEIGHT: usize = 16;
/// Ground albedo the multiple-scattering bounce assumes.
pub const GROUND_ALBEDO: f64 = 0.3;

/// Bilinear lookup of the transmittance table, as the GPU filters it.
pub fn transmittance_lookup(table: &[f32], p: &AtmosphereParams, r: f64, mu: f64) -> Rgb {
    let (x, y) = transmittance_coords(p, r.min(p.top_radius), mu);
    bilinear(table, TRANSMITTANCE_WIDTH, TRANSMITTANCE_HEIGHT, x, y)
}

/// Sunlight reaching radius r with the sun at zenith cosine mu: zero below the ground's horizon
/// (hard edge on the CPU).
pub fn sunlight_at(table: &[f32], p: &AtmosphereParams, r: f64, mu: f64) -> Rgb {
    if ray_hits_ground(p, r, mu) {
        [0.0; 3]
    } else {
        transmittance_lookup(table, p, r, mu)
    }
}

pub fn table_coords(p: &AtmosphereParams, r: f64, sun_mu: f64) -> (f64, f64) {
    let height = (r - p.bottom_radius)
        .max(0.0)
        .min(p.top_radius - p.bottom_radius);
    (
        (sun_mu.clamp(-1.0, 1.0) + 1.0) / 2.0,
        (height / (p.top_radius - p.bottom_radius)).sqrt(),
    )
}

pub fn multiple_scattering_lookup(table: &[f32], p: &AtmosphereParams, r: f64, sun_mu: f64) -> Rgb {
    let (x, y) = table_coords(p, r, sun_mu);
    bilinear(
        table,
        MULTIPLE_SCATTERING_SIZE,
        MULTIPLE_SCATTERING_SIZE,
        x,
        y,
    )
}

pub fn irradiance_lookup(table: &[f32], p: &AtmosphereParams, r: f64, sun_mu: f64) -> Rgb {
    let (x, y) = table_coords(p, r, sun_mu);
    bilinear(table, IRRADIANCE_WIDTH, IRRADIANCE_HEIGHT, x, y)
}

/// Directions spread evenly over the sphere (a Fibonacci lattice).
pub fn sphere_directions(count: usize) -> Vec<DVec3> {
    let golden = std::f64::consts::PI * (3.0 - 5f64.sqrt());
    (0..count)
        .map(|i| {
            let z = 1.0 - (2.0 * (i as f64 + 0.5)) / count as f64;
            let s = (1.0 - z * z).sqrt();
            DVec3::new(
                s * f64::cos(golden * i as f64),
                s * f64::sin(golden * i as f64),
                z,
            )
        })
        .collect()
}

fn sun_at(sun_mu: f64) -> DVec3 {
    DVec3::new((1.0 - sun_mu * sun_mu).max(0.0).sqrt(), 0.0, sun_mu)
}

/// Hillaire's multiple-scattering table Ψ: light scattered twice or more, as an isotropic source
/// per unit scattering coefficient. For each height and sun angle: the second-order light L2
/// arriving from every direction (single scattering with an isotropic phase, plus the ground's
/// bounce), and the share f of light the air around re-scatters; all orders sum to L2 / (1 − f).
pub fn build_multiple_scattering_table(
    p: &AtmosphereParams,
    transmittance: &[f32],
    direction_count: usize,
    steps: usize,
) -> Vec<f32> {
    let n = MULTIPLE_SCATTERING_SIZE;
    let mut data = vec![0f32; n * n * 4];
    let directions = sphere_directions(direction_count);
    let count = directions.len() as f64;
    let isotropic = 1.0 / (4.0 * std::f64::consts::PI);
    for j in 0..n {
        for i in 0..n {
            let sun_mu = (i as f64 / (n - 1) as f64) * 2.0 - 1.0;
            let y = j as f64 / (n - 1) as f64;
            let height = y * y * (p.top_radius - p.bottom_radius);
            let r0 = p.bottom_radius + height.min(p.top_radius - p.bottom_radius - 1.0);
            let sun = sun_at(sun_mu);
            let mut second = [0.0; 3];
            let mut transfer = [0.0; 3];
            for direction in &directions {
                let (length, hits_ground) = ray_length(p, r0, direction.z);
                let dt = length / steps as f64;
                let mut t3 = [1.0; 3];
                for s in 0..steps {
                    let t = (s as f64 + 0.5) * dt;
                    let (px, py, pz) = (direction.x * t, direction.y * t, r0 + direction.z * t);
                    let r = DVec3::new(px, py, pz).length();
                    let h = r - p.bottom_radius;
                    let d = densities_at(p, h);
                    let e = extinction_at(p, h);
                    let sunlight = sunlight_at(
                        transmittance,
                        p,
                        r,
                        (px * sun.x + py * sun.y + pz * sun.z) / r,
                    );
                    for c in 0..3 {
                        let scattering =
                            p.rayleigh_scattering[c] * d.rayleigh + p.mie_scattering * d.mie;
                        let step = f64::exp(-e[c] * dt);
                        // Integral of f64::exp(-extinction * s) over this step. In vacuum its
                        // exact limit is dt; the quotient would otherwise evaluate 0/0.
                        let absorbed = if e[c] == 0.0 { dt } else { (1.0 - step) / e[c] };
                        second[c] +=
                            (t3[c] * scattering * sunlight[c] * isotropic * absorbed) / count;
                        transfer[c] += (t3[c] * scattering * absorbed) / count;
                        t3[c] *= step;
                    }
                }
                if hits_ground {
                    let (px, py, pz) = (
                        direction.x * length,
                        direction.y * length,
                        r0 + direction.z * length,
                    );
                    let r = DVec3::new(px, py, pz).length();
                    let ground_sun_mu = (px * sun.x + py * sun.y + pz * sun.z) / r;
                    let sunlight = sunlight_at(transmittance, p, p.bottom_radius, ground_sun_mu);
                    for c in 0..3 {
                        second[c] += (t3[c]
                            * sunlight[c]
                            * ground_sun_mu.max(0.0)
                            * (GROUND_ALBEDO / std::f64::consts::PI))
                            / count;
                    }
                }
            }
            let k = (j * n + i) * 4;
            for c in 0..3 {
                assert!(
                    transfer[c] < 1.0,
                    "build_multiple_scattering_table: transfer {} would not converge",
                    transfer[c]
                );
                data[k + c] = (second[c] / (1.0 - transfer[c])) as f32;
            }
            data[k + 3] = 1.0;
        }
    }
    data
}

/// What `march_sky` returns: scattered light and the transmittance along the ray.
#[derive(Clone, Copy, Debug)]
pub struct SkyMarch {
    pub radiance: Rgb,
    pub transmittance: Rgb,
}

/// The sky shader's march on the CPU: `steps` samples crowded toward the viewer, Hillaire's step
/// integral, sunlight from the transmittance table and, when a multiple-scattering table is given,
/// its isotropic term. Per unit sun illuminance, from radius r0 (viewer's up is +z) along unit
/// `direction`.
pub fn march_sky(
    p: &AtmosphereParams,
    transmittance: &[f32],
    multiple: Option<&[f32]>,
    r0: f64,
    direction: DVec3,
    sun: DVec3,
    steps: usize,
) -> SkyMarch {
    let (length, _) = ray_length(p, r0, direction.z);
    let cos_theta = direction.x * sun.x + direction.y * sun.y + direction.z * sun.z;
    let (phase_r, phase_m) = (
        rayleigh_phase(cos_theta),
        mie_phase(p.mie_anisotropy, cos_theta),
    );
    let mut t3 = [1.0; 3];
    let mut radiance = [0.0; 3];
    let n = steps as f64;
    for i in 0..steps {
        let (s0, s1, sm) = (i as f64 / n, (i as f64 + 1.0) / n, (i as f64 + 0.5) / n);
        let (t, dt) = (length * sm * sm, length * (s1 * s1 - s0 * s0));
        let r = (r0 * r0 + 2.0 * r0 * direction.z * t + t * t).sqrt();
        let height = (r - p.bottom_radius).max(0.0);
        let d = densities_at(p, height);
        let e = extinction_at(p, height);
        let sun_mu = (r0 * sun.z + t * cos_theta) / r;
        let sunlight = sunlight_at(transmittance, p, r, sun_mu);
        let ms = multiple.map_or([0.0; 3], |m| multiple_scattering_lookup(m, p, r, sun_mu));
        for c in 0..3 {
            let (rayleigh, mie) = (
                p.rayleigh_scattering[c] * d.rayleigh,
                p.mie_scattering * d.mie,
            );
            let source =
                (rayleigh * phase_r + mie * phase_m) * sunlight[c] + (rayleigh + mie) * ms[c];
            let step = f64::exp(-e[c] * dt);
            radiance[c] += if e[c] == 0.0 {
                t3[c] * source * dt
            } else {
                (t3[c] * (source - source * step)) / e[c]
            };
            t3[c] *= step;
        }
    }
    SkyMarch {
        radiance,
        transmittance: t3,
    }
}

/// Irradiance from the whole sky (not the sun's direct beam) on level ground at each height and sun
/// angle, per unit sun illuminance: the sky's radiance, with multiple scattering, weighted by the
/// cosine over the upper hemisphere.
pub fn build_irradiance_table(
    p: &AtmosphereParams,
    transmittance: &[f32],
    multiple: &[f32],
    direction_count: usize,
    steps: usize,
) -> Vec<f32> {
    let mut data = vec![0f32; IRRADIANCE_WIDTH * IRRADIANCE_HEIGHT * 4];
    let upper: Vec<DVec3> = sphere_directions(direction_count)
        .into_iter()
        .filter(|d| d.z > 0.0)
        .collect();
    let solid_angle = (2.0 * std::f64::consts::PI) / upper.len() as f64;
    for j in 0..IRRADIANCE_HEIGHT {
        for i in 0..IRRADIANCE_WIDTH {
            let sun_mu = (i as f64 / (IRRADIANCE_WIDTH - 1) as f64) * 2.0 - 1.0;
            let y = j as f64 / (IRRADIANCE_HEIGHT - 1) as f64;
            let height = y * y * (p.top_radius - p.bottom_radius);
            let r0 = p.bottom_radius + height.min(p.top_radius - p.bottom_radius - 1.0);
            let sun = sun_at(sun_mu);
            let mut sum = [0.0; 3];
            for direction in &upper {
                let SkyMarch { radiance, .. } =
                    march_sky(p, transmittance, Some(multiple), r0, *direction, sun, steps);
                for c in 0..3 {
                    sum[c] += radiance[c] * direction.z * solid_angle;
                }
            }
            let k = (j * IRRADIANCE_WIDTH + i) * 4;
            data[k..k + 4].copy_from_slice(&[sum[0] as f32, sum[1] as f32, sum[2] as f32, 1.0]);
        }
    }
    data
}

/// Length of a ray from radius r0 with zenith cosine mu to the ground or the top of the air, and
/// whether it ends at the ground.
pub fn ray_length(p: &AtmosphereParams, r0: f64, mu: f64) -> (f64, bool) {
    if ray_hits_ground(p, r0, mu) {
        let d = (r0 * r0 * (mu * mu - 1.0) + p.bottom_radius * p.bottom_radius).max(0.0);
        return (-r0 * mu - d.sqrt(), true);
    }
    (
        -r0 * mu + (r0 * r0 * (mu * mu - 1.0) + p.top_radius * p.top_radius).sqrt(),
        false,
    )
}

fn bilinear(table: &[f32], width: usize, height: usize, x: f64, y: f64) -> Rgb {
    let fx = x.clamp(0.0, 1.0) * (width - 1) as f64;
    let fy = y.clamp(0.0, 1.0) * (height - 1) as f64;
    let i = (width - 2).min(fx.floor() as usize);
    let j = (height - 2).min(fy.floor() as usize);
    let (u, v) = (fx - i as f64, fy - j as f64);
    let at = |a: usize, b: usize, c: usize| f64::from(table[(b * width + a) * 4 + c]);
    std::array::from_fn(|c| {
        (1.0 - v) * ((1.0 - u) * at(i, j, c) + u * at(i + 1, j, c))
            + v * ((1.0 - u) * at(i, j + 1, c) + u * at(i + 1, j + 1, c))
    })
}
