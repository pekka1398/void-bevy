//! Coupled trial attitude for the existing translational propagator. Geometry and module state
//! are immutable until the leg is accepted. Torque is predicted at the midpoint (second order);
//! every translation stage sees the attitude and angular velocity at its own stage time.
use glam::{DQuat, DVec3};
use std::sync::Arc;
use void_frames::State;
use void_modules::VesselAir;
use void_orbit::{AirSource, EphemerisSource};
use void_rotation::{Mat3, rotation_step};

pub(super) struct RigidFlightSource {
    pub air: Option<Arc<VesselAir>>,
    pub full_air: bool,
    pub water: Arc<void_modules::water::VesselWater>,
    pub start: f64,
    pub rotation: DQuat,
    pub angular_velocity: DVec3,
    pub inertia: Mat3,
    pub torque_local: DVec3,
    pub force_local: DVec3,
}
impl RigidFlightSource {
    pub fn attitude(&self, t: f64) -> (DQuat, DVec3) {
        assert!(
            t.is_finite() && t >= self.start,
            "rigid trial predates accepted leg"
        );
        if t == self.start {
            return (self.rotation, self.angular_velocity);
        }
        rotation_step(
            self.rotation,
            self.angular_velocity,
            &self.inertia,
            self.torque_local,
            DVec3::ZERO,
            t - self.start,
        )
    }
}
impl AirSource for RigidFlightSource {
    fn acceleration(
        &self,
        ephemeris: &dyn EphemerisSource,
        t: f64,
        position: DVec3,
        velocity: DVec3,
        mass: f64,
    ) -> DVec3 {
        let (q, w) = self.attitude(t);
        let force = self.air.as_ref().map_or(DVec3::ZERO, |air| {
            if self.full_air {
                air.wrench(ephemeris, t, State { position, velocity }, q, w)
                    .force
            } else {
                air.acceleration(ephemeris, t, position, velocity, mass) * mass
            }
        });
        (force
            + self
                .water
                .wrench(ephemeris, t, State { position, velocity }, q, w)
                .force
            + q * self.force_local)
            / mass
    }
}

/// Only scene-coordinate transforms are valid here. The independently integrated bubble origin
/// is prepared at both ends; cubic Hermite interpolation supplies its force-trial midpoint.
pub(super) struct SceneStepSource<'a> {
    pub fleet: &'a super::Fleet,
    pub scene: u64,
    pub start: f64,
    pub end: f64,
}
impl void_frames::FrameSource for SceneStepSource<'_> {
    fn system_state(
        &self,
        id: void_frames::SystemId,
        t: f64,
    ) -> (void_frames::SplitPosition, DVec3) {
        self.fleet.ephemeris.system_state(id, t)
    }
    fn body_in_system(&self, id: void_frames::BodyId, t: f64) -> (DVec3, DVec3) {
        self.fleet.ephemeris.body_in_system(id, t)
    }
    fn dynamic_split_state(&self, key: u64, t: f64) -> (void_frames::SplitPosition, DVec3) {
        match self.fleet.dynamic[&key] {
            super::Dynamic::SceneAnchor(scene) if scene == self.scene => {
                (self.fleet.scenes[&scene].anchor, DVec3::ZERO)
            }
            _ => panic!("scene trial source cannot read other split anchors at {t}"),
        }
    }
    fn dynamic_motion(&self, key: u64, t: f64) -> void_frames::Motion {
        assert!(
            t >= self.start && t <= self.end,
            "scene trial outside accepted step"
        );
        match self.fleet.dynamic[&key] {
            super::Dynamic::Bubble(scene) if scene == self.scene => {
                let super::SceneFrame::Bubble(f) = &self.fleet.scenes[&scene].world.frame else {
                    unreachable!()
                };
                let a = f.origin(self.start);
                let b = f.origin(self.end);
                let h = self.end - self.start;
                let u = (t - self.start) / h;
                let delta = b.position - a.position;
                let p = a.position
                    + a.velocity * (h * (u * u * u - 2.0 * u * u + u))
                    + delta * (-2.0 * u * u * u + 3.0 * u * u)
                    + b.velocity * (h * (u * u * u - u * u));
                let v = a.velocity * (3.0 * u * u - 4.0 * u + 1.0)
                    + delta * ((-6.0 * u * u + 6.0 * u) / h)
                    + b.velocity * (3.0 * u * u - 2.0 * u);
                void_frames::Motion::new(p, v, DQuat::IDENTITY, DVec3::ZERO)
            }
            super::Dynamic::Floating(scene) if scene == self.scene => {
                void_frames::Motion::fixed(self.fleet.scenes[&scene].world.origin, DQuat::IDENTITY)
            }
            _ => panic!("scene trial source cannot read other live dynamic frames"),
        }
    }
}

/// Existing ideal maneuver pointing: the guidance actuator prescribes orientation and cancels
/// passive moments. The air uses that *same* prescribed attitude at every translation trial.
/// This keeps ideal maneuver mode distinct from finite-inertia manual/SAS dynamics.
pub(super) struct GuidedAirSource {
    pub air: Option<Arc<VesselAir>>,
    pub full_air: bool,
    pub water: Arc<void_modules::water::VesselWater>,
    pub rotation: DQuat,
    pub thrust_axis: DVec3,
    pub law: void_orbit::AttitudeLaw,
    // Only the existing law evaluator is used; this object never integrates a trajectory.
    pub evaluator: std::sync::Mutex<void_orbit::VesselPropagator>,
}
impl GuidedAirSource {
    pub fn attitude(&self, e: &dyn EphemerisSource, t: f64, state: State) -> DQuat {
        let direction = self
            .evaluator
            .lock()
            .expect("guided attitude evaluator poisoned")
            .thrust_direction(e, &self.law, t, state.position, state.velocity);
        (DQuat::from_rotation_arc(self.rotation * self.thrust_axis, direction) * self.rotation)
            .normalize()
    }
}
impl AirSource for GuidedAirSource {
    fn acceleration(
        &self,
        e: &dyn EphemerisSource,
        t: f64,
        position: DVec3,
        velocity: DVec3,
        mass: f64,
    ) -> DVec3 {
        let state = State { position, velocity };
        let q = self.attitude(e, t, state);
        (self.air.as_ref().map_or(DVec3::ZERO, |air| {
            if self.full_air {
                air.wrench(e, t, state, q, DVec3::ZERO).force
            } else {
                air.acceleration(e, t, position, velocity, mass) * mass
            }
        }) + self.water.wrench(e, t, state, q, DVec3::ZERO).force)
            / mass
    }
}
