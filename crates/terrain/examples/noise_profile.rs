//! Small CPU-only benchmark. Run with --release; compare checksums before comparing time.
use std::{hint::black_box, time::Instant};
use void_terrain::noise::{noise_with_gradient, noise_with_gradient_scalar};
fn main() {
    let points: Vec<_> = (0..8192)
        .map(|i| {
            let t = i as f64;
            [t * 0.173 - 71.0, t * -0.071 + 11.0, t * 0.031 - 0.19]
        })
        .collect();
    for run in 0..5 {
        for (backend, sample) in [
            (
                "scalar",
                noise_with_gradient_scalar as fn(f64, f64, f64) -> (f64, [f64; 3]),
            ),
            ("auto", noise_with_gradient),
        ] {
            let started = Instant::now();
            let mut checksum = 0.0;
            for _ in 0..32 {
                for p in &points {
                    let (v, g) = sample(black_box(p[0]), p[1], p[2]);
                    checksum += v + g[0] + g[1] + g[2];
                }
            }
            println!(
                "run={run} backend={backend} ms={:.3} checksum={:016x}",
                started.elapsed().as_secs_f64() * 1e3,
                black_box(checksum).to_bits()
            );
        }
    }
}
