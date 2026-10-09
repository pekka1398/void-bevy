//! Cube-sphere quadtree selection.

use std::f64::consts::FRAC_PI_4;
use std::sync::Arc;
use std::time::Instant;

use glam::DVec3;

use crate::adjacency::{FACE_EDGES, selected_neighbor};
use crate::cube::{CUBE_FACES, MAX_CODED_LEVEL, TileKey, cube_to_sphere, face_frame};
use crate::mesh::{TileMeshData, cell_meters};
use crate::ordered::OrderedMap;

/// Priority offset that queues culled children of split tiles after every visible request.
const CULLED_PREFETCH_PENALTY: f64 = 1e15;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TileRequest {
    pub key: TileKey,
    /// Higher builds first.
    pub priority: f64,
}

#[derive(Clone, Debug)]
pub struct PlanetLodOptions {
    pub radius_meters: f64,
    /// Declared terrain range, for validation and a global horizon bound.
    pub min_surface_height_meters: f64,
    pub max_surface_height_meters: f64,
    /// Surface radius nothing can be seen through; used for horizon culling.
    pub occluder_radius_meters: f64,
    /// Fixed outward band ignored by the LOD distance test; never sampled from terrain.
    pub lod_surface_band_meters: f64,
    pub resolution: usize,
    pub max_level: u32,
    /// Split distance at each level, divided by the reference radius.
    pub split_distance_ratios: Vec<f64>,
    /// Tiles with no use for this many selections may be evicted.
    pub retain_frames: u64,
    pub max_cached_tiles: usize,
}

/// HolmanDev Planet.cs distance table (Size = 1,000,000), normalised by Size.
pub const HOLMAN_SPLIT_DISTANCE_RATIOS: [f64; 16] = [
    f64::INFINITY,
    f64::INFINITY,
    f64::INFINITY,
    0.45,
    0.2,
    0.1,
    0.05,
    0.03,
    0.016,
    0.008,
    0.004,
    0.0023,
    0.0014,
    0.00075,
    0.0005,
    0.0003,
];

/// The viewing camera's part in selection: its own split distance test and, when present, the
/// only horizon used for culling, so what is drawn is what the camera can see.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LodCamera {
    /// Body-fixed camera position, meters.
    pub position: DVec3,
    /// Multiplies the split thresholds for the camera, on top of the view's distance scale.
    pub distance_scale: f64,
    /// The camera splits no tile at or beyond this level; observers may still.
    pub max_level: u32,
    /// Viewport height over 2 tan(vertical fov / 2): pixels per meter at one meter of distance.
    pub focal_pixels: f64,
    /// An observer splits a tile only while the children's cells would still be at least this
    /// many pixels at the tile's nearest point to the camera.
    pub min_observer_cell_pixels: f64,
}

#[derive(Clone, Debug)]
pub struct LodView {
    /// Body-fixed observation points (the craft). A tile splits for its nearest observer.
    /// Without a camera, a tile is horizon-culled only when below every observer's horizon.
    pub observer_positions: Vec<DVec3>,
    pub camera: Option<LodCamera>,
    pub distance_scale: f64,
    pub horizon_culling: bool,
}

#[derive(Clone, Debug)]
pub struct LodNode {
    pub key: TileKey,
    pub code: u64,
    pub parent: Option<u64>,
    pub children: Option<[u64; 4]>,
    /// Fixed reference-sphere centre, used only for the LOD distance.
    pub lod_center: DVec3,
    pub lod_axis_u: DVec3,
    pub lod_axis_v: DVec3,
    pub center_direction: DVec3,
    /// Conservative angular cap of this UV patch, independent of the terrain mesh.
    pub angular_radius: f64,
    pub data: Option<Arc<TileMeshData>>,
    pub last_used_frame: u64,
    /// Diagnostics from the most recent selection.
    pub split_priority: f64,
}

/// One temporary coarsening by neighbour balancing: finer selected tiles under `parent` replaced
/// by it, because a neighbour at least two levels coarser could not be refined yet.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LodCollapse {
    pub parent: TileKey,
    /// Selected tiles the parent replaced, and the finest level among them.
    pub replaced: usize,
    pub finest_replaced_level: u32,
    /// The coarse neighbour that forced it.
    pub coarse_neighbor: TileKey,
}

#[derive(Clone, Debug)]
pub struct LodSelection {
    pub frame: u64,
    /// Tile codes to draw, in traversal order.
    pub render: Vec<u64>,
    pub requests: Vec<TileRequest>,
    pub horizon_culled: usize,
    /// Balancing coarsenings applied this frame; empty when refinement alone balanced it.
    pub balance_collapses: Vec<LodCollapse>,
    pub visited: usize,
    pub select_seconds: f64,
    pub traversal_seconds: f64,
    pub balance_seconds: f64,
    pub eviction_seconds: f64,
}

/// Cube-sphere quadtree with per-level distance thresholds.
///
/// Invariants:
/// - a node is only replaced by its children once all four visible children have data, so
///   refinement never opens a hole;
/// - a selected node has mesh data; missing data is an invariant failure.
pub struct PlanetLod {
    pub options: PlanetLodOptions,
    roots: [u64; 6],
    nodes: OrderedMap<LodNode>,
    frame: u64,
    ready_count: usize,
    ready_mesh_bytes: usize,
    /// Builds in flight keep their node alive until the result is accepted.
    pinned_builds: std::collections::HashSet<u64>,
}

struct Margin {
    margin: f64,
    distance: f64,
    threshold: f64,
}

impl PlanetLod {
    pub fn new(options: PlanetLodOptions) -> Self {
        let o = &options;
        assert!(
            o.radius_meters.is_finite()
                && o.radius_meters > 0.0
                && o.occluder_radius_meters.is_finite()
                && o.occluder_radius_meters > 0.0
                && o.max_surface_height_meters.is_finite()
                && o.lod_surface_band_meters.is_finite()
                && o.lod_surface_band_meters >= 0.0
                && o.lod_surface_band_meters <= o.max_surface_height_meters
                && o.radius_meters + o.max_surface_height_meters >= o.occluder_radius_meters,
            "planet LOD: invalid radii or surface band: {o:?}"
        );
        assert!(
            o.max_level <= MAX_CODED_LEVEL && o.split_distance_ratios.len() >= o.max_level as usize,
            "planet LOD: max level {} with {} split ratios",
            o.max_level,
            o.split_distance_ratios.len()
        );
        for level in 0..o.max_level as usize {
            assert!(
                o.split_distance_ratios[level] > 0.0,
                "planet LOD: split ratio {} at level {level}",
                o.split_distance_ratios[level]
            );
        }
        let mut lod = Self {
            options,
            roots: [0; 6],
            nodes: OrderedMap::new(),
            frame: 0,
            ready_count: 0,
            ready_mesh_bytes: 0,
            pinned_builds: Default::default(),
        };
        for face in CUBE_FACES {
            lod.roots[usize::from(face)] = lod.create_node(TileKey::root(face), None);
        }
        lod
    }

    pub fn cached_tile_count(&self) -> usize {
        self.ready_count
    }

    pub fn cached_mesh_bytes(&self) -> usize {
        self.ready_mesh_bytes
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn node(&self, code: u64) -> Option<&LodNode> {
        self.nodes.get(code)
    }

    fn node_ref(&self, code: u64) -> &LodNode {
        self.nodes
            .get(code)
            .unwrap_or_else(|| panic!("planet LOD: no node {code}"))
    }

    fn node_mut(&mut self, code: u64) -> &mut LodNode {
        self.nodes
            .get_mut(code)
            .unwrap_or_else(|| panic!("planet LOD: no node {code}"))
    }

    pub fn pin_build(&mut self, key: TileKey) {
        let code = key.code();
        let node = self
            .nodes
            .get(code)
            .unwrap_or_else(|| panic!("pin build: {key} has no node"));
        assert!(node.data.is_none(), "pin build: {key} already has a mesh");
        assert!(
            self.pinned_builds.insert(code),
            "pin build: {key} is already in flight"
        );
    }

    pub fn unpin_build(&mut self, key: TileKey) {
        assert!(
            self.pinned_builds.remove(&key.code()),
            "unpin build: {key} was not pinned"
        );
        assert!(
            self.nodes.get(key.code()).is_some_and(|n| n.data.is_some()),
            "unpin build: {key} has no accepted mesh"
        );
    }

    /// Nominal grid spacing at a level, meters.
    pub fn spacing_meters(&self, level: u32) -> f64 {
        cell_meters(self.options.radius_meters, level, self.options.resolution)
    }

    pub fn accept_tile(&mut self, tile: Arc<TileMeshData>) {
        let (min, max) = (
            self.options.min_surface_height_meters,
            self.options.max_surface_height_meters,
        );
        assert!(
            tile.min_height_meters.is_finite()
                && tile.max_height_meters.is_finite()
                && tile.min_height_meters >= min
                && tile.max_height_meters <= max
                && tile.min_height_meters <= tile.max_height_meters,
            "accept tile: {} heights [{}, {}] exceed the declared [{min}, {max}]",
            tile.key,
            tile.min_height_meters,
            tile.max_height_meters
        );
        let frame = self.frame;
        let node = self
            .nodes
            .get_mut(tile.key.code())
            .unwrap_or_else(|| panic!("accept tile: unknown tile {}", tile.key));
        match &node.data {
            None => self.ready_count += 1,
            Some(old) => self.ready_mesh_bytes -= old.buffer_bytes(),
        }
        self.ready_mesh_bytes += tile.buffer_bytes();
        node.data = Some(tile);
        node.last_used_frame = frame;
    }

    pub fn select(&mut self, view: &LodView) -> LodSelection {
        let started = Instant::now();
        self.frame += 1;
        assert!(
            view.distance_scale.is_finite() && view.distance_scale > 0.0,
            "select: distance scale {}",
            view.distance_scale
        );
        assert!(!view.observer_positions.is_empty(), "select: no observers");
        for observer in &view.observer_positions {
            assert!(observer.is_finite(), "select: invalid observer {observer}");
        }
        if let Some(c) = &view.camera {
            assert!(
                c.position.is_finite()
                    && c.distance_scale.is_finite()
                    && c.distance_scale > 0.0
                    && c.max_level <= self.options.max_level
                    && c.focal_pixels.is_finite()
                    && c.focal_pixels > 0.0
                    && c.min_observer_cell_pixels.is_finite()
                    && c.min_observer_cell_pixels >= 0.0,
                "select: invalid camera {c:?} for max level {}",
                self.options.max_level
            );
        }

        let mut walk = Walk {
            render: Vec::new(),
            requests: OrderedMap::new(),
            pending_splits: Vec::new(),
            culled: 0,
            visited: 0,
        };
        for root in self.roots {
            if self.node_ref(root).data.is_none() {
                walk.request(self.node_ref(root), f64::INFINITY);
            }
        }
        if self
            .roots
            .iter()
            .all(|&root| self.node_ref(root).data.is_some())
        {
            for root in self.roots {
                self.visit(root, view, &mut walk);
            }
        }
        let traversal_finished = Instant::now();
        let mut collapses = Vec::new();
        let balanced = self.balance_selection(&walk.render, &mut walk.requests, &mut collapses);
        self.prefetch_balance_children(&walk.pending_splits, &balanced, &mut walk.requests);
        let balance_finished = Instant::now();
        let frame = self.frame;
        for &code in &balanced {
            self.node_mut(code).last_used_frame = frame;
        }
        self.evict(&balanced);
        let finished = Instant::now();
        LodSelection {
            frame,
            render: balanced,
            requests: walk.requests.values().copied().collect(),
            horizon_culled: walk.culled,
            balance_collapses: collapses,
            visited: walk.visited,
            select_seconds: (finished - started).as_secs_f64(),
            traversal_seconds: (traversal_finished - started).as_secs_f64(),
            balance_seconds: (balance_finished - traversal_finished).as_secs_f64(),
            eviction_seconds: (finished - balance_finished).as_secs_f64(),
        }
    }

    fn culled(&self, node: &LodNode, view: &LodView) -> bool {
        if !view.horizon_culling {
            return false;
        }
        match &view.camera {
            Some(c) => self.below_horizon(node, c.position),
            None => view
                .observer_positions
                .iter()
                .all(|&o| self.below_horizon(node, o)),
        }
    }

    fn patch_distance(&self, node: &LodNode, point: DVec3) -> f64 {
        let half_side = self.options.radius_meters / f64::from(1_u32 << node.key.level);
        let delta = point - node.lod_center;
        let u = (delta.dot(node.lod_axis_u).abs() - half_side).max(0.0);
        let v = (delta.dot(node.lod_axis_v).abs() - half_side).max(0.0);
        let radial = (delta.dot(node.center_direction).abs()
            - self.options.lod_surface_band_meters)
            .max(0.0);
        DVec3::new(u, v, radial).length()
    }

    /// Largest threshold − distance over the observers and the camera; positive means split. A
    /// zero threshold (a level nobody may split) still ranks requests by distance. With a camera,
    /// the observers' threshold is zero once the children's cells would be smaller than
    /// `min_observer_cell_pixels` on screen.
    fn split_margin(&self, node: &LodNode, view: &LodView) -> Margin {
        let level = node.key.level;
        let table_threshold = if level < self.options.max_level {
            self.options.radius_meters
                * self.options.split_distance_ratios[level as usize]
                * view.distance_scale
        } else {
            0.0
        };
        let mut threshold = table_threshold;
        let mut distance = view
            .observer_positions
            .iter()
            .map(|&o| self.patch_distance(node, o))
            .fold(f64::INFINITY, f64::min);
        let Some(camera) = &view.camera else {
            return Margin {
                margin: threshold - distance,
                distance,
                threshold,
            };
        };
        let camera_distance = self.patch_distance(node, camera.position);
        let child_cell_pixels =
            self.spacing_meters(level + 1) * camera.focal_pixels / camera_distance;
        if child_cell_pixels < camera.min_observer_cell_pixels {
            threshold = 0.0;
        }
        let mut margin = threshold - distance;
        let camera_threshold = if level < camera.max_level {
            table_threshold * camera.distance_scale
        } else {
            0.0
        };
        if camera_threshold - camera_distance > margin {
            margin = camera_threshold - camera_distance;
            distance = camera_distance;
            threshold = camera_threshold;
        }
        Margin {
            margin,
            distance,
            threshold,
        }
    }

    fn visit(&mut self, code: u64, view: &LodView, walk: &mut Walk) {
        walk.visited += 1;
        let frame = self.frame;
        self.node_mut(code).last_used_frame = frame;
        let node = self.node_ref(code);
        if self.culled(node, view) {
            walk.culled += 1;
            return;
        }
        assert!(
            node.data.is_some(),
            "visit: visible node {} has no mesh",
            node.key
        );
        let Margin {
            margin,
            distance,
            threshold,
        } = self.split_margin(node, view);
        let wants_split = margin > 0.0 && node.key.level < self.options.max_level;
        self.node_mut(code).split_priority = margin;
        if !wants_split {
            walk.render.push(code);
            return;
        }
        let children = self.ensure_children(code);
        let mut ready = true;
        for child in children {
            self.node_mut(child).last_used_frame = frame;
            let child_node = self.node_ref(child);
            if child_node.data.is_some() {
                continue;
            }
            // A child we cannot see need not exist before we split, but it is built anyway (after
            // every visible tile): when it comes over a widening horizon the parent can stay split,
            // instead of redrawing coarse and forcing balancing to collapse the ground beside it.
            if self.culled(child_node, view) {
                walk.request(child_node, margin - CULLED_PREFETCH_PENALTY);
                continue;
            }
            ready = false;
            walk.request(child_node, margin);
        }
        if ready {
            for child in children {
                self.visit(child, view, walk);
            }
        } else {
            // Keeping a ready parent visible while its children build is the defined split
            // transition, not recovery from an invalid state.
            let node = self.node_ref(code);
            assert!(
                node.data.is_some(),
                "split parent {} has no mesh (distance {distance}, threshold {threshold})",
                node.key
            );
            walk.render.push(code);
            walk.pending_splits.push(code);
        }
    }

    /// Refine coarse neighbours when ready; otherwise request them and temporarily coarsen the
    /// fine side.
    fn balance_selection(
        &mut self,
        render: &[u64],
        requests: &mut OrderedMap<TileRequest>,
        collapses: &mut Vec<LodCollapse>,
    ) -> Vec<u64> {
        let mut selected: OrderedMap<()> = OrderedMap::new();
        for &code in render {
            selected.insert(code, ());
        }
        let mut refining = true;
        for _pass in 0..10_000 {
            let mut collapse: OrderedMap<()> = OrderedMap::new();
            let mut collapse_cause: OrderedMap<u64> = OrderedMap::new();
            let mut split: OrderedMap<[u64; 4]> = OrderedMap::new();
            for code in selected.key_snapshot() {
                let key = self.node_ref(code).key;
                for edge in FACE_EDGES {
                    let neighbor = selected_neighbor(key, edge, |c| selected.contains_key(c));
                    let Some(neighbor) = neighbor else { continue };
                    let neighbor_level = self.node_ref(neighbor).key.level;
                    if key.level.saturating_sub(neighbor_level) <= 1 {
                        continue;
                    }
                    let children = if refining {
                        Some(self.ensure_children(neighbor))
                    } else {
                        self.node_ref(neighbor).children
                    };
                    let all_ready = children.is_some_and(|c| {
                        c.iter().all(|&child| self.node_ref(child).data.is_some())
                    });
                    if refining && all_ready {
                        split.insert(neighbor, children.expect("checked"));
                        continue;
                    }
                    if refining && let Some(children) = children {
                        let priority = self.node_ref(code).split_priority;
                        for child in children {
                            let child_node = self.node_ref(child);
                            if child_node.data.is_none() && !requests.contains_key(child) {
                                requests.insert(
                                    child,
                                    TileRequest {
                                        key: child_node.key,
                                        priority,
                                    },
                                );
                            }
                        }
                    }
                    let parent = self.node_ref(code).parent;
                    let parent = parent
                        .filter(|&p| self.node_ref(p).data.is_some())
                        .unwrap_or_else(|| {
                            panic!(
                                "balance: fine tile {key} has no ready parent (neighbour {})",
                                self.node_ref(neighbor).key
                            )
                        });
                    collapse.insert(parent, ());
                    if !collapse_cause.contains_key(parent) {
                        collapse_cause.insert(parent, neighbor);
                    }
                }
            }
            if split.is_empty() && refining {
                // A stable selection needs neither the collapse pass nor another neighbour scan.
                if collapse.is_empty() {
                    return selected.key_snapshot();
                }
                refining = false;
                continue;
            }
            if collapse.is_empty() && split.is_empty() {
                return selected.key_snapshot();
            }
            for code in split.key_snapshot() {
                if selected.remove(code).is_none() {
                    continue;
                }
                for child in *split.get(code).expect("listed") {
                    selected.insert(child, ());
                }
            }
            if refining {
                continue;
            }
            let mut parents = collapse.key_snapshot();
            parents.sort_by_key(|&p| self.node_ref(p).key.level);
            for parent in parents {
                let parent_key = self.node_ref(parent).key;
                let mut ancestor = self.node_ref(parent).parent;
                let mut covered = false;
                while let Some(a) = ancestor {
                    if selected.contains_key(a) {
                        covered = true;
                        break;
                    }
                    ancestor = self.node_ref(a).parent;
                }
                if covered {
                    continue;
                }
                let (mut replaced, mut finest) = (0, parent_key.level);
                for code in selected.key_snapshot() {
                    let key = self.node_ref(code).key;
                    if key.level < parent_key.level || key.face != parent_key.face {
                        continue;
                    }
                    let shift = key.level - parent_key.level;
                    if key.x >> shift == parent_key.x && key.y >> shift == parent_key.y {
                        selected.remove(code);
                        replaced += 1;
                        finest = finest.max(key.level);
                    }
                }
                selected.insert(parent, ());
                let cause = *collapse_cause
                    .get(parent)
                    .expect("every collapse records its cause");
                collapses.push(LodCollapse {
                    parent: parent_key,
                    replaced,
                    finest_replaced_level: finest,
                    coarse_neighbor: self.node_ref(cause).key,
                });
            }
        }
        panic!(
            "balance: no convergence at frame {} with {} selected",
            self.frame,
            selected.len()
        );
    }

    /// When a waiting split lands, its children sit two levels finer than any coarser drawn
    /// neighbour, and balancing must refine that neighbour; requesting the neighbour's children
    /// with the split's lets both arrive at once.
    fn prefetch_balance_children(
        &mut self,
        pending_splits: &[u64],
        balanced: &[u64],
        requests: &mut OrderedMap<TileRequest>,
    ) {
        if pending_splits.is_empty() {
            return;
        }
        let mut selected: OrderedMap<()> = OrderedMap::new();
        for &code in balanced {
            selected.insert(code, ());
        }
        let frame = self.frame;
        for &code in pending_splits {
            if !selected.contains_key(code) {
                continue;
            }
            let (key, priority) = {
                let node = self.node_ref(code);
                (node.key, node.split_priority)
            };
            for edge in FACE_EDGES {
                let Some(neighbor) = selected_neighbor(key, edge, |c| selected.contains_key(c))
                else {
                    continue;
                };
                if self.node_ref(neighbor).key.level >= key.level {
                    continue;
                }
                for child in self.ensure_children(neighbor) {
                    let child_node = self.node_mut(child);
                    child_node.last_used_frame = frame;
                    if child_node.data.is_none() && !requests.contains_key(child) {
                        let request = TileRequest {
                            key: child_node.key,
                            priority,
                        };
                        requests.insert(child, request);
                    }
                }
            }
        }
    }

    fn ensure_children(&mut self, code: u64) -> [u64; 4] {
        if let Some(children) = self.node_ref(code).children {
            return children;
        }
        let keys = self.node_ref(code).key.children();
        let children = keys.map(|key| self.create_node(key, Some(code)));
        self.node_mut(code).children = Some(children);
        children
    }

    fn create_node(&mut self, key: TileKey, parent: Option<u64>) -> u64 {
        let b = key.uv_bounds();
        let center_direction = cube_to_sphere(key.face, (b.u0 + b.u1) / 2.0, (b.v0 + b.v1) / 2.0);
        let lod_axis_u = tangent_axis(face_frame(key.face).a, center_direction);
        let code = key.code();
        self.nodes.insert(
            code,
            LodNode {
                key,
                code,
                parent,
                children: None,
                lod_center: center_direction * self.options.radius_meters,
                lod_axis_u,
                lod_axis_v: center_direction.cross(lod_axis_u),
                center_direction,
                angular_radius: tile_angular_radius(b.u0, b.v0, b.u1, b.v1),
                data: None,
                last_used_frame: self.frame,
                split_priority: 0.0,
            },
        );
        code
    }

    /// Hidden only when the tile's whole direction cap lies beyond the largest horizon angle the
    /// declared global surface radius allows; no per-tile mesh heights are used.
    fn below_horizon(&self, node: &LodNode, observer: DVec3) -> bool {
        let observer_radius = observer.length();
        assert!(
            observer_radius.is_finite(),
            "below horizon: invalid observer {observer}"
        );
        let occluder = self.options.occluder_radius_meters;
        if observer_radius <= occluder {
            return false;
        }
        let top = self.options.radius_meters + self.options.max_surface_height_meters;
        let horizon_angle = f64::acos(occluder / observer_radius) + f64::acos(occluder / top);
        let center_angle =
            f64::acos((observer.dot(node.center_direction) / observer_radius).clamp(-1.0, 1.0));
        center_angle - node.angular_radius > horizon_angle
    }

    fn evict(&mut self, rendered: &[u64]) {
        if self.ready_count <= self.options.max_cached_tiles {
            return;
        }
        let rendered: std::collections::HashSet<u64> = rendered.iter().copied().collect();
        let mut candidates: Vec<(u64, u64)> = self
            .nodes
            .values()
            .filter(|n| {
                n.data.is_some()
                    && n.children.is_none()
                    && n.key.level > 1
                    && !rendered.contains(&n.code)
                    && self.frame - n.last_used_frame > self.options.retain_frames
            })
            .map(|n| (n.code, n.last_used_frame))
            .collect();
        // Stable, so equal frames keep the nodes' insertion order.
        candidates.sort_by_key(|&(_, frame)| frame);
        let floor = self.options.max_cached_tiles as f64 * 0.85;
        for (code, _) in candidates {
            if self.ready_count as f64 <= floor {
                break;
            }
            let data = self
                .node_mut(code)
                .data
                .take()
                .expect("an eviction candidate has a mesh");
            self.ready_mesh_bytes -= data.buffer_bytes();
            self.ready_count -= 1;
        }
        self.prune_unused_branches();
    }

    /// Drop subtrees with no data anywhere below and no recent use.
    fn prune_unused_branches(&mut self) {
        for root in self.roots {
            self.prune(root);
        }
    }

    fn prune(&mut self, code: u64) -> bool {
        let unused = |lod: &Self, node: &LodNode| {
            node.data.is_none()
                && !lod.pinned_builds.contains(&node.code)
                && lod.frame - node.last_used_frame > lod.options.retain_frames
        };
        let Some(children) = self.node_ref(code).children else {
            return unused(self, self.node_ref(code));
        };
        // Every child is visited, even after one is found not prunable.
        let removable = children.map(|child| self.prune(child)).iter().all(|&r| r);
        if removable {
            for child in children {
                self.nodes.remove(child);
            }
            self.node_mut(code).children = None;
        }
        removable && unused(self, self.node_ref(code))
    }
}

/// The traversal's output and the requests it makes.
struct Walk {
    render: Vec<u64>,
    requests: OrderedMap<TileRequest>,
    pending_splits: Vec<u64>,
    culled: usize,
    visited: usize,
}

impl Walk {
    fn request(&mut self, node: &LodNode, priority: f64) {
        if node.data.is_none() && !self.requests.contains_key(node.code) {
            self.requests.insert(
                node.code,
                TileRequest {
                    key: node.key,
                    priority,
                },
            );
        }
    }
}

fn tangent_axis(axis: DVec3, radial: DVec3) -> DVec3 {
    let tangent = axis - radial * axis.dot(radial);
    let length = tangent.length();
    assert!(
        length.is_finite() && length >= 1e-12,
        "tangent axis: invalid basis from {axis} and {radial}"
    );
    tangent / length
}

/// Upper angular radius of a tangent-warped cube-face UV rectangle.
fn tile_angular_radius(u0: f64, v0: f64, u1: f64, v1: f64) -> f64 {
    let center_u = f64::tan((u0 + u1) * FRAC_PI_4 / 2.0);
    let center_v = f64::tan((v0 + v1) * FRAC_PI_4 / 2.0);
    let du = (f64::tan(u0 * FRAC_PI_4) - center_u)
        .abs()
        .max((f64::tan(u1 * FRAC_PI_4) - center_u).abs());
    let dv = (f64::tan(v0 * FRAC_PI_4) - center_v)
        .abs()
        .max((f64::tan(v1 * FRAC_PI_4) - center_v).abs());
    // Normalising cube vectors of length ≥ 1 cannot enlarge their chord distance.
    2.0 * f64::asin((du.hypot(dv) / 2.0).min(1.0))
}
