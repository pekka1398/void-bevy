//! Positions belong to frames, and frames form a tree rooted at the solar system barycentre.
//! Physics stays in f64 within each frame; only the render edge becomes camera-relative f32.
//! Design: `lab/void-bevy/docs/frames.md`.

mod motion;
mod spin;
mod tree;

pub use motion::{Motion, State};
pub use spin::Spin;
pub use tree::{BodyId, BodyStates, FrameId, FrameTree, Snapshot, Transform};
