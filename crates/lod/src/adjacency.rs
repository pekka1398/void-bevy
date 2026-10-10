//! Tile neighbours across edges and cube faces.

use std::sync::LazyLock;

use glam::DVec3;

use crate::cube::{CUBE_FACES, CubeFace, TileKey, cube_to_sphere, face_frame, tile_code_of};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FaceEdge {
    UMinus,
    UPlus,
    VMinus,
    VPlus,
}

pub const FACE_EDGES: [FaceEdge; 4] = [
    FaceEdge::UMinus,
    FaceEdge::UPlus,
    FaceEdge::VMinus,
    FaceEdge::VPlus,
];

impl FaceEdge {
    /// True for the u− and u+ edges, which run along v.
    fn is_u(self) -> bool {
        matches!(self, FaceEdge::UMinus | FaceEdge::UPlus)
    }

    fn uv(self, t: f64) -> (f64, f64) {
        match self {
            FaceEdge::UMinus => (-1.0, t),
            FaceEdge::UPlus => (1.0, t),
            FaceEdge::VMinus => (t, -1.0),
            FaceEdge::VPlus => (t, 1.0),
        }
    }

    pub fn index(self) -> usize {
        self as usize
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FaceNeighbor {
    pub face: CubeFace,
    pub edge: FaceEdge,
    /// Whether increasing the source edge coordinate decreases the neighbour's.
    pub reversed: bool,
}

fn cube_point(face: CubeFace, u: f64, v: f64) -> DVec3 {
    let f = face_frame(face);
    f.n + (f.a * u + f.b * v)
}

fn project(face: CubeFace, point: DVec3) -> (f64, f64) {
    let f = face_frame(face);
    let denominator = point.dot(f.n);
    assert!(denominator == 1.0, "project: {point} is not on face {face}");
    (point.dot(f.a), point.dot(f.b))
}

fn edge_of(u: f64, v: f64) -> FaceEdge {
    if u == -1.0 {
        FaceEdge::UMinus
    } else if u == 1.0 {
        FaceEdge::UPlus
    } else if v == -1.0 {
        FaceEdge::VMinus
    } else if v == 1.0 {
        FaceEdge::VPlus
    } else {
        panic!("edge of: ({u}, {v}) is not on an edge")
    }
}

fn derive(face: CubeFace, edge: FaceEdge) -> FaceNeighbor {
    let (u, v) = edge.uv(0.0);
    let point = cube_point(face, u, v);
    let candidates: Vec<CubeFace> = CUBE_FACES
        .into_iter()
        .filter(|&other| other != face && point.dot(face_frame(other).n) == 1.0)
        .collect();
    assert!(
        candidates.len() == 1,
        "face {face} edge {edge:?}: expected one neighbour, found {candidates:?}"
    );
    let neighbor = candidates[0];
    let (mid_u, mid_v) = project(neighbor, point);
    let neighbor_edge = edge_of(mid_u, mid_v);
    let (end_u, end_v) = edge.uv(1.0);
    let (out_u, out_v) = project(neighbor, cube_point(face, end_u, end_v));
    let end = if neighbor_edge.is_u() { out_v } else { out_u };
    assert!(
        end.abs() == 1.0,
        "face {face} edge {edge:?}: invalid orientation on face {neighbor}"
    );
    FaceNeighbor {
        face: neighbor,
        edge: neighbor_edge,
        reversed: end == -1.0,
    }
}

/// Generated from the face frames and checked once: reciprocity, and sampled edge directions
/// (corners included) agreeing from both faces.
pub static FACE_ADJACENCY: LazyLock<[[FaceNeighbor; 4]; 6]> = LazyLock::new(|| {
    let table = CUBE_FACES.map(|face| FACE_EDGES.map(|edge| derive(face, edge)));
    for face in CUBE_FACES {
        for edge in FACE_EDGES {
            let neighbor = table[usize::from(face)][edge.index()];
            let back = table[usize::from(neighbor.face)][neighbor.edge.index()];
            assert!(
                back.face == face && back.edge == edge && back.reversed == neighbor.reversed,
                "face {face} edge {edge:?}: round trip through {neighbor:?} gives {back:?}"
            );
            for t in [-1.0, -0.75, -0.5, 0.0, 0.5, 0.75, 1.0] {
                let (u, v) = edge.uv(t);
                let (ou, ov) = neighbor.edge.uv(if neighbor.reversed { -t } else { t });
                let error =
                    (cube_to_sphere(face, u, v) - cube_to_sphere(neighbor.face, ou, ov)).length();
                assert!(
                    error <= 1e-14,
                    "face {face} edge {edge:?} at {t}: directions differ by {error}"
                );
            }
        }
    }
    table
});

pub fn face_neighbor(face: CubeFace, edge: FaceEdge) -> FaceNeighbor {
    FACE_ADJACENCY[usize::from(face)][edge.index()]
}

/// The same-level tile across an edge, including a cube face boundary.
pub fn neighbor_key(key: TileKey, edge: FaceEdge) -> TileKey {
    let side = key.side();
    let TileKey { face, level, x, y } = key;
    match edge {
        FaceEdge::UMinus if x > 0 => return TileKey { x: x - 1, ..key },
        FaceEdge::UPlus if x < side - 1 => return TileKey { x: x + 1, ..key },
        FaceEdge::VMinus if y > 0 => return TileKey { y: y - 1, ..key },
        FaceEdge::VPlus if y < side - 1 => return TileKey { y: y + 1, ..key },
        _ => {}
    }
    let adjacent = face_neighbor(face, edge);
    let along = if edge.is_u() { y } else { x };
    let mapped = if adjacent.reversed {
        side - 1 - along
    } else {
        along
    };
    let face = adjacent.face;
    match adjacent.edge {
        FaceEdge::UMinus => TileKey {
            face,
            level,
            x: 0,
            y: mapped,
        },
        FaceEdge::UPlus => TileKey {
            face,
            level,
            x: side - 1,
            y: mapped,
        },
        FaceEdge::VMinus => TileKey {
            face,
            level,
            x: mapped,
            y: 0,
        },
        FaceEdge::VPlus => TileKey {
            face,
            level,
            x: mapped,
            y: side - 1,
        },
    }
}

/// The code of the selected same-level or coarser neighbour across an edge, where `selected`
/// says whether a tile code is selected. A coarser tile across an edge is always on the same face
/// as the same-level neighbour, so the walk halves x and y in place.
pub fn selected_neighbor(
    key: TileKey,
    edge: FaceEdge,
    selected: impl Fn(u64) -> bool,
) -> Option<u64> {
    let TileKey {
        face,
        mut level,
        mut x,
        mut y,
    } = neighbor_key(key, edge);
    loop {
        let code = tile_code_of(face, level, x, y);
        if selected(code) {
            return Some(code);
        }
        if level == 0 {
            return None;
        }
        level -= 1;
        x >>= 1;
        y >>= 1;
    }
}

/// Which edge of the neighbour across `edge` is shared with this tile.
pub fn same_edge_on_neighbor(key: TileKey, edge: FaceEdge) -> FaceEdge {
    let side = key.side();
    match edge {
        FaceEdge::UMinus if key.x > 0 => FaceEdge::UPlus,
        FaceEdge::UPlus if key.x < side - 1 => FaceEdge::UMinus,
        FaceEdge::VMinus if key.y > 0 => FaceEdge::VPlus,
        FaceEdge::VPlus if key.y < side - 1 => FaceEdge::VMinus,
        _ => face_neighbor(key.face, edge).edge,
    }
}

pub fn edge_reversed_on_neighbor(key: TileKey, edge: FaceEdge) -> bool {
    let side = key.side();
    let cross_face = match edge {
        FaceEdge::UMinus => key.x == 0,
        FaceEdge::UPlus => key.x == side - 1,
        FaceEdge::VMinus => key.y == 0,
        FaceEdge::VPlus => key.y == side - 1,
    };
    cross_face && face_neighbor(key.face, edge).reversed
}
