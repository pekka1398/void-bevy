//! Launch sites near another one on a planet's surface.
use glam::DVec3;

pub fn nearby_site(d: DVec3, metres: f64, radius: f64) -> DVec3 {
    let east = if d.x.hypot(d.y) > 1e-9 {
        DVec3::new(-d.y, d.x, 0.0).normalize()
    } else {
        DVec3::X
    };
    d * (metres / radius).cos() + east * (metres / radius).sin()
}
