//! Every planet: the drawn terrain is the collision terrain.

use std::sync::Arc;

use glam::DVec3;
use void_landing::{ContactWorldOptions, landing_lod_options, level_for_tile_size};
use void_lod::{
    FACE_EDGES, LodView, PlanetLod, TileMeshData, TileMeshOptions, build_tile_mesh, neighbor_key,
    tile_containing, tiles_around,
};
use void_terrain::Terrain;
use void_testkit::{pebble, planet_by_id};

const PLANETS: [&str; 4] = ["pebble", "luna", "terra", "aurelia"];

fn contact(radius: f64) -> ContactWorldOptions {
    ContactWorldOptions {
        step_seconds: 1.0 / 60.0,
        tile_level: level_for_tile_size(radius, 300.0),
        tile_resolution: 33,
        tile_reach_meters: 300.0,
        tile_keep_meters: 600.0,
        recenter_meters: 1000.0,
        sleeping: true,
    }
}

fn stub(key: void_lod::TileKey) -> TileMeshData {
    TileMeshData {
        key,
        origin: DVec3::ZERO,
        positions: Vec::new(),
        normals: Vec::new(),
        colors: Vec::new(),
        heights: Vec::new(),
        grid: Vec::new(),
        min_height_meters: 0.0,
        max_height_meters: 0.0,
        error_meters: 0.0,
        skirt_depth_meters: 0.0,
        build_seconds: 0.0,
    }
}

#[test]
fn terrain_rebuilt_from_its_config_is_identical() {
    let planet = pebble();
    let key = tile_containing(DVec3::new(0.6, -0.2, 0.77), contact(100e3).tile_level);
    let options = TileMeshOptions {
        radius_meters: planet.terrain.radius_meters,
        resolution: 33,
    };
    let main = build_tile_mesh(key, &*planet.terrain, options);
    let rebuilt = build_tile_mesh(key, &Terrain::from_config(&planet.terrain_config), options);
    assert!(
        main.positions == rebuilt.positions && main.origin == rebuilt.origin,
        "{key}: rebuilt terrain differs"
    );
}

#[test]
fn drawn_terrain_equals_collision_terrain_near_every_part() {
    for id in PLANETS {
        let terrain = planet_by_id(id).terrain;
        let c = contact(terrain.radius_meters);
        let options = landing_lod_options(&terrain, &c);
        let directions: Vec<DVec3> = [
            DVec3::new(1.0, 1.0, 1.0),
            DVec3::new(1.0, 0.001, 0.3),
            DVec3::new(-0.2, 0.5, -0.8),
        ]
        .map(|d| d.normalize())
        .to_vec();
        let highest = terrain.radius_meters + terrain.max_height_meters + c.tile_reach_meters;
        let mut cases: Vec<(&str, Vec<DVec3>)> = Vec::new();
        for d in &directions {
            cases.push((
                "on the ground",
                vec![*d * (terrain.radius_meters + terrain.height(*d))],
            ));
            cases.push(("at the top of collision range", vec![*d * highest]));
        }
        // A second part 20 km along the surface from the first.
        let d0 = directions[2];
        let angle = 20_000.0 / terrain.radius_meters;
        let t = DVec3::new(-d0.y, d0.x, 0.0) / DVec3::new(-d0.y, d0.x, 0.0).length();
        let d1 = DVec3::new(
            d0.x * angle.cos() + t.x * angle.sin(),
            d0.y * angle.cos() + t.y * angle.sin(),
            d0.z * angle.cos(),
        );
        cases.push(("two parts 20 km apart", vec![d0 * highest, d1 * highest]));
        let (mut covered, mut bad, mut worst_drawn) = (0, 0, 0);
        let mut problems = Vec::new();
        for (label, observers) in &cases {
            let mut lod = PlanetLod::new(void_lod::PlanetLodOptions {
                max_cached_tiles: 50_000,
                ..options.clone()
            });
            // The same view as the game's terrain: the parts are the observers, horizon culling on.
            let view = LodView {
                observer_positions: observers.clone(),
                camera: None,
                distance_scale: 1.0,
                horizon_culling: true,
            };
            let mut selected = lod.select(&view);
            let mut iteration = 0;
            while !selected.requests.is_empty() && iteration < 4 * options.max_level {
                for request in &selected.requests {
                    lod.accept_tile(Arc::new(stub(request.key)));
                }
                selected = lod.select(&view);
                iteration += 1;
            }
            worst_drawn = worst_drawn.max(selected.render.len());
            let drawn: std::collections::HashSet<u64> = selected.render.iter().copied().collect();
            if !selected.requests.is_empty() {
                bad += 1;
                problems.push(format!("{label}: selection did not settle"));
            }
            for observer in observers {
                for k in tiles_around(
                    *observer,
                    c.tile_keep_meters,
                    c.tile_level,
                    terrain.radius_meters,
                ) {
                    covered += 1;
                    let missing: Vec<_> = std::iter::once(k)
                        .chain(FACE_EDGES.iter().map(|&e| neighbor_key(k, e)))
                        .filter(|n| !drawn.contains(&n.code()))
                        .collect();
                    if !missing.is_empty() {
                        bad += 1;
                        problems.push(format!("{label}: {k} lacks {missing:?}"));
                    }
                }
            }
        }
        println!(
            "drawn terrain equals collision terrain near every part ({id}): {} cases, {covered} collision tiles within {} m all drawn at L{} with same-level neighbours; at most {worst_drawn} tiles drawn",
            cases.len(),
            c.tile_keep_meters,
            c.tile_level
        );
        assert!(bad == 0, "{id}: {problems:?}");
    }
}
