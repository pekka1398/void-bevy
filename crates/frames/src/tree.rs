use glam::{DQuat, DVec3};

use crate::{Motion, Spin, State};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FrameId(u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BodyId(pub usize);

/// Barycentric states of the bodies, from the ephemeris.
pub trait BodyStates {
    /// Position and velocity in the root (barycentric ecliptic) frame at time t.
    /// A time the ephemeris does not cover must panic, never extrapolate.
    fn body_state(&self, body: BodyId, t: f64) -> (DVec3, DVec3);
}

#[derive(Clone, Debug)]
enum Kind {
    /// The solar system barycentre, ecliptic axes.
    Root,
    /// Centred on the body, non-rotating equatorial axes. Parent: the root.
    BodyInertial { body: BodyId, axes: DQuat },
    /// Turning with the body about its spin axis. Parent: the body's inertial frame.
    BodySurface { spin: Spin },
    /// A constant motion relative to the parent.
    Fixed(Motion),
    /// Written by the simulation; valid only at the time it was written.
    Free(Option<(f64, Motion)>),
}

#[derive(Clone, Debug)]
struct Node {
    parent: Option<FrameId>,
    depth: u32,
    kind: Kind,
}

/// The frame tree. The root is the solar system barycentre.
#[derive(Clone, Debug)]
pub struct FrameTree {
    nodes: Vec<Node>,
}

impl Default for FrameTree {
    fn default() -> Self {
        Self::new()
    }
}

impl FrameTree {
    pub const ROOT: FrameId = FrameId(0);

    pub fn new() -> Self {
        Self {
            nodes: vec![Node {
                parent: None,
                depth: 0,
                kind: Kind::Root,
            }],
        }
    }

    fn add(&mut self, parent: FrameId, kind: Kind) -> FrameId {
        let depth = self.node(parent).depth + 1;
        let id = FrameId(u32::try_from(self.nodes.len()).expect("frame count exceeds u32"));
        self.nodes.push(Node {
            parent: Some(parent),
            depth,
            kind,
        });
        id
    }

    fn node(&self, id: FrameId) -> &Node {
        self.nodes
            .get(id.0 as usize)
            .unwrap_or_else(|| panic!("{id:?} is not in this tree"))
    }

    /// A body's inertial and surface frames, in that order.
    pub fn add_body(&mut self, body: BodyId, spin: Spin) -> (FrameId, FrameId) {
        spin.assert_valid();
        let inertial = self.add(
            Self::ROOT,
            Kind::BodyInertial {
                body,
                axes: spin.equatorial_axes(),
            },
        );
        let surface = self.add(inertial, Kind::BodySurface { spin });
        (inertial, surface)
    }

    pub fn add_fixed(&mut self, parent: FrameId, motion: Motion) -> FrameId {
        motion.assert_valid();
        self.add(parent, Kind::Fixed(motion))
    }

    /// A frame the simulation moves; it must be written with `set_free` before any snapshot uses it.
    pub fn add_free(&mut self, parent: FrameId) -> FrameId {
        self.add(parent, Kind::Free(None))
    }

    pub fn set_free(&mut self, id: FrameId, t: f64, motion: Motion) {
        assert!(t.is_finite(), "free frame time {t}");
        motion.assert_valid();
        let node = self
            .nodes
            .get_mut(id.0 as usize)
            .unwrap_or_else(|| panic!("{id:?} is not in this tree"));
        match &mut node.kind {
            Kind::Free(slot) => *slot = Some((t, motion)),
            other => panic!("{id:?} is not a free frame: {other:?}"),
        }
    }

    pub fn parent(&self, id: FrameId) -> Option<FrameId> {
        self.node(id).parent
    }

    /// The frames at time t.
    pub fn at<'a, B: BodyStates>(&'a self, t: f64, bodies: &'a B) -> Snapshot<'a, B> {
        assert!(t.is_finite(), "snapshot time {t}");
        Snapshot {
            tree: self,
            bodies,
            t,
        }
    }
}

/// The tree evaluated at one time. Nothing is cached yet; add caching where measurements ask for it.
pub struct Snapshot<'a, B: BodyStates> {
    tree: &'a FrameTree,
    bodies: &'a B,
    t: f64,
}

impl<B: BodyStates> Snapshot<'_, B> {
    pub fn time(&self) -> f64 {
        self.t
    }

    /// Motion of a frame relative to its parent. The root has none.
    pub fn motion_to_parent(&self, id: FrameId) -> Motion {
        match &self.tree.node(id).kind {
            Kind::Root => panic!("the root has no parent"),
            Kind::BodyInertial { body, axes } => {
                let (position, velocity) = self.bodies.body_state(*body, self.t);
                Motion::new(position, velocity, *axes, DVec3::ZERO)
            }
            Kind::BodySurface { spin } => Motion {
                translation: DVec3::ZERO,
                velocity: DVec3::ZERO,
                rotation: DQuat::from_rotation_z(spin.angle(self.t)),
                angular_velocity: DVec3::new(0.0, 0.0, spin.rate()),
            },
            Kind::Fixed(motion) => *motion,
            Kind::Free(None) => panic!("free {id:?} used before it was written"),
            Kind::Free(Some((written, motion))) => {
                assert!(
                    *written == self.t,
                    "free {id:?} written at t = {written}, used at t = {}",
                    self.t
                );
                *motion
            }
        }
    }

    /// Motion of `id` relative to its ancestor `ancestor` (identity when they are the same).
    fn to_ancestor(&self, mut id: FrameId, ancestor: FrameId) -> Motion {
        let mut motion = Motion::IDENTITY;
        while id != ancestor {
            motion = motion.then(&self.motion_to_parent(id));
            id = self
                .tree
                .node(id)
                .parent
                .unwrap_or_else(|| panic!("{ancestor:?} is not an ancestor"));
        }
        motion
    }

    /// The nearest frame that is an ancestor of both (either may be the other).
    pub fn common_ancestor(&self, mut a: FrameId, mut b: FrameId) -> FrameId {
        let tree = self.tree;
        while tree.node(a).depth > tree.node(b).depth {
            a = tree.node(a).parent.expect("depth above zero has a parent");
        }
        while tree.node(b).depth > tree.node(a).depth {
            b = tree.node(b).parent.expect("depth above zero has a parent");
        }
        while a != b {
            a = tree
                .node(a)
                .parent
                .expect("frames in one tree meet at the root");
            b = tree
                .node(b)
                .parent
                .expect("frames in one tree meet at the root");
        }
        a
    }

    /// Maps coordinates in `from` to coordinates in `to`, through their nearest common ancestor
    /// only, so the error is set by the distances below it, never by the root's.
    pub fn transform(&self, from: FrameId, to: FrameId) -> Transform {
        let meet = self.common_ancestor(from, to);
        Transform {
            up: self.to_ancestor(from, meet),
            down: self.to_ancestor(to, meet),
        }
    }

    /// As `transform`, but always through the root. For checks that measure what the
    /// common ancestor saves.
    pub fn transform_via_root(&self, from: FrameId, to: FrameId) -> Transform {
        Transform {
            up: self.to_ancestor(from, FrameTree::ROOT),
            down: self.to_ancestor(to, FrameTree::ROOT),
        }
    }
}

/// From one frame to another: up to their common ancestor, then down, subtracting before
/// turning on the way down, as the orbit lab's `toFrame`.
#[derive(Clone, Copy, Debug)]
pub struct Transform {
    /// `from` relative to the common ancestor.
    up: Motion,
    /// `to` relative to the common ancestor.
    down: Motion,
}

impl Transform {
    pub fn apply_point(&self, p: DVec3) -> DVec3 {
        self.down.unapply_point(self.up.apply_point(p))
    }

    pub fn apply_direction(&self, d: DVec3) -> DVec3 {
        self.down.rotation.inverse() * (self.up.rotation * d)
    }

    pub fn apply_state(&self, s: State) -> State {
        self.down.unapply_state(self.up.apply_state(s))
    }

    /// `from` axes to `to` axes.
    pub fn rotation(&self) -> DQuat {
        (self.down.rotation.inverse() * self.up.rotation).normalize()
    }

    /// As one motion, for composing further; applying it loses the subtract-first precision.
    pub fn to_motion(&self) -> Motion {
        self.up.then(&self.down.inverse())
    }
}
