//! Coordinates and coupling from centimetres to light-years: split
//! positions (integer cells plus float64 offsets), several star systems in one direct N-body
//! world, frames that follow a system's barycentre, and a probe coasting between systems. No
//! Bevy.
//!
mod fixtures;
mod traveller;
mod world;

pub use fixtures::*;
pub use traveller::*;
pub use world::*;

mod ephemeris;
pub use ephemeris::*;
