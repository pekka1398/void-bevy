use std::f64::consts::{PI, TAU};

use glam::DVec3;
use serde::Deserialize;

/// Closed elliptic orbit, angles in radians, referred to the ecliptic frame.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EllipticElements {
    pub semi_major_axis_meters: f64,
    pub eccentricity: f64,
    pub inclination_radians: f64,
    pub longitude_of_ascending_node_radians: f64,
    pub argument_of_periapsis_radians: f64,
    pub mean_anomaly_radians: f64,
}

impl EllipticElements {
    pub fn assert_valid(&self, label: &str) {
        let values = [
            self.semi_major_axis_meters,
            self.eccentricity,
            self.inclination_radians,
            self.longitude_of_ascending_node_radians,
            self.argument_of_periapsis_radians,
            self.mean_anomaly_radians,
        ];
        assert!(
            values.iter().all(|v| v.is_finite()),
            "{label}: non-finite orbital element"
        );
        assert!(
            self.semi_major_axis_meters > 0.0,
            "{label}: semi-major axis {} must be positive",
            self.semi_major_axis_meters
        );
        assert!(
            (0.0..1.0).contains(&self.eccentricity),
            "{label}: eccentricity {} is not elliptic",
            self.eccentricity
        );
        assert!(
            (0.0..=PI).contains(&self.inclination_radians),
            "{label}: inclination {} outside [0, pi]",
            self.inclination_radians
        );
    }
}

pub fn orbital_period_seconds(semi_major_axis_meters: f64, gm: f64) -> f64 {
    assert!(
        semi_major_axis_meters > 0.0 && gm > 0.0,
        "orbital period: a = {semi_major_axis_meters}, gm = {gm}"
    );
    TAU * (semi_major_axis_meters.powi(3) / gm).sqrt()
}

/// Solve M = E - e sin E for 0 <= e < 1. Non-convergence is a bug, not a case to paper over.
pub fn solve_kepler_elliptic(mean_anomaly_radians: f64, eccentricity: f64) -> f64 {
    assert!(
        mean_anomaly_radians.is_finite(),
        "solve Kepler: M = {mean_anomaly_radians}"
    );
    assert!(
        (0.0..1.0).contains(&eccentricity),
        "solve Kepler: e = {eccentricity} is not elliptic"
    );
    let m = mean_anomaly_radians - TAU * (mean_anomaly_radians / TAU).floor();
    // Starting at pi for high eccentricity keeps Newton monotone (Danby 1987).
    let mut e = if eccentricity < 0.8 { m } else { PI };
    for _ in 0..64 {
        let f = e - eccentricity * e.sin() - m;
        let df = 1.0 - eccentricity * e.cos();
        let step = f / df;
        e -= step;
        if step.abs() <= 4e-16 * e.abs().max(1.0) {
            return e;
        }
    }
    panic!("solve Kepler: no convergence for M = {mean_anomaly_radians}, e = {eccentricity}");
}

pub fn true_anomaly(eccentric_anomaly: f64, eccentricity: f64) -> f64 {
    let e = eccentricity;
    2.0 * ((1.0 + e).sqrt() * (eccentric_anomaly / 2.0).sin())
        .atan2((1.0 - e).sqrt() * (eccentric_anomaly / 2.0).cos())
}

/// Relative two-body state for the gravitational parameter G (m1 + m2).
pub fn state_from_elements(el: &EllipticElements, gm: f64) -> (DVec3, DVec3) {
    el.assert_valid("state from elements");
    assert!(gm > 0.0, "state from elements: gm = {gm}");
    let a = el.semi_major_axis_meters;
    let e = el.eccentricity;
    let big_e = solve_kepler_elliptic(el.mean_anomaly_radians, e);
    let nu = true_anomaly(big_e, e);
    let r = a * (1.0 - e * big_e.cos());
    let p = a * (1.0 - e * e);
    let (px, py) = (r * nu.cos(), r * nu.sin());
    let v_scale = (gm / p).sqrt();
    let (vx, vy) = (-v_scale * nu.sin(), v_scale * (e + nu.cos()));

    let (s_o, c_o) = el.longitude_of_ascending_node_radians.sin_cos();
    let (sw, cw) = el.argument_of_periapsis_radians.sin_cos();
    let (si, ci) = el.inclination_radians.sin_cos();
    let m11 = c_o * cw - s_o * sw * ci;
    let m12 = -c_o * sw - s_o * cw * ci;
    let m21 = s_o * cw + c_o * sw * ci;
    let m22 = -s_o * sw + c_o * cw * ci;
    let m31 = sw * si;
    let m32 = cw * si;
    (
        DVec3::new(
            m11 * px + m12 * py,
            m21 * px + m22 * py,
            m31 * px + m32 * py,
        ),
        DVec3::new(
            m11 * vx + m12 * vy,
            m21 * vx + m22 * vy,
            m31 * vx + m32 * vy,
        ),
    )
}

/// Osculating two-body quantities defined for every conic; angles undefined for circular or
/// equatorial orbits are left out.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OsculatingOrbit {
    /// Negative for hyperbolic orbits, infinite for exactly parabolic.
    pub semi_major_axis_meters: f64,
    pub eccentricity: f64,
    /// NaN for a radial trajectory, which has no plane.
    pub inclination_radians: f64,
    pub periapsis_radius_meters: f64,
    /// Infinite when the orbit is open (e >= 1).
    pub apoapsis_radius_meters: f64,
    /// Infinite when the orbit is open (e >= 1).
    pub period_seconds: f64,
    pub specific_energy: f64,
}

pub fn osculating_orbit(
    relative_position: DVec3,
    relative_velocity: DVec3,
    gm: f64,
) -> OsculatingOrbit {
    assert!(gm > 0.0, "osculating orbit: gm = {gm}");
    let r = relative_position.length();
    assert!(r > 0.0, "osculating orbit: zero relative position");
    let v2 = relative_velocity.length_squared();
    let h = relative_position.cross(relative_velocity);
    let h_len = h.length();
    let energy = v2 / 2.0 - gm / r;
    let rv = relative_position.dot(relative_velocity);
    let e_vec = ((v2 - gm / r) * relative_position - rv * relative_velocity) / gm;
    let eccentricity = e_vec.length();
    let p = h_len * h_len / gm;
    let closed = eccentricity < 1.0;
    let semi_major_axis_meters = -gm / (2.0 * energy);
    OsculatingOrbit {
        semi_major_axis_meters,
        eccentricity,
        inclination_radians: if h_len > 0.0 {
            (h.z / h_len).clamp(-1.0, 1.0).acos()
        } else {
            f64::NAN
        },
        periapsis_radius_meters: p / (1.0 + eccentricity),
        apoapsis_radius_meters: if closed {
            p / (1.0 - eccentricity)
        } else {
            f64::INFINITY
        },
        period_seconds: if closed {
            orbital_period_seconds(semi_major_axis_meters, gm)
        } else {
            f64::INFINITY
        },
        specific_energy: energy,
    }
}
