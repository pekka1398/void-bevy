//! Ideal orbital guidance, using the same direction law as the plan's propagator.
//! This controls an orbital burn; it does not teleport or steer contact bodies.
use super::*;
use void_orbit::{AttitudeLaw, ThrustControl};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum GuidanceStatus {
    Armed,
    Completed,
    Aborted(String),
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GuidedBurn {
    pub start_time: f64,
    pub end_time: f64,
    pub attitude: AttitudeLaw,
    pub status: GuidanceStatus,
    /// Vacuum full-throttle ratings: a changed propulsion group invalidates this burn.
    pub(super) force: DVec3,
    pub(super) flow: f64,
}
impl Fleet {
    pub(super) fn effective_throttle(&self, v: &Vessel, time: f64) -> f64 {
        if self.command_control_unavailable_of(v) {
            return 0.0;
        }
        let id = &v.id;
        if let Some(g) = self.guidance.get(id)
            && g.status == GuidanceStatus::Armed
        {
            if time >= g.start_time && time < g.end_time {
                1.0
            } else {
                0.0
            }
        } else {
            self.controls[id].throttle
        }
    }
    pub(super) fn full_rating_of(&self, v: &Vessel) -> Propulsion {
        propulsion(
            &self.parts,
            &v.members,
            1.0,
            self.centre(&v.members),
            &Conditions::VACUUM,
        )
    }
    pub fn guidance(&self, id: &str) -> Option<&GuidedBurn> {
        self.vessel(id);
        self.guidance.get(id)
    }
    pub fn arm_guided_burn(
        &mut self,
        id: &str,
        start_time: f64,
        end_time: f64,
        attitude: AttitudeLaw,
    ) -> Result<(), String> {
        let v = self.vessel(id);
        if !matches!(v.owner, Owner::Orbit { .. }) {
            return Err("maneuver guidance requires orbital ownership".into());
        }
        if !self.commanded(v) {
            return Err("vessel has no command part".into());
        }
        let rcs = self.rcs_control(id);
        if rcs.enabled && (rcs.force != DVec3::ZERO || rcs.torque != DVec3::ZERO) {
            return Err("manual RCS control is active".into());
        }
        assert!(
            start_time.is_finite() && end_time.is_finite() && end_time > start_time,
            "guidance: invalid burn times"
        );
        if start_time < self.time {
            return Err("maneuver ignition is in the past".into());
        }
        if self
            .guidance
            .get(id)
            .is_some_and(|g| g.status == GuidanceStatus::Armed)
        {
            return Err("a maneuver is already armed on this vessel".into());
        }
        let p = self.full_throttle_vacuum_thrust(id);
        if p.force.length() == 0.0 || p.flow_kg_per_second == 0.0 {
            return Err("no staged engine with accessible fuel".into());
        }
        if p.torque.length() > 1e-6 {
            return Err("unbalanced engine torque cannot use ideal orbital guidance".into());
        }
        if end_time - start_time > p.seconds_to_flameout + 1e-9 {
            return Err("maneuver crosses a fuel-group flameout".into());
        }
        // Have the orbit propagator validate the law as part of a physically valid control.
        // thrust_direction alone assumes its caller has validated unit vectors/body indices.
        let control = Control::Thrust(ThrustControl {
            thrust_newtons: p.force.length(),
            exhaust_velocity: p.force.length() / p.flow_kg_per_second,
            minimum_mass_kg: self.mass(&v.members) - p.flow_kg_per_second * (end_time - start_time),
            attitude,
        });
        control.assert_valid(self.ephemeris.bodies().len());
        self.sas.remove(id); // Guidance owns attitude; an old hold target must not fight the burn.
        self.controls.insert(
            id.into(),
            VesselControl {
                throttle: 1.0,
                turn: DVec3::ZERO,
            },
        );
        self.guidance.insert(
            id.into(),
            GuidedBurn {
                start_time,
                end_time,
                attitude,
                status: GuidanceStatus::Armed,
                force: p.force,
                flow: p.flow_kg_per_second,
            },
        );
        Ok(())
    }
    /// Explicit cancellation is retained for the plan manager and HUD; never silently coast.
    pub fn cancel_guidance(&mut self, id: &str, reason: &str) {
        if let Some(g) = self.guidance.get_mut(id)
            && g.status == GuidanceStatus::Armed
        {
            g.status = GuidanceStatus::Aborted(reason.into());
            self.controls
                .get_mut(id)
                .expect("guidance vessel control")
                .throttle = 0.0;
        }
    }
}
