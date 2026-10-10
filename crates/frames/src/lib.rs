//! Positions belong to frames, and frames form one tree: the galaxy at the root, star systems
//! under it, then bodies, their surfaces, contact scenes, vessels and the camera. Physics stays in
//! f64 within each frame; systems sit at split positions so frames that meet at the galaxy
//! subtract exactly; only the render edge becomes camera-relative f32.

mod motion;
mod spin;
mod split;
mod tree;

pub use motion::{Motion, State};
pub use spin::Spin;
pub use split::{CELL_METERS, SplitPosition};
pub use tree::{
    BodyId, BodyStates, FrameId, FrameSource, FrameTree, Snapshot, SystemId, Transform,
};
