//! What surrounds a point: every body's gravity, and one body's air, ground and sea.
//!
//! A query is a state in any frame of the tree, usually a body's or a scene's, not the root; the
//! answer comes back in that frame's axes. Part modules compute drag, heating, buoyancy and engine
//! back pressure from it. How a vessel moves (rails, the integrator, contact) is the solver's
//! business: it adds its own frame's terms (origin acceleration, tides, centrifugal, Coriolis).
//! See `docs/environment.md`.

mod atmosphere;

pub use atmosphere::*;

use std::sync::Arc;

use glam::{DQuat, DVec3};
use void_frames::{FrameId, FrameSource, Snapshot, State, SystemId};
use void_orbit::{CelestialBody, EphemerisSource, SystemFrames, gravity};
use void_terrain::Terrain;

/// One body's air, ground and sea. Heights are metres above the body's radius
/// (`CelestialBody::radius_meters`, which is also its terrain's reference sphere).
#[derive(Clone, Debug)]
pub struct BodyEnvironment {
    pub atmosphere: Option<Atmosphere>,
    /// The atmosphere's altitude zero.
    pub air_datum_meters: f64,
    pub terrain: Option<Arc<Terrain>>,
    /// None: no sea.
    pub sea_level_meters: Option<f64>,
}

impl BodyEnvironment {
    /// Ground only: no air, no sea.
    pub fn airless(terrain: Arc<Terrain>) -> Self {
        Self {
            atmosphere: None,
            air_datum_meters: 0.0,
            terrain: Some(terrain),
            sea_level_meters: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AirSample {
    /// Above the air datum.
    pub altitude: f64,
    pub air: Air,
    /// Velocity relative to the air, which turns with the body; query axes.
    pub airspeed: DVec3,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GroundSample {
    /// Terrain under the point, above the reference sphere.
    pub height: f64,
    /// The point above that terrain; negative underground.
    pub clearance: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SeaSample {
    /// Below the sea surface; negative above it.
    pub depth: f64,
    /// Velocity relative to water rotating with the body, in query axes.
    pub velocity: DVec3,
    /// Sea exists above local solid terrain, and query point is outside solid terrain.
    pub water_present: bool,
}

/// One body's conditions at a point.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Surroundings {
    pub body: usize,
    /// Away from the body's centre; query axes.
    pub up: DVec3,
    /// From the body's centre.
    pub radius: f64,
    /// None without an atmosphere, or at or above its ceiling.
    pub air: Option<AirSample>,
    /// None without terrain.
    pub ground: Option<GroundSample>,
    /// None without a sea.
    pub sea: Option<SeaSample>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    /// Every body's pull (`void_orbit::gravity`); query axes; none of the query frame's own terms.
    pub gravity: DVec3,
    pub surroundings: Surroundings,
}

/// A state in a body's surface frame, and the turn back to the query's axes.
struct InBody {
    local: State,
    radius: f64,
    direction: DVec3,
    back: DQuat,
}

#[derive(Clone, Debug)]
pub struct Environment {
    bodies: Vec<CelestialBody>,
    places: Vec<Option<BodyEnvironment>>,
    frames: SystemFrames,
    body_systems: Vec<SystemId>,
    origin_system: SystemId,
}

impl Environment {
    /// Gravity of the ephemeris's bodies and nothing else; `with` adds a body's air, ground and
    /// sea.
    pub fn new(ephemeris: &dyn EphemerisSource) -> Self {
        let bodies = ephemeris.bodies();
        assert!(!bodies.is_empty(), "environment: no bodies");
        Self {
            bodies: bodies.to_vec(),
            places: vec![None; bodies.len()],
            frames: SystemFrames::new(ephemeris),
            body_systems: (0..bodies.len()).map(|i| ephemeris.system_of(i)).collect(),
            origin_system: ephemeris.origin_system(),
        }
    }

    pub fn with(mut self, body: usize, place: BodyEnvironment) -> Self {
        let b = self
            .bodies
            .get(body)
            .unwrap_or_else(|| panic!("environment: unknown body {body}"));
        assert!(
            self.places[body].is_none(),
            "environment: {} is already described",
            b.id
        );
        assert!(
            place.air_datum_meters.is_finite(),
            "environment: {} air datum {}",
            b.id,
            place.air_datum_meters
        );
        if let Some(terrain) = &place.terrain {
            assert_eq!(
                terrain.radius_meters, b.radius_meters,
                "environment: {} terrain is not on the body's sphere",
                b.id
            );
        }
        if let Some(sea) = place.sea_level_meters {
            assert!(sea.is_finite(), "environment: {} sea level {sea}", b.id);
        }
        self.places[body] = Some(place);
        self
    }

    /// Reject a different world's catalog or frame layout before a solver uses this environment.
    /// Sample times and live positions are intentionally not compared: they belong to the source.
    pub fn assert_compatible(&self, ephemeris: &dyn EphemerisSource) {
        assert_eq!(
            self.bodies.len(),
            ephemeris.bodies().len(),
            "environment: body count differs from ephemeris"
        );
        assert_eq!(
            self.frames.systems.len(),
            ephemeris.system_count(),
            "environment: system count differs from ephemeris"
        );
        assert_eq!(
            self.origin_system,
            ephemeris.origin_system(),
            "environment: origin system differs from ephemeris"
        );
        for (expected, actual) in self.bodies.iter().zip(ephemeris.bodies()) {
            assert_eq!(
                (
                    expected.index,
                    &expected.id,
                    expected.mass_kg,
                    expected.gm,
                    expected.radius_meters,
                    expected.rotation,
                    expected.j2,
                    expected.j2_reference_radius_meters,
                    expected.parent_index
                ),
                (
                    actual.index,
                    &actual.id,
                    actual.mass_kg,
                    actual.gm,
                    actual.radius_meters,
                    actual.rotation,
                    actual.j2,
                    actual.j2_reference_radius_meters,
                    actual.parent_index
                ),
                "environment: body {} differs from ephemeris",
                expected.id
            );
            assert_eq!(
                self.body_systems[expected.index],
                ephemeris.system_of(actual.index),
                "environment: body {} belongs to another system",
                expected.id
            );
        }
    }

    pub fn bodies(&self) -> &[CelestialBody] {
        &self.bodies
    }

    /// The ephemeris's own frames, for callers that hold no tree of their own (an integrator's
    /// air, for one): evaluate `frames().tree` with the ephemeris and query from `frames().origin`.
    pub fn frames(&self) -> &SystemFrames {
        &self.frames
    }

    pub fn body(&self, body: usize) -> Option<&BodyEnvironment> {
        self.places
            .get(body)
            .unwrap_or_else(|| panic!("environment: unknown body {body}"))
            .as_ref()
    }

    /// Every body's pull at `position` in `from`, in `from`'s axes. Each body's pull is taken in
    /// its own inertial frame (spin axis +z), reached through the frames' common ancestor.
    pub fn gravity<S: FrameSource + ?Sized>(
        &self,
        at: &Snapshot<S>,
        frames: &SystemFrames,
        from: FrameId,
        position: DVec3,
    ) -> DVec3 {
        assert_eq!(
            frames.inertial.len(),
            self.bodies.len(),
            "environment: frames for another ephemeris"
        );
        assert!(position.is_finite(), "environment: position {position}");
        let mut g = DVec3::ZERO;
        for (body, &inertial) in self.bodies.iter().zip(&frames.inertial) {
            let to = at.transform(from, inertial);
            let r = to.apply_point(position);
            assert!(
                r != DVec3::ZERO,
                "environment: at the centre of {}",
                body.id
            );
            let a = gravity::pull(body.gm, gravity::oblateness(body), DVec3::Z, r);
            g += to.rotation().inverse() * a;
        }
        g
    }

    /// `body`'s air, ground and sea at `state` in `from`, read in the body's surface frame.
    pub fn surroundings<S: FrameSource + ?Sized>(
        &self,
        at: &Snapshot<S>,
        frames: &SystemFrames,
        from: FrameId,
        state: State,
        body: usize,
    ) -> Surroundings {
        self.read(body, &self.in_body(at, frames, from, state, body))
    }

    /// `surroundings` for a state already in `body`'s surface frame (body-fixed, turning with
    /// it); no tree is needed and the answer is in that frame's axes.
    pub fn surroundings_local(&self, body: usize, local: State) -> Surroundings {
        self.read(body, &self.local(body, local, DQuat::IDENTITY))
    }

    fn read(&self, body: usize, place: &InBody) -> Surroundings {
        let (b, local, radius) = (&self.bodies[body], place.local, place.radius);
        let described = self.places[body].as_ref();
        let air = described.and_then(|p| {
            let atmosphere = p.atmosphere.as_ref()?;
            let altitude = radius - b.radius_meters - p.air_datum_meters;
            (altitude < atmosphere.ceiling_meters()).then(|| AirSample {
                altitude,
                air: atmosphere.sample(altitude),
                airspeed: place.back * local.velocity,
            })
        });
        let sea = described.and_then(|p| {
            let level = p.sea_level_meters?;
            let ground = self.ground_under(body, place);
            let water_present = !ground.is_some_and(|g| g.height >= level || g.clearance < 0.0);
            Some(SeaSample {
                depth: b.radius_meters + level - radius,
                velocity: place.back * local.velocity,
                water_present,
            })
        });
        Surroundings {
            body,
            up: place.back * place.direction,
            radius,
            air,
            ground: self.ground_under(body, place),
            sea,
        }
    }

    /// Only `body`'s terrain at `position` in `from`: for clearance checks that must not depend on
    /// the air model's domain.
    pub fn ground<S: FrameSource + ?Sized>(
        &self,
        at: &Snapshot<S>,
        frames: &SystemFrames,
        from: FrameId,
        position: DVec3,
        body: usize,
    ) -> Option<GroundSample> {
        let still = State {
            position,
            velocity: DVec3::ZERO,
        };
        self.ground_under(body, &self.in_body(at, frames, from, still, body))
    }

    fn ground_under(&self, body: usize, place: &InBody) -> Option<GroundSample> {
        let terrain = self.places[body].as_ref()?.terrain.as_ref()?;
        let height = terrain.height(place.direction);
        Some(GroundSample {
            height,
            clearance: place.radius - self.bodies[body].radius_meters - height,
        })
    }

    fn in_body<S: FrameSource + ?Sized>(
        &self,
        at: &Snapshot<S>,
        frames: &SystemFrames,
        from: FrameId,
        state: State,
        body: usize,
    ) -> InBody {
        assert!(body < self.bodies.len(), "environment: unknown body {body}");
        assert!(
            state.position.is_finite() && state.velocity.is_finite(),
            "environment: state {state:?}"
        );
        let to = at.transform(from, frames.surface[body]);
        self.local(body, to.apply_state(state), to.rotation().inverse())
    }

    /// A body-fixed state, with `back` turning surface axes to the query's.
    fn local(&self, body: usize, local: State, back: DQuat) -> InBody {
        let b = self
            .bodies
            .get(body)
            .unwrap_or_else(|| panic!("environment: unknown body {body}"));
        assert!(
            local.position.is_finite() && local.velocity.is_finite(),
            "environment: state {local:?}"
        );
        let radius = local.position.length();
        assert!(radius > 0.0, "environment: at the centre of {}", b.id);
        InBody {
            local,
            radius,
            direction: local.position / radius,
            back,
        }
    }

    /// `gravity` and `surroundings` together.
    pub fn sample<S: FrameSource + ?Sized>(
        &self,
        at: &Snapshot<S>,
        frames: &SystemFrames,
        from: FrameId,
        state: State,
        body: usize,
    ) -> Sample {
        Sample {
            gravity: self.gravity(at, frames, from, state.position),
            surroundings: self.surroundings(at, frames, from, state, body),
        }
    }
}
