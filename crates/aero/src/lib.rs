//! Aerodynamics and heating: body and wing forces at each element's own airflow, and two-layer
//! part heating with a finite ablator. No Bevy.

mod aero;
mod thermal;

pub use aero::*;
pub use thermal::*;

use glam::{DQuat, DVec3};

/// The atmosphere model lives in `void-environment`; aero keeps the names it always had.
pub use void_environment::{Air, Atmosphere, EarthAtmosphere, smooth, validate_air};

pub const DEG: f64 = std::f64::consts::PI / 180.0;

pub(crate) fn finite(value: f64, label: &str) -> f64 {
    assert!(value.is_finite(), "{label}: non-finite");
    value
}

pub(crate) fn positive(value: f64, label: &str) -> f64 {
    finite(value, label);
    assert!(value > 0.0, "{label}: must be positive");
    value
}

pub(crate) fn finite_vec(v: DVec3, label: &str) {
    assert!(
        v.is_finite(),
        "{label}: non-finite vector ({}, {}, {})",
        v.x,
        v.y,
        v.z
    );
}

pub(crate) fn validate_rotation(q: DQuat) {
    for v in [q.x, q.y, q.z, q.w] {
        finite(v, "rotation");
    }
    assert!(
        (q.length() - 1.0).abs() <= 1e-8,
        "Rotation must be a unit quaternion"
    );
}

/// The unit vector along `v`; a zero or non-finite vector has no direction.
pub fn normalize(v: DVec3) -> DVec3 {
    let len = v.length();
    assert!(
        len > 0.0 && len.is_finite(),
        "normalize: vector ({}, {}, {}) has no direction",
        v.x,
        v.y,
        v.z
    );
    DVec3::new(v.x / len, v.y / len, v.z / len)
}
