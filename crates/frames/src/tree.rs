use glam::{DQuat, DVec3};

use crate::{Motion, Spin, SplitPosition, State};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FrameId(u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BodyId(pub usize);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SystemId(pub usize);

/// Barycentric states of the bodies, from the ephemeris.
pub trait BodyStates {
    /// Position and velocity in the ephemeris' own barycentric ecliptic frame at time t.
    /// A time the ephemeris does not cover must panic, never extrapolate.
    fn body_state(&self, body: BodyId, t: f64) -> (DVec3, DVec3);
}

/// What the tree's moving nodes follow: star systems in the galaxy and bodies in their system.
pub trait FrameSource {
    /// A system's barycentre relative to the galaxy root, split so light-years keep centimetres.
    /// Every system shares the root's axes. A time not covered must panic.
    fn system_state(&self, system: SystemId, t: f64) -> (SplitPosition, DVec3);
    /// A body's centre relative to its own system's barycentre, in the system's axes.
    fn body_in_system(&self, body: BodyId, t: f64) -> (DVec3, DVec3);
    /// The motion of a dynamic frame relative to its parent, computed from the owner's live
    /// state (a contact scene, a vessel). Sources without dynamic frames panic.
    fn dynamic_motion(&self, key: u64, t: f64) -> Motion {
        panic!("dynamic frame {key} at t = {t}: this source has no dynamic frames")
    }
}

#[derive(Clone, Debug)]
enum Kind {
    /// The galaxy: non-rotating axes shared by every system.
    Root,
    /// A star system's barycentre. Parent: the root. Its axes are the root's.
    System(SystemId),
    /// Centred on the body, non-rotating equatorial axes. Parent: the body's system.
    BodyInertial { body: BodyId, axes: DQuat },
    /// Turning with the body about its spin axis. Parent: the body's inertial frame.
    BodySurface { spin: Spin },
    /// A constant motion relative to the parent.
    Fixed(Motion),
    /// Written by the simulation; valid only at the time it was written.
    Free(Option<(f64, Motion)>),
    /// Asked of the source every time, so it always follows the owner's live state.
    Dynamic(u64),
}

#[derive(Clone, Debug)]
struct Node {
    parent: Option<FrameId>,
    depth: u32,
    children: u32,
    kind: Kind,
}

/// The frame tree. The root is the galaxy; star systems hang under it and everything else under
/// a system. Removed frames leave their id unused for good.
#[derive(Clone, Debug)]
pub struct FrameTree {
    nodes: Vec<Option<Node>>,
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
            nodes: vec![Some(Node {
                parent: None,
                depth: 0,
                children: 0,
                kind: Kind::Root,
            })],
        }
    }

    fn add(&mut self, parent: FrameId, kind: Kind) -> FrameId {
        let depth = self.node(parent).depth + 1;
        let id = FrameId(u32::try_from(self.nodes.len()).expect("frame count exceeds u32"));
        self.node_mut(parent).children += 1;
        self.nodes.push(Some(Node {
            parent: Some(parent),
            depth,
            children: 0,
            kind,
        }));
        id
    }

    fn node(&self, id: FrameId) -> &Node {
        self.nodes
            .get(id.0 as usize)
            .and_then(Option::as_ref)
            .unwrap_or_else(|| panic!("{id:?} is not in this tree"))
    }

    fn node_mut(&mut self, id: FrameId) -> &mut Node {
        self.nodes
            .get_mut(id.0 as usize)
            .and_then(Option::as_mut)
            .unwrap_or_else(|| panic!("{id:?} is not in this tree"))
    }

    /// A star system's barycentre frame, under the root.
    pub fn add_system(&mut self, system: SystemId) -> FrameId {
        self.add(Self::ROOT, Kind::System(system))
    }

    /// A body's inertial and surface frames, in that order, under its system's frame.
    pub fn add_body(&mut self, system: FrameId, body: BodyId, spin: Spin) -> (FrameId, FrameId) {
        assert!(
            matches!(self.node(system).kind, Kind::System(_)),
            "{system:?} is not a system frame"
        );
        spin.assert_valid();
        let inertial = self.add(
            system,
            Kind::BodyInertial {
                body,
                axes: spin.equatorial_axes(),
            },
        );
        let surface = self.add(inertial, Kind::BodySurface { spin });
        (inertial, surface)
    }

    pub fn add_fixed(&mut self, parent: FrameId, motion: Motion) -> FrameId {
        assert_ne!(
            parent,
            Self::ROOT,
            "frames below the galaxy hang under a system"
        );
        motion.assert_valid();
        self.add(parent, Kind::Fixed(motion))
    }

    /// A frame whose motion the source computes from its owner's state on every query.
    pub fn add_dynamic(&mut self, parent: FrameId, key: u64) -> FrameId {
        assert_ne!(
            parent,
            Self::ROOT,
            "frames below the galaxy hang under a system"
        );
        self.add(parent, Kind::Dynamic(key))
    }

    /// Replaces a fixed frame's motion (a floating origin that moved, a tile reused).
    pub fn set_fixed(&mut self, id: FrameId, motion: Motion) {
        motion.assert_valid();
        match &mut self.node_mut(id).kind {
            Kind::Fixed(m) => *m = motion,
            other => panic!("{id:?} is not a fixed frame: {other:?}"),
        }
    }

    /// A frame the simulation moves; it must be written with `set_free` before any snapshot uses it.
    pub fn add_free(&mut self, parent: FrameId) -> FrameId {
        assert_ne!(
            parent,
            Self::ROOT,
            "frames below the galaxy hang under a system"
        );
        self.add(parent, Kind::Free(None))
    }

    pub fn set_free(&mut self, id: FrameId, t: f64, motion: Motion) {
        assert!(t.is_finite(), "free frame time {t}");
        motion.assert_valid();
        match &mut self.node_mut(id).kind {
            Kind::Free(slot) => *slot = Some((t, motion)),
            other => panic!("{id:?} is not a free frame: {other:?}"),
        }
    }

    /// Moves a free or fixed frame under another parent; a free frame must be written again.
    pub fn reparent(&mut self, id: FrameId, parent: FrameId) {
        assert!(
            matches!(
                self.node(id).kind,
                Kind::Free(_) | Kind::Fixed(_) | Kind::Dynamic(_)
            ),
            "only free, fixed and dynamic frames move between parents"
        );
        assert_ne!(
            parent,
            Self::ROOT,
            "frames below the galaxy hang under a system"
        );
        assert_eq!(self.node(id).children, 0, "{id:?} still has children");
        let mut up = Some(parent);
        while let Some(p) = up {
            assert_ne!(p, id, "{id:?} cannot hang under itself");
            up = self.node(p).parent;
        }
        let old = self.node(id).parent.expect("only the root has no parent");
        self.node_mut(old).children -= 1;
        self.node_mut(parent).children += 1;
        let depth = self.node(parent).depth + 1;
        let node = self.node_mut(id);
        node.parent = Some(parent);
        node.depth = depth;
        if let Kind::Free(slot) = &mut node.kind {
            *slot = None;
        }
    }

    /// Removes a leaf frame. Its id is never handed out again.
    pub fn remove(&mut self, id: FrameId) {
        assert_ne!(id, Self::ROOT, "the root stays");
        let node = self.node(id);
        assert_eq!(node.children, 0, "{id:?} still has children");
        let parent = node.parent.expect("only the root has no parent");
        self.node_mut(parent).children -= 1;
        self.nodes[id.0 as usize] = None;
    }

    pub fn contains(&self, id: FrameId) -> bool {
        self.nodes.get(id.0 as usize).is_some_and(Option::is_some)
    }

    pub fn parent(&self, id: FrameId) -> Option<FrameId> {
        self.node(id).parent
    }

    /// The system frame `id` lies in, or None for the root.
    pub fn system_of(&self, mut id: FrameId) -> Option<FrameId> {
        loop {
            let node = self.node(id);
            if let Kind::System(_) = node.kind {
                return Some(id);
            }
            id = node.parent?;
        }
    }

    /// The frames at time t.
    pub fn at<'a, S: FrameSource + ?Sized>(&'a self, t: f64, source: &'a S) -> Snapshot<'a, S> {
        assert!(t.is_finite(), "snapshot time {t}");
        Snapshot {
            tree: self,
            source,
            t,
        }
    }
}

/// The tree evaluated at one time. Nothing is cached yet; add caching where measurements ask for it.
pub struct Snapshot<'a, S: FrameSource + ?Sized> {
    tree: &'a FrameTree,
    source: &'a S,
    t: f64,
}

impl<S: FrameSource + ?Sized> Snapshot<'_, S> {
    pub fn time(&self) -> f64 {
        self.t
    }

    pub fn tree(&self) -> &FrameTree {
        self.tree
    }

    /// Motion of a frame relative to its parent. The root has none; a system's split position
    /// becomes float64 here, so paths that meet at the root use `transform` instead.
    pub fn motion_to_parent(&self, id: FrameId) -> Motion {
        match &self.tree.node(id).kind {
            Kind::Root => panic!("the root has no parent"),
            Kind::System(system) => {
                let (position, velocity) = self.source.system_state(*system, self.t);
                Motion::new(position.vector(), velocity, DQuat::IDENTITY, DVec3::ZERO)
            }
            Kind::BodyInertial { body, axes } => {
                let (position, velocity) = self.source.body_in_system(*body, self.t);
                Motion::new(position, velocity, *axes, DVec3::ZERO)
            }
            Kind::BodySurface { spin } => Motion {
                translation: DVec3::ZERO,
                velocity: DVec3::ZERO,
                rotation: DQuat::from_rotation_z(spin.angle(self.t)),
                angular_velocity: DVec3::new(0.0, 0.0, spin.rate()),
            },
            Kind::Fixed(motion) => *motion,
            Kind::Dynamic(key) => {
                let motion = self.source.dynamic_motion(*key, self.t);
                motion.assert_valid();
                motion
            }
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

    /// The galaxy-level position of a system frame, or of the root itself.
    fn top(&self, system: Option<FrameId>) -> (SplitPosition, DVec3) {
        match system.map(|s| &self.tree.node(s).kind) {
            None => (SplitPosition::ORIGIN, DVec3::ZERO),
            Some(Kind::System(id)) => self.source.system_state(*id, self.t),
            Some(other) => unreachable!("system_of returned {other:?}"),
        }
    }

    /// Maps coordinates in `from` to coordinates in `to`, through their nearest common ancestor
    /// only, so the error is set by the distances below it, never by the root's. Paths that meet
    /// at the galaxy subtract the two systems' split positions exactly.
    pub fn transform(&self, from: FrameId, to: FrameId) -> Transform {
        let meet = self.common_ancestor(from, to);
        if meet != FrameTree::ROOT {
            return Transform {
                up: self.to_ancestor(from, meet),
                bridge: None,
                down: self.to_ancestor(to, meet),
            };
        }
        let (upper, lower) = (self.tree.system_of(from), self.tree.system_of(to));
        let (a, va) = self.top(upper);
        let (b, vb) = self.top(lower);
        Transform {
            up: self.to_ancestor(from, upper.unwrap_or(FrameTree::ROOT)),
            bridge: Some((a.difference(&b), va - vb)),
            down: self.to_ancestor(to, lower.unwrap_or(FrameTree::ROOT)),
        }
    }

    /// As `transform`, but always through the root in float64. For checks that measure what the
    /// common ancestor saves.
    pub fn transform_via_root(&self, from: FrameId, to: FrameId) -> Transform {
        Transform {
            up: self.to_ancestor(from, FrameTree::ROOT),
            bridge: None,
            down: self.to_ancestor(to, FrameTree::ROOT),
        }
    }
}

/// From one frame to another: up to their common ancestor, across the galaxy if they meet
/// there, then down, subtracting before turning on the way down, as the orbit lab's `toFrame`.
#[derive(Clone, Copy, Debug)]
pub struct Transform {
    /// `from` relative to the common ancestor (its system, when they meet at the root).
    up: Motion,
    /// When the frames meet at the root: `from`'s system relative to `to`'s, exactly, and the
    /// velocity between them. Systems share the root's axes, so no turn.
    bridge: Option<(SplitPosition, DVec3)>,
    /// `to` relative to the common ancestor (its system, when they meet at the root).
    down: Motion,
}

impl Transform {
    fn across(&self, p: DVec3) -> DVec3 {
        match &self.bridge {
            None => p,
            Some((offset, _)) => offset.translate(p).vector(),
        }
    }

    fn bridge_velocity(&self) -> DVec3 {
        self.bridge.map_or(DVec3::ZERO, |(_, v)| v)
    }

    pub fn apply_point(&self, p: DVec3) -> DVec3 {
        self.down.unapply_point(self.across(self.up.apply_point(p)))
    }

    pub fn apply_direction(&self, d: DVec3) -> DVec3 {
        self.down.rotation.inverse() * (self.up.rotation * d)
    }

    pub fn apply_state(&self, s: State) -> State {
        let s = self.up.apply_state(s);
        self.down.unapply_state(State {
            position: self.across(s.position),
            velocity: s.velocity + self.bridge_velocity(),
        })
    }

    /// `from` axes to `to` axes.
    pub fn rotation(&self) -> DQuat {
        (self.down.rotation.inverse() * self.up.rotation).normalize()
    }

    /// As one motion, for composing further; applying it loses the subtract-first precision, and
    /// across the galaxy the split offset becomes float64.
    pub fn to_motion(&self) -> Motion {
        let up = match &self.bridge {
            None => self.up,
            Some((offset, velocity)) => self.up.then(&Motion {
                translation: offset.vector(),
                velocity: *velocity,
                rotation: DQuat::IDENTITY,
                angular_velocity: DVec3::ZERO,
            }),
        };
        up.then(&self.down.inverse())
    }
}
