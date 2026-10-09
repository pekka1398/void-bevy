//! Planet terrain in Bevy: the LOD quadtree's selection drawn as one entity per tile, each tile its
//! own anchor (its f64 origin minus the f64 camera position becomes the f32 translation). Tiles
//! build in the background on Bevy's compute pool; edges against a coarser neighbour are stitched.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

use bevy::asset::RenderAssetUsages;
use bevy::camera::primitives::MeshAabb;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::ecs::query::QueryFilter;
use bevy::mesh::{Indices, MeshAccessError, MeshVertexAttribute};
use bevy::pbr::wireframe::{Wireframe, WireframeColor};
use bevy::prelude::*;
use bevy::render::render_resource::{PrimitiveTopology, VertexFormat};
use bevy::tasks::{AsyncComputeTaskPool, Task, futures::check_ready};
use glam::DVec3;
use void_lod::{
    FACE_EDGES, LodSelection, LodView, PlanetLod, PlanetLodOptions, SurfaceSample, SurfaceSampler,
    TileMeshData, TileMeshOptions, build_tile_indices, build_tile_mesh, selected_neighbor,
    stitch_edges,
};

/// Each vertex's surface height above the reference radius, metres (lab/lod's `height` attribute).
pub const ATTRIBUTE_HEIGHT: MeshVertexAttribute =
    MeshVertexAttribute::new("Height", 917_330_201, VertexFormat::Float32);

/// Physical sampling cell for frequency-aware material detail, constant over each tile.
pub const ATTRIBUTE_CELL: MeshVertexAttribute =
    MeshVertexAttribute::new("TerrainCell", 917_330_202, VertexFormat::Float32);

/// Marks a terrain tile entity.
#[derive(Component)]
pub struct Tile;

/// A tile or object at a body-fixed f64 position, relative to the camera at the origin: the f64
/// subtraction happens before anything becomes f32.
pub fn anchor(position: DVec3, camera: DVec3) -> Transform {
    Transform::from_translation((position - camera).as_vec3())
}

/// A level's colour: hue around the wheel, so neighbouring levels differ.
pub fn level_color(level: u32) -> [f32; 3] {
    let c: Srgba = Color::hsl((level as f32 * 47.0) % 360.0, 0.55, 0.55).into();
    [c.red, c.green, c.blue]
}

/// Last update only: workers never accumulate samples outside an explicit capture.
#[derive(Default)]
pub struct TileProfile {
    pub traversal_ms: f64,
    pub balance_ms: f64,
    pub balance_cache_hits: usize,
    pub eviction_ms: f64,
    pub finish_ms: f64,
    pub schedule_ms: f64,
    pub draw_ms: f64,
    pub mesh_ms: f64,
    pub visited: usize,
    pub scheduled: usize,
    pub completed: usize,
    pub created: usize,
    pub removed: usize,
    pub seam_rebuilds: usize,
    pub worker_ms: Vec<f64>,
    pub worker_queue_ms: Vec<f64>,
    pub finish_lag_ms: Vec<f64>,
    pub mesh_upload_bytes: usize,
    pub main_mesh_payload_bytes: usize,
}
struct BuiltTile {
    mesh: TileMeshData,
    build_ms: Option<f64>,
    queue_ms: Option<f64>,
    completed_at: Option<Instant>,
}

/// The quadtree, its background builds and its drawn tiles, drawn with material `M`.
pub struct TileField<M: Material = StandardMaterial> {
    pub lod: PlanetLod,
    /// The surface tiles are built on; None draws a smooth sphere coloured by tile level.
    pub terrain: Option<Arc<dyn SurfaceSampler + Send + Sync>>,
    building: HashMap<u64, Task<BuiltTile>>,
    pub profile: TileProfile,
    profiling: bool,
    build_slots: usize,
    drawn_selection: Vec<u64>,
    wanted: HashMap<u64, [Option<u64>; 4]>,
    /// Drawn tiles: entity and the coarse neighbours its seams are stitched to.
    drawn: HashMap<u64, (Entity, [Option<u64>; 4])>,
    owned_meshes: HashMap<Entity, AssetId<Mesh>>,
    indices: Indices,
    mesh_asset_usage: RenderAssetUsages,
    gpu_packing: bool,
    material: Handle<M>,
    render: Vec<u64>,
    pub last_requests: usize,
    pub last_select_ms: f64,
    /// Finest and coarsest drawn level.
    pub levels: (u32, u32),
    /// Leave tiles out of Bevy's frustum culling, for a material that moves vertices beyond the
    /// mesh's bounds (the sea is raised in the vertex shader).
    pub no_frustum_culling: bool,
    /// Opt-in conservative bound for material vertex displacement, in local metres.
    pub vertex_displacement_bound: Option<f32>,
    /// Draw every tile's triangle edges (Bevy's wireframe; needs `WireframePlugin`).
    wireframe: bool,
    /// Their colour; white by default.
    pub wireframe_color: Color,
}

impl<M: Material> TileField<M> {
    pub fn new(
        options: PlanetLodOptions,
        terrain: Option<Arc<dyn SurfaceSampler + Send + Sync>>,
        material: Handle<M>,
    ) -> Self {
        let (indices, grid) = build_tile_indices(options.resolution);
        let force_u16 = std::env::args().any(|arg| arg == "--lod-u16-indices");
        let force_u32 = std::env::args().any(|arg| arg == "--lod-u32-indices");
        assert!(!(force_u16 && force_u32), "choose one LOD index format");
        // Both representations have identical topology; use U16 when every index fits.
        let fits_u16 = indices[..grid]
            .iter()
            .all(|&index| u16::try_from(index).is_ok());
        let indices = if force_u16 || (!force_u32 && fits_u16) {
            Indices::U16(
                indices[..grid]
                    .iter()
                    .map(|&i| u16::try_from(i).expect("tile index exceeds requested U16 format"))
                    .collect(),
            )
        } else {
            Indices::U32(indices[..grid].to_vec())
        };
        let render_only = std::env::args().any(|arg| arg == "--lod-render-only");
        let retain_main = std::env::args().any(|arg| arg == "--lod-main-world-meshes");
        assert!(
            !(render_only && retain_main),
            "choose one LOD mesh residency policy"
        );
        let mesh_asset_usage = if retain_main {
            RenderAssetUsages::default()
        } else {
            // The authoritative f64/raw tile stays in PlanetLod; Bevy can release its
            // duplicate CPU upload arrays while keeping the handle and cached AABB.
            RenderAssetUsages::RENDER_WORLD
        };
        let mut lod = PlanetLod::new(options);
        lod.set_balance_cache_enabled(!std::env::args().any(|arg| arg == "--lod-full-balance"));
        Self {
            lod,
            terrain,
            building: HashMap::new(),
            profile: TileProfile::default(),
            profiling: false,
            build_slots: std::thread::available_parallelism().map_or(4, |n| n.get()) * 2,
            drawn_selection: Vec::new(),
            wanted: HashMap::new(),
            drawn: HashMap::new(),
            // Skirts are left out: seams are stitched, as the LOD lab draws by default.
            indices,
            mesh_asset_usage,
            gpu_packing: crate::gpu_lod::enabled(),
            material,
            owned_meshes: HashMap::new(),
            render: Vec::new(),
            last_requests: 0,
            last_select_ms: 0.0,
            levels: (0, 0),
            no_frustum_culling: false,
            vertex_displacement_bound: None,
            wireframe: false,
            wireframe_color: Color::WHITE,
        }
    }

    /// Cancel this scene's pending work and release all app-owned rendered meshes.
    /// A dropped Task cannot deliver into a replacement TileField/generation.
    pub fn unload(&mut self, commands: &mut Commands, meshes: &mut Assets<Mesh>) {
        self.building.clear();
        for (_, (entity, _)) in self.drawn.drain() {
            commands.entity(entity).despawn();
        }
        for (_, mesh) in self.owned_meshes.drain() {
            meshes.remove(mesh);
        }
        self.render.clear();
        self.drawn_selection.clear();
        self.wanted.clear();
    }
    pub fn owned_mesh_count(&self) -> usize {
        self.owned_meshes.len()
    }
    pub fn wireframe(&self) -> bool {
        self.wireframe
    }

    /// Triangle edges on every drawn tile, as the LOD lab's mesh-edge overlay.
    pub fn set_wireframe(&mut self, commands: &mut Commands, on: bool) {
        self.wireframe = on;
        for (entity, _) in self.drawn.values() {
            if on {
                commands.entity(*entity).insert((
                    Wireframe,
                    WireframeColor {
                        color: self.wireframe_color,
                    },
                ));
            } else {
                commands
                    .entity(*entity)
                    .remove::<(Wireframe, WireframeColor)>();
            }
        }
    }

    /// Every drawn tile's four edges as polylines relative to the camera at `eye` (the LOD lab's
    /// red tile boundaries), along the tile's own grid vertices.
    pub fn boundaries(&self, eye: DVec3) -> Vec<Vec<Vec3>> {
        let n = self.lod.options.resolution;
        let mut lines = Vec::with_capacity(self.drawn.len() * 4);
        for code in self.drawn.keys() {
            let Some(data) = self.lod.node(*code).and_then(|node| node.data.as_ref()) else {
                continue;
            };
            let offset = (data.origin - eye).as_vec3();
            let at = |i: usize, j: usize| Vec3::from_array(data.positions[j * n + i]) + offset;
            lines.push((0..n).map(|i| at(i, 0)).collect());
            lines.push((0..n).map(|i| at(i, n - 1)).collect());
            lines.push((0..n).map(|j| at(0, j)).collect());
            lines.push((0..n).map(|j| at(n - 1, j)).collect());
        }
        lines
    }

    pub fn drawn_count(&self) -> usize {
        self.drawn.len()
    }

    pub fn building_count(&self) -> usize {
        self.building.len()
    }

    pub fn set_profiling(&mut self, enabled: bool) {
        self.profiling = enabled;
        self.profile = TileProfile::default();
    }

    pub fn record_profile(&self, profiler: &mut void_diagnostics::Profiler) {
        let p = &self.profile;
        for (name, ms) in [
            ("lod_traversal", p.traversal_ms),
            ("lod_balance", p.balance_ms),
            ("lod_eviction", p.eviction_ms),
            ("lod_finish_builds", p.finish_ms),
            ("lod_schedule", p.schedule_ms),
            ("lod_draw", p.draw_ms),
            ("lod_create_mesh", p.mesh_ms),
        ] {
            profiler.sample(name, ms);
        }
        for &ms in &p.worker_ms {
            profiler.sample("lod_worker_build", ms);
        }
        for &ms in &p.worker_queue_ms {
            profiler.sample("lod_worker_queue_wait", ms);
        }
        for &ms in &p.finish_lag_ms {
            profiler.sample("lod_worker_finish_lag", ms);
        }
        for (name, count) in [
            ("lod_visited", p.visited),
            ("lod_balance_cache_hits", p.balance_cache_hits),
            ("lod_scheduled", p.scheduled),
            ("lod_completed", p.completed),
            ("lod_mesh_created", p.created),
            ("lod_mesh_removed", p.removed),
            ("lod_mesh_upload_payload_bytes", p.mesh_upload_bytes),
            ("lod_main_mesh_payload_bytes", p.main_mesh_payload_bytes),
            ("lod_seam_rebuilds", p.seam_rebuilds),
            ("lod_pending_builds", self.building.len()),
            ("lod_requests", self.last_requests),
            ("lod_drawn", self.drawn.len()),
            (
                "lod_async_workers",
                AsyncComputeTaskPool::get().thread_num(),
            ),
            ("lod_cached_mesh_bytes", self.lod.cached_mesh_bytes()),
        ] {
            profiler.counter(name, count as f64);
        }
    }

    /// Accept tiles whose background build has finished.
    pub fn finish_builds(&mut self) {
        let started = self.profiling.then(Instant::now);
        let mut done = Vec::new();
        for (code, task) in &mut self.building {
            if let Some(tile) = check_ready(task) {
                done.push((*code, tile));
            }
        }
        for (code, tile) in done {
            self.building.remove(&code);
            self.profile.completed += usize::from(self.profiling);
            if self.profiling
                && let Some(ms) = tile.build_ms
            {
                self.profile.worker_ms.push(ms);
            }
            if self.profiling {
                if let Some(ms) = tile.queue_ms {
                    self.profile.worker_queue_ms.push(ms);
                }
                if let Some(completed) = tile.completed_at {
                    self.profile
                        .finish_lag_ms
                        .push(completed.elapsed().as_secs_f64() * 1e3);
                }
            }
            let key = tile.mesh.key;
            self.lod.accept_tile(Arc::new(tile.mesh));
            self.lod.unpin_build(key);
        }
        if let Some(started) = started {
            self.profile.finish_ms = started.elapsed().as_secs_f64() * 1e3;
        }
    }

    /// Select for a view and start the most urgent builds, two per core.
    pub fn select(&mut self, view: &LodView) {
        let LodSelection {
            render,
            mut requests,
            select_seconds,
            traversal_seconds,
            balance_seconds,
            balance_cache_hit,
            eviction_seconds,
            visited,
            ..
        } = self.lod.select(view);
        if self.profiling {
            self.profile.traversal_ms = traversal_seconds * 1e3;
            self.profile.balance_ms = balance_seconds * 1e3;
            self.profile.balance_cache_hits = usize::from(balance_cache_hit);
            self.profile.eviction_ms = eviction_seconds * 1e3;
            self.profile.visited = visited;
        }
        let started = self.profiling.then(Instant::now);
        self.last_select_ms = select_seconds * 1e3;
        self.last_requests = requests.len();
        self.render = render;
        requests.sort_by(|a, b| b.priority.total_cmp(&a.priority));
        let slots = self.build_slots;
        let options = TileMeshOptions {
            radius_meters: self.lod.options.radius_meters,
            resolution: self.lod.options.resolution,
        };
        for request in requests {
            if self.building.len() >= slots {
                break;
            }
            let code = request.key.code();
            if self.building.contains_key(&code) {
                continue;
            }
            self.lod.pin_build(request.key);
            let key = request.key;
            let terrain = self.terrain.clone();
            let profiling = self.profiling;
            let queued_at = profiling.then(Instant::now);
            let task = AsyncComputeTaskPool::get().spawn(async move {
                let started = profiling.then(Instant::now);
                let queue_ms = queued_at.map(|queued| {
                    started
                        .expect("profiled worker start")
                        .duration_since(queued)
                        .as_secs_f64()
                        * 1e3
                });
                let mesh = match terrain {
                    Some(terrain) => build_tile_mesh(
                        key,
                        &|d: DVec3, cell: f64| terrain.sample(d, cell),
                        options,
                    ),
                    None => {
                        let color = level_color(key.level);
                        let sphere = |_direction: DVec3, _cell: f64| SurfaceSample {
                            height_meters: 0.0,
                            color,
                        };
                        build_tile_mesh(key, &sphere, options)
                    }
                };
                let completed_at = profiling.then(Instant::now);
                BuiltTile {
                    mesh,
                    build_ms: started
                        .map(|s| completed_at.unwrap().duration_since(s).as_secs_f64() * 1e3),
                    queue_ms,
                    completed_at,
                }
            });
            self.building.insert(code, task);
            self.profile.scheduled += usize::from(self.profiling);
        }
        if let Some(started) = started {
            self.profile.schedule_ms = started.elapsed().as_secs_f64() * 1e3;
        }
    }

    /// Topology depends only on selected keys, not camera motion or mesh contents.
    /// Main-game producers do not replace a ready tile in place; replacing a scene creates a
    /// new field. Like the original draw path, matching tile keys/seams retain their GPU mesh.
    fn refresh_topology(&mut self) -> bool {
        if self.drawn_selection == self.render {
            return false;
        }
        let selected: HashSet<u64> = self.render.iter().copied().collect();
        self.wanted.clear();
        for &code in &self.render {
            let key = self.lod.node(code).expect("a selected tile has a node").key;
            let seams = FACE_EDGES.map(|edge| {
                selected_neighbor(key, edge, |c| selected.contains(&c)).filter(|&nb| {
                    self.lod
                        .node(nb)
                        .is_some_and(|node| node.key.level + 1 == key.level)
                })
            });
            self.wanted.insert(code, seams);
        }
        self.drawn_selection.clone_from(&self.render);
        true
    }

    /// Spawn, move and despawn tile entities for the last selection, relative to the camera at `eye`.
    /// `tiles` reaches the tile entities' transforms; its filter is the caller's, so it can stay
    /// disjoint from the caller's other transform queries.
    pub fn draw<F: QueryFilter>(
        &mut self,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        tiles: &mut Query<&mut Transform, F>,
        eye: DVec3,
    ) {
        self.draw_with_gpu(commands, meshes, tiles, eye, None);
    }
    pub fn draw_with_gpu<F: QueryFilter>(
        &mut self,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        tiles: &mut Query<&mut Transform, F>,
        eye: DVec3,
        mut gpu: Option<&mut crate::gpu_lod::UploadContext<'_>>,
    ) {
        if self.gpu_packing {
            let context = gpu
                .as_ref()
                .expect("GPU LOD packing requires its plugin and upload context");
            if !context.shared.ready() {
                return;
            }
        }
        let started = self.profiling.then(Instant::now);
        let n = self.lod.options.resolution;
        let topology_changed = self.refresh_topology();
        // Tiles that left the selection, or whose stitched seams changed, are dropped and redrawn.
        if topology_changed {
            let wanted = &self.wanted;
            self.drawn.retain(|code, (entity, seams)| {
                let keep = wanted.get(code) == Some(seams);
                if !keep {
                    if self.profiling {
                        self.profile.removed += 1;
                        self.profile.seam_rebuilds += usize::from(wanted.contains_key(code));
                    }
                    commands.entity(*entity).despawn();
                    if let Some(mesh) = self.owned_meshes.remove(entity) {
                        meshes.remove(mesh);
                    }
                }
                keep
            });
        }
        let mut levels = (u32::MAX, 0);
        for (&code, &seams) in &self.wanted {
            let node = self.lod.node(code).expect("selected");
            levels = (levels.0.min(node.key.level), levels.1.max(node.key.level));
            let data = node.data.as_ref().expect("a selected tile has a mesh");
            if let Some((entity, _)) = self.drawn.get(&code) {
                if let Ok(mut transform) = tiles.get_mut(*entity) {
                    *transform = anchor(data.origin, eye);
                }
                continue;
            }
            let coarse = seams.map(|s| {
                s.map(|c| {
                    self.lod
                        .node(c)
                        .and_then(|n| n.data.as_deref())
                        .expect("a drawn neighbour has a mesh")
                })
            });
            let mesh_started = self.profiling.then(Instant::now);
            let (mesh, base_bounds, gpu_handle) = if self.gpu_packing {
                let context = gpu.as_deref_mut().unwrap();
                let upload = crate::gpu_lod::upload_data(
                    data,
                    coarse,
                    n,
                    &self.indices,
                    self.lod.options.radius_meters,
                );
                let base_bounds = upload.bounds;
                if self.profiling {
                    self.profile.mesh_upload_bytes += upload.bytes_len();
                }
                let mesh = meshes.add(upload.metadata_mesh());
                let handle = context
                    .assets
                    .add(upload.into_asset(mesh.clone(), context.shared));
                (
                    mesh,
                    Some(base_bounds),
                    Some(crate::gpu_lod::GpuTileHandle(handle)),
                )
            } else {
                let cpu_mesh = tile_mesh_for_upload(
                    data,
                    coarse,
                    n,
                    self.indices.clone(),
                    self.lod.options.radius_meters,
                    self.mesh_asset_usage,
                );
                let base_bounds = self
                    .vertex_displacement_bound
                    .map(|_| cpu_mesh.compute_aabb().expect("terrain mesh bounds"));
                if self.profiling {
                    self.profile.mesh_upload_bytes += mesh_payload_bytes(&cpu_mesh);
                }
                (meshes.add(cpu_mesh), base_bounds, None)
            };
            let bounds = base_bounds.map(|mut bounds| {
                if let Some(displacement) = self.vertex_displacement_bound {
                    assert!(displacement.is_finite() && displacement >= 0.0);
                    bounds.half_extents += displacement;
                }
                bounds
            });
            if let Some(started) = mesh_started {
                self.profile.mesh_ms += started.elapsed().as_secs_f64() * 1e3;
                self.profile.created += 1;
            }
            let entity = commands
                .spawn((
                    Tile,
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(self.material.clone()),
                    anchor(data.origin, eye),
                ))
                .id();
            if let Some(handle) = gpu_handle {
                commands.entity(entity).insert(handle);
            }
            if let Some(bounds) = bounds {
                commands.entity(entity).insert(bounds);
            }
            if self.no_frustum_culling && self.vertex_displacement_bound.is_none() {
                commands.entity(entity).insert(NoFrustumCulling);
            }
            if self.wireframe {
                commands.entity(entity).insert((
                    Wireframe,
                    WireframeColor {
                        color: self.wireframe_color,
                    },
                ));
            }
            self.owned_meshes.insert(entity, mesh.id());
            self.drawn.insert(code, (entity, seams));
        }
        self.levels = (levels.0.min(levels.1), levels.1);
        if let Some(started) = started {
            self.profile.draw_ms = started.elapsed().as_secs_f64() * 1e3;
            // This gauge scan is instrumentation work, excluded from lod_draw.
            self.profile.main_mesh_payload_bytes = self
                .owned_meshes
                .values()
                .map(|id| mesh_payload_bytes(meshes.get(*id).expect("app-owned tile mesh")))
                .sum();
        }
    }
}

fn mesh_payload_bytes(mesh: &Mesh) -> usize {
    match mesh.try_attributes() {
        Ok(attributes) => {
            let vertices: usize = attributes.map(|(_, values)| values.get_bytes().len()).sum();
            if vertices == 0 {
                return 0;
            }
            vertices
                + match mesh.indices().expect("terrain indices") {
                    Indices::U16(indices) => indices.len() * size_of::<u16>(),
                    Indices::U32(indices) => indices.len() * size_of::<u32>(),
                }
        }
        Err(MeshAccessError::ExtractedToRenderWorld) => 0,
        Err(error) => panic!("invalid terrain mesh payload: {error}"),
    }
}

#[cfg(test)]
fn tile_mesh(
    data: &TileMeshData,
    coarse: [Option<&TileMeshData>; 4],
    n: usize,
    indices: &[u32],
    radius: f64,
) -> Mesh {
    tile_mesh_for_upload(
        data,
        coarse,
        n,
        Indices::U32(indices.to_vec()),
        radius,
        RenderAssetUsages::default(),
    )
}

fn tile_mesh_for_upload(
    data: &TileMeshData,
    coarse: [Option<&TileMeshData>; 4],
    n: usize,
    indices: Indices,
    radius: f64,
    usage: RenderAssetUsages,
) -> Mesh {
    let (mut positions, mut normals, mut heights) = stitch_edges(data, coarse, n);
    let count = n * n;
    assert!(positions.len() >= count && normals.len() >= count && heights.len() >= count);
    positions.truncate(count);
    normals.truncate(count);
    heights.truncate(count);
    let colors: Vec<[f32; 4]> = data.colors[..count]
        .iter()
        .map(|c| [c[0], c[1], c[2], 1.0])
        .collect();
    Mesh::new(PrimitiveTopology::TriangleList, usage)
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
        .with_inserted_attribute(ATTRIBUTE_HEIGHT, heights)
        .with_inserted_attribute(
            ATTRIBUTE_CELL,
            vec![void_lod::cell_meters(radius, data.key.level, n) as f32; count],
        )
        .with_inserted_indices(indices)
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    #[test]
    fn render_only_u16_mesh_preserves_vertices_topology_and_cached_bounds() {
        for n in [33, 65] {
            let radius = 6_371_000.0;
            let data = build_tile_mesh(
                void_lod::TileKey {
                    face: 0,
                    level: 14,
                    x: 8192,
                    y: 8192,
                },
                &|d: DVec3, _: f64| SurfaceSample {
                    height_meters: 5000.0 + d.z * 10.0,
                    color: [0.2, 0.3, 0.4],
                },
                TileMeshOptions {
                    radius_meters: radius,
                    resolution: n,
                },
            );
            let (indices, grid) = build_tile_indices(n);
            let reference = tile_mesh(&data, [None; 4], n, &indices[..grid], radius);
            let short = Indices::U16(
                indices[..grid]
                    .iter()
                    .map(|&i| u16::try_from(i).unwrap())
                    .collect(),
            );
            let mut candidate = tile_mesh_for_upload(
                &data,
                [None; 4],
                n,
                short,
                radius,
                RenderAssetUsages::RENDER_WORLD,
            );
            assert_eq!(
                reference.create_packed_vertex_buffer_data(),
                candidate.create_packed_vertex_buffer_data()
            );
            assert_eq!(
                reference.indices().unwrap().iter().collect::<Vec<_>>(),
                candidate.indices().unwrap().iter().collect::<Vec<_>>()
            );
            assert_eq!(
                mesh_payload_bytes(&reference) - mesh_payload_bytes(&candidate),
                grid * 2
            );
            let before_bounds = candidate.compute_aabb().unwrap();
            let extracted = candidate.take_gpu_data().unwrap();
            assert_eq!(
                reference.create_packed_vertex_buffer_data(),
                extracted.create_packed_vertex_buffer_data()
            );
            assert_eq!(candidate.compute_aabb().unwrap(), before_bounds);
            assert!(matches!(
                candidate.try_attributes(),
                Err(MeshAccessError::ExtractedToRenderWorld)
            ));
            assert_eq!(mesh_payload_bytes(&candidate), 0);
        }
    }

    #[test]
    fn stable_selection_reuses_topology_and_changes_refresh_it() {
        let planet = void_landing::pebble();
        let demo = void_landing::demo_rocket(&planet.terrain);
        let options = void_landing::landing_lod_options(&planet.terrain, &demo.options.contact);
        let mut field: TileField = TileField::new(options, None, Handle::default());
        field.render = (0..6)
            .map(|face| {
                void_lod::TileKey {
                    face,
                    level: 0,
                    x: 0,
                    y: 0,
                }
                .code()
            })
            .collect();
        assert!(field.refresh_topology());
        let wanted = field.wanted.clone();
        assert_eq!(wanted.len(), 6);
        assert!(wanted.values().all(|seams| *seams == [None; 4]));
        assert!(!field.refresh_topology());
        assert_eq!(field.wanted, wanted);
        field.render.pop();
        assert!(field.refresh_topology());
        assert_eq!(field.wanted.len(), 5);
        assert!(!field.refresh_topology());
        // Topology caching does not cache camera-relative transforms.
        let origin = DVec3::splat(1e12);
        assert_ne!(
            anchor(origin, origin).translation,
            anchor(origin, origin + DVec3::X).translation
        );
    }

    #[test]
    fn mesh_attributes_equal_stitched_grid_without_skirts() {
        use bevy::mesh::VertexAttributeValues;
        let n = 5;
        let radius = 100e3;
        let data = build_tile_mesh(
            void_lod::TileKey {
                face: 0,
                level: 0,
                x: 0,
                y: 0,
            },
            &|d: DVec3, _: f64| SurfaceSample {
                height_meters: d.x * 10.0,
                color: [0.2, 0.3, 0.4],
            },
            TileMeshOptions {
                radius_meters: radius,
                resolution: n,
            },
        );
        let (indices, grid) = build_tile_indices(n);
        let (positions, normals, heights) = stitch_edges(&data, [None; 4], n);
        let mesh = tile_mesh(&data, [None; 4], n, &indices[..grid], radius);
        let Some(VertexAttributeValues::Float32x3(actual)) =
            mesh.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            panic!("positions")
        };
        assert_eq!(actual, &positions[..n * n]);
        let Some(VertexAttributeValues::Float32x3(actual)) = mesh.attribute(Mesh::ATTRIBUTE_NORMAL)
        else {
            panic!("normals")
        };
        assert_eq!(actual, &normals[..n * n]);
        let Some(VertexAttributeValues::Float32(actual)) = mesh.attribute(ATTRIBUTE_HEIGHT) else {
            panic!("heights")
        };
        assert_eq!(actual, &heights[..n * n]);
        let Some(Indices::U32(actual)) = mesh.indices() else {
            panic!("indices")
        };
        assert_eq!(actual, &indices[..grid]);
    }

    #[test]
    fn stitched_coarse_neighbor_mesh_matches_original_attributes() {
        use bevy::mesh::VertexAttributeValues;
        let n = 9;
        let options = TileMeshOptions {
            radius_meters: 100_000.0,
            resolution: n,
        };
        let sampler = |d: DVec3, _: f64| SurfaceSample {
            height_meters: 100.0 + d.z * 10.0,
            color: [0.2, 0.3, 0.4],
        };
        let coarse_key = void_lod::TileKey {
            face: 0,
            level: 1,
            x: 0,
            y: 0,
        };
        let coarse = build_tile_mesh(coarse_key, &sampler, options);
        let fine_key = void_lod::TileKey {
            face: 0,
            level: 2,
            x: 2,
            y: 0,
        };
        let fine = build_tile_mesh(fine_key, &sampler, options);
        let mut neighbors = [None; 4];
        neighbors[void_lod::FaceEdge::UMinus.index()] = Some(&coarse);
        let (positions, normals, heights) = stitch_edges(&fine, neighbors, n);
        assert_ne!(
            positions, fine.positions,
            "must exercise an actual seam correction"
        );
        let (indices, grid) = build_tile_indices(n);
        let mesh = tile_mesh(&fine, neighbors, n, &indices[..grid], options.radius_meters);
        assert_eq!(
            mesh.attribute(Mesh::ATTRIBUTE_POSITION),
            Some(&VertexAttributeValues::Float32x3(
                positions[..n * n].to_vec()
            ))
        );
        assert_eq!(
            mesh.attribute(Mesh::ATTRIBUTE_NORMAL),
            Some(&VertexAttributeValues::Float32x3(normals[..n * n].to_vec()))
        );
        assert_eq!(
            mesh.attribute(ATTRIBUTE_HEIGHT),
            Some(&VertexAttributeValues::Float32(heights[..n * n].to_vec()))
        );
    }

    #[test]
    fn expanded_bounds_contain_ocean_vertex_displacement_near_and_far() {
        let radius = 6_371_000.0;
        let sea = 5000.0_f32;
        let tile = build_tile_mesh(
            void_lod::TileKey {
                face: 0,
                level: 14,
                x: 8192,
                y: 8192,
            },
            &|_: DVec3, _: f64| SurfaceSample {
                height_meters: 0.0,
                color: [0.0; 3],
            },
            TileMeshOptions {
                radius_meters: radius,
                resolution: 9,
            },
        );
        let (indices, grid) = build_tile_indices(9);
        let mesh = tile_mesh(&tile, [None; 4], 9, &indices[..grid], radius);
        let mut bounds = mesh.compute_aabb().unwrap();
        bounds.half_extents += sea + 64.0 * f32::EPSILON * radius as f32;
        for altitude in [10.0, 500_000.0, 10_000_000.0] {
            let eye = tile.origin.normalize() * (radius + altitude);
            for rotation in [Quat::IDENTITY, Quat::from_rotation_z(1.3)] {
                let translation = rotation * (tile.origin - eye).as_vec3();
                let center = rotation * (-eye).as_vec3();
                for position in &tile.positions[..81] {
                    let world = rotation * Vec3::from_array(*position) + translation;
                    let displaced = world + (world - center).normalize() * sea;
                    let local = rotation.conjugate() * (displaced - translation);
                    let delta = (local - glam::Vec3::from(bounds.center)).abs();
                    assert!(
                        delta.cmple(glam::Vec3::from(bounds.half_extents)).all(),
                        "ocean escapes bounds at altitude {altitude}"
                    );
                }
            }
        }
    }

    #[test]
    fn unload_releases_entities_meshes_and_cannot_accept_old_jobs() {
        let planet = void_landing::pebble();
        let demo = void_landing::demo_rocket(&planet.terrain);
        let options = void_landing::landing_lod_options(&planet.terrain, &demo.options.contact);
        let mut field: TileField = TileField::new(
            options.clone(),
            Some(planet.terrain.clone()),
            Handle::default(),
        );
        let mut world = World::new();
        let mut meshes = Assets::<Mesh>::default();
        let pool = AsyncComputeTaskPool::get_or_init(bevy::tasks::TaskPool::new);
        let key = void_lod::TileKey {
            face: 0,
            level: 0,
            x: 0,
            y: 0,
        };
        let task = pool.spawn(async move {
            let mesh = build_tile_mesh(
                key,
                &|_: DVec3, _: f64| SurfaceSample {
                    height_meters: 123.0,
                    color: [1.0, 0.0, 0.0],
                },
                TileMeshOptions {
                    radius_meters: 100e3,
                    resolution: 33,
                },
            );
            BuiltTile {
                mesh,
                build_ms: None,
                queue_ms: None,
                completed_at: None,
            }
        });
        field.building.insert(key.code(), task);
        let mesh = meshes.add(Sphere::new(1.0).mesh().uv(8, 4));
        let entity = world.spawn((Tile, Mesh3d(mesh.clone()))).id();
        field.drawn.insert(key.code(), (entity, [None; 4]));
        field.owned_meshes.insert(entity, mesh.id());
        field.unload(&mut world.commands(), &mut meshes);
        world.flush();
        assert!(world.get_entity(entity).is_err());
        assert!(meshes.get(mesh.id()).is_none());
        assert_eq!(
            (
                field.building_count(),
                field.drawn_count(),
                field.owned_mesh_count()
            ),
            (0, 0, 0)
        );
        // A replacement scene owns a different job map. Even a previously completed old job is
        // discarded rather than accepted as the same TileKey on a different body/configuration.
        let mut replacement: TileField =
            TileField::new(options, Some(planet.terrain), Handle::default());
        replacement.finish_builds();
        assert!(replacement.lod.node(key.code()).unwrap().data.is_none());
    }
}
