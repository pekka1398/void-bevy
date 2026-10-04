//! Assembly geometry and pressure ratings: the part modules (drag, engine back pressure) that turn
//! the world's `Environment` into forces for Fleet.
use glam::{DQuat, DVec3};
use std::{collections::HashMap, sync::Arc};
use void_aero::{AeroElement, AeroShape, AeroState, BodyAero, NEUTRAL, aerodynamic_forces};
use void_assembly::{Connection, Module, Shape};
use void_environment::Environment;
use void_frames::State;
use void_orbit::{AirSource, EphemerisSource};
use void_vessels::{ForcePart, ForceSample, PartForces, VesselSnapshot};

/// Engines lose `nozzleExitAreaM2` × ambient pressure of their catalog thrust.
pub struct FleetAir {
    /// The body whose air the craft flies through.
    pub body_index: usize,
}
impl FleetAir {
    pub fn new(body_index: usize) -> Self {
        Self { body_index }
    }
}
/// The air seen at a state of the ephemeris's physics view, through the environment's own frames.
fn air_at(
    environment: &Environment,
    ephemeris: &dyn EphemerisSource,
    t: f64,
    state: State,
    body: usize,
) -> Option<void_environment::AirSample> {
    let frames = environment.frames();
    environment
        .surroundings(
            &frames.tree.at(t, ephemeris),
            frames,
            frames.origin,
            state,
            body,
        )
        .air
}
struct CraftAir {
    environment: Arc<Environment>,
    body: usize,
    rotation: DQuat,
    elements: Vec<AeroElement>,
}
impl AirSource for CraftAir {
    fn acceleration(
        &self,
        ephemeris: &dyn EphemerisSource,
        t: f64,
        position: DVec3,
        velocity: DVec3,
        mass: f64,
    ) -> DVec3 {
        let state = State { position, velocity };
        let Some(air) = air_at(&self.environment, ephemeris, t, state, self.body) else {
            return DVec3::ZERO;
        };
        // Drag depends on the airflow and attitude only, so it is evaluated in the ephemeris axes.
        let state = AeroState {
            center: position,
            velocity: air.airspeed,
            rotation: self.rotation,
            // Match the current game's force-only scope; no aerodynamic torque or spin damping.
            angular_velocity: DVec3::ZERO,
        };
        aerodynamic_forces(&self.elements, &state, &air.air, DVec3::ZERO, &NEUTRAL).force / mass
    }
}
impl PartForces for FleetAir {
    fn sample(
        &self,
        environment: &Arc<Environment>,
        ephemeris: &dyn EphemerisSource,
        time: f64,
        vessel: &VesselSnapshot,
        parts: &[ForcePart],
        connections: &[Connection],
    ) -> ForceSample {
        let state = State {
            position: vessel.position,
            velocity: vessel.velocity,
        };
        let pressure = air_at(environment, ephemeris, time, state, self.body_index)
            .map_or(0.0, |air| air.air.pressure_pa);
        let mut thrust_scales = HashMap::new();
        let mut elements = Vec::new();
        for part in parts {
            let d = part.definition;
            for module in &d.modules {
                if let Module::Engine {
                    thrust_newtons,
                    nozzle_exit_area_m2: area,
                    ..
                } = *module
                {
                    assert!(
                        area.is_finite() && area >= 0.0,
                        "fleet air: invalid nozzle area"
                    );
                    thrust_scales.insert(
                        part.id.clone(),
                        (1.0 - area * pressure / thrust_newtons).max(0.0),
                    );
                }
            }
            let exposed = |node: &str| {
                let mut covered = 0.0_f64;
                for link in connections {
                    let other = if link.a == part.id && link.node_a == node {
                        Some(&link.b)
                    } else if link.b == part.id && link.node_b == node {
                        Some(&link.a)
                    } else {
                        None
                    };
                    if let Some(other) = other {
                        // Ignore edges outside this connected vessel, including separated stages.
                        if let Some(p) = parts.iter().find(|p| p.id == *other) {
                            covered = covered.max(p.definition.radius);
                        }
                    }
                }
                std::f64::consts::PI * (d.radius.powi(2) - covered.powi(2)).max(0.0)
            };
            elements.push(AeroElement {
                id: part.id.clone(),
                point: part.pose.position,
                shape: AeroShape::Body(BodyAero {
                    axis: part.pose.rotation * DVec3::Y,
                    front_area: exposed("top"),
                    rear_area: exposed("bottom"),
                    side_area: 2.0 * d.radius * d.height,
                    wet_area: std::f64::consts::TAU * d.radius * d.height,
                    length_meters: d.height,
                    front_cd: if d.shape == Shape::Cone { 0.25 } else { 0.6 },
                    rear_cd: 0.8,
                    side_cd: 1.1,
                }),
            });
        }
        ForceSample {
            air: Some(Arc::new(CraftAir {
                environment: environment.clone(),
                body: self.body_index,
                rotation: vessel.rotation,
                elements,
            })),
            thrust_scales,
        }
    }
}
