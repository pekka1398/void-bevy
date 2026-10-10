//! Landing on and taking off from a rotating planet with terrain: the planet's body-fixed frame,
//! the Rapier contact world that runs in it near the ground, and coast prediction above it.

mod coast;
mod contact_world;
mod planet_frame;
mod planets;

pub use coast::{
    CoastPrediction, EncounterPairState, EncounterPhysicsGate, EncounterRanges, predict_coast,
};
pub use contact_world::{
    BodyColliderMesh, BodyShape, ContactBodySpec, ContactRayHit, ContactWorld,
    ContactWorldCheckpoint, ContactWorldOptions, ExtraAcceleration, Piece, PieceMass, SimpleShape,
    TileCollider, surface_indices,
};
pub use planet_frame::{ContactFrame, PlanetFrame, upright_at};
pub use planets::{LandingPlanet, landing_lod_options, level_for_tile_size};
