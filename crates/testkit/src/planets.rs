//! Small planets for tests: their systems, terrain, environment and ephemeris. The game's own
//! world is `void_fleet_flight::world::main_game`.
use std::f64::consts::PI;
use std::sync::Arc;
use void_environment::{Atmosphere, BodyEnvironment, EarthAtmosphere, Environment};
use void_landing::LandingPlanet;
use void_orbit::{
    BodySpec, Ephemeris, EphemerisOptions, EphemerisSource, GRAVITATIONAL_CONSTANT, RotationSpec,
    SpinSpec, SystemSpec, build_system, suggested_step_seconds,
};
use void_terrain::{HillsOptions, Terrain, TerrainConfig};

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

/// The Earth analogue inside the full Sol system, with terra's placeholder hills.
pub fn aurelia() -> LandingPlanet {
    let system = SystemSpec::sol();
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
    LandingPlanet {
        label: format!(
            "AURELIA · SOL SYSTEM · {:.0} km RADIUS · {gravity:.2} m/s² · {:.1} h DAY",
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

/// The planets by id.
pub fn planet_by_id(id: &str) -> LandingPlanet {
    match id {
        "pebble" => pebble(),
        "luna" => moon_size(),
        "terra" => earth_size(),
        "aurelia" => aurelia(),
        other => panic!("planets: unknown planet {other:?}; valid: pebble, luna, terra, aurelia"),
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
