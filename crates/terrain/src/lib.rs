//! A planet's solid surface, as lab/landing's terrain contract (`Surface.ts`, `TerrainConfig.ts`,
//! `SurfaceContract.ts`) with scenery's layered planet and landing's hills. The same `Terrain`
//! builds drawn tiles and collision tiles, so what is drawn is what is collided with.

mod hills;
mod layered;
pub mod noise;

use glam::DVec3;
use serde::{Deserialize, Serialize};
use void_lod::{SurfaceSample, SurfaceSampler};
use void_math::hypot;

pub use hills::{Hills, HillsOptions};
pub use layered::{DEFAULT_LAYERED, Layered, LayeredOptions, MAX_HEIGHT, SEA_LEVEL};

/// Plain data that builds a terrain, as landing's `TerrainConfig` (the same JSON):
/// `{"kind": "hills", "options": {...}}` or `{"kind": "layered", "options": {...}}`.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(tag = "kind", content = "options", rename_all = "lowercase")]
pub enum TerrainConfig {
    Hills(HillsOptions),
    Layered(LayeredOptions),
}

#[derive(Clone, Debug)]
enum Kind {
    Hills(Hills),
    Layered(Layered),
}

/// A planet's solid surface: the sampler plus the bounds everything else relies on.
#[derive(Clone, Debug)]
pub struct Terrain {
    pub name: String,
    pub radius_meters: f64,
    pub max_height_meters: f64,
    kind: Kind,
}

impl Terrain {
    pub fn from_config(config: &TerrainConfig) -> Self {
        match config {
            TerrainConfig::Hills(o) => Self {
                name: o.name.clone(),
                radius_meters: o.radius_meters,
                max_height_meters: o.max_height_meters,
                kind: Kind::Hills(Hills::new(o)),
            },
            TerrainConfig::Layered(o) => Self {
                name: "Scenery layered terrain".into(),
                radius_meters: o.radius_meters,
                max_height_meters: MAX_HEIGHT,
                kind: Kind::Layered(Layered::new(*o)),
            },
        }
    }

    /// Height above the reference radius and a linear colour at a unit body-fixed direction (z the
    /// spin axis, x the prime meridian). `cell_meters` band-limits geometry for a tile of that cell
    /// size; point queries pass None for full detail. Anything but a unit direction panics.
    pub fn sample(&self, direction: DVec3, cell_meters: Option<f64>) -> (f64, [f64; 3]) {
        match &self.kind {
            Kind::Hills(h) => {
                let length = hypot([direction.x, direction.y, direction.z]);
                assert!(
                    length.is_finite() && (length - 1.0).abs() <= 1e-9,
                    "{} terrain: expected a unit direction, got {direction} (length {length})",
                    self.name
                );
                h.sample(direction)
            }
            Kind::Layered(l) => l.sample(direction, cell_meters.unwrap_or(1.0)),
        }
    }

    /// Full-detail height at a direction.
    pub fn height(&self, direction: DVec3) -> f64 {
        self.sample(direction, None).0
    }
}

/// Tiles: lod's mesh builder samples with each tile's cell size.
impl SurfaceSampler for Terrain {
    fn sample(&self, direction: DVec3, cell_meters: f64) -> SurfaceSample {
        let (height_meters, color) = Terrain::sample(self, direction, Some(cell_meters));
        SurfaceSample {
            height_meters,
            color: color.map(|c| c as f32),
        }
    }
}

/// Deterministic directions spread over the sphere (a Fibonacci lattice), as landing's
/// `latticeDirections`.
pub fn lattice_directions(count: usize) -> Vec<DVec3> {
    let golden = std::f64::consts::PI * (3.0 - 5.0_f64.sqrt());
    (0..count)
        .map(|i| {
            let z = 1.0 - 2.0 * (i as f64 + 0.5) / count as f64;
            let r = (1.0 - z * z).sqrt();
            DVec3::new(
                void_math::cos(golden * i as f64) * r,
                void_math::sin(golden * i as f64) * r,
                z,
            )
        })
        .collect()
}

/// One violated rule of the terrain contract.
#[derive(Clone, Debug, PartialEq)]
pub struct ContractFailure {
    pub rule: &'static str,
    pub detail: String,
}

/// landing's `checkTerrainContract`: what every consumer (collision tiles, rendering, impact
/// prediction) relies on:
/// - non-unit directions panic;
/// - the same direction always gives the same sample;
/// - heights stay in [0, max height] and colours in [0, 1];
/// - no jumps: a 1 cm step along the surface changes the height by less than 1 m.
pub fn check_terrain_contract(terrain: &Terrain, samples: usize) -> Vec<ContractFailure> {
    let mut failures = Vec::new();
    let mut fail = |rule: &'static str, detail: String| {
        if failures.len() < 20 {
            failures.push(ContractFailure { rule, detail });
        }
    };
    for bad in [
        DVec3::new(2.0, 0.0, 0.0),
        DVec3::ZERO,
        DVec3::new(f64::NAN, 0.0, 1.0),
    ] {
        let panicked =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| terrain.sample(bad, None)))
                .is_err();
        if !panicked {
            fail("non-unit direction panics", format!("{bad}"));
        }
    }
    let step = 0.01 / terrain.radius_meters;
    for d in lattice_directions(samples) {
        let a = terrain.sample(d, None);
        let b = terrain.sample(DVec3::new(d.x, d.y, d.z), None);
        if a != b {
            fail("deterministic", format!("{d}"));
        }
        if !(a.0 >= 0.0 && a.0 <= terrain.max_height_meters) {
            fail("height bounds", format!("{} m at {d}", a.0));
        }
        if a.1.iter().any(|c| !(0.0..=1.0).contains(c)) {
            fail("colour bounds", format!("{:?}", a.1));
        }
        // A tangent step of 1 cm.
        let t = if d.z.abs() < 0.9 {
            DVec3::new(-d.y, d.x, 0.0)
        } else {
            DVec3::new(0.0, -d.z, d.y)
        };
        let n = d + t / hypot([t.x, t.y, t.z]) * step;
        let near = terrain.sample(n / hypot([n.x, n.y, n.z]), None);
        // Written this way so a NaN height counts as a jump.
        #[allow(clippy::neg_cmp_op_on_partial_ord)]
        if !((near.0 - a.0).abs() < 1.0) {
            fail(
                "continuity",
                format!("{} -> {} m over 1 cm at {d}", a.0, near.0),
            );
        }
    }
    failures
}
