//! Aerodynamics and heating: body and wing forces at each element's own airflow, and two-layer
//! part heating with a finite ablator. No Bevy.

mod aero;
mod thermal;

pub use aero::*;
pub use thermal::*;

use glam::{DQuat, DVec3};

pub use void_assembly::rotate;
/// The atmosphere model lives in `void-environment`; aero keeps the names it always had.
pub use void_environment::{Air, Atmosphere, EarthAtmosphere, smooth, validate_air};

pub const DEG: f64 = std::f64::consts::PI / 180.0;

/// `x` clamped to [lo, hi].
pub fn clamp(x: f64, lo: f64, hi: f64) -> f64 {
    lo.max(hi.min(x))
}

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

/// A vector's length.
pub fn length(v: DVec3) -> f64 {
    v.length()
}

/// The unit vector along `v`; a zero or non-finite vector has no direction.
pub fn normalize(v: DVec3) -> DVec3 {
    let len = length(v);
    assert!(
        len > 0.0 && len.is_finite(),
        "normalize: vector ({}, {}, {}) has no direction",
        v.x,
        v.y,
        v.z
    );
    DVec3::new(v.x / len, v.y / len, v.z / len)
}

pub fn unit(q: DQuat) -> DQuat {
    let n = positive(q.length(), "quaternion length");
    DQuat::from_xyzw(q.x / n, q.y / n, q.z / n, q.w / n)
}

pub fn inverse(q: DQuat) -> DQuat {
    DQuat::from_xyzw(-q.x, -q.y, -q.z, q.w)
}

/// The shortest rotation taking `from` to `to`.
pub fn align(from: DVec3, to: DVec3) -> DQuat {
    let (a, b) = (normalize(from), normalize(to));
    let d = a.dot(b);
    if d < -0.999999999 {
        let other = if a.x.abs() < 0.8 { DVec3::X } else { DVec3::Y };
        let axis = normalize(a.cross(other));
        return DQuat::from_xyzw(axis.x, axis.y, axis.z, 0.0);
    }
    let c = a.cross(b);
    unit(DQuat::from_xyzw(c.x, c.y, c.z, 1.0 + d))
}
