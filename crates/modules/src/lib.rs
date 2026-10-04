//! Part modules: what each part does in its surroundings, for its vessel to sum and hand to its
//! physics owner. Thrust-like modules act for a whole leg or step with the surroundings read once
//! at the vessel's centre of mass; fluid-like ones (drag) keep their shape for the leg and read
//! the air again at every integrator stage. No Bevy and no Fleet; see `docs/part-modules.md`.
mod air;
pub mod body;
pub mod engine;
pub use air::{Conditions, VesselAir, has_atmosphere, vessel_air};
