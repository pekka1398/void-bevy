//! The main game's wiring that needs no window, as the TS game's `src/GamePlanet.ts`,
//! `src/FlightFrame.ts` and the time-rate rules in `src/main.ts`: which planet and terrain, where
//! the rocket stands, how fast time may run, and the vessel's axes for the navball.

use std::sync::Arc;

use glam::{DQuat, DVec3};
use void_landing::{LandingPlanet, PartJointRocket, PhysicsMode, RocketPart, planet_by_id};
use void_orbit::Ephemeris;
use void_terrain::{DEFAULT_LAYERED, LayeredOptions, SEA_LEVEL, Terrain, TerrainConfig};

pub const PARTS: [RocketPart; 2] = [RocketPart::Upper, RocketPart::Booster];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameTerrain {
    Layered,
    Hills,
}

/// A planet as the game flies it: the landing lab's planet with its terrain, air and sea.
#[derive(Clone, Debug)]
pub struct GamePlanet {
    pub planet: LandingPlanet,
    pub terrain_id: GameTerrain,
    pub atmosphere: bool,
    pub ocean: bool,
    pub sea_level: f64,
    pub rock_height: f64,
    pub snow_height: f64,
    /// Body-fixed unit direction of the launch site; None keeps the demo rocket's.
    pub launch_site: Option<DVec3>,
}

/// Only the Earth analogues have air and water; lone small-body labs stay airless. `requested`
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
            launch_site: None,
        };
    }
    let terrain_config = TerrainConfig::Layered(LayeredOptions {
        radius_meters: original.terrain.radius_meters,
        ..DEFAULT_LAYERED
    });
    let terrain = Arc::new(Terrain::from_config(&terrain_config));
    // This scenery lab location is on dry lowland, not the old underwater hills site.
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
            ..original
        },
        terrain_id,
        atmosphere: true,
        ocean: true,
        sea_level: SEA_LEVEL,
        rock_height: SEA_LEVEL + 2600.0,
        snow_height: SEA_LEVEL + 4800.0,
        launch_site: Some(launch_site),
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

/// The highest time rate allowed now, and what holds it there: the engine and parts moving near
/// the ground keep it at physics rates, and the lowest part in orbital flight caps the on-rails
/// rate.
pub fn warp_limit(
    rocket: &PartJointRocket,
    ephemeris: &Ephemeris,
    throttle: f64,
    radius: f64,
) -> (f64, Option<String>) {
    if let Some(blocker) = rocket.rails_blocker(throttle) {
        return (PHYSICS_MAX_RATE, Some(format!("coasting ({blocker})")));
    }
    let clearance = PARTS
        .into_iter()
        .filter(|&p| rocket.part_mode(p) == PhysicsMode::Flight)
        .map(|p| rocket.part_clearance(ephemeris, p))
        .fold(f64::INFINITY, f64::min);
    if clearance == f64::INFINITY {
        return (TIME_RATES[TIME_RATES.len() - 1], None);
    }
    let mut allowed = PHYSICS_MAX_RATE;
    for rate in TIME_RATES.into_iter().filter(|&r| r > PHYSICS_MAX_RATE) {
        let need = rails_min_clearance_radii(rate) * radius;
        if clearance < need {
            return (
                allowed,
                Some(format!("{} above the ground", distance_text(need))),
            );
        }
        allowed = rate;
    }
    (allowed, None)
}

/// The navball's vessel axes from the upper stage's attitude: the nose is its local +y (thrust
/// axis) and the top its local +z, where S (a positive torque about local +x) pitches the nose.
pub fn vessel_axes(attitude: DQuat) -> (DVec3, DVec3) {
    (attitude * DVec3::Y, attitude * DVec3::Z)
}

pub fn distance_text(m: f64) -> String {
    let a = m.abs();
    if a >= 1e9 {
        format!("{:.3} Gm", m / 1e9)
    } else if a >= 1e4 {
        format!("{:.1} km", m / 1e3)
    } else {
        format!("{m:.1} m")
    }
}

/// T+ [d] hh:mm:ss.
pub fn mission_time(seconds: f64) -> String {
    let whole = seconds.floor() as i64;
    let days = whole / 86_400;
    let rest = format!(
        "{:02}:{:02}:{:02}",
        whole % 86_400 / 3600,
        whole % 3600 / 60,
        whole % 60
    );
    if days > 0 {
        format!("T+ {days}d {rest}")
    } else {
        format!("T+ {rest}")
    }
}

/// A time rate as the HUD writes it: 1×, 20×, 1k×.
pub fn rate_text(rate: f64) -> String {
    if rate >= 1000.0 {
        format!("{}kx", rate / 1000.0)
    } else {
        format!("{rate}x")
    }
}
