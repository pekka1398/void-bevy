//! Export an actual Vesper sampler transect through a supplied body-fixed fixture direction.
//! Usage: cargo run -p void-terrain --example volcanic_profile -- X Y Z > profile.dat
use glam::DVec3;
use void_terrain::{Volcanic, VolcanicOptions};
fn main() {
    let coordinates: Vec<f64> = std::env::args()
        .skip(1)
        .map(|v| v.parse().expect("direction number"))
        .collect();
    assert_eq!(coordinates.len(), 3, "expected body-fixed X Y Z");
    let direction = DVec3::new(coordinates[0], coordinates[1], coordinates[2]);
    assert!((direction.length() - 1.0).abs() < 1e-9);
    let east = DVec3::Z.cross(direction).normalize();
    let options = VolcanicOptions::vesper(6_051_800.0);
    let field = Volcanic::new(&options);
    for i in -400..=400 {
        let distance = f64::from(i) * 250.0;
        let at = (direction + east * distance / options.radius_meters).normalize();
        println!(
            "{} {} {}",
            distance,
            field.sample(at, 0.25).0,
            field.sample(at, 100.0).0
        );
    }
}
