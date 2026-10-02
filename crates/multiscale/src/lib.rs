//! Coordinates and coupling from centimetres to light-years, ported from `lab/multiscale`: split
//! positions (integer cells plus float64 offsets), several star systems in one direct N-body
//! world, frames that follow a system's barycentre, and a probe coasting between systems. No
//! Bevy.
//!
//! The lab's two-ship encounter (its `FrameEphemeris` and `Encounter`) runs `vessels`' `Fleet`
//! in a moving system's frame and is not here yet.

mod fixtures;
mod split;
mod traveller;
mod world;

pub use fixtures::*;
pub use split::*;
pub use traveller::*;
pub use world::*;
