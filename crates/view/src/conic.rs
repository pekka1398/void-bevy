//! Bodies' orbits as osculating two-body ellipses, as `lab/view/src/ConicPath.ts`. The vessel's
//! path is its N-body prediction instead.

use glam::DVec3;
use void_math::{atan2, hypot};
use void_orbit::solve_kepler_elliptic;

fn length(v: DVec3) -> f64 {
    hypot([v.x, v.y, v.z])
}

fn normalize(v: DVec3) -> DVec3 {
    let l = length(v);
    assert!(
        l > 0.0 && l.is_finite(),
        "conic: vector {v} has no direction"
    );
    DVec3::new(v.x / l, v.y / l, v.z / l)
}

fn dot(a: DVec3, b: DVec3) -> f64 {
    a.x * b.x + a.y * b.y + a.z * b.z
}

fn cross(a: DVec3, b: DVec3) -> DVec3 {
    DVec3::new(
        a.y * b.z - a.z * b.y,
        a.z * b.x - a.x * b.z,
        a.x * b.y - a.y * b.x,
    )
}

/// The orbit's shape: eccentricity vector, its length, angular momentum, and the in-plane axes
/// (p toward periapsis, or toward the body on a circle; q ninety degrees ahead).
struct Shape {
    e: f64,
    p: DVec3,
    q: DVec3,
}

fn shape(name: &str, r: DVec3, v: DVec3, gm: f64, count: usize) -> (Shape, DVec3) {
    assert!(gm > 0.0, "{name}: gm={gm}");
    assert!(count >= 3, "{name}: count={count}");
    let rl = length(r);
    assert!(rl > 0.0, "{name}: zero relative position");
    let v2 = dot(v, v);
    let rv = dot(r, v);
    let h = cross(r, v);
    assert!(length(h) > 0.0, "{name}: radial motion has no orbit plane");
    let k = v2 - gm / rl;
    let e_vector = DVec3::new(
        (k * r.x - rv * v.x) / gm,
        (k * r.y - rv * v.y) / gm,
        (k * r.z - rv * v.z) / gm,
    );
    let e = length(e_vector);
    assert!(e < 1.0, "{name}: open orbit e={e}");
    // A circle has no periapsis; its points start at the current position instead.
    let p = if e > 1e-12 {
        normalize(e_vector)
    } else {
        normalize(r)
    };
    let q = normalize(cross(h, p));
    (Shape { e, p, q }, h)
}

/// `count` points of the osculating ellipse of a relative state about `gm`, equally spaced in
/// true anomaly from periapsis, relative to the central body. Open orbits have no closed path
/// and panic.
pub fn ellipse_points(position: DVec3, velocity: DVec3, gm: f64, count: usize) -> Vec<DVec3> {
    let (Shape { e, p, q }, h) = shape("ellipse points", position, velocity, gm, count);
    let hl = length(h);
    let semi_latus_rectum = hl * hl / gm;
    (0..count)
        .map(|i| {
            let nu = 2.0 * std::f64::consts::PI * i as f64 / count as f64;
            let radius = semi_latus_rectum / (1.0 + e * nu.cos());
            let (c, s) = (radius * nu.cos(), radius * nu.sin());
            DVec3::new(p.x * c + q.x * s, p.y * c + q.y * s, p.z * c + q.z * s)
        })
        .collect()
}

/// The same osculating ellipse sampled at equal steps of time from the current position over one
/// period: point k is where the body is k × period / count seconds from now. Drawing in a
/// rotating frame needs each point's time. Returns the points and the period.
pub fn ellipse_points_in_time(
    position: DVec3,
    velocity: DVec3,
    gm: f64,
    count: usize,
) -> (Vec<DVec3>, f64) {
    let (Shape { e, p, q }, _) = shape("ellipse points in time", position, velocity, gm, count);
    let r = length(position);
    let v2 = dot(velocity, velocity);
    let a = -gm / (2.0 * (v2 / 2.0 - gm / r));
    let b = a * (1.0 - e * e).sqrt();
    // Eccentric and mean anomaly now, from the position in the orbit's own axes.
    let e0 = atan2(dot(position, q) / b, dot(position, p) / a + e);
    let m0 = e0 - e * e0.sin();
    let period = 2.0 * std::f64::consts::PI * (a.powi(3) / gm).sqrt();
    let points = (0..count)
        .map(|i| {
            let anomaly =
                solve_kepler_elliptic(m0 + 2.0 * std::f64::consts::PI * i as f64 / count as f64, e);
            let (c, s) = (a * (anomaly.cos() - e), b * anomaly.sin());
            DVec3::new(p.x * c + q.x * s, p.y * c + q.y * s, p.z * c + q.z * s)
        })
        .collect();
    (points, period)
}
