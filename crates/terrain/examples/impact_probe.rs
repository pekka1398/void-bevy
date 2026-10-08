//! Diagnostic image of the actual terrain sampler (not a game or a prebaked game asset).
//! cargo run -p void-terrain --example impact_probe -- /tmp/cinder.ppm [longitude radians] [cinder|ares]
use glam::DVec3;
use std::io::Write;
use void_terrain::{AresOptions, ImpactOptions, Terrain, TerrainConfig};
fn main() {
    let args: Vec<_> = std::env::args().collect();
    let path = args.get(1).expect("output PPM path");
    let longitude: f64 = args.get(2).map_or(0.0, |s| s.parse().unwrap());
    let config = match args.get(3).map(String::as_str).unwrap_or("cinder") {
        "cinder" => TerrainConfig::Impact(ImpactOptions::cinder(2_439_700.0)),
        "ares" => TerrainConfig::Ares(AresOptions::ares(3_389_500.0)),
        _ => panic!("probe terrain must be cinder or ares"),
    };
    let t = Terrain::from_config(&config);
    let eye = DVec3::new(longitude.cos(), longitude.sin(), 0.18).normalize();
    let right = DVec3::Z.cross(eye).normalize();
    let up = eye.cross(right);
    let sun = (eye * 0.45 + right * 0.82 + up * 0.24).normalize();
    let size = 900;
    let mut out = std::io::BufWriter::new(std::fs::File::create(path).unwrap());
    write!(out, "P6\n{size} {size}\n255\n").unwrap();
    let start = std::time::Instant::now();
    for y in 0..size {
        for x in 0..size {
            let xx = (2.0 * (x as f64 + 0.5) / size as f64 - 1.0) * 1.035;
            let yy = (1.0 - 2.0 * (y as f64 + 0.5) / size as f64) * 1.035;
            let q = xx * xx + yy * yy;
            let mut rgb = [0; 3];
            if q < 1.0 {
                let d = eye * (1.0 - q).sqrt() + right * xx + up * yy;
                let tangent = d.cross(DVec3::Z).normalize();
                let bitangent = d.cross(tangent);
                let delta = 2000.0 / t.radius_meters;
                let (h, c) = t.sample(d, Some(2000.0));
                let dhx =
                    (t.sample((d + tangent * delta).normalize(), Some(2000.0)).0 - h) / 2000.0;
                let dhy = (t
                    .sample((d + bitangent * delta).normalize(), Some(2000.0))
                    .0
                    - h)
                    / 2000.0;
                let n = (d - tangent * dhx - bitangent * dhy).normalize();
                let mu0 = n.dot(sun).max(0.0);
                let mu = n.dot(eye).max(0.0);
                let light = if d.dot(sun) > 0.0 {
                    0.68 * 2.0 * mu0 / (mu0 + mu).max(0.001) + 0.32 * mu0
                } else {
                    0.0
                };
                rgb = c.map(|v| ((1.0 - (-v * light * 4.5).exp()).powf(1.0 / 2.2) * 255.0) as u8);
            }
            out.write_all(&rgb).unwrap();
        }
    }
    eprintln!("actual sampler probe: {:?}", start.elapsed());
}
