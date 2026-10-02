//! lab/landing, ported: landing on and taking off from a rotating planet with terrain. Free flight
//! uses the orbit crate's inertial propagator above the terrain band; near the ground, bodies are
//! handed to Rapier in the planet-fixed frame.

mod air;
mod coast;
mod contact_world;
mod demo_rocket;
mod lander;
mod planet_frame;
mod planets;
mod rocket;

pub use air::{AirField, PlanetAir};
pub use coast::{
    CoastPrediction, EncounterPairState, EncounterPhysicsGate, EncounterRanges, predict_coast,
};
pub use contact_world::{
    BodyShape, ContactBodySpec, ContactWorld, ContactWorldOptions, ExtraAcceleration, Piece,
    PieceMass, SimpleShape, TileCollider, surface_indices,
};
pub use demo_rocket::{DemoRocket, booster_pieces, demo_rocket, upper_pieces};
pub use lander::{
    Lander, LanderControl, LanderMode, LanderOptions, LanderSpec, ModeChange, STANDARD_GRAVITY,
    rotate, surface_direction, upright_at,
};
pub use planet_frame::{ContactFrame, FrameState, PlanetFrame};
pub use planets::{
    LandingPlanet, aurelia, aurelia_fast, earth_size, landing_lod_options, level_for_tile_size,
    moon_size, pebble, planet_by_id, planet_ephemeris,
};
pub use rocket::{
    AttitudeSample, Crash, PARTS, PartJointRocket, PhysicsMode, RocketModeChange, RocketPart,
    STEERING_TORQUE, SteerFn, Steering,
};
