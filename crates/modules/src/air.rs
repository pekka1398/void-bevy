//! The air a vessel flies through, from the world's `Environment`.
use crate::body;
use glam::{DQuat, DVec3};
use std::sync::Arc;
use void_aero::{AeroElement, AeroState, NEUTRAL, aerodynamic_forces};
use void_assembly::PartGraph;
use void_environment::{AirSample, Environment};
use void_frames::State;
use void_orbit::{AirSource, EphemerisSource};

/// The bodies with an atmosphere, by index.
fn atmospheric(environment: &Environment) -> Vec<usize> {
    (0..environment.bodies().len())
        .filter(|&b| environment.body(b).is_some_and(|p| p.atmosphere.is_some()))
        .collect()
}

pub fn has_atmosphere(environment: &Environment) -> bool {
    (0..environment.bodies().len())
        .any(|b| environment.body(b).is_some_and(|p| p.atmosphere.is_some()))
}

/// The air at a state of the ephemeris's physics view: the first of `bodies` whose atmosphere
/// holds it, read through the environment's own frames.
fn air_at(
    environment: &Environment,
    bodies: &[usize],
    ephemeris: &dyn EphemerisSource,
    t: f64,
    state: State,
) -> Option<AirSample> {
    let frames = environment.frames();
    let at = frames.tree.at(t, ephemeris);
    bodies.iter().find_map(|&body| {
        environment
            .surroundings(&at, frames, frames.origin, state, body)
            .air
    })
}

/// What a vessel's parts act in for one leg or step, read once at its centre of mass.
#[derive(Clone, Copy, Debug)]
pub struct Conditions {
    pub air: Option<AirSample>,
}

impl Conditions {
    pub const VACUUM: Self = Self { air: None };

    /// At `state` (the vessel's centre of mass in the ephemeris's physics view) at `t`.
    pub fn at(
        environment: &Environment,
        ephemeris: &dyn EphemerisSource,
        t: f64,
        state: State,
    ) -> Self {
        Self {
            air: air_at(environment, &atmospheric(environment), ephemeris, t, state),
        }
    }

    /// Zero outside any atmosphere.
    pub fn ambient_pressure_pa(&self) -> f64 {
        self.air.map_or(0.0, |air| air.air.pressure_pa)
    }
}

/// A vessel's drag for one leg: its parts' bodies and its attitude are frozen, and the air is read
/// again at every integrator stage. Force only: no aerodynamic torque or spin damping.
pub struct VesselAir {
    environment: Arc<Environment>,
    bodies: Vec<usize>,
    rotation: DQuat,
    elements: Vec<AeroElement>,
}

impl VesselAir {
    /// The parts' bodies, in `members` order.
    pub fn elements(&self) -> &[AeroElement] {
        &self.elements
    }
}

impl AirSource for VesselAir {
    fn acceleration(
        &self,
        ephemeris: &dyn EphemerisSource,
        t: f64,
        position: DVec3,
        velocity: DVec3,
        mass: f64,
    ) -> DVec3 {
        let state = State { position, velocity };
        let Some(air) = air_at(&self.environment, &self.bodies, ephemeris, t, state) else {
            return DVec3::ZERO;
        };
        // Drag depends on the airflow and attitude only, so it is evaluated in the ephemeris axes.
        let state = AeroState {
            center: position,
            velocity: air.airspeed,
            rotation: self.rotation,
            angular_velocity: DVec3::ZERO,
        };
        aerodynamic_forces(&self.elements, &state, &air.air, DVec3::ZERO, &NEUTRAL).force / mass
    }
}

/// The air for a vessel made of `members`, whose centre of mass is `centre` in its parts frame and
/// whose parts frame has `rotation`; `None` where no body has an atmosphere.
pub fn vessel_air(
    environment: &Arc<Environment>,
    graph: &PartGraph,
    members: &[String],
    centre: DVec3,
    rotation: DQuat,
) -> Option<VesselAir> {
    let bodies = atmospheric(environment);
    if bodies.is_empty() {
        return None;
    }
    Some(VesselAir {
        environment: environment.clone(),
        bodies,
        rotation,
        elements: members
            .iter()
            .map(|id| body::element(graph, members, id, centre))
            .collect(),
    })
}
