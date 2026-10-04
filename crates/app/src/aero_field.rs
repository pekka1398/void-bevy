//! The air a planet's atmosphere puts on the demo rocket: landing says where the rocket is and
//! how fast it is moving over the ground, the aero crate says what the air does about it.
//!
//! Each stage is one aerodynamic body, a cylinder along its own +y, with the aero lab's rocket
//! coefficients. In free flight the stack is a single unit and the faces where the stages meet are
//! covered; under contact physics each Rapier body is asked separately, so those two faces are
//! left exposed. That overestimates drag slightly while attached, where the rocket is near the
//! ground and slow and the whole force is small.

use std::f64::consts::PI;
use std::sync::Arc;

use glam::{DQuat, DVec3};
use void_aero::{AeroElement, AeroShape, AeroState, BodyAero, NEUTRAL, aerodynamic_forces};
use void_environment::{AirSample, Environment};
use void_frames::State;
use void_landing::{AirField, DemoRocket, FrameState, LandingPlanet, RocketPart};
use void_orbit::EphemerisSource;

/// One stage as the air sees it.
#[derive(Clone, Copy, Debug)]
struct Stage {
    radius: f64,
    length: f64,
    /// Centre along the stack's +y, relative to the attached stack's reference point.
    offset: f64,
}

pub struct RocketAir {
    /// The planet's air, from the world's environment.
    environment: Arc<Environment>,
    body: usize,
    upper: Stage,
    booster: Stage,
}

impl RocketAir {
    /// The field for `planet`, body `body` of `ephemeris`, or None where there is no air. `demo`
    /// gives the stages their size.
    pub fn for_planet(
        planet: &LandingPlanet,
        ephemeris: &dyn EphemerisSource,
        body: usize,
        demo: &DemoRocket,
    ) -> Option<Arc<dyn AirField>> {
        planet.air_density_scale?;
        let stage = |spec: &void_landing::LanderSpec, offset: f64| Stage {
            radius: spec.half_extents.x,
            length: 2.0 * spec.half_extents.y,
            offset,
        };
        // The booster sits below the upper stage; the stack's reference point is between them.
        let upper = stage(&demo.upper, demo.upper.half_extents.y);
        let booster = stage(&demo.booster, -demo.booster.half_extents.y);
        Some(Arc::new(Self {
            environment: void_landing::planet_environment(planet, ephemeris, body, true),
            body,
            upper,
            booster,
        }))
    }

    /// `stage` as an aerodynamic element. While the stages are attached each hides the other's
    /// mating face, but only as far across as it reaches: the booster is the wider of the two, so
    /// the shoulder around the upper stage stays in the airflow.
    fn element(
        &self,
        which: RocketPart,
        stage: Stage,
        covered: Option<(Face, f64)>,
        centre: f64,
    ) -> AeroElement {
        let area = PI * stage.radius * stage.radius;
        let exposed = |face: Face| match covered {
            Some((hidden, radius)) if hidden == face => (area - PI * radius * radius).max(0.0),
            _ => area,
        };
        AeroElement {
            id: format!("{which:?}"),
            point: DVec3::new(0.0, stage.offset - centre, 0.0),
            shape: AeroShape::Body(BodyAero {
                axis: DVec3::Y,
                front_area: exposed(Face::Front),
                rear_area: exposed(Face::Rear),
                side_area: 2.0 * stage.radius * stage.length,
                wet_area: 2.0 * PI * stage.radius * stage.length,
                length_meters: stage.length,
                // The aero lab's cylinder stage: blunt ahead, a base behind, and a long side.
                front_cd: 0.6,
                rear_cd: 0.8,
                side_cd: 1.1,
            }),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Face {
    Front,
    Rear,
}

impl RocketAir {
    /// The air at a body-fixed state, or None above the atmosphere.
    fn air(&self, local: FrameState) -> Option<AirSample> {
        self.environment
            .surroundings_local(
                self.body,
                State {
                    position: local.position,
                    velocity: local.velocity,
                },
            )
            .air
    }
}

impl AirField for RocketAir {
    fn pressure_pa(&self, position: DVec3) -> f64 {
        let still = FrameState {
            position,
            velocity: DVec3::ZERO,
        };
        self.air(still).map_or(0.0, |a| a.air.pressure_pa)
    }

    fn force(
        &self,
        parts: &[RocketPart],
        local: FrameState,
        rotation: DQuat,
        _mass_kg: f64,
    ) -> DVec3 {
        let Some(air) = self.air(local) else {
            return DVec3::ZERO;
        };
        let attached = parts.len() == 2;
        // One part alone acts about its own centre; the stack about the reference point the
        // propagator carries, which is where the two stages meet.
        let centre = if attached {
            0.0
        } else if parts[0] == RocketPart::Upper {
            self.upper.offset
        } else {
            self.booster.offset
        };
        let elements: Vec<AeroElement> = parts
            .iter()
            .map(|&which| match which {
                RocketPart::Upper => self.element(
                    which,
                    self.upper,
                    attached.then_some((Face::Rear, self.booster.radius)),
                    centre,
                ),
                RocketPart::Booster => self.element(
                    which,
                    self.booster,
                    attached.then_some((Face::Front, self.upper.radius)),
                    centre,
                ),
            })
            .collect();
        let state = AeroState {
            center: local.position,
            velocity: air.airspeed,
            rotation,
            // The air's own damping of a spin is a torque, which this field does not yet carry.
            angular_velocity: DVec3::ZERO,
        };
        aerodynamic_forces(&elements, &state, &air.air, DVec3::ZERO, &NEUTRAL).force
    }
}
