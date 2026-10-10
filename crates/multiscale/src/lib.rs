//! Coordinates and coupling from centimetres to light-years: split
//! positions (integer cells plus float64 offsets), several star systems in one direct N-body
//! world, frames that follow a system's barycentre, and a probe coasting between systems. No
//! Bevy.
//!
mod traveller;
mod world;

pub use traveller::*;
pub use world::*;

mod ephemeris;
pub use ephemeris::*;

pub const LIGHT_YEAR: f64 = 299_792_458.0 * 365.25 * 86400.0;
