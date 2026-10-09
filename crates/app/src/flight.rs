//! The main game's wiring that needs no window: which planet and terrain, where the rocket stands,
//! and how fast time may run.

use std::sync::Arc;

use glam::DVec3;
use void_landing::{LandingPlanet, planet_by_id};
use void_terrain::{DEFAULT_LAYERED, LayeredOptions, SEA_LEVEL, Terrain, TerrainConfig};

/// The launch site on a planet flown with Hills terrain, as a body-fixed unit direction.
pub fn hills_launch_site() -> DVec3 {
    DVec3::new(0.8, 0.55, 0.25).normalize()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameTerrain {
    Layered,
    Hills,
}

/// A planet as the game flies it: the planet with its terrain, air and sea.
#[derive(Clone, Debug)]
pub struct GamePlanet {
    pub planet: LandingPlanet,
    pub terrain_id: GameTerrain,
    pub atmosphere: bool,
    pub ocean: bool,
    pub sea_level: f64,
    pub rock_height: f64,
    pub snow_height: f64,
    /// Body-fixed unit direction of the launch site.
    pub launch_site: DVec3,
}

/// Only the Earth analogues have air and water; the small bodies stay airless. `requested`
/// picks the terrain (`layered` or `hills`); the default is layered where there is air.
pub fn game_planet_by_id(id: &str, requested: Option<&str>) -> GamePlanet {
    let original = planet_by_id(id);
    let has_air = ["aurelia", "aurelia-fast", "terra"].contains(&id);
    let terrain_id = match requested.unwrap_or(if has_air { "layered" } else { "hills" }) {
        "layered" => GameTerrain::Layered,
        "hills" => GameTerrain::Hills,
        other => panic!("game planet: unknown terrain {other:?}"),
    };
    assert!(
        terrain_id == GameTerrain::Hills || has_air,
        "game planet: layered terrain is not configured for {id}"
    );
    if terrain_id == GameTerrain::Hills {
        let max = original.terrain.max_height_meters;
        return GamePlanet {
            planet: original,
            terrain_id,
            atmosphere: has_air,
            ocean: false,
            sea_level: if has_air { 1800.0 } else { 0.0 },
            rock_height: max * 0.5625,
            snow_height: max * 0.75,
            launch_site: hills_launch_site(),
        };
    }
    let terrain_config = TerrainConfig::Layered(LayeredOptions {
        radius_meters: original.terrain.radius_meters,
        ..DEFAULT_LAYERED
    });
    let terrain = Arc::new(Terrain::from_config(&terrain_config));
    // On dry lowland; the Hills site would be underwater here.
    let (latitude, longitude) = (0.3_f64, 0.5_f64);
    let launch_site = DVec3::new(
        latitude.cos() * longitude.cos(),
        latitude.cos() * longitude.sin(),
        latitude.sin(),
    );
    assert!(
        terrain.height(launch_site) > SEA_LEVEL,
        "game planet: layered launch site is underwater"
    );
    GamePlanet {
        planet: LandingPlanet {
            label: format!("{} | SCENERY TERRAIN", original.label),
            terrain_config,
            terrain,
            air_datum: SEA_LEVEL,
            sea_level: Some(SEA_LEVEL),
            ..original
        },
        terrain_id,
        atmosphere: true,
        ocean: true,
        sea_level: SEA_LEVEL,
        rock_height: SEA_LEVEL + 2600.0,
        snow_height: SEA_LEVEL + 4800.0,
        launch_site,
    }
}

/// One row of time rates. Up to `PHYSICS_MAX_RATE` everything is simulated and the engine may
/// burn; above it the rocket is on rails: coasting only, and no part moving near the ground.
pub const TIME_RATES: [f64; 9] = [1.0, 2.0, 4.0, 5.0, 20.0, 100.0, 1000.0, 10_000.0, 100_000.0];
pub const PHYSICS_MAX_RATE: f64 = 4.0;

/// Lowest clearance of any part in orbital flight each on-rails rate needs, in radii of the
/// planet (Aurelia: 6.4 km, 9.6 km, 12.7 km, 64 km, 319 km, 1,274 km). A first guess, to tune.
pub fn rails_min_clearance_radii(rate: f64) -> f64 {
    match rate as u64 {
        5 => 0.001,
        20 => 0.0015,
        100 => 0.002,
        1000 => 0.01,
        10_000 => 0.05,
        100_000 => 0.2,
        _ => panic!("flight: no clearance limit for {rate}x"),
    }
}
