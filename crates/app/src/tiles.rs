//! Planet terrain in Bevy: the LOD quadtree's selection drawn as one entity per tile, each tile its
//! own anchor (its f64 origin minus the f64 camera position becomes the f32 translation). Tiles
//! build in the background on Bevy's compute pool; edges against a coarser neighbour are stitched.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use bevy::asset::RenderAssetUsages;
use bevy::ecs::query::QueryFilter;
use bevy::mesh::Indices;
use bevy::prelude::*;
use bevy::render::render_resource::PrimitiveTopology;
use bevy::tasks::{AsyncComputeTaskPool, Task, futures::check_ready};
use glam::DVec3;
use void_lod::{
    FACE_EDGES, LodSelection, LodView, PlanetLod, PlanetLodOptions, SurfaceSample, TileMeshData,
    TileMeshOptions, build_tile_indices, build_tile_mesh, selected_neighbor, stitch_edges,
};
use void_terrain::Terrain;

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

/// The quadtree, its background builds and its drawn tiles.
pub struct TileField {
    pub lod: PlanetLod,
    /// None draws a smooth sphere coloured by tile level.
    pub terrain: Option<Arc<Terrain>>,
    building: HashMap<u64, Task<TileMeshData>>,
    /// Drawn tiles: entity and the coarse neighbours its seams are stitched to.
    drawn: HashMap<u64, (Entity, [Option<u64>; 4])>,
    indices: Vec<u32>,
    material: Handle<StandardMaterial>,
    render: Vec<u64>,
    pub last_requests: usize,
    pub last_select_ms: f64,
    /// Finest and coarsest drawn level.
    pub levels: (u32, u32),
}

impl TileField {
    pub fn new(
        options: PlanetLodOptions,
        terrain: Option<Arc<Terrain>>,
        material: Handle<StandardMaterial>,
    ) -> Self {
        let (indices, grid) = build_tile_indices(options.resolution);
        Self {
            lod: PlanetLod::new(options),
            terrain,
            building: HashMap::new(),
            drawn: HashMap::new(),
            // Skirts are left out: seams are stitched, as the LOD lab draws by default.
            indices: indices[..grid].to_vec(),
            material,
            render: Vec::new(),
            last_requests: 0,
            last_select_ms: 0.0,
            levels: (0, 0),
        }
    }

    pub fn drawn_count(&self) -> usize {
        self.drawn.len()
    }

    pub fn building_count(&self) -> usize {
        self.building.len()
    }

    /// Accept tiles whose background build has finished.
    pub fn finish_builds(&mut self) {
        let mut done = Vec::new();
        for (code, task) in &mut self.building {
            if let Some(tile) = check_ready(task) {
                done.push((*code, tile));
            }
        }
        for (code, tile) in done {
            self.building.remove(&code);
            let key = tile.key;
            self.lod.accept_tile(Arc::new(tile));
            self.lod.unpin_build(key);
        }
    }

    /// Select for a view and start the most urgent builds, two per core.
    pub fn select(&mut self, view: &LodView) {
        let LodSelection {
            render,
            mut requests,
            select_seconds,
            ..
        } = self.lod.select(view);
        self.last_select_ms = select_seconds * 1e3;
        self.last_requests = requests.len();
        self.render = render;
        requests.sort_by(|a, b| b.priority.total_cmp(&a.priority));
        let slots = std::thread::available_parallelism().map_or(4, |n| n.get()) * 2;
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
            let task = AsyncComputeTaskPool::get().spawn(async move {
                match terrain {
                    Some(terrain) => build_tile_mesh(key, &*terrain, options),
                    None => {
                        let color = level_color(key.level);
                        let sphere = |_direction: DVec3, _cell: f64| SurfaceSample {
                            height_meters: 0.0,
                            color,
                        };
                        build_tile_mesh(key, &sphere, options)
                    }
                }
            });
            self.building.insert(code, task);
        }
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
        let n = self.lod.options.resolution;
        let selected: HashSet<u64> = self.render.iter().copied().collect();
        // Tiles that left the selection, or whose stitched seams changed, are dropped and redrawn.
        let mut wanted: HashMap<u64, [Option<u64>; 4]> = HashMap::new();
        for &code in &self.render {
            let key = self.lod.node(code).expect("a selected tile has a node").key;
            let seams = FACE_EDGES.map(|edge| {
                selected_neighbor(key, edge, |c| selected.contains(&c)).filter(|&nb| {
                    self.lod
                        .node(nb)
                        .is_some_and(|node| node.key.level + 1 == key.level)
                })
            });
            wanted.insert(code, seams);
        }
        self.drawn.retain(|code, (entity, seams)| {
            let keep = wanted.get(code) == Some(seams);
            if !keep {
                commands.entity(*entity).despawn();
            }
            keep
        });
        let mut levels = (u32::MAX, 0);
        for (&code, &seams) in &wanted {
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
            let mesh = meshes.add(tile_mesh(data, coarse, n, &self.indices));
            let entity = commands
                .spawn((
                    Tile,
                    Mesh3d(mesh),
                    MeshMaterial3d(self.material.clone()),
                    anchor(data.origin, eye),
                ))
                .id();
            self.drawn.insert(code, (entity, seams));
        }
        self.levels = (levels.0.min(levels.1), levels.1);
    }
}

fn tile_mesh(
    data: &TileMeshData,
    coarse: [Option<&TileMeshData>; 4],
    n: usize,
    indices: &[u32],
) -> Mesh {
    let (positions, normals, _) = stitch_edges(data, coarse, n);
    let count = n * n;
    let colors: Vec<[f32; 4]> = data.colors[..count]
        .iter()
        .map(|c| [c[0], c[1], c[2], 1.0])
        .collect();
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions[..count].to_vec())
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals[..count].to_vec())
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
    .with_inserted_indices(Indices::U32(indices.to_vec()))
}
