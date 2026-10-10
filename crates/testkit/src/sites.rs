//! A one-tank pod and a level launch site, for tests.
use glam::DVec3;
use void_assembly::{Craft, add_part, fresh_craft};
use void_landing::LandingPlanet;

pub fn pod_tank(name: &str) -> Craft {
    let mut c = add_part(&fresh_craft(), "tank-small", "p1", "bottom", "top").unwrap();
    c.name = name.into();
    c
}
pub fn flat_site(planet: &LandingPlanet) -> DVec3 {
    let t = &planet.terrain;
    let base = DVec3::new(0.8, 0.55, 0.25).normalize();
    let east = DVec3::new(-base.y, base.x, 0.0).normalize();
    let north = base.cross(east);
    let mut best = base;
    let mut slope = f64::INFINITY;
    for i in -20..20 {
        for j in -20..20 {
            let d = (base
                + east * (i as f64 * 50.0 / t.radius_meters)
                + north * (j as f64 * 50.0 / t.radius_meters))
                .normalize();
            let h = |e: f64, n: f64| {
                t.height(
                    (d + east * (e / t.radius_meters) + north * (n / t.radius_meters)).normalize(),
                )
            };
            let value = (h(3.0, 0.0) - h(-3.0, 0.0))
                .abs()
                .max((h(0.0, 3.0) - h(0.0, -3.0)).abs())
                / 6.0;
            if value < slope {
                slope = value;
                best = d;
            }
        }
    }
    best
}
