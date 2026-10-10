//! Stack-node craft model: part catalog, attachment graph, resources and crew. No Bevy dependency;
//! the editor lives in `void-assembly-lab`.
mod graph;
mod model;
pub use graph::*;
pub use model::*;

mod thermal;
pub use thermal::*;

mod wheel;
pub use wheel::*;

mod aircraft;
pub use aircraft::*;
mod crew;
pub use crew::*;
