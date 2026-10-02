//! Aerodynamics, heating and reentry, ported from `lab/aerodynamics`: a layered Earth atmosphere,
//! body and wing forces at each element's own airflow, two-layer part heating with a finite
//! ablator, three test vehicles, an aircraft on Rapier and a 6-DOF capsule reentry. No Bevy.
//!
//! The vector and quaternion operations keep the lab's operation order, so the results agree with
//! it bit for bit except where V8's own `sin`, `cos` and `pow` round differently (see `void_math`).

mod aero;
mod atmosphere;
mod entry;
mod flight;
mod loads;
mod thermal;
mod vehicle;

pub use aero::*;
pub use atmosphere::*;
pub use entry::*;
pub use flight::*;
pub use loads::*;
pub use thermal::*;
pub use vehicle::*;

use glam::{DQuat, DVec3};
use void_math::hypot;

pub use void_assembly::rotate;

pub const DEG: f64 = std::f64::consts::PI / 180.0;

/// `Math.max(lo, Math.min(hi, x))`.
pub fn clamp(x: f64, lo: f64, hi: f64) -> f64 {
    lo.max(hi.min(x))
}

/// Smoothstep from a to b.
pub fn smooth(a: f64, b: f64, x: f64) -> f64 {
    let t = clamp((x - a) / (b - a), 0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
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
        (hypot([q.x, q.y, q.z, q.w]) - 1.0).abs() <= 1e-8,
        "Rotation must be a unit quaternion"
    );
}

/// Length as `Math.hypot`.
pub fn length(v: DVec3) -> f64 {
    hypot([v.x, v.y, v.z])
}

/// The lab's `normalize`: a zero or non-finite vector has no direction.
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
    let n = positive(hypot([q.x, q.y, q.z, q.w]), "quaternion length");
    DQuat::from_xyzw(q.x / n, q.y / n, q.z / n, q.w / n)
}

pub fn inverse(q: DQuat) -> DQuat {
    DQuat::from_xyzw(-q.x, -q.y, -q.z, q.w)
}

/// Hamilton product a b, in the lab's term order.
pub fn quat_multiply(a: DQuat, b: DQuat) -> DQuat {
    DQuat::from_xyzw(
        a.w * b.x + a.x * b.w + a.y * b.z - a.z * b.y,
        a.w * b.y - a.x * b.z + a.y * b.w + a.z * b.x,
        a.w * b.z + a.x * b.y - a.y * b.x + a.z * b.w,
        a.w * b.w - a.x * b.x - a.y * b.y - a.z * b.z,
    )
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
