use std::f64::consts::TAU;

use glam::{DMat3, DQuat, DVec3};

/// A body's spin, as the orbit lab's `RotationSpec` (`lab/orbit/src/orbit/SystemSpec.ts`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spin {
    /// Sidereal period, seconds, > 0. Retrograde spin uses obliquity > 90 degrees.
    pub period_seconds: f64,
    /// Tilt of the spin axis from the ecliptic pole.
    pub obliquity_radians: f64,
    /// Ecliptic longitude of the spin axis.
    pub pole_longitude_radians: f64,
    /// Prime meridian angle from the equator's node at t = 0.
    pub angle_at_epoch_radians: f64,
}

impl Spin {
    pub fn assert_valid(&self) {
        assert!(
            self.period_seconds > 0.0 && self.period_seconds.is_finite(),
            "spin period {}",
            self.period_seconds
        );
        assert!(
            self.obliquity_radians.is_finite()
                && self.pole_longitude_radians.is_finite()
                && self.angle_at_epoch_radians.is_finite(),
            "spin angles not finite: {self:?}"
        );
    }

    /// The spin axis in ecliptic coordinates.
    pub fn axis(&self) -> DVec3 {
        let (ob, lon) = (self.obliquity_radians, self.pole_longitude_radians);
        DVec3::new(ob.sin() * lon.cos(), ob.sin() * lon.sin(), ob.cos())
    }

    /// Non-rotating equatorial axes (the body's ECI), as `equatorialAxes` in BodyRotation.ts:
    /// z is the spin axis, x the node of the equator on the ecliptic, (-sin lon, cos lon, 0).
    pub fn equatorial_axes(&self) -> DQuat {
        let [x, y, z] = self.equatorial_basis();
        DQuat::from_mat3(&DMat3::from_cols(x, y, z)).normalize()
    }

    /// The same axes as ecliptic vectors, for mapping without a quaternion's rounding.
    pub fn equatorial_basis(&self) -> [DVec3; 3] {
        let pole = self.axis();
        let lon = self.pole_longitude_radians;
        let x = DVec3::new(-lon.sin(), lon.cos(), 0.0);
        [x, pole.cross(x), pole]
    }

    /// Angle of the prime meridian from the node at time t.
    ///
    /// The turns are removed with an exact remainder before scaling, so the angle keeps full
    /// precision however large t is; what remains is t's own spacing (1.2e-7 s at t = 1e9 s).
    pub fn angle(&self, t: f64) -> f64 {
        self.angle_at_epoch_radians + TAU * (t.rem_euclid(self.period_seconds) / self.period_seconds)
    }

    /// Spin rate, radians per second.
    pub fn rate(&self) -> f64 {
        TAU / self.period_seconds
    }
}
