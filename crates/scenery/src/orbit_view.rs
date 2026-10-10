//! An orbit camera (left drag pans, right drag orbits the planet centre, Shift + left drag turns,
//! the wheel zooms) that works from space down to the ground:
//!
//! - Zoom and pan scale with the height above the surface under the camera, not the distance to the
//!   centre, so a wheel notch at 2 m moves centimetres and at 20,000 km thousands of kilometres.
//! - Orbiting slows with height the same way (at most 0.005 rad per pixel).
//! - Tilt runs from straight down (0) past the horizon (π/2) to nearly straight up.
//! - The camera never goes below `MIN_HEIGHT` above the surface (the caller enforces it).
//!
//! State: the unit direction `p` of the point the camera orbits over, a unit tangent `north` (screen
//! up when looking straight down; no pole singularity), a pan offset in metres, the distance from
//! the centre and the tilt.

use glam::{DQuat, DVec3};

#[derive(Clone, Debug)]
pub struct OrbitView {
    p: DVec3,
    north: DVec3,
    offset: DVec3,
    distance: f64,
    tilt: f64,
    pub max_distance_meters: f64,
}

/// The camera's position and unit forward and up, body-fixed.
#[derive(Clone, Copy, Debug)]
pub struct Pose {
    pub position: DVec3,
    pub forward: DVec3,
    pub up: DVec3,
}

impl OrbitView {
    pub const MIN_HEIGHT: f64 = 1.5;

    pub fn new(
        start: DVec3,
        distance_meters: f64,
        tilt_radians: f64,
        max_distance_meters: f64,
    ) -> Self {
        let p = normalize(start);
        assert!(
            distance_meters > 0.0 && max_distance_meters > 0.0,
            "OrbitView: distance={distance_meters}, max={max_distance_meters}"
        );
        Self {
            p,
            north: normalize(reject(DVec3::Z, p)),
            offset: DVec3::ZERO,
            distance: distance_meters,
            tilt: clamp_tilt(tilt_radians),
            max_distance_meters,
        }
    }

    pub fn tilt_radians(&self) -> f64 {
        self.tilt
    }

    pub fn pose(&self) -> Pose {
        let down = -self.p;
        let forward = down * f64::cos(self.tilt) + self.north * f64::sin(self.tilt);
        let up = self.p * f64::sin(self.tilt) + self.north * f64::cos(self.tilt);
        Pose {
            position: self.p * self.distance + self.offset,
            forward: normalize(forward),
            up: normalize(up),
        }
    }

    /// Right, up and backward (camera axes looking down −z) in body-fixed axes.
    pub fn basis(&self) -> (DVec3, DVec3, DVec3) {
        let Pose { forward, up, .. } = self.pose();
        (normalize(forward.cross(up)), up, -forward)
    }

    /// Left drag: translate in the view plane, `height` metres of travel per screen height at a 1:1 scale.
    pub fn pan_screen(
        &mut self,
        dx_pixels: f64,
        dy_pixels: f64,
        fov_y_radians: f64,
        viewport_height_pixels: f64,
        height: f64,
    ) {
        finite(
            "pan_screen",
            &[
                dx_pixels,
                dy_pixels,
                fov_y_radians,
                viewport_height_pixels,
                height,
            ],
        );
        let Pose { forward, up, .. } = self.pose();
        let right = normalize(forward.cross(up));
        let meters = (height * 2.0 * f64::tan(fov_y_radians / 2.0)) / viewport_height_pixels;
        self.offset += right * (-dx_pixels * meters) + up * (dy_pixels * meters);
    }

    /// Right drag: turn the camera's position and orientation about the planet centre (the pole,
    /// then the screen's horizontal).
    pub fn orbit_around_center(&mut self, horizontal_radians: f64, vertical_radians: f64) {
        finite(
            "orbit_around_center",
            &[horizontal_radians, vertical_radians],
        );
        let pole = DVec3::Z;
        let turn = DQuat::from_axis_angle(pole, horizontal_radians);
        self.p = turn * self.p;
        self.north = turn * self.north;
        self.offset = turn * self.offset;
        let right = normalize(self.north.cross(self.p));
        let turn = DQuat::from_axis_angle(right, vertical_radians);
        self.p = normalize(turn * self.p);
        self.north = normalize(turn * self.north);
        self.offset = turn * self.offset;
    }

    /// Shift + left drag: turn the heading about the local vertical and change the tilt.
    pub fn turn(&mut self, heading_radians: f64, tilt_radians: f64) {
        finite("turn", &[heading_radians, tilt_radians]);
        let east = self.north.cross(self.p);
        self.north =
            normalize(self.north * f64::cos(heading_radians) + east * f64::sin(heading_radians));
        self.tilt = clamp_tilt(self.tilt + tilt_radians);
    }

    /// Scale the camera's whole position about the planet centre until it is at `radius` from it
    /// (the pan offset scales with it, so the planet centre stays where it is on screen).
    pub fn set_radius(&mut self, radius: f64) {
        finite("set_radius", &[radius]);
        let position = self.pose().position;
        let current = position.length();
        let next = self.max_distance_meters.min(radius);
        self.offset *= next / current;
        self.distance *= next / current;
    }

    /// Put the camera over `direction`, `radius` from the centre, heading `heading_radians` from
    /// true north, with no pan.
    pub fn place(
        &mut self,
        direction: DVec3,
        radius: f64,
        heading_radians: f64,
        tilt_radians: f64,
    ) {
        self.p = normalize(direction);
        let north = normalize(reject(DVec3::Z, self.p));
        let east = north.cross(self.p);
        self.north =
            normalize(north * f64::cos(heading_radians) + east * f64::sin(heading_radians));
        self.offset = DVec3::ZERO;
        self.distance = radius;
        self.tilt = clamp_tilt(tilt_radians);
    }
}

/// Straight down to 0.01 rad short of straight up.
fn clamp_tilt(tilt: f64) -> f64 {
    tilt.clamp(0.0, std::f64::consts::PI - 0.01)
}

fn finite(at: &str, values: &[f64]) {
    assert!(
        values.iter().all(|v| v.is_finite()),
        "OrbitView.{at}: {values:?}"
    );
}

fn normalize(a: DVec3) -> DVec3 {
    let length = a.length();
    assert!(
        length.is_finite() && length >= 1e-9,
        "OrbitView: cannot normalize {a:?}"
    );
    a * (1.0 / length)
}

/// Component of `v` perpendicular to unit `n`.
fn reject(v: DVec3, n: DVec3) -> DVec3 {
    let r = v - n * v.dot(n);
    // Over a pole, grid north runs down the prime meridian, as the navball's does.
    if r.length() < 1e-12 {
        return if n.z > 0.0 { -DVec3::X } else { DVec3::X };
    }
    r
}
