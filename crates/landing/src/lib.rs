//! lab/landing, ported: landing on and taking off from a rotating planet with terrain. Free flight
//! uses the orbit crate's inertial propagator above the terrain band; near the ground, bodies are
//! handed to Rapier in the planet-fixed frame.

mod contact_world;
mod planet_frame;
mod planets;

pub use contact_world::{
    BodyShape, ContactBodySpec, ContactWorld, ContactWorldOptions, ExtraAcceleration, Piece,
    PieceMass, SimpleShape, TileCollider, surface_indices,
};
pub use planet_frame::{ContactFrame, FrameState, PlanetFrame};
pub use planets::{
    LandingPlanet, aurelia, aurelia_fast, earth_size, level_for_tile_size, moon_size, pebble,
    planet_by_id, planet_ephemeris,
};
