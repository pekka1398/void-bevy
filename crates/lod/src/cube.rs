//! The cube sphere and its tiles, as `lab/lod/src/lod/CubeSphere.ts`, `TileKey.ts` and
//! `TileSearch.ts`. Directions are body-fixed; distances are physical meters.

use std::collections::BTreeMap;
use std::f64::consts::{FRAC_PI_2, FRAC_PI_4};

use glam::DVec3;

use crate::math::{atan, length, tan};

/// Cube face index: 0 +X, 1 −X, 2 +Y, 3 −Y, 4 +Z, 5 −Z.
pub type CubeFace = u8;

pub const CUBE_FACES: [CubeFace; 6] = [0, 1, 2, 3, 4, 5];

pub struct FaceFrame {
    /// Outward face normal.
    pub n: DVec3,
    /// +u axis.
    pub a: DVec3,
    /// +v axis. a × b = n for every face, so grid winding is outward-CCW everywhere.
    pub b: DVec3,
}

pub const FACE_FRAMES: [FaceFrame; 6] = [
    FaceFrame {
        n: DVec3::X,
        a: DVec3::NEG_Z,
        b: DVec3::Y,
    },
    FaceFrame {
        n: DVec3::NEG_X,
        a: DVec3::Z,
        b: DVec3::Y,
    },
    FaceFrame {
        n: DVec3::Y,
        a: DVec3::X,
        b: DVec3::NEG_Z,
    },
    FaceFrame {
        n: DVec3::NEG_Y,
        a: DVec3::X,
        b: DVec3::Z,
    },
    FaceFrame {
        n: DVec3::Z,
        a: DVec3::X,
        b: DVec3::Y,
    },
    FaceFrame {
        n: DVec3::NEG_Z,
        a: DVec3::NEG_X,
        b: DVec3::Y,
    },
];

pub fn face_frame(face: CubeFace) -> &'static FaceFrame {
    &FACE_FRAMES[usize::from(face)]
}

/// Face parameters (u, v) in [−1, 1] to a unit body-fixed direction. The tangent warp keeps
/// cell areas far more uniform than a plain normalize. Shared edges evaluate the same cube point
/// from either face.
pub fn cube_to_sphere(face: CubeFace, u: f64, v: f64) -> DVec3 {
    let FaceFrame { n, a, b } = face_frame(face);
    let p = *n + *a * tan(u * FRAC_PI_4) + *b * tan(v * FRAC_PI_4);
    p * (1.0 / length(p))
}

/// Inverse of `cube_to_sphere`: the face whose normal is closest to the direction, and its
/// face parameters. Any nonzero length is accepted.
pub fn sphere_to_cube(direction: DVec3) -> (CubeFace, f64, f64) {
    let d = direction.abs();
    assert!(
        d.is_finite() && (d.x > 0.0 || d.y > 0.0 || d.z > 0.0),
        "sphere to cube: invalid direction {direction}"
    );
    let face = if d.x >= d.y && d.x >= d.z {
        if direction.x > 0.0 { 0 } else { 1 }
    } else if d.y >= d.z {
        if direction.y > 0.0 { 2 } else { 3 }
    } else if direction.z > 0.0 {
        4
    } else {
        5
    };
    let FaceFrame { n, a, b } = face_frame(face);
    let dn = direction.dot(*n);
    let (su, sv) = (direction.dot(*a) / dn, direction.dot(*b) / dn);
    (face, atan(su) / FRAC_PI_4, atan(sv) / FRAC_PI_4)
}

/// One quadtree node on one cube face; x and y count tiles along the face's u and v axes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TileKey {
    pub face: CubeFace,
    pub level: u32,
    pub x: u32,
    pub y: u32,
}

/// Deepest level a tile code can pack: x and y each need `level` bits.
pub const MAX_CODED_LEVEL: u32 = 21;

/// (face, level, x, y) packed as the orbit lab's `tileCodeOf`: ((level·6 + face)·2²¹ + x)·2²¹ + y.
pub fn tile_code_of(face: CubeFace, level: u32, x: u32, y: u32) -> u64 {
    ((u64::from(level) * 6 + u64::from(face)) << MAX_CODED_LEVEL | u64::from(x)) << MAX_CODED_LEVEL
        | u64::from(y)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UvBounds {
    pub u0: f64,
    pub v0: f64,
    pub u1: f64,
    pub v1: f64,
}

impl TileKey {
    pub fn root(face: CubeFace) -> Self {
        Self {
            face,
            level: 0,
            x: 0,
            y: 0,
        }
    }

    pub fn code(&self) -> u64 {
        tile_code_of(self.face, self.level, self.x, self.y)
    }

    pub fn side(&self) -> u32 {
        1 << self.level
    }

    /// Children in (x, y) order: (0,0) (1,0) (0,1) (1,1).
    pub fn children(&self) -> [TileKey; 4] {
        let (face, level, x, y) = (self.face, self.level + 1, self.x * 2, self.y * 2);
        [
            TileKey { face, level, x, y },
            TileKey {
                face,
                level,
                x: x + 1,
                y,
            },
            TileKey {
                face,
                level,
                x,
                y: y + 1,
            },
            TileKey {
                face,
                level,
                x: x + 1,
                y: y + 1,
            },
        ]
    }

    pub fn parent(&self) -> TileKey {
        assert!(self.level > 0, "the root {self} has no parent");
        TileKey {
            face: self.face,
            level: self.level - 1,
            x: self.x / 2,
            y: self.y / 2,
        }
    }

    /// Face-parameter bounds in [−1, 1].
    pub fn uv_bounds(&self) -> UvBounds {
        let size = 2.0 / f64::from(self.side());
        let u0 = -1.0 + f64::from(self.x) * size;
        let v0 = -1.0 + f64::from(self.y) * size;
        UvBounds {
            u0,
            v0,
            u1: u0 + size,
            v1: v0 + size,
        }
    }
}

impl std::fmt::Display for TileKey {
    /// The lab's `tileId`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}/{}/{}", self.face, self.level, self.x, self.y)
    }
}

/// The tile at a level whose face-parameter square contains a direction; u = 1 (or v = 1)
/// belongs to the last tile.
pub fn tile_containing(direction: DVec3, level: u32) -> TileKey {
    let (face, u, v) = sphere_to_cube(direction);
    let side = 1_u32 << level;
    let index = |p: f64| (((p + 1.0) / 2.0 * f64::from(side)).floor() as u32).min(side - 1);
    TileKey {
        face,
        level,
        x: index(u),
        y: index(v),
    }
}

/// Every tile at a level touching the surface within `reach_meters` of a body-fixed point's
/// ground position, including tiles on neighbouring faces. Samples a tangent-plane grid at half
/// the smallest tile width, over the reach plus one tile, so every touching tile has a sample.
pub fn tiles_around(
    point: DVec3,
    reach_meters: f64,
    level: u32,
    radius_meters: f64,
) -> Vec<TileKey> {
    let r = length(point);
    assert!(
        r > 0.0 && r.is_finite(),
        "tiles around: invalid point {point}"
    );
    assert!(
        reach_meters >= 0.0 && radius_meters > 0.0,
        "tiles around: reach {reach_meters}, radius {radius_meters}"
    );
    let d = point / r;
    let t1 = if d.z.abs() < 0.9 {
        DVec3::new(-d.y, d.x, 0.0)
    } else {
        DVec3::new(0.0, -d.z, d.y)
    };
    let t1 = t1 / length(t1);
    let t2 = d.cross(t1);
    // The tangent warp keeps every tile within a factor 1.5 of the face-centre width.
    let smallest = FRAC_PI_2 * radius_meters / f64::from(1_u32 << level) / 1.5;
    let extent = reach_meters + smallest;
    let steps = ((2.0 * extent) / (smallest / 2.0)).ceil().max(1.0) as u32;
    let mut found = BTreeMap::new();
    for a in 0..=steps {
        for b in 0..=steps {
            let s = (-extent + 2.0 * extent * f64::from(a) / f64::from(steps)) / radius_meters;
            let q = (-extent + 2.0 * extent * f64::from(b) / f64::from(steps)) / radius_meters;
            let key = tile_containing(d + t1 * s + t2 * q, level);
            found.insert(key.code(), key);
        }
    }
    found.into_values().collect()
}
