//! CPU-only tile throughput probe; not a frame-time or GPU benchmark.
use glam::DVec3;
use std::sync::Arc;
use std::{
    hint::black_box,
    sync::atomic::{AtomicUsize, Ordering},
    time::Instant,
};
use void_lod::{
    LodCamera, LodView, PlanetLod, PlanetLodOptions, TileKey, TileMeshOptions, build_tile_mesh,
    stitch_edges,
};
use void_terrain::{
    AresOptions, DEFAULT_LAYERED, ImpactOptions, Terrain, TerrainConfig, VolcanicOptions,
};
fn main() {
    let configs = [
        TerrainConfig::Layered(DEFAULT_LAYERED),
        TerrainConfig::Impact(ImpactOptions::cinder(2_439_700.0)),
        TerrainConfig::Ares(AresOptions::ares(3_389_500.0)),
        TerrainConfig::Volcanic(VolcanicOptions::vesper(6_051_800.0)),
    ];
    let mut rows = Vec::new();
    for config in configs {
        let terrain = Terrain::from_config(&config);
        for level in [4, 14, 18] {
            let keys: Vec<_> = (0..64)
                .map(|i| TileKey {
                    face: (i % 6) as u8,
                    level,
                    x: (1 << level) / 2 + (i / 6) % 4,
                    y: (1 << level) / 2 + (i / 24) % 4,
                })
                .collect();
            let options = TileMeshOptions {
                radius_meters: terrain.radius_meters,
                resolution: 33,
            };
            black_box(build_tile_mesh(keys[0], &terrain, options));
            for workers in [1, 2, 4, 8, 16] {
                let mut runs = Vec::new();
                for _ in 0..3 {
                    let next = AtomicUsize::new(0);
                    let started = Instant::now();
                    std::thread::scope(|scope| {
                        for _ in 0..workers {
                            let next = &next;
                            let keys = &keys;
                            let terrain = &terrain;
                            scope.spawn(move || {
                                loop {
                                    let i = next.fetch_add(1, Ordering::Relaxed);
                                    let Some(&key) = keys.get(i) else { break };
                                    black_box(build_tile_mesh(key, terrain, options));
                                }
                            });
                        }
                    });
                    runs.push(started.elapsed().as_secs_f64() * 1000.0);
                }
                rows.push(serde_json::json!({"terrain":terrain.name,"level":level,"workers":workers,"tiles":keys.len(),"batch_wall_ms":runs}));
            }
            let tile = build_tile_mesh(keys[0], &terrain, options);
            let started = Instant::now();
            for _ in 0..1000 {
                black_box(stitch_edges(&tile, [None; 4], 33));
            }
            rows.push(serde_json::json!({"terrain":terrain.name,"level":level,"no_seam_clone_us":started.elapsed().as_secs_f64()*1000.0,"payload_bytes":tile.buffer_bytes()}));
        }
    }
    let terrain = Terrain::from_config(&TerrainConfig::Layered(DEFAULT_LAYERED));
    for (scene, altitude) in [("surface", 16_050.0), ("orbit", 500_000.0)] {
        let mut lod = PlanetLod::new(PlanetLodOptions {
            radius_meters: terrain.radius_meters,
            min_surface_height_meters: 0.0,
            max_surface_height_meters: terrain.max_height_meters,
            occluder_radius_meters: terrain.radius_meters,
            lod_surface_band_meters: terrain.max_height_meters,
            resolution: 33,
            max_level: 18,
            split_distance_ratios: (0..18)
                .map(|l| {
                    if l < 3 {
                        f64::INFINITY
                    } else {
                        0.45 / 2.0_f64.powi(l - 3)
                    }
                })
                .collect(),
            retain_frames: 90,
            max_cached_tiles: 2500,
        });
        let eye = DVec3::new(0.48, 0.33, 0.81).normalize() * (terrain.radius_meters + altitude);
        let view = LodView {
            observer_positions: vec![eye],
            camera: Some(LodCamera {
                position: eye,
                distance_scale: 1.0,
                max_level: 18,
                focal_pixels: 935.0,
                min_observer_cell_pixels: 2.0,
            }),
            distance_scale: 1.0,
            horizon_culling: true,
        };
        let started = Instant::now();
        let mut built = 0;
        let mut settled = false;
        for _ in 0..128 {
            let selection = lod.select(&view);
            if selection.requests.is_empty() {
                settled = true;
                break;
            }
            for request in selection.requests {
                lod.accept_tile(Arc::new(build_tile_mesh(
                    request.key,
                    &terrain,
                    TileMeshOptions {
                        radius_meters: terrain.radius_meters,
                        resolution: 33,
                    },
                )));
                built += 1;
            }
        }
        assert!(settled, "selection failed to settle");
        let cold_ms = started.elapsed().as_secs_f64() * 1000.0;
        let mut timings = vec![];
        for _ in 0..300 {
            let s = lod.select(&view);
            assert!(s.requests.is_empty());
            timings.push(serde_json::json!({"select_ms":s.select_seconds*1000.0,"traversal_ms":s.traversal_seconds*1000.0,"balance_ms":s.balance_seconds*1000.0,"eviction_ms":s.eviction_seconds*1000.0,"visited":s.visited,"drawn":s.render.len()}));
        }
        rows.push(serde_json::json!({"synthetic_selection_scene":scene,"cold_serial_ms":cold_ms,"built":built,"cached_bytes":lod.cached_mesh_bytes(),"samples":timings}));
    }
    println!("{}", serde_json::to_string_pretty(&serde_json::json!({"measurement":"CPU tile batch wall time; includes thread startup; fixed patches, no GPU, no Bevy scheduling; 3 repetitions", "rows":rows})).unwrap());
}
