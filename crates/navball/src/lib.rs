//! The navball: the sky and ground around the vessel, seen from
//! outside along its nose. The ball's centre is where the nose points; screen up is the vessel's
//! top (where the nose goes on pitch up), screen right its right.
//!
//! Everything is plain vectors in one caller-chosen frame, so the ball does not care whether that
//! frame is body-fixed or inertial. [`NavballPainter`] draws it into an RGBA buffer; its text
//! labels are handed back for the caller to draw.

use glam::DVec3;

mod paint;

pub use paint::{NavballLabel, NavballPainter};

/// Below this length of pole × up the vessel is at a pole to double precision. No field of north
/// can be continuous over the whole sphere (the hairy ball theorem), so the pole needs its own
/// north: grid north, along the prime meridian, as polar navigation uses.
const POLE_EPSILON: f64 = 1e-12;
/// Below this speed, m/s, the velocity has no useful direction and the markers are not drawn.
pub const MARKER_MIN_SPEED: f64 = 0.1;
const UNIT_TOLERANCE: f64 = 1e-5;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NavballInput {
    /// The vessel's nose (thrust axis) and top (where the nose turns on pitch up). Unit and
    /// perpendicular.
    pub nose: DVec3,
    pub top: DVec3,
    /// Local vertical, away from the body's centre. Unit.
    pub up: DVec3,
    /// The body's north pole (spin axis). Unit.
    pub pole: DVec3,
    /// The body's prime meridian (longitude 0) on its equator. Unit, perpendicular to `pole`.
    pub prime_meridian: DVec3,
    /// Velocity for the prograde and retrograde markers; below `MARKER_MIN_SPEED` they are hidden.
    pub velocity: DVec3,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NavballBasis {
    /// Screen axes in the caller's frame: right, up (the vessel's top) and out of the screen (the
    /// nose).
    pub right: DVec3,
    pub top: DVec3,
    pub nose: DVec3,
    /// Local horizon axes.
    pub up: DVec3,
    pub north: DVec3,
    pub east: DVec3,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NavballReadout {
    /// The nose's heading, degrees from north through east, [0, 360).
    pub heading: f64,
    /// The nose's pitch above the horizon, degrees.
    pub pitch: f64,
    pub speed: f64,
}

fn require_unit(name: &str, v: DVec3) {
    assert!(
        (v.length() - 1.0).abs() <= UNIT_TOLERANCE,
        "navball: {name} is not a unit vector (length {})",
        v.length()
    );
}

/// North and east on the local horizon: true north, or grid north exactly at a pole.
pub fn horizon_axes(up: DVec3, pole: DVec3, prime_meridian: DVec3) -> (DVec3, DVec3) {
    for (name, v) in [
        ("up", up),
        ("pole", pole),
        ("primeMeridian", prime_meridian),
    ] {
        require_unit(name, v);
    }
    assert!(
        pole.dot(prime_meridian).abs() <= UNIT_TOLERANCE,
        "navball: prime meridian is not on the equator (dot with the pole {})",
        pole.dot(prime_meridian)
    );
    let east_raw = pole.cross(up);
    let east_length = east_raw.length();
    if east_length < POLE_EPSILON {
        // At a pole up is along the pole, so the prime meridian already lies on the horizon.
        return (prime_meridian, prime_meridian.cross(up));
    }
    let east = east_raw * (1.0 / east_length);
    (up.cross(east), east)
}

pub fn navball_basis(input: &NavballInput) -> NavballBasis {
    require_unit("nose", input.nose);
    require_unit("top", input.top);
    assert!(
        input.nose.dot(input.top).abs() <= UNIT_TOLERANCE,
        "navball: nose and top are not perpendicular (dot {})",
        input.nose.dot(input.top)
    );
    let (north, east) = horizon_axes(input.up, input.pole, input.prime_meridian);
    NavballBasis {
        // MIRRORED FROM KSP: top × nose makes the ball a globe seen from outside. Facing east, 60
        // is on the right and 120 on the left; facing north, W is on the right. KSP shows the
        // opposite (E on the right when facing north). The game's yaw keys follow this same axis, so the ball and the keys agree with each other but both are
        // mirrored from KSP. Fixing it means changing this axis and the steering together.
        right: input.top.cross(input.nose),
        top: input.top,
        nose: input.nose,
        up: input.up,
        north,
        east,
    }
}

/// A direction on the ball: x right, y up, z toward the viewer (visible when z ≥ 0), on the unit
/// disc.
pub fn to_ball(basis: &NavballBasis, direction: DVec3) -> DVec3 {
    DVec3::new(
        direction.dot(basis.right),
        direction.dot(basis.top),
        direction.dot(basis.nose),
    )
}

/// Heading (degrees from north through east, [0, 360)) and pitch (degrees above the horizon) of
/// a direction.
pub fn heading_pitch(basis: &NavballBasis, direction: DVec3) -> (f64, f64) {
    let (n, e, u) = (
        direction.dot(basis.north),
        direction.dot(basis.east),
        direction.dot(basis.up),
    );
    let heading = f64::atan2(e, n) * 180.0 / std::f64::consts::PI;
    (
        if heading < 0.0 {
            heading + 360.0
        } else {
            heading
        },
        f64::atan2(u, n.hypot(e)) * 180.0 / std::f64::consts::PI,
    )
}

/// The direction at a heading and pitch, degrees.
pub fn horizon_direction(basis: &NavballBasis, heading_degrees: f64, pitch_degrees: f64) -> DVec3 {
    let (h, p) = (
        heading_degrees * std::f64::consts::PI / 180.0,
        pitch_degrees * std::f64::consts::PI / 180.0,
    );
    let c = p.cos();
    let (n, e, u) = (c * h.cos(), c * h.sin(), p.sin());
    DVec3::new(
        n * basis.north.x + e * basis.east.x + u * basis.up.x,
        n * basis.north.y + e * basis.east.y + u * basis.up.y,
        n * basis.north.z + e * basis.east.z + u * basis.up.z,
    )
}
