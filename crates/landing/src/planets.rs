//! Planets to land on, as `lab/landing/src/planet/Planets.ts`: gravity and spin as an orbit
//! system, and a terrain.

use std::f64::consts::PI;
use std::sync::Arc;

use void_environment::{Atmosphere, BodyEnvironment, EarthAtmosphere, Environment};
use void_orbit::{
    BodySpec, Ephemeris, EphemerisOptions, EphemerisSource, GRAVITATIONAL_CONSTANT, RotationSpec,
    SpinSpec, SystemSpec, build_system, suggested_step_seconds,
};
use void_terrain::{HillsOptions, Terrain, TerrainConfig};

/// The orbit crate's Sol preset (the orbit lab's `SYSTEM_PRESETS.sol`).
const SOL: &str = include_str!("../../orbit/systems/sol.json");

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

struct PlanetParameters {
    air_density_scale: Option<f64>,
    id: &'static str,
    name: &'static str,
    color: &'static str,
    radius_meters: f64,
    surface_gravity: f64,
    rotation_period_seconds: f64,
    max_height_meters: f64,
    wavelength_meters: f64,
    octaves: u32,
}

fn landing_planet(p: PlanetParameters) -> LandingPlanet {
    let terrain_config = TerrainConfig::Hills(HillsOptions {
        name: format!("{} hills", p.name),
        radius_meters: p.radius_meters,
        max_height_meters: p.max_height_meters,
        wavelength_meters: p.wavelength_meters,
        octaves: p.octaves,
    });
    let hours = p.rotation_period_seconds / 3600.0;
    let radius = if p.radius_meters >= 1e6 {
        format!("{:.0} km", p.radius_meters / 1e3)
    } else {
        format!("{} km", p.radius_meters / 1e3)
    };
    let day = if hours < 48.0 {
        format!("{hours:.1} h")
    } else {
        format!("{:.1} d", hours / 24.0)
    };
    LandingPlanet {
        label: format!(
            "{} · {radius} RADIUS · {} m/s² · {day} DAY",
            p.name.to_uppercase(),
            p.surface_gravity
        ),
        body_id: p.id.into(),
        system: SystemSpec {
            name: p.name.into(),
            root: BodySpec {
                id: p.id.into(),
                name: p.name.into(),
                color: p.color.into(),
                mass_kg: p.surface_gravity * p.radius_meters.powi(2) / GRAVITATIONAL_CONSTANT,
                radius_meters: p.radius_meters,
                rotation: RotationSpec::Spin(SpinSpec {
                    period_seconds: p.rotation_period_seconds,
                    obliquity_radians: 0.0,
                    pole_longitude_radians: 0.0,
                    angle_at_epoch_radians: 0.0,
                }),
                orbit: None,
                orbit_plane: None,
                gravity_field: None,
                children: Vec::new(),
            },
        },
        terrain: Arc::new(Terrain::from_config(&terrain_config)),
        terrain_config,
        air_density_scale: p.air_density_scale,
        air_datum: 0.0,
        sea_level: None,
    }
}

/// Small starter planet: 100 km radius, Moon-like surface gravity 1.6 m/s² (far denser than real
/// rock, for gameplay), and a fast 3.5 h spin so the equator moves at 50 m/s and rotating-frame
/// effects are large enough to test.
pub fn pebble() -> LandingPlanet {
    let radius_meters = 100e3;
    landing_planet(PlanetParameters {
        id: "pebble",
        air_density_scale: None,
        name: "Pebble",
        color: "#6f8f5a",
        radius_meters,
        surface_gravity: 1.6,
        rotation_period_seconds: 2.0 * PI * radius_meters / 50.0,
        max_height_meters: 3000.0,
        wavelength_meters: 8000.0,
        octaves: 6,
    })
}

/// The Moon's radius, gravity and 27.3-day spin, with placeholder hills up to 6 km.
pub fn moon_size() -> LandingPlanet {
    landing_planet(PlanetParameters {
        id: "luna",
        air_density_scale: None,
        name: "Luna",
        color: "#9a9a92",
        radius_meters: 1_737_400.0,
        surface_gravity: 1.62,
        rotation_period_seconds: 27.321661 * 86_400.0,
        max_height_meters: 6000.0,
        wavelength_meters: 30_000.0,
        octaves: 8,
    })
}

/// Earth's radius, gravity and sidereal day, with placeholder hills up to 8 km.
pub fn earth_size() -> LandingPlanet {
    landing_planet(PlanetParameters {
        id: "terra",
        air_density_scale: Some(1.0),
        name: "Terra",
        color: "#4f7f4a",
        radius_meters: 6_371_000.0,
        surface_gravity: 9.81,
        rotation_period_seconds: 86_164.1,
        max_height_meters: 8000.0,
        wavelength_meters: 40_000.0,
        octaves: 8,
    })
}

/// The orbit lab's Earth analogue inside its full Sol system, with terra's placeholder hills.
pub fn aurelia() -> LandingPlanet {
    aurelia_with_spin(1.0)
}

/// Aurelia spinning ten times faster (a 2.4 h day), to make the rotating frame plain to see.
pub fn aurelia_fast() -> LandingPlanet {
    aurelia_with_spin(10.0)
}

fn aurelia_with_spin(spin_factor: f64) -> LandingPlanet {
    let sol = SystemSpec::from_json(SOL);
    let system = if spin_factor == 1.0 {
        sol
    } else {
        with_faster_spin(&sol, "aurelia", spin_factor)
    };
    let built = build_system(&system);
    let body = built
        .bodies
        .iter()
        .find(|b| b.id == "aurelia")
        .expect("the Sol preset has Aurelia");
    let terrain_config = TerrainConfig::Hills(HillsOptions {
        name: "Aurelia hills".into(),
        radius_meters: body.radius_meters,
        max_height_meters: 8000.0,
        wavelength_meters: 40_000.0,
        octaves: 8,
    });
    let gravity = body.gm / body.radius_meters.powi(2);
    let spin = if spin_factor == 1.0 {
        String::new()
    } else {
        format!(" (SPIN ×{spin_factor})")
    };
    LandingPlanet {
        label: format!(
            "AURELIA{spin} · SOL SYSTEM · {:.0} km RADIUS · {gravity:.2} m/s² · {:.1} h DAY",
            body.radius_meters / 1e3,
            body.rotation.period_seconds / 3600.0
        ),
        body_id: body.id.clone(),
        system,
        terrain: Arc::new(Terrain::from_config(&terrain_config)),
        terrain_config,
        air_density_scale: Some(1.0),
        air_datum: 0.0,
        sea_level: None,
    }
}

/// A copy of the system with one body's (free, not locked) spin sped up by `factor`.
fn with_faster_spin(system: &SystemSpec, body_id: &str, factor: f64) -> SystemSpec {
    fn copy(node: &BodySpec, body_id: &str, factor: f64, found: &mut bool) -> BodySpec {
        let mut node = node.clone();
        if node.id == body_id {
            match &mut node.rotation {
                RotationSpec::Spin(spin) => spin.period_seconds /= factor,
                RotationSpec::Locked(_) => {
                    panic!("planets: {body_id} is tidally locked; its spin follows its orbit")
                }
            }
            *found = true;
        }
        node.children = node
            .children
            .iter()
            .map(|child| copy(child, body_id, factor, found))
            .collect();
        node
    }
    let mut found = false;
    let root = copy(&system.root, body_id, factor, &mut found);
    assert!(found, "planets: {body_id} is not in system {}", system.name);
    SystemSpec {
        name: system.name.clone(),
        root,
    }
}

/// The planets by id, as the lab's `PLANETS`.
pub fn planet_by_id(id: &str) -> LandingPlanet {
    match id {
        "pebble" => pebble(),
        "luna" => moon_size(),
        "terra" => earth_size(),
        "aurelia" => aurelia(),
        "aurelia-fast" => aurelia_fast(),
        other => panic!(
            "planets: unknown planet {other:?}; valid: pebble, luna, terra, aurelia, aurelia-fast"
        ),
    }
}

/// The world around the planet: every body's gravity, the planet's terrain and its sea, and, with
/// `air`, its atmosphere at the planet's density scale. Altitude is from the sea where the terrain
/// has one (`LandingPlanet::air_datum_meters`).
pub fn planet_environment(
    planet: &LandingPlanet,
    ephemeris: &dyn EphemerisSource,
    body: usize,
    air: bool,
) -> Arc<Environment> {
    let atmosphere = air.then(|| {
        let scale = planet
            .air_density_scale
            .unwrap_or_else(|| panic!("planets: air requested on airless {}", planet.label));
        Atmosphere::Earth(EarthAtmosphere::new(scale))
    });
    Arc::new(Environment::new(ephemeris).with(
        body,
        BodyEnvironment {
            atmosphere,
            air_datum_meters: planet.air_datum_meters(),
            terrain: Some(planet.terrain.clone()),
            sea_level_meters: planet.sea_level,
        },
    ))
}

/// The planet's system integrated as an ephemeris, and the planet's index in it. A lone planet has
/// no orbits to size a step from; its ephemeris is trivial and steps a minute.
pub fn planet_ephemeris(planet: &LandingPlanet) -> (Ephemeris, usize) {
    let system = build_system(&planet.system);
    let index = system
        .bodies
        .iter()
        .position(|b| b.id == planet.body_id)
        .unwrap_or_else(|| {
            panic!(
                "planets: {} is not in system {}",
                planet.body_id, planet.system.name
            )
        });
    let step_seconds = if system.bodies.len() > 1 {
        suggested_step_seconds(&system.bodies, 256.0)
    } else {
        60.0
    };
    let mut ephemeris = Ephemeris::new(
        &system,
        EphemerisOptions {
            step_seconds,
            chunk_steps: 1024,
        },
    );
    ephemeris.extend_to(step_seconds);
    (ephemeris, index)
}

/// The finest level whose tiles are at least `tile_size_meters` across at the equator of a face,
/// as `TerrainTiles.ts`'s `levelForTileSize`.
pub fn level_for_tile_size(radius_meters: f64, tile_size_meters: f64) -> u32 {
    assert!(
        radius_meters > 0.0 && tile_size_meters > 0.0,
        "level for tile size({radius_meters}, {tile_size_meters})"
    );
    // A face spans a quarter circumference.
    let face_span = PI / 2.0 * radius_meters;
    (face_span / tile_size_meters).log2().floor().max(0.0) as u32
}

/// The LOD quadtree's options for a landing planet, as `TerrainView.ts`'s `landingLodOptions`. Its
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
        // The LOD lab's defaults.
        retain_frames: 90,
        max_cached_tiles: 2_500,
    }
}
