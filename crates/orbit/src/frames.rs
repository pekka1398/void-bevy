//! The fixed part of the frame tree for an ephemeris: one frame per star system, and each body's
//! inertial and surface frames under its system.

use void_frames::{BodyId, FrameId, FrameTree, SystemId};

use crate::EphemerisSource;

#[derive(Clone, Debug)]
pub struct SystemFrames {
    pub tree: FrameTree,
    /// By `SystemId`.
    pub systems: Vec<FrameId>,
    /// By body index: centred on the body, non-rotating equatorial axes.
    pub inertial: Vec<FrameId>,
    /// By body index: turning with the body.
    pub surface: Vec<FrameId>,
    /// The system the ephemeris' physics view is relative to.
    pub origin: FrameId,
}

impl SystemFrames {
    pub fn new(ephemeris: &dyn EphemerisSource) -> Self {
        let mut tree = FrameTree::new();
        let systems: Vec<_> = (0..ephemeris.system_count())
            .map(|s| tree.add_system(SystemId(s)))
            .collect();
        assert!(!systems.is_empty(), "system frames: no systems");
        let (inertial, surface) = ephemeris
            .bodies()
            .iter()
            .enumerate()
            .map(|(i, body)| {
                assert_eq!(body.index, i, "system frames: bodies out of order");
                let system = systems[ephemeris.system_of(i).0];
                tree.add_body(system, BodyId(i), body.rotation)
            })
            .unzip();
        let origin = systems[ephemeris.origin_system().0];
        Self {
            tree,
            systems,
            inertial,
            surface,
            origin,
        }
    }
}
