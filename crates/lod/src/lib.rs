//! The LOD lab's cube-sphere quadtree (`lab/lod/src/lod`), ported to Rust: tile keys and
//! neighbours, tile meshes and seam stitching, and selection. Rendering and workers are the
//! engine's; nothing here depends on Bevy.

mod adjacency;
mod cube;
mod demo;
mod mesh;
mod ordered;
mod planet_lod;

pub use adjacency::{
    FACE_ADJACENCY, FACE_EDGES, FaceEdge, FaceNeighbor, edge_reversed_on_neighbor, face_neighbor,
    neighbor_key, same_edge_on_neighbor, selected_neighbor,
};
pub use cube::{
    CUBE_FACES, CubeFace, FACE_FRAMES, FaceFrame, MAX_CODED_LEVEL, TileKey, UvBounds,
    cube_to_sphere, face_frame, sphere_to_cube, tile_code_of, tile_containing, tiles_around,
};
pub use demo::{DemoTerrain, DemoTerrainParams, PRESETS_JSON};
pub use mesh::{
    SurfaceSample, SurfaceSampler, TileMeshData, TileMeshOptions, build_tile_indices,
    build_tile_mesh, cell_meters, stitch_edges,
};
pub use ordered::OrderedMap;
pub use planet_lod::{
    HOLMAN_SPLIT_DISTANCE_RATIOS, LodCamera, LodCollapse, LodNode, LodSelection, LodView,
    PlanetLod, PlanetLodOptions, TileRequest,
};
pub use void_math::{hypot, length};
