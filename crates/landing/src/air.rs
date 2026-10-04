//! Air as a force on a vessel. Landing knows no aerodynamics: it says where and how fast a vessel
//! is relative to the ground, and whoever supplies the field says what the air does about it.
//!
//! The field is asked in the planet's body-fixed frame, where the air is at rest, so an
//! implementation never deals with the planet's spin or its path around the system. Contact
//! physics already runs in that frame; free flight does not, so `PlanetAir` converts for it.

use std::sync::Arc;

use glam::{DQuat, DVec3};
use void_orbit::{AirSource, EphemerisSource, body_orientation};

use crate::planet_frame::{FrameState, PlanetFrame};
use crate::rocket::RocketPart;

/// Rapier stores attitudes in f32, so one read back as f64 is a unit quaternion only to about
/// 1e-7. The air field is given a normalised one: the error is the storage's, not the attitude's.
pub(crate) fn unit(q: DQuat) -> DQuat {
    let l = (q.x * q.x + q.y * q.y + q.z * q.z + q.w * q.w).sqrt();
    assert!(
        l > 0.0 && l.is_finite(),
        "air: attitude {q:?} has no length"
    );
    DQuat::from_xyzw(q.x / l, q.y / l, q.z / l, q.w / l)
}

/// What the air does to the parts flying as one rigid unit. In the planet's body-fixed frame: the
/// air is at rest there, so `local.velocity` is already the airspeed, and altitude is read from
/// `local.position` against the planet's own radius.
///
/// Only a force, not yet a torque: the game's attitude is commanded, so a vessel does not
/// weathervane into the airflow on its own. Adding that means a torque here and a path for it
/// through both the contact step and the flight attitude.
pub trait AirField: Send + Sync {
    /// Force in newtons, in the planet's body-fixed frame, on the `parts` flying together with
    /// `mass_kg` and attitude `rotation` (also body-fixed). Zero outside the atmosphere.
    fn force(
        &self,
        parts: &[RocketPart],
        local: FrameState,
        rotation: DQuat,
        mass_kg: f64,
    ) -> DVec3;

    /// Ambient pressure in pascals at a body-fixed position, which an engine pushes against. Zero
    /// outside the atmosphere.
    fn pressure_pa(&self, position: DVec3) -> f64;
}

/// An `AirField` seen from the inertial frame the orbit propagator integrates in, for one unit of
/// parts at a fixed attitude. The owner rebuilds it whenever the unit or its attitude changes; it
/// is a pure function of its arguments while it lives, as `AirSource` requires. Each stage's
/// state goes to the body-fixed frame through the frame tree, at that stage's time.
pub struct PlanetAir {
    field: Arc<dyn AirField>,
    frame: PlanetFrame,
    parts: Vec<RocketPart>,
    rotation: DQuat,
}

impl PlanetAir {
    pub fn new(
        field: Arc<dyn AirField>,
        frame: &PlanetFrame,
        parts: &[RocketPart],
        rotation: DQuat,
    ) -> Self {
        Self {
            field,
            frame: frame.clone(),
            parts: parts.to_vec(),
            rotation: unit(rotation),
        }
    }
}

impl AirSource for PlanetAir {
    fn acceleration(
        &self,
        ephemeris: &dyn EphemerisSource,
        t: f64,
        position: DVec3,
        velocity: DVec3,
        mass_kg: f64,
    ) -> DVec3 {
        let local = self
            .frame
            .to_body_fixed(ephemeris, t, FrameState { position, velocity });
        let force = self.field.force(&self.parts, local, self.rotation, mass_kg);
        if force == DVec3::ZERO {
            return DVec3::ZERO;
        }
        // Body axes back to the ecliptic, as PlanetFrame does.
        let axes = body_orientation(&self.frame.body.rotation, t);
        let world = DVec3::new(
            force.x * axes[0].x + force.y * axes[1].x + force.z * axes[2].x,
            force.x * axes[0].y + force.y * axes[1].y + force.z * axes[2].y,
            force.x * axes[0].z + force.y * axes[1].z + force.z * axes[2].z,
        );
        world / mass_kg
    }
}
