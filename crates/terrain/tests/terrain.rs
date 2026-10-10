//! The terrains keep their contract, and a drawn tile stands on the terrain it was built from.

use glam::DVec3;
use void_lod::{TileKey, TileMeshOptions, build_tile_mesh};
use void_terrain::{
    DEFAULT_LAYERED, HillsOptions, LayeredOptions, Terrain, TerrainConfig, check_terrain_contract,
    lattice_directions,
};

fn terrains() -> Vec<Terrain> {
    vec![
        Terrain::from_config(&TerrainConfig::Hills(HillsOptions {
            name: "hills".into(),
            radius_meters: 100e3,
            max_height_meters: 2000.0,
            wavelength_meters: 12_000.0,
            octaves: 5,
        })),
        Terrain::from_config(&TerrainConfig::Layered(LayeredOptions {
            radius_meters: 6_371_000.0,
            ..DEFAULT_LAYERED
        })),
    ]
}

#[test]
fn terrains_keep_their_contract() {
    for terrain in terrains() {
        let failures = check_terrain_contract(&terrain, 20_000);
        assert!(failures.is_empty(), "{}: {failures:?}", terrain.name);
    }
}

#[test]
fn lattice_directions_are_unit_and_spread_over_the_sphere() {
    let directions = lattice_directions(1000);
    assert!(directions.iter().all(|d| (d.length() - 1.0).abs() < 1e-15));
    // Evenly spread: the mean is near the centre and every octant has its share.
    let mean = directions.iter().sum::<DVec3>() / 1000.0;
    assert!(mean.length() < 0.01, "mean {mean}");
    for octant in 0..8 {
        let sign = |bit: i32| if octant & bit == 0 { 1.0 } else { -1.0 };
        let count = directions
            .iter()
            .filter(|d| d.x * sign(1) > 0.0 && d.y * sign(2) > 0.0 && d.z * sign(4) > 0.0)
            .count();
        assert!((100..150).contains(&count), "octant {octant}: {count}");
    }
}

#[test]
fn a_tile_stands_on_the_terrain_it_was_built_from() {
    for terrain in terrains() {
        let key = TileKey {
            face: 2,
            level: 6,
            x: 21,
            y: 40,
        };
        let tile = build_tile_mesh(
            key,
            &terrain,
            TileMeshOptions {
                radius_meters: terrain.radius_meters,
                resolution: 33,
            },
        );
        // The mesh samples at its own grid spacing, which the terrain uses to filter detail.
        let cell = void_lod::cell_meters(terrain.radius_meters, key.level, 33);
        let mut worst = 0.0_f64;
        for (p, h) in tile.positions.iter().zip(&tile.heights).take(33 * 33) {
            let world = tile.origin + DVec3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2]));
            let direction = world.normalize();
            let (height, _) = terrain.sample(direction, cell);
            worst = worst
                .max((f64::from(*h) - height).abs())
                .max((world.length() - terrain.radius_meters - height).abs());
        }
        // f32 vertices relative to the tile origin: centimetres at most.
        assert!(worst < 0.05, "{}: {worst} m", terrain.name);
    }
}

#[test]
fn layered_options_reject_unknown_fields() {
    let bad = r#"{"kind": "layered", "options": {"radiusMeters": 6371000, "seed": 7, "extra": 1}}"#;
    assert!(serde_json::from_str::<TerrainConfig>(bad).is_err());
}
