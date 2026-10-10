//! Planets, star systems, craft and sites that only tests and examples use. Crates list this
//! under `[dev-dependencies]` only; the game never depends on it.
mod crafts;
mod galaxy;
mod planets;
mod sites;

pub use crafts::*;
pub use galaxy::*;
pub use planets::*;
pub use sites::*;
