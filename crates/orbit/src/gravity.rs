//! The one gravity law vessels feel: each body's point mass plus its zonal J2 about the spin axis.
//! The propagator, the ground and free-fall frames, multiscale and the environment all call it;
//! each adds its own frame's terms (origin acceleration, tides, centrifugal, Coriolis) itself.
use crate::CelestialBody;
use glam::DVec3;

/// 1.5 J2 GM R², the zonal coefficient `pull` takes; 0 for a point mass.
pub fn oblateness(body: &CelestialBody) -> f64 {
    1.5 * body.j2 * body.gm * body.j2_reference_radius_meters.powi(2)
}

/// Acceleration from a body of `gm` and `oblateness` `c` at `r` = point − body centre, with `axis`
/// the body's unit spin axis in the same axes as `r`:
/// a = −GM r / r³ + c / r⁵ [(5 u² / r² − 1) r − 2 u k], u = r·k.
#[inline]
pub fn pull(gm: f64, c: f64, axis: DVec3, r: DVec3) -> DVec3 {
    let mut a = DVec3::ZERO;
    add_pull(&mut a, gm, c, axis, r);
    a
}

/// `a += pull(gm, c, axis, r)`, adding the point mass and then the bulge.
#[inline]
pub fn add_pull(a: &mut DVec3, gm: f64, c: f64, axis: DVec3, r: DVec3) {
    let r2 = r.x * r.x + r.y * r.y + r.z * r.z;
    let rl = r2.sqrt();
    let s = gm / (r2 * rl);
    *a += -r * s;
    if c != 0.0 {
        let u = r.x * axis.x + r.y * axis.y + r.z * axis.z;
        let f = c / (r2 * r2 * rl);
        let radial = f * (5.0 * u * u / r2 - 1.0);
        *a += r * radial - axis * (2.0 * f * u);
    }
}

/// `pull` for one of the ephemeris's bodies, `r` and the result in ephemeris (ecliptic) axes.
pub fn body_pull(body: &CelestialBody, r: DVec3) -> DVec3 {
    pull(body.gm, oblateness(body), body.rotation.axis(), r)
}
