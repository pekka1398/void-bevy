//! A planet's atmosphere as single scattering: Rayleigh (air), Mie (haze) and ozone absorption,
//! each with its own density profile. Everything here is in metres and runs on the CPU: the
//! transmittance table uploaded to the GPU, and reference versions of what the shaders compute.
//!
//! The parameters and the table layout follow Hillaire's "A Scalable and Production Ready Sky and
//! Atmosphere Rendering Technique" (2020), which uses Bruneton's Earth values.

use glam::DVec3;

pub type Rgb = [f64; 3];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AtmosphereParams {
    /// The ground sphere the air sits on (the planet's reference radius), metres.
    pub bottom_radius: f64,
    /// Where the air is taken to end, metres from the centre.
    pub top_radius: f64,
    /// Scattering at the bottom, per metre, for red, green and blue.
    pub rayleigh_scattering: Rgb,
    pub rayleigh_scale_height: f64,
    pub mie_scattering: f64,
    /// Mie scatters most of what it removes; the rest is absorbed.
    pub mie_extinction: f64,
    pub mie_scale_height: f64,
    /// Mie phase asymmetry: 0 scatters evenly, near 1 mostly forward.
    pub mie_anisotropy: f64,
    /// Ozone absorption at the peak of its layer, per metre.
    pub ozone_absorption: Rgb,
    /// Ozone density is a tent: 1 at this height, 0 at half the width above and below.
    pub ozone_center_height: f64,
    pub ozone_width: f64,
}

/// Earth's air over a planet of the given radius, 100 km deep.
pub fn earth_like_atmosphere(bottom_radius: f64) -> AtmosphereParams {
    assert!(
        bottom_radius > 0.0,
        "earth_like_atmosphere: bottom_radius {bottom_radius}"
    );
    AtmosphereParams {
        bottom_radius,
        top_radius: bottom_radius + 100e3,
        rayleigh_scattering: [5.802e-6, 13.558e-6, 33.1e-6],
        rayleigh_scale_height: 8e3,
        mie_scattering: 3.996e-6,
        mie_extinction: 4.44e-6,
        mie_scale_height: 1.2e3,
        mie_anisotropy: 0.8,
        ozone_absorption: [0.65e-6, 1.881e-6, 0.085e-6],
        ozone_center_height: 25e3,
        ozone_width: 30e3,
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Densities {
    pub rayleigh: f64,
    pub mie: f64,
    pub ozone: f64,
}

/// Relative density of each constituent at a height above the bottom radius.
pub fn densities_at(p: &AtmosphereParams, height: f64) -> Densities {
    Densities {
        rayleigh: f64::exp(-height / p.rayleigh_scale_height),
        mie: f64::exp(-height / p.mie_scale_height),
        ozone: (1.0 - (height - p.ozone_center_height).abs() / (p.ozone_width / 2.0)).max(0.0),
    }
}

/// Extinction (scattering plus absorption) per metre at a height.
pub fn extinction_at(p: &AtmosphereParams, height: f64) -> Rgb {
    let d = densities_at(p, height);
    std::array::from_fn(|c| {
        p.rayleigh_scattering[c] * d.rayleigh
            + p.mie_extinction * d.mie
            + p.ozone_absorption[c] * d.ozone
    })
}

/// Distance along a ray from radius r with zenith cosine mu to where it leaves a sphere of `radius`;
/// the ray starts inside it.
pub fn distance_to_sphere_exit(r: f64, mu: f64, radius: f64) -> f64 {
    let discriminant = r * r * (mu * mu - 1.0) + radius * radius;
    assert!(
        discriminant >= 0.0,
        "distance_to_sphere_exit: r={r} is outside radius={radius}"
    );
    (-r * mu + discriminant.sqrt()).max(0.0)
}

/// Whether a ray from radius r with zenith cosine mu hits the bottom sphere.
pub fn ray_hits_ground(p: &AtmosphereParams, r: f64, mu: f64) -> bool {
    mu < 0.0 && r * r * (mu * mu - 1.0) + p.bottom_radius * p.bottom_radius >= 0.0
}

const TRANSMITTANCE_STEPS: usize = 120;

/// Transmittance from radius r along zenith cosine mu to the top of the air, by the midpoint rule.
/// Only for rays that miss the ground.
pub fn transmittance_to_top(p: &AtmosphereParams, r: f64, mu: f64) -> Rgb {
    assert!(
        r >= p.bottom_radius && r <= p.top_radius && (-1.0..=1.0).contains(&mu),
        "transmittance_to_top: r={r}, mu={mu}"
    );
    assert!(
        !ray_hits_ground(p, r, mu),
        "transmittance_to_top: the ray from r={r}, mu={mu} hits the ground"
    );
    let length = distance_to_sphere_exit(r, mu, p.top_radius);
    let dt = length / TRANSMITTANCE_STEPS as f64;
    let mut depth = [0.0; 3];
    for i in 0..TRANSMITTANCE_STEPS {
        let t = (i as f64 + 0.5) * dt;
        let height = (r * r + 2.0 * r * mu * t + t * t).sqrt() - p.bottom_radius;
        let e = extinction_at(p, height);
        for c in 0..3 {
            depth[c] += e[c] * dt;
        }
    }
    depth.map(|d| f64::exp(-d))
}

/// The transmittance table: 256 zenith cosines by 64 heights, Hillaire's layout. x is where the ray
/// leaves the air, between straight up and the horizon; y is the distance to the horizon.
pub const TRANSMITTANCE_WIDTH: usize = 256;
pub const TRANSMITTANCE_HEIGHT: usize = 64;

/// Table coordinates (0–1 at the first and last texel centres) of a ray that misses the ground.
pub fn transmittance_coords(p: &AtmosphereParams, r: f64, mu: f64) -> (f64, f64) {
    let horizon = (p.top_radius * p.top_radius - p.bottom_radius * p.bottom_radius).sqrt();
    let rho = (r * r - p.bottom_radius * p.bottom_radius).max(0.0).sqrt();
    let d = distance_to_sphere_exit(r, mu, p.top_radius);
    let d_min = p.top_radius - r;
    let d_max = rho + horizon;
    ((d - d_min) / (d_max - d_min), rho / horizon)
}

/// Inverse of `transmittance_coords`: (r, mu).
pub fn transmittance_ray(p: &AtmosphereParams, x: f64, y: f64) -> (f64, f64) {
    let horizon = (p.top_radius * p.top_radius - p.bottom_radius * p.bottom_radius).sqrt();
    let rho = horizon * y;
    let r = (rho * rho + p.bottom_radius * p.bottom_radius).sqrt();
    let d_min = p.top_radius - r;
    let d_max = rho + horizon;
    let d = d_min + x * (d_max - d_min);
    let mu = if d == 0.0 {
        1.0
    } else {
        (horizon * horizon - rho * rho - d * d) / (2.0 * r * d)
    };
    (r, mu.clamp(-1.0, 1.0))
}

/// RGBA float texels, row by row from y = 0; alpha is 1.
pub fn build_transmittance_table(p: &AtmosphereParams) -> Vec<f32> {
    let mut data = vec![0f32; TRANSMITTANCE_WIDTH * TRANSMITTANCE_HEIGHT * 4];
    for j in 0..TRANSMITTANCE_HEIGHT {
        for i in 0..TRANSMITTANCE_WIDTH {
            let (r, mu) = transmittance_ray(
                p,
                i as f64 / (TRANSMITTANCE_WIDTH - 1) as f64,
                j as f64 / (TRANSMITTANCE_HEIGHT - 1) as f64,
            );
            // The horizon column of the bottom row grazes the ground; nudge it up by the float error of the inverse.
            let t = transmittance_to_top(
                p,
                r,
                if ray_hits_ground(p, r, mu) {
                    mu + 1e-12
                } else {
                    mu
                },
            );
            let k = (j * TRANSMITTANCE_WIDTH + i) * 4;
            data[k..k + 4].copy_from_slice(&[t[0] as f32, t[1] as f32, t[2] as f32, 1.0]);
        }
    }
    data
}

pub fn rayleigh_phase(cos_theta: f64) -> f64 {
    (3.0 / (16.0 * std::f64::consts::PI)) * (1.0 + cos_theta * cos_theta)
}

/// Cornette–Shanks phase function.
pub fn mie_phase(g: f64, cos_theta: f64) -> f64 {
    let k = (3.0 / (8.0 * std::f64::consts::PI)) * ((1.0 - g * g) / (2.0 + g * g));
    (k * (1.0 + cos_theta * cos_theta)) / f64::powf(1.0 + g * g - 2.0 * g * cos_theta, 1.5)
}

/// Light scattered toward a viewer at `altitude` above the bottom radius, looking along unit
/// `direction` in the viewer's local frame (z up), from a sun of illuminance 1 along unit `sun`:
/// the reference for the sky shader, at many more steps. Stops at the ground or the top of the air.
pub fn sky_radiance(
    p: &AtmosphereParams,
    altitude: f64,
    direction: DVec3,
    sun: DVec3,
    steps: usize,
) -> Rgb {
    let r0 = p.bottom_radius + altitude;
    assert!(
        r0 <= p.top_radius,
        "sky_radiance: altitude {altitude} is above the air"
    );
    let mu = direction.z;
    let length = if ray_hits_ground(p, r0, mu) {
        -r0 * mu - (r0 * r0 * (mu * mu - 1.0) + p.bottom_radius * p.bottom_radius).sqrt()
    } else {
        distance_to_sphere_exit(r0, mu, p.top_radius)
    };
    let cos_theta = direction.x * sun.x + direction.y * sun.y + direction.z * sun.z;
    let phase_r = rayleigh_phase(cos_theta);
    let phase_m = mie_phase(p.mie_anisotropy, cos_theta);
    let dt = length / steps as f64;
    let mut radiance = [0.0; 3];
    let mut depth = [0.0; 3];
    for i in 0..steps {
        let t = (i as f64 + 0.5) * dt;
        let (px, py, pz) = (direction.x * t, direction.y * t, r0 + direction.z * t);
        let r = DVec3::new(px, py, pz).length();
        let height = r - p.bottom_radius;
        let e = extinction_at(p, height);
        let d = densities_at(p, height);
        let sun_mu = (px * sun.x + py * sun.y + pz * sun.z) / r;
        let to_sun = if ray_hits_ground(p, r, sun_mu) {
            [0.0; 3]
        } else {
            transmittance_to_top(p, r.min(p.top_radius), sun_mu)
        };
        for c in 0..3 {
            let view_t = f64::exp(-(depth[c] + e[c] * dt / 2.0));
            let scattering = p.rayleigh_scattering[c] * d.rayleigh * phase_r
                + p.mie_scattering * d.mie * phase_m;
            radiance[c] += view_t * scattering * to_sun[c] * dt;
            depth[c] += e[c] * dt;
        }
    }
    radiance
}
