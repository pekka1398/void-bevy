//! A planet to land on: its system, its terrain and air, and the LOD options its ground tiles use.

use std::f64::consts::PI;
use std::sync::Arc;

use void_orbit::SystemSpec;
use void_terrain::{Terrain, TerrainConfig};

#[derive(Clone, Debug)]
pub struct LandingPlanet {
    /// Short label for a badge.
    pub label: String,
    pub system: SystemSpec,
    pub body_id: String,
    /// Data the terrain is built from; tile builders rebuild the same terrain from it.
    pub terrain_config: TerrainConfig,
    pub terrain: Arc<Terrain>,
    /// Sea-level air density as a multiple of Earth's 1.225 kg/m³, or None for an airless world.
    /// Only the amount of air is a planet's own: the profile it thins out along is the aero
    /// crate's, so this is meaningful on an Earth-size planet and a liberty elsewhere.
    pub air_density_scale: Option<f64>,
    /// Explicit environment datum; world descriptions may differ from the terrain's baked sea.
    pub air_datum: f64,
    pub sea_level: Option<f64>,
}

impl LandingPlanet {
    /// The atmosphere's altitude zero above the terrain's reference sphere: the sea where the
    /// terrain has one, else the sphere. Physics' air and the drawn sky both start here.
    pub fn air_datum_meters(&self) -> f64 {
        self.air_datum
    }
}

/// The finest level whose tiles are at least `tile_size_meters` across at the equator of a face.
pub fn level_for_tile_size(radius_meters: f64, tile_size_meters: f64) -> u32 {
    assert!(
        radius_meters > 0.0 && tile_size_meters > 0.0,
        "level for tile size({radius_meters}, {tile_size_meters})"
    );
    // A face spans a quarter circumference.
    let face_span = PI / 2.0 * radius_meters;
    (face_span / tile_size_meters).log2().floor().max(0.0) as u32
}

/// The LOD quadtree's options for a landing planet. Its
/// finest level is the collision level. Within reach of any observer, every tile and its neighbours
/// are at that level, so no drawn edge there is stitched to a coarser tile and the drawn triangles
/// are the ones Rapier collides with.
pub fn landing_lod_options(
    terrain: &Terrain,
    contact: &crate::contact_world::ContactWorldOptions,
) -> void_lod::PlanetLodOptions {
    let max_level = contact.tile_level;
    // Widest tile at the finest level; the tangent warp keeps tiles within 1.5× of the face-centre width.
    let widest = PI / 2.0 * terrain.radius_meters / f64::from(1_u32 << max_level) * 1.5;
    // A finest-level tile's parent splits within this distance: the collision keep radius, one
    // neighbouring tile beyond it, its parent's half width, and the reach above the terrain band.
    let finest_split = contact.tile_keep_meters + 2.0 * widest + contact.tile_reach_meters;
    let split_distance_ratios = (0..max_level)
        .map(|level| {
            if level < 3 {
                f64::INFINITY
            } else {
                finest_split * f64::from(1_u32 << (max_level - 1 - level)) / terrain.radius_meters
            }
        })
        .collect();
    void_lod::PlanetLodOptions {
        radius_meters: terrain.radius_meters,
        min_surface_height_meters: 0.0,
        max_surface_height_meters: terrain.max_height_meters,
        occluder_radius_meters: terrain.radius_meters,
        lod_surface_band_meters: terrain.max_height_meters,
        resolution: contact.tile_resolution,
        max_level,
        split_distance_ratios,
        retain_frames: 90,
        max_cached_tiles: 2_500,
    }
}
