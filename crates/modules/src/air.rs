//! The air a vessel flies through, from the world's `Environment`.
use crate::body;
use glam::{DQuat, DVec3};
use std::sync::Arc;
use void_aero::{
    AeroElement, AeroShape, AeroState, ControlSurface, NEUTRAL, WingAero, aerodynamic_forces,
};
use void_assembly::PartGraph;
use void_environment::{AirSample, Environment};
use void_frames::{FrameId, FrameSource, Snapshot, State};
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

/// Immutable part geometry and accepted parachute state for pure trial wrench evaluation.
pub struct VesselAir {
    environment: Arc<Environment>,
    bodies: Vec<usize>,
    rotation: DQuat,
    elements: Vec<AeroElement>,
    parachutes: Vec<(
        DVec3,
        void_assembly::ParachuteDefinition,
        void_assembly::ParachuteState,
    )>,
    start_time: f64,
}

impl VesselAir {
    /// Pure trial load about the COM. Rotation maps parts axes into `query`; angular velocity
    /// is relative to `query` in its axes. Each location gets its own environment sample, so
    /// the frame/environment supplies the local wind exactly once.
    pub fn wrench_in<S: FrameSource + ?Sized>(
        &self,
        at: &Snapshot<'_, S>,
        query: FrameId,
        state: State,
        rotation: DQuat,
        angular_velocity: DVec3,
    ) -> crate::Wrench {
        assert!(
            at.time() >= self.start_time,
            "air trial predates source start"
        );
        assert!(angular_velocity.is_finite(), "invalid air angular velocity");
        let frames = self.environment.frames();
        let sample = |state| {
            self.bodies.iter().find_map(|&body| {
                self.environment
                    .surroundings(at, frames, query, state, body)
                    .air
            })
        };
        let mut result = crate::Wrench::zero(query, state.position);
        for element in &self.elements {
            let arm = rotation * element.point;
            let point = state.position + arm;
            if let Some(air) = sample(State {
                position: point,
                velocity: state.velocity + angular_velocity.cross(arm),
            }) {
                // The sample already includes point velocity: no second ω × r in aero.
                let loads = aerodynamic_forces(
                    std::slice::from_ref(element),
                    &AeroState {
                        center: state.position,
                        velocity: air.airspeed,
                        rotation,
                        angular_velocity: DVec3::ZERO,
                    },
                    &air.air,
                    DVec3::ZERO,
                    &NEUTRAL,
                );
                result.add(crate::Wrench {
                    frame: query,
                    reference_point: state.position,
                    force: loads.force,
                    torque: loads.torque,
                });
            }
        }
        for (offset, parameters, chute) in &self.parachutes {
            let arm = rotation * offset;
            let point = state.position + arm;
            if let Some(air) = sample(State {
                position: point,
                velocity: state.velocity + angular_velocity.cross(arm),
            }) {
                let force = crate::parachute::force(
                    *chute,
                    parameters,
                    at.time() - self.start_time,
                    air.air.density,
                    air.airspeed,
                );
                result.add(crate::Wrench::at_offset(
                    query,
                    state.position,
                    arm,
                    force,
                    DVec3::ZERO,
                ));
            }
        }
        result
    }
    /// Legacy force-only sampling for callers without an attitude integrator.
    pub fn acceleration_in<S: FrameSource + ?Sized>(
        &self,
        at: &Snapshot<'_, S>,
        query: FrameId,
        state: State,
        mass: f64,
    ) -> DVec3 {
        let rotation = (at
            .transform(self.environment.frames().origin, query)
            .rotation()
            * self.rotation)
            .normalize();
        self.force_only_in(at, query, state, rotation) / mass
    }
    /// Historical COM air/body sampling with no rotational point velocity. This is a named
    /// configuration, not a fallback when full wrench evaluation cannot run.
    fn force_only_in<S: FrameSource + ?Sized>(
        &self,
        at: &Snapshot<'_, S>,
        query: FrameId,
        state: State,
        rotation: DQuat,
    ) -> DVec3 {
        assert!(
            at.time() >= self.start_time,
            "air trial predates source start"
        );
        let frames = self.environment.frames();
        let sample = |state| {
            self.bodies.iter().find_map(|&body| {
                self.environment
                    .surroundings(at, frames, query, state, body)
                    .air
            })
        };
        let mut force = sample(state).map_or(DVec3::ZERO, |air| {
            aerodynamic_forces(
                &self.elements,
                &AeroState {
                    center: state.position,
                    velocity: air.airspeed,
                    rotation,
                    angular_velocity: DVec3::ZERO,
                },
                &air.air,
                DVec3::ZERO,
                &NEUTRAL,
            )
            .force
        });
        for (offset, p, s) in &self.parachutes {
            if let Some(air) = sample(State {
                position: state.position + rotation * offset,
                velocity: state.velocity,
            }) {
                force += crate::parachute::force(
                    *s,
                    p,
                    at.time() - self.start_time,
                    air.air.density,
                    air.airspeed,
                );
            }
        }
        force
    }
    pub fn wrench(
        &self,
        ephemeris: &dyn EphemerisSource,
        t: f64,
        state: State,
        rotation: DQuat,
        angular_velocity: DVec3,
    ) -> crate::Wrench {
        let frames = self.environment.frames();
        self.wrench_in(
            &frames.tree.at(t, ephemeris),
            frames.origin,
            state,
            rotation,
            angular_velocity,
        )
    }
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
        let frames = self.environment.frames();
        self.force_only_in(
            &frames.tree.at(t, ephemeris),
            frames.origin,
            State { position, velocity },
            self.rotation,
        ) / mass
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
    vessel_air_at(environment, graph, members, centre, rotation, 0.0)
}
pub fn vessel_air_at(
    environment: &Arc<Environment>,
    graph: &PartGraph,
    members: &[String],
    centre: DVec3,
    rotation: DQuat,
    start_time: f64,
) -> Option<VesselAir> {
    let bodies = atmospheric(environment);
    if bodies.is_empty() {
        return None;
    }
    Some(VesselAir {
        environment: environment.clone(),
        bodies,
        rotation,
        start_time,
        parachutes: members
            .iter()
            .flat_map(|id| {
                let part = graph.part(id);
                part.definition.modules.iter().filter_map(move |m| {
                    if let void_assembly::Module::Parachute { id, parameters } = m {
                        let void_assembly::ModuleState::Parachute { state } = part.modules[id]
                        else {
                            panic!("parachute state mismatch")
                        };
                        Some((
                            part.pose.position + part.pose.rotation * parameters.point - centre,
                            *parameters,
                            state,
                        ))
                    } else {
                        None
                    }
                })
            })
            .collect(),
        elements: members
            .iter()
            .flat_map(|id| {
                let part = graph.part(id);
                let mut elements = vec![body::element(graph, members, id, centre)];
                for module in &part.definition.modules {
                    if let void_assembly::Module::LiftingSurface { id, parameters: p } = module {
                        elements.push(AeroElement {
                            id: format!("{}/{}", part.id, id),
                            point: part.pose.position + part.pose.rotation * p.point - centre,
                            shape: AeroShape::Wing(WingAero {
                                chord: part.pose.rotation * p.chord,
                                normal: part.pose.rotation * p.normal,
                                area: p.area_m2,
                                aspect_ratio: p.aspect_ratio,
                                chord_meters: p.chord_meters,
                                sweep_radians: p.sweep_radians,
                                incidence_radians: p.incidence_radians,
                                zero_lift_radians: p.zero_lift_radians,
                                stall_radians: p.stall_radians,
                                cd0: p.cd0,
                                efficiency: p.efficiency,
                                pitching_moment: p.pitching_moment,
                                control: ControlSurface::None,
                                control_sign: 1.0,
                                max_deflection_radians: 0.0,
                            }),
                        });
                    }
                }
                elements
            })
            .collect(),
    })
}
