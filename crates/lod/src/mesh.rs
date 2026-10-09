//! Tile meshes, and seam stitching, which is geometry and so lives here rather than with the
//! renderer.

use std::f64::consts::FRAC_PI_2;
use std::time::Instant;

use glam::DVec3;

use crate::adjacency::{
    FACE_EDGES, FaceEdge, edge_reversed_on_neighbor, neighbor_key, same_edge_on_neighbor,
};
use crate::cube::{TileKey, cube_to_sphere};

/// Rendered surface above the reference radius, and its display colour (linear 0–1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SurfaceSample {
    pub height_meters: f64,
    pub color: [f32; 3],
}

/// Height and colour at a unit body-fixed direction. `cell_meters` is the nominal grid spacing
/// of the tile being built: a sampler may leave out detail finer than it, as a mipmap does, so
/// coarse tiles do not alias it.
pub trait SurfaceSampler {
    fn sample(&self, direction: DVec3, cell_meters: f64) -> SurfaceSample;
}

impl<F: Fn(DVec3, f64) -> SurfaceSample> SurfaceSampler for F {
    fn sample(&self, direction: DVec3, cell_meters: f64) -> SurfaceSample {
        self(direction, cell_meters)
    }
}

#[derive(Clone, Debug)]
pub struct TileMeshData {
    pub key: TileKey,
    /// Body-fixed f64 origin at the terrain surface of the tile centre; every vertex position is
    /// f32 relative to it.
    pub origin: DVec3,
    /// N·N grid vertices followed by 4·N skirt vertices (bottom, top, left, right edge).
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub colors: Vec<[f32; 3]>,
    /// Surface height above the reference radius at every vertex; skirts repeat their edge's.
    pub heights: Vec<f32>,
    /// (i, j, skirt): integer grid coordinate; skirts repeat their edge vertex's with skirt = 1.
    pub grid: Vec<[f32; 3]>,
    pub min_height_meters: f64,
    pub max_height_meters: f64,
    /// Largest 3D deviation between this tile's grid and its own half-resolution interpolation.
    pub error_meters: f64,
    pub skirt_depth_meters: f64,
    pub build_seconds: f64,
}

impl TileMeshData {
    /// Resident vertex payload in bytes.
    pub fn buffer_bytes(&self) -> usize {
        (self.positions.len() * 3
            + self.normals.len() * 3
            + self.colors.len() * 3
            + self.heights.len()
            + self.grid.len() * 3)
            * size_of::<f32>()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct TileMeshOptions {
    pub radius_meters: f64,
    /// Vertices per tile side; N − 1 must be even for the error estimate.
    pub resolution: usize,
}

/// Nominal grid spacing at a level: a face-centre tile's width over its N − 1 cells (the tangent
/// warp keeps the others within 1.5×).
pub fn cell_meters(radius_meters: f64, level: u32, resolution: usize) -> f64 {
    radius_meters * FRAC_PI_2 / f64::from(1_u32 << level) / (resolution - 1) as f64
}

fn f32x3(v: DVec3) -> [f32; 3] {
    [v.x as f32, v.y as f32, v.z as f32]
}

/// Pure tile generation. Samples an (N+2)² grid with a one-vertex apron so border normals come
/// from real neighbours, identical on both sides of any tile edge whichever tile (or face)
/// built it.
pub fn build_tile_mesh(
    key: TileKey,
    sampler: &impl SurfaceSampler,
    options: TileMeshOptions,
) -> TileMeshData {
    let started = Instant::now();
    let n = options.resolution;
    assert!(
        n >= 3 && (n - 1).is_multiple_of(2),
        "tile resolution {n}: N − 1 must be even"
    );
    let radius = options.radius_meters;
    let e = n + 2;
    let b = key.uv_bounds();
    let du = (b.u1 - b.u0) / (n - 1) as f64;
    let dv = (b.v1 - b.v0) / (n - 1) as f64;

    // The surface point at the tile centre: vertex offsets from it stay about a tile size even
    // on tall terrain, so their f32 copies keep sub-0.1 mm precision.
    let cell = cell_meters(radius, key.level, n);
    let center = cube_to_sphere(key.face, (b.u0 + b.u1) / 2.0, (b.v0 + b.v1) / 2.0);
    let origin = center * (radius + sampler.sample(center, cell).height_meters);

    // Extended grid in f64, relative to the origin.
    let mut ex = vec![DVec3::ZERO; e * e];
    let mut dirs = vec![DVec3::ZERO; n * n];
    let mut heights = vec![0.0_f64; n * n];
    let mut colors = vec![[0.0_f32; 3]; n * n + 4 * n];
    let (mut min_height, mut max_height) = (f64::INFINITY, f64::NEG_INFINITY);
    for j in 0..e {
        for i in 0..e {
            let dir = cube_to_sphere(
                key.face,
                b.u0 + (i as f64 - 1.0) * du,
                b.v0 + (j as f64 - 1.0) * dv,
            );
            let sample = sampler.sample(dir, cell);
            ex[j * e + i] = dir * (radius + sample.height_meters) - origin;
            if (1..=n).contains(&i) && (1..=n).contains(&j) {
                let g = (j - 1) * n + (i - 1);
                dirs[g] = dir;
                heights[g] = sample.height_meters;
                colors[g] = sample.color;
                min_height = min_height.min(sample.height_meters);
                max_height = max_height.max(sample.height_meters);
            }
        }
    }

    let vertex_count = n * n + 4 * n;
    let mut positions = vec![[0.0_f32; 3]; vertex_count];
    let mut normals = vec![[0.0_f32; 3]; vertex_count];
    let mut grid = vec![[0.0_f32; 3]; vertex_count];
    let mut vertex_heights = vec![0.0_f32; vertex_count];
    for j in 0..n {
        for i in 0..n {
            let g = j * n + i;
            let c = (j + 1) * e + (i + 1);
            positions[g] = f32x3(ex[c]);
            let tu = ex[c + 1] - ex[c - 1];
            let tv = ex[c + e] - ex[c - e];
            let normal = tu.cross(tv);
            let length = normal.length();
            assert!(
                length.is_finite() && length >= 1e-12,
                "degenerate tile normal at {key} ({i}, {j})"
            );
            normals[g] = f32x3(normal * (1.0 / length));
            grid[g] = [i as f32, j as f32, 0.0];
            vertex_heights[g] = heights[g] as f32;
        }
    }

    let error_meters = half_resolution_error(&positions, n);
    // Deep enough to cover the largest crack a coarser neighbour can open along this edge.
    let skirt_depth_meters = (error_meters * 2.0).max(cell * 0.25).max(1.0);
    for edge in 0..4 {
        for s in 0..n {
            let g = skirt_edge_vertex(edge, s, n);
            let k = n * n + edge * n + s;
            positions[k] = f32x3(dirs[g] * (radius + heights[g] - skirt_depth_meters) - origin);
            normals[k] = normals[g];
            colors[k] = colors[g];
            grid[k] = [grid[g][0], grid[g][1], 1.0];
            vertex_heights[k] = heights[g] as f32;
        }
    }

    TileMeshData {
        key,
        origin,
        positions,
        normals,
        colors,
        heights: vertex_heights,
        grid,
        min_height_meters: min_height,
        max_height_meters: max_height,
        error_meters,
        skirt_depth_meters,
        build_seconds: started.elapsed().as_secs_f64(),
    }
}

/// Grid vertex of skirt edge 0..4 (bottom j = 0, top j = n − 1, left i = 0, right i = n − 1).
fn skirt_edge_vertex(edge: usize, s: usize, n: usize) -> usize {
    match edge {
        0 => s,
        1 => (n - 1) * n + s,
        2 => s * n,
        3 => s * n + n - 1,
        _ => unreachable!("four skirt edges"),
    }
}

fn half_resolution_error(positions: &[[f32; 3]], n: usize) -> f64 {
    let at = |i: usize, j: usize, axis: usize| f64::from(positions[j * n + i][axis]);
    let mut worst = 0.0_f64;
    for j in 0..n {
        for i in 0..n {
            let (odd_i, odd_j) = (i & 1 == 1, j & 1 == 1);
            if !odd_i && !odd_j {
                continue;
            }
            let mut delta = [0.0; 3];
            for (axis, d) in delta.iter_mut().enumerate() {
                let interpolated = if odd_i && odd_j {
                    (at(i - 1, j - 1, axis)
                        + at(i + 1, j - 1, axis)
                        + at(i - 1, j + 1, axis)
                        + at(i + 1, j + 1, axis))
                        / 4.0
                } else if odd_i {
                    (at(i - 1, j, axis) + at(i + 1, j, axis)) / 2.0
                } else {
                    (at(i, j - 1, axis) + at(i, j + 1, axis)) / 2.0
                };
                *d = at(i, j, axis) - interpolated;
            }
            worst = worst.max(DVec3::from_array(delta).length());
        }
    }
    worst
}

/// Shared index buffer for every tile of resolution N. The grid occupies `[0, grid_index_count)`;
/// skirts follow, so a draw range toggles them.
pub fn build_tile_indices(n: usize) -> (Vec<u32>, usize) {
    let mut indices = Vec::with_capacity(((n - 1) * (n - 1) + 4 * (n - 1)) * 6);
    for j in 0..n - 1 {
        for i in 0..n - 1 {
            let a = (j * n + i) as u32;
            let (b, c) = (a + 1, a + n as u32);
            let d = c + 1;
            indices.extend_from_slice(&[a, b, d, a, d, c]);
        }
    }
    let grid_index_count = indices.len();
    // With a × b = n, a skirt triangle (e_s, e_s+1, skirt_s) faces outward on the top and left
    // edges and inward on the bottom and right, so those two are reversed.
    let reversed = [true, false, false, true];
    for (edge, reversed) in reversed.into_iter().enumerate() {
        for s in 0..n - 1 {
            let e0 = skirt_edge_vertex(edge, s, n) as u32;
            let e1 = skirt_edge_vertex(edge, s + 1, n) as u32;
            let s0 = (n * n + edge * n + s) as u32;
            let s1 = s0 + 1;
            if reversed {
                indices.extend_from_slice(&[e0, s0, e1, e1, s0, s1]);
            } else {
                indices.extend_from_slice(&[e0, e1, s0, e1, s1, s0]);
            }
        }
    }
    (indices, grid_index_count)
}

/// Grid vertex `s` along a face edge.
fn edge_vertex(edge: FaceEdge, s: usize, n: usize) -> usize {
    match edge {
        FaceEdge::UMinus => s * n,
        FaceEdge::UPlus => s * n + n - 1,
        FaceEdge::VMinus => s,
        FaceEdge::VPlus => (n - 1) * n + s,
    }
}

/// Stitched copies of a tile's positions, normals and heights: along each edge whose drawn
/// neighbour is one level coarser (`coarse[edge]`), the edge vertices are moved onto the coarse
/// tile's edge segments, so no crack opens. Corners are shared and stay put.
pub fn stitch_edges(
    data: &TileMeshData,
    coarse: [Option<&TileMeshData>; 4],
    n: usize,
) -> (Vec<[f32; 3]>, Vec<[f32; 3]>, Vec<f32>) {
    let mut positions = data.positions.clone();
    let mut normals = data.normals.clone();
    let mut heights = data.heights.clone();
    for edge in FACE_EDGES {
        let Some(c) = coarse[edge.index()] else {
            continue;
        };
        assert!(
            data.key.level == c.key.level + 1,
            "stitch: {} is not one level finer than {}",
            data.key,
            c.key
        );
        let same_level = neighbor_key(data.key, edge);
        let coarse_edge = same_edge_on_neighbor(data.key, edge);
        let reversed = edge_reversed_on_neighbor(data.key, edge);
        let along = if matches!(coarse_edge, FaceEdge::UMinus | FaceEdge::UPlus) {
            same_level.y
        } else {
            same_level.x
        };
        let half = (along % 2) as f64;
        let skirt_edge = match edge {
            FaceEdge::VMinus => 0,
            FaceEdge::VPlus => 1,
            FaceEdge::UMinus => 2,
            FaceEdge::UPlus => 3,
        };
        for s in 1..n - 1 {
            let destination = edge_vertex(edge, s, n);
            let neighbor_index = if reversed { n - 1 - s } else { s };
            let coarse_position = half * (n - 1) as f64 / 2.0 + neighbor_index as f64 / 2.0;
            let (lower, upper) = (coarse_position.floor(), coarse_position.ceil());
            let blend = coarse_position - lower;
            let a = edge_vertex(coarse_edge, lower as usize, n);
            let b = edge_vertex(coarse_edge, upper as usize, n);
            let skirt = n * n + skirt_edge * n + s;
            // Blends run in f64 and round to f32 only when stored.
            heights[destination] =
                (f64::from(c.heights[a]) * (1.0 - blend) + f64::from(c.heights[b]) * blend) as f32;
            heights[skirt] = heights[destination];
            let shift = c.origin - data.origin;
            for axis in 0..3 {
                let target = shift[axis]
                    + f64::from(c.positions[a][axis]) * (1.0 - blend)
                    + f64::from(c.positions[b][axis]) * blend;
                let delta = target - f64::from(positions[destination][axis]);
                positions[destination][axis] = target as f32;
                // Skirt vertices keep their depth under the moved edge.
                positions[skirt][axis] = (f64::from(positions[skirt][axis]) + delta) as f32;
                normals[destination][axis] = (f64::from(c.normals[a][axis]) * (1.0 - blend)
                    + f64::from(c.normals[b][axis]) * blend)
                    as f32;
                normals[skirt][axis] = normals[destination][axis];
            }
        }
    }
    (positions, normals, heights)
}
