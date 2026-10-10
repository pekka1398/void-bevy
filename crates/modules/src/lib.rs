//! Pure part-module loads, shared by Fleet owners. Wrenches name their frame and moment
//! reference; the caller supplies trial attitude/point velocity and commits state only after
//! accepted time. Geometry, resources, and deployment states are never mutated by evaluation.
//! No Bevy and no Fleet.
mod air;
pub mod wrench;
pub use wrench::Wrench;
pub mod body;
pub mod engine;
pub mod parachute;
pub use air::{AirData, Conditions, VesselAir, has_atmosphere, vessel_air, vessel_air_at};

pub mod rcs;

pub mod thermal;

pub mod water;
