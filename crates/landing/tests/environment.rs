//! The ground frame's equation of motion is the environment's gravity plus that frame's own
//! terms: minus the planet centre's acceleration, plus centrifugal and Coriolis.
use std::sync::Arc;

use glam::DVec3;
use void_environment::{Atmosphere, EarthAtmosphere, Environment};
use void_frames::State;
use void_landing::{
    ContactFrame, LandingPlanet, PlanetFrame, planet_by_id, planet_environment, planet_ephemeris,
};
use void_orbit::{SystemFrames, gravity};
use void_terrain::{DEFAULT_LAYERED, LayeredOptions, SEA_LEVEL, Terrain, TerrainConfig};

/// Layered terrain is shaped around its sea, so the world's air starts there and the sea is in the
/// world; hills have no sea and their air starts at the reference sphere.
#[test]
fn the_air_starts_at_the_sea_where_the_terrain_has_one() {
    let hills = planet_by_id("aurelia");
    let config = TerrainConfig::Layered(LayeredOptions {
        radius_meters: hills.terrain.radius_meters,
        ..DEFAULT_LAYERED
    });
    let layered = LandingPlanet {
        terrain: Arc::new(Terrain::from_config(&config)),
        terrain_config: config,
        air_datum: SEA_LEVEL,
        sea_level: Some(SEA_LEVEL),
        ..hills.clone()
    };
    let (e, b) = planet_ephemeris(&hills);
    let atmosphere = Atmosphere::Earth(EarthAtmosphere::new(
        hills.air_density_scale.expect("Aurelia has air"),
    ));
    for (planet, sea) in [(&hills, None), (&layered, Some(SEA_LEVEL))] {
        let env = planet_environment(planet, &e, b, true);
        let place = env.body(b).expect("the planet is described");
        assert_eq!(place.sea_level_meters, sea, "{}", planet.label);
        assert_eq!(
            place.air_datum_meters,
            sea.unwrap_or(0.0),
            "{}",
            planet.label
        );
        assert_eq!(planet.air_datum_meters(), place.air_datum_meters);
        // 80 m above the sea, or above the sphere without one.
        let above = 80.0;
        let s = env.surroundings_local(
            b,
            State {
                position: DVec3::Z * (e.bodies()[b].radius_meters + sea.unwrap_or(0.0) + above),
                velocity: DVec3::ZERO,
            },
        );
        let air = s.air.expect("air");
        assert!((air.altitude - above).abs() < 1e-8, "{}", air.altitude);
        assert_eq!(air.air, atmosphere.sample(air.altitude));
        match (s.sea, sea) {
            (Some(water), Some(_)) => assert!((water.depth + above).abs() < 1e-8, "{water:?}"),
            (None, None) => {}
            other => panic!("{}: sea {other:?}", planet.label),
        }
    }
    let airless = planet_environment(&layered, &e, b, false);
    let place = airless.body(b).expect("the planet is described");
    assert!(place.atmosphere.is_none() && place.sea_level_meters == Some(SEA_LEVEL));
}

#[test]
fn planet_frame_is_environment_gravity_plus_its_frame_terms() {
    let (mut e, b) = planet_ephemeris(&planet_by_id("aurelia"));
    e.extend_to(100_000.0);
    let frame = PlanetFrame::new(&e, b);
    let env = Environment::new(&e);
    let frames = SystemFrames::new(&e);
    let (surface, origin) = (frames.surface[b], frames.origin);
    let radius = e.bodies()[b].radius_meters;
    let w = frame.spin().z;
    for (t, r, v) in [
        (0.0, DVec3::new(radius + 100.0, 0.0, 0.0), DVec3::ZERO),
        (
            3600.0,
            DVec3::new(0.3, -0.5, 0.8).normalize() * (radius + 8000.0),
            DVec3::new(120.0, -40.0, 15.0),
        ),
        (
            90_000.0,
            DVec3::new(-0.7, 0.1, -0.2).normalize() * (radius + 400e3),
            DVec3::new(10.0, 7600.0, 0.0),
        ),
    ] {
        let at = frames.tree.at(t, &e);
        let mut positions = vec![DVec3::ZERO; e.bodies().len()];
        e.positions_at(t, &mut positions);
        let mut centre = DVec3::ZERO;
        for (k, o) in e.bodies().iter().enumerate() {
            if k != b {
                centre += gravity::body_pull(o, positions[b] - positions[k]);
            }
        }
        let want = env.gravity(&at, &frames, surface, r)
            - at.transform(origin, surface).apply_direction(centre)
            + DVec3::new(
                w * w * r.x + 2.0 * w * v.y,
                w * w * r.y - 2.0 * w * v.x,
                0.0,
            );
        let got = frame.acceleration(&e, t, r, v);
        let error = (got - want).length() / got.length();
        println!("t {t}: {error:.1e}");
        assert!(error < 1e-14, "t {t}: {error}");
    }
}
