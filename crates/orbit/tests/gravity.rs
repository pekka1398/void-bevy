//! The one gravity law: analytic values, the gradient of its potential, and rotation.
use glam::{DQuat, DVec3};
use void_orbit::gravity::pull;

const GM: f64 = 3.986e14;
const R: f64 = 6.371e6;
const J2: f64 = 1.0826e-3;

fn c() -> f64 {
    1.5 * J2 * GM * R * R
}

/// U = −GM/r + J2 GM R² (3 u² / r² − 1) / (2 r³), whose −∇ is the pull.
fn potential(axis: DVec3, r: DVec3) -> f64 {
    let l = r.length();
    let s = r.dot(axis) / l;
    -GM / l + J2 * GM * R * R * (3.0 * s * s - 1.0) / (2.0 * l.powi(3))
}

#[test]
fn pull_matches_the_pole_and_the_equator() {
    let r = R + 400e3;
    let pole = pull(GM, c(), DVec3::Z, DVec3::new(0.0, 0.0, r));
    let equator = pull(GM, c(), DVec3::Z, DVec3::new(r, 0.0, 0.0));
    // g_r = GM/r² [1 − 1.5 J2 (R/r)² (3 sin² φ − 1)], inward.
    let g = |sin2: f64| GM / (r * r) * (1.0 - 1.5 * J2 * (R / r).powi(2) * (3.0 * sin2 - 1.0));
    assert!(pole.x == 0.0 && pole.y == 0.0);
    assert!(equator.y == 0.0 && equator.z == 0.0);
    assert!((-pole.z / g(1.0) - 1.0).abs() < 1e-15, "{pole}");
    assert!((-equator.x / g(0.0) - 1.0).abs() < 1e-15, "{equator}");
    // A point mass has no bulge term.
    let point = pull(GM, 0.0, DVec3::Z, DVec3::new(r, 0.0, 0.0));
    assert_eq!(point, DVec3::new(-GM / (r * r * r) * r, 0.0, 0.0));
}

#[test]
fn pull_is_the_gradient_of_the_j2_potential() {
    let axis = DVec3::new(0.2, -0.3, 0.9).normalize();
    for r in [
        DVec3::new(7.0e6, 1.0e6, 2.5e6),
        DVec3::new(-3.0e6, 4.0e6, -5.5e6),
        DVec3::new(1.0e5, -2.0e5, 6.9e6),
    ] {
        let h = 1.0;
        let gradient = DVec3::new(
            potential(axis, r + DVec3::X * h) - potential(axis, r - DVec3::X * h),
            potential(axis, r + DVec3::Y * h) - potential(axis, r - DVec3::Y * h),
            potential(axis, r + DVec3::Z * h) - potential(axis, r - DVec3::Z * h),
        ) / (2.0 * h);
        let a = pull(GM, c(), axis, r);
        assert!(
            (a + gradient).length() < 1e-7 * a.length(),
            "{a} vs {}",
            -gradient
        );
    }
}

#[test]
fn pull_turns_with_its_axes() {
    let axis = DVec3::new(0.1, 0.4, 0.8).normalize();
    let r = DVec3::new(5.0e6, -4.0e6, 3.0e6);
    let turn = DQuat::from_axis_angle(DVec3::new(1.0, 2.0, -0.5).normalize(), 0.7);
    let a = pull(GM, c(), axis, r);
    let b = pull(GM, c(), turn * axis, turn * r);
    assert!(
        (turn * a - b).length() < 1e-14 * a.length(),
        "{}",
        (turn * a - b).length()
    );
}

/// `add_pull` over several bodies rounds exactly as the orbit lab's propagator summed (d = body −
/// vessel; point mass added, then the bulge subtracted), which the lab's impact times rely on.
#[test]
fn summed_pull_rounds_as_the_lab() {
    let bodies = [
        (
            GM,
            c(),
            DVec3::new(0.1, -0.2, 0.97).normalize(),
            DVec3::new(1.2e11, -3.4e10, 2.0e9),
        ),
        (
            4.9e12,
            0.0,
            DVec3::Z,
            DVec3::new(1.2e11 + 3.8e8, -3.4e10, 2.1e9),
        ),
        (1.3e20, 0.0, DVec3::Z, DVec3::ZERO),
    ];
    let vessel = DVec3::new(1.2e11 + 7.1e6, -3.4e10 + 1.3e5, 2.0e9 - 4.4e5);
    let (mut ax, mut ay, mut az) = (0.0, 0.0, 0.0);
    for &(gm, c, k, p) in &bodies {
        let (dx, dy, dz) = (p.x - vessel.x, p.y - vessel.y, p.z - vessel.z);
        let r2 = dx * dx + dy * dy + dz * dz;
        let s = gm / (r2 * r2.sqrt());
        ax += dx * s;
        ay += dy * s;
        az += dz * s;
        if c != 0.0 {
            let u = -(dx * k.x + dy * k.y + dz * k.z);
            let f = c / (r2 * r2 * r2.sqrt());
            let radial = f * (5.0 * u * u / r2 - 1.0);
            ax -= radial * dx + 2.0 * f * u * k.x;
            ay -= radial * dy + 2.0 * f * u * k.y;
            az -= radial * dz + 2.0 * f * u * k.z;
        }
    }
    let mut a = DVec3::ZERO;
    for &(gm, c, k, p) in &bodies {
        void_orbit::gravity::add_pull(&mut a, gm, c, k, vessel - p);
    }
    assert_eq!(a, DVec3::new(ax, ay, az));
}
