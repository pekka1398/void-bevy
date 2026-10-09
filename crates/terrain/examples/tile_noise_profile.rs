//! CPU-only full tile benchmark. Scalar and auto backends must produce identical checksums.
use glam::DVec3;
use std::{hint::black_box, time::Instant};
use void_lod::{SurfaceSample, TileKey, TileMeshOptions, build_tile_mesh};
use void_terrain::{DEFAULT_LAYERED, Layered};
fn main() {
    let terrain = Layered::new(DEFAULT_LAYERED);
    let keys: Vec<_> = [2, 8, 14]
        .into_iter()
        .flat_map(|level| {
            (0..6).map(move |face| TileKey {
                face,
                level,
                x: (1 << level) / 3,
                y: (1 << level) / 2,
            })
        })
        .collect();
    for run in 0..7 {
        for (backend, sample) in [
            (
                "scalar",
                Layered::sample_scalar as fn(&Layered, DVec3, f64) -> (f64, [f64; 3]),
            ),
            ("auto", Layered::sample),
        ] {
            let sampler = |direction: DVec3, cell: f64| {
                let (height_meters, color) = sample(&terrain, direction, cell);
                SurfaceSample {
                    height_meters,
                    color: color.map(|c| c as f32),
                }
            };
            let started = Instant::now();
            let mut checksum = 0.0;
            for key in &keys {
                let mesh = build_tile_mesh(
                    black_box(*key),
                    &sampler,
                    TileMeshOptions {
                        radius_meters: DEFAULT_LAYERED.radius_meters,
                        resolution: 33,
                    },
                );
                checksum += mesh.error_meters + mesh.min_height_meters + mesh.max_height_meters;
                black_box(mesh);
            }
            println!(
                "run={run} backend={backend} tiles={} ms={:.3} checksum={:016x}",
                keys.len(),
                started.elapsed().as_secs_f64() * 1e3,
                black_box(checksum).to_bits()
            );
        }
    }
}
