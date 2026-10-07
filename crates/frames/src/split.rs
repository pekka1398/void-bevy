//! Split positions, as the lab's `SplitPosition.ts`: per axis an integer cell of 2^32 m and a
//! float64 offset in [−2^31, 2^31). Differences subtract the cells exactly before any float64 is
//! formed, so centimetres survive at any placement. The lab's cells are bigints; here `i128`
//! (±1.7e38 cells, far past any galaxy).

use glam::DVec3;

/// Power-of-two cells, ~0.029 AU. A canonical offset has sub-micrometre float64 spacing.
pub const CELL_METERS: f64 = 4_294_967_296.0;
const HALF: f64 = CELL_METERS / 2.0;
/// `Number.MAX_SAFE_INTEGER`.
const SAFE: f64 = 9_007_199_254_740_991.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SplitPosition {
    pub cell: [i128; 3],
    pub offset: DVec3,
}

impl Default for SplitPosition {
    fn default() -> Self {
        Self::ORIGIN
    }
}

impl SplitPosition {
    pub const ORIGIN: Self = Self {
        cell: [0; 3],
        offset: DVec3::ZERO,
    };

    /// `offset` metres from cell `cell`, normalised: whole cells of the offset carry into the
    /// cell exactly. Panics on a non-finite offset or one too large to carry exactly.
    pub fn new(offset: DVec3, cell: [i128; 3]) -> Self {
        assert!(
            offset.is_finite(),
            "split position: non-finite vector ({}, {}, {})",
            offset.x,
            offset.y,
            offset.z
        );
        let mut out = Self { cell, offset };
        for k in 0..3 {
            let carry = ((out.offset[k] + HALF) / CELL_METERS).floor();
            assert!(
                carry.abs() <= SAFE,
                "split position: displacement exceeds exact cell range"
            );
            out.cell[k] = add_cells(out.cell[k], carry as i128);
            out.offset[k] -= carry * CELL_METERS;
            // Rounding at the upper boundary can produce HALF; keep the interval half-open.
            if out.offset[k] >= HALF {
                out.offset[k] -= CELL_METERS;
                out.cell[k] = add_cells(out.cell[k], 1);
            }
            if out.offset[k] < -HALF {
                out.offset[k] += CELL_METERS;
                out.cell[k] = add_cells(out.cell[k], -1);
            }
        }
        out
    }

    /// Metres from the origin.
    pub fn at(offset: DVec3) -> Self {
        Self::new(offset, [0; 3])
    }

    pub fn translate(&self, delta: DVec3) -> Self {
        Self::new(self.offset + delta, self.cell)
    }

    /// `self` + `other`, as positions relative to a common origin.
    pub fn compose(&self, other: &Self) -> Self {
        Self::new(
            self.offset + other.offset,
            [0, 1, 2].map(|k| add_cells(self.cell[k], other.cell[k])),
        )
    }

    /// `self` − `other`, the cells subtracted exactly first. Never flatten two absolute positions
    /// before subtracting them.
    pub fn difference(&self, other: &Self) -> Self {
        Self::new(
            self.offset - other.offset,
            [0, 1, 2].map(|k| {
                self.cell[k]
                    .checked_sub(other.cell[k])
                    .expect("split position: cell overflow")
            }),
        )
    }

    /// A float64 vector, for forces and drawing; the split position stays the authoritative
    /// state. Panics past the cells float64 holds exactly (2^53).
    pub fn vector(&self) -> DVec3 {
        let mut out = DVec3::ZERO;
        for k in 0..3 {
            let n = self.cell[k] as f64;
            assert!(
                n.abs() <= SAFE,
                "split position: relative cell range exceeds float64 vector conversion"
            );
            out[k] = n * CELL_METERS + self.offset[k];
        }
        assert!(out.is_finite(), "relative vector: non-finite");
        out
    }

    /// `self` − `other` as a vector.
    pub fn relative(&self, other: &Self) -> DVec3 {
        self.difference(other).vector()
    }

    /// Move by `delta` with Kahan compensation in the bounded offset (`correction` carries the
    /// lost low bits between calls); the carry is an exact power-of-two subtraction.
    pub fn drift(&self, delta: DVec3, correction: &mut DVec3) -> Self {
        let mut offset = self.offset;
        for k in 0..3 {
            let y = delta[k] - correction[k];
            let sum = offset[k] + y;
            correction[k] = (sum - offset[k]) - y;
            offset[k] = sum;
        }
        Self::new(offset, self.cell)
    }

    /// JSON with the cells as decimal strings, as the lab's.
    pub fn serialize(&self) -> String {
        serde_json::json!({
            "cell": self.cell.map(|c| c.to_string()),
            "offset": self.offset.to_array(),
        })
        .to_string()
    }

    pub fn deserialize(text: &str) -> Result<Self, String> {
        let invalid = || "split position: invalid JSON".to_string();
        let value: serde_json::Value = serde_json::from_str(text).map_err(|_| invalid())?;
        let (Some(cell), Some(offset)) = (value["cell"].as_array(), value["offset"].as_array())
        else {
            return Err(invalid());
        };
        if cell.len() != 3 || offset.len() != 3 {
            return Err(invalid());
        }
        let mut cells = [0_i128; 3];
        let mut offsets = DVec3::ZERO;
        for k in 0..3 {
            let digits = cell[k].as_str().ok_or_else(invalid)?;
            let unsigned = digits.strip_prefix('-').unwrap_or(digits);
            if unsigned.is_empty() || !unsigned.bytes().all(|b| b.is_ascii_digit()) {
                return Err(invalid());
            }
            cells[k] = digits.parse().map_err(|_| invalid())?;
            offsets[k] = offset[k]
                .as_f64()
                .filter(|v| v.is_finite())
                .ok_or_else(invalid)?;
        }
        Ok(Self::new(offsets, cells))
    }
}

fn add_cells(a: i128, b: i128) -> i128 {
    a.checked_add(b).expect("split position: cell overflow")
}

// Keep cell integers as decimal strings in every durable artifact; JSON numbers cannot
// represent galaxy cells exactly. Deserialize rejects unknown fields and noncanonical state.
impl serde::Serialize for SplitPosition {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(serde::Serialize)]
        struct Wire {
            cell: [String; 3],
            offset: [f64; 3],
        }
        Wire {
            cell: self.cell.map(|v| v.to_string()),
            offset: self.offset.to_array(),
        }
        .serialize(serializer)
    }
}
impl<'de> serde::Deserialize<'de> for SplitPosition {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            cell: [String; 3],
            offset: [f64; 3],
        }
        let wire = Wire::deserialize(deserializer)?;
        let mut cell = [0; 3];
        for (i, digits) in wire.cell.iter().enumerate() {
            cell[i] = digits.parse().map_err(serde::de::Error::custom)?;
            if cell[i].to_string() != *digits {
                return Err(serde::de::Error::custom(
                    "split position: noncanonical cell",
                ));
            }
        }
        let offset = DVec3::from_array(wire.offset);
        if !offset.is_finite() || offset.to_array().iter().any(|x| *x < -HALF || *x >= HALF) {
            return Err(serde::de::Error::custom(
                "split position: invalid canonical offset",
            ));
        }
        Ok(Self { cell, offset })
    }
}
