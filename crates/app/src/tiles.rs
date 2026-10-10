//! Planet terrain in Bevy: the LOD quadtree's selection drawn as one entity per tile, each tile its
//! own anchor (its f64 origin minus the f64 camera position becomes the f32 translation). Tiles
//! build in the background on Bevy's compute pool; edges against a coarser neighbour are stitched.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::ecs::query::QueryFilter;
use bevy::mesh::{Indices, MeshVertexAttribute};
use bevy::pbr::wireframe::{Wireframe, WireframeColor};
use bevy::prelude::*;
use bevy::render::render_resource::{PrimitiveTopology, VertexFormat};
use bevy::tasks::{AsyncComputeTaskPool, Task, futures::check_ready};

/// Tile meshes building at once per tile field, independent of the machine's core count. Each
/// visible body has its own field; the async compute pool bounds the threads.
const MAX_TILE_BUILDS: usize = 8;
use glam::DVec3;
use void_lod::{
    FACE_EDGES, LodSelection, LodView, PlanetLod, PlanetLodOptions, SurfaceSample, SurfaceSampler,
    TileMeshData, TileMeshOptions, build_tile_indices, build_tile_mesh, selected_neighbor,
    stitch_edges,
};

/// Each vertex's surface height above the reference radius, metres.
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

/// The quadtree, its background builds and its drawn tiles, drawn with material `M`.
pub struct TileField<M: Material = StandardMaterial> {
    pub lod: PlanetLod,
    /// The surface tiles are built on; None draws a smooth sphere coloured by tile level.
    pub terrain: Option<Arc<dyn SurfaceSampler + Send + Sync>>,
    building: HashMap<u64, Task<TileMeshData>>,
    /// Drawn tiles: entity and the coarse neighbours its seams are stitched to.
    drawn: HashMap<u64, (Entity, [Option<u64>; 4])>,
    owned_meshes: HashMap<Entity, AssetId<Mesh>>,
    indices: Vec<u32>,
    material: Handle<M>,
    render: Vec<u64>,
    pub last_requests: usize,
    pub last_select_ms: f64,
    /// Finest and coarsest drawn level.
    pub levels: (u32, u32),
    /// Leave tiles out of Bevy's frustum culling, for a material that moves vertices beyond the
    /// mesh's bounds (the sea is raised in the vertex shader).
    pub no_frustum_culling: bool,
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
        Self {
            lod: PlanetLod::new(options),
            terrain,
            building: HashMap::new(),
            drawn: HashMap::new(),
            // Skirts are left out: seams are stitched.
            indices: indices[..grid].to_vec(),
            material,
            owned_meshes: HashMap::new(),
            render: Vec::new(),
            last_requests: 0,
            last_select_ms: 0.0,
            levels: (0, 0),
            no_frustum_culling: false,
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
    }
    pub fn owned_mesh_count(&self) -> usize {
        self.owned_meshes.len()
    }
    pub fn wireframe(&self) -> bool {
        self.wireframe
    }

    /// Triangle edges on every drawn tile.
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

    /// Every drawn tile's four edges as polylines relative to the camera at `eye`, along the tile's
    /// own grid vertices.
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
        let options = TileMeshOptions {
            radius_meters: self.lod.options.radius_meters,
            resolution: self.lod.options.resolution,
        };
        for request in requests {
            if self.building.len() >= MAX_TILE_BUILDS {
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
                if let Some(mesh) = self.owned_meshes.remove(entity) {
                    meshes.remove(mesh);
                }
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
            let mesh = meshes.add(tile_mesh(
                data,
                coarse,
                n,
                &self.indices,
                self.lod.options.radius_meters,
            ));
            let entity = commands
                .spawn((
                    Tile,
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(self.material.clone()),
                    anchor(data.origin, eye),
                ))
                .id();
            if self.no_frustum_culling {
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
    }
}

fn tile_mesh(
    data: &TileMeshData,
    coarse: [Option<&TileMeshData>; 4],
    n: usize,
    indices: &[u32],
    radius: f64,
) -> Mesh {
    let (positions, normals, heights) = stitch_edges(data, coarse, n);
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
    .with_inserted_attribute(ATTRIBUTE_HEIGHT, heights[..count].to_vec())
    .with_inserted_attribute(
        ATTRIBUTE_CELL,
        vec![void_lod::cell_meters(radius, data.key.level, n) as f32; count],
    )
    .with_inserted_indices(Indices::U32(indices.to_vec()))
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    #[test]
    fn unload_releases_entities_meshes_and_cannot_accept_old_jobs() {
        let planet = void_landing::pebble();
        let options = void_landing::landing_lod_options(
            &planet.terrain,
            &void_fleet_flight::world::ground_tiles(&planet.terrain),
        );
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
            build_tile_mesh(
                key,
                &|_: DVec3, _: f64| SurfaceSample {
                    height_meters: 123.0,
                    color: [1.0, 0.0, 0.0],
                },
                TileMeshOptions {
                    radius_meters: 100e3,
                    resolution: 33,
                },
            )
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
