//! The terrains against the labs (golden data from `golden/terrain.ts`).

use glam::DVec3;
use serde_json::Value;
use void_lod::{TileKey, TileMeshOptions, build_tile_mesh};
use void_terrain::{Terrain, TerrainConfig, check_terrain_contract, lattice_directions};

fn golden() -> Value {
    let path = format!("{}/tests/golden/terrain.json", env!("CARGO_MANIFEST_DIR"));
    serde_json::from_str(&std::fs::read_to_string(&path).expect(&path)).expect(&path)
}

fn f(v: &Value) -> f64 {
    v.as_f64().unwrap_or_else(|| panic!("not a number: {v}"))
}

fn v3(v: &Value) -> DVec3 {
    DVec3::new(f(&v[0]), f(&v[1]), f(&v[2]))
}

#[test]
fn terrains_match_the_labs() {
    let g = golden();
    let directions: Vec<DVec3> = g["directions"].as_array().unwrap().iter().map(v3).collect();
    // The lattice only generates test directions; every sample below uses the lab's own. Its
    // sin and cos are neither fdlibm (libm differs from V8 on about 1% of inputs) nor the system
    // glibc (3%), so it agrees to an ulp rather than bit for bit.
    let lattice = lattice_directions(directions.len());
    let lattice_error = lattice
        .iter()
        .zip(&directions)
        .map(|(a, b)| (*a - *b).length())
        .fold(0.0, f64::max);
    assert!(
        lattice_error <= 2.3e-16,
        "lattice directions differ by {lattice_error:e}"
    );

    for t in g["terrains"].as_array().unwrap() {
        let id = t["id"].as_str().unwrap();
        let config: TerrainConfig =
            serde_json::from_value(t["config"].clone()).expect("a landing TerrainConfig");
        let terrain = Terrain::from_config(&config);
        assert_eq!(terrain.name, t["name"].as_str().unwrap(), "{id}: name");
        assert_eq!(
            (terrain.radius_meters, terrain.max_height_meters),
            (f(&t["radiusMeters"]), f(&t["maxHeightMeters"])),
            "{id}: bounds"
        );
        let (mut height_error, mut color_error) = (0.0_f64, 0.0_f64);
        for s in t["samples"].as_array().unwrap() {
            let cell = s["cell"].as_f64();
            for ((d, h), c) in directions
                .iter()
                .zip(s["heights"].as_array().unwrap())
                .zip(s["colors"].as_array().unwrap())
            {
                let (height, color) = terrain.sample(*d, cell);
                height_error = height_error.max((height - f(h)).abs());
                for (ours, lab) in color.iter().zip(c.as_array().unwrap()) {
                    color_error = color_error.max((ours - f(lab)).abs());
                }
            }
        }
        let failures = check_terrain_contract(&terrain, 20_000);
        println!(
            "{id}: heights {height_error:.1e} m, colours {color_error:.1e}; contract {}",
            if failures.is_empty() {
                "holds".into()
            } else {
                format!("{failures:?}")
            }
        );
        assert!(
            height_error == 0.0 && color_error == 0.0,
            "{id}: heights or colours differ from the lab"
        );
        assert!(failures.is_empty(), "{id}: contract failures {failures:?}");
    }
}

#[test]
fn layered_tiles_match_the_labs() {
    let g = golden();
    let config: TerrainConfig = serde_json::from_value(g["terrains"][0]["config"].clone()).unwrap();
    let terrain = Terrain::from_config(&config);
    for lab in g["tiles"].as_array().unwrap() {
        let k = lab["key"].as_array().unwrap();
        let key = TileKey {
            face: k[0].as_u64().unwrap() as u8,
            level: k[1].as_u64().unwrap() as u32,
            x: k[2].as_u64().unwrap() as u32,
            y: k[3].as_u64().unwrap() as u32,
        };
        let tile = build_tile_mesh(
            key,
            &terrain,
            TileMeshOptions {
                radius_meters: terrain.radius_meters,
                resolution: 33,
            },
        );
        let flat =
            |name: &str| -> Vec<f64> { lab[name].as_array().unwrap().iter().map(f).collect() };
        let worst = |ours: Vec<f64>, theirs: Vec<f64>| -> f64 {
            assert_eq!(ours.len(), theirs.len(), "{key}: lengths");
            ours.iter()
                .zip(&theirs)
                .map(|(a, b)| (a - b).abs())
                .fold(0.0, f64::max)
        };
        let three = |v: &[[f32; 3]]| {
            v.iter()
                .flatten()
                .map(|x| f64::from(*x))
                .collect::<Vec<f64>>()
        };
        let origin = (tile.origin - v3(&lab["origin"])).length();
        let positions = worst(three(&tile.positions), flat("positions"));
        let normals = worst(three(&tile.normals), flat("normals"));
        let colors = worst(three(&tile.colors), flat("colors"));
        let heights = worst(
            tile.heights.iter().map(|h| f64::from(*h)).collect(),
            flat("heights"),
        );
        println!(
            "{key}: origin {origin:.1e} m, positions {positions:.1e} m, normals {normals:.1e}, colours {colors:.1e}, heights {heights:.1e} m"
        );
        assert!(
            origin == 0.0 && positions == 0.0 && normals == 0.0 && colors == 0.0 && heights == 0.0,
            "{key}: differs from the lab"
        );
    }
}

#[test]
fn layered_options_reject_unknown_fields() {
    let bad = r#"{"kind": "layered", "options": {"radiusMeters": 6371000, "seed": 7, "extra": 1}}"#;
    assert!(serde_json::from_str::<TerrainConfig>(bad).is_err());
}

#[test]
fn layered_gradient_backends_preserve_height_and_color_bits() {
    use void_terrain::{DEFAULT_LAYERED, Layered};
    let terrain = Layered::new(DEFAULT_LAYERED);
    for direction in lattice_directions(1024) {
        for cell in [0.125, 1.0, 16.0, 512.0, 16384.0] {
            let reference = terrain.sample_scalar(direction, cell);
            let automatic = terrain.sample(direction, cell);
            assert_eq!(reference.0.to_bits(), automatic.0.to_bits());
            assert_eq!(reference.1.map(f64::to_bits), automatic.1.map(f64::to_bits));
        }
    }
}

#[test]
fn layered_tile_meshes_are_bitwise_equal_between_gradient_backends() {
    use void_lod::SurfaceSample;
    use void_terrain::{DEFAULT_LAYERED, Layered};
    let terrain = Layered::new(DEFAULT_LAYERED);
    for level in [2, 8, 14] {
        for face in 0..6 {
            let key = TileKey {
                face,
                level,
                x: (1 << level) / 3,
                y: (1 << level) / 2,
            };
            let options = TileMeshOptions {
                radius_meters: DEFAULT_LAYERED.radius_meters,
                resolution: 33,
            };
            let sample = |d: DVec3, cell: f64, scalar: bool| {
                let (height_meters, color) = if scalar {
                    terrain.sample_scalar(d, cell)
                } else {
                    terrain.sample(d, cell)
                };
                SurfaceSample {
                    height_meters,
                    color: color.map(|c| c as f32),
                }
            };
            let reference = build_tile_mesh(key, &|d, c| sample(d, c, true), options);
            let automatic = build_tile_mesh(key, &|d, c| sample(d, c, false), options);
            assert_eq!(
                reference.origin.to_array().map(f64::to_bits),
                automatic.origin.to_array().map(f64::to_bits)
            );
            for (a, b) in reference.positions.iter().zip(&automatic.positions) {
                assert_eq!(a.map(f32::to_bits), b.map(f32::to_bits));
            }
            for (a, b) in reference.normals.iter().zip(&automatic.normals) {
                assert_eq!(a.map(f32::to_bits), b.map(f32::to_bits));
            }
            for (a, b) in reference.colors.iter().zip(&automatic.colors) {
                assert_eq!(a.map(f32::to_bits), b.map(f32::to_bits));
            }
            for (a, b) in reference.heights.iter().zip(&automatic.heights) {
                assert_eq!(a.to_bits(), b.to_bits());
            }
            assert_eq!(
                reference.error_meters.to_bits(),
                automatic.error_meters.to_bits()
            );
            assert_eq!(
                reference.min_height_meters.to_bits(),
                automatic.min_height_meters.to_bits()
            );
            assert_eq!(
                reference.max_height_meters.to_bits(),
                automatic.max_height_meters.to_bits()
            );
        }
    }
}
