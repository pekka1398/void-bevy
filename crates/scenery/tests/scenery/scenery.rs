//! The sky, clouds, stars and orbit camera against what they must physically satisfy.

use glam::DVec3;
use void_scenery::atmosphere::{
    build_transmittance_table, earth_like_atmosphere, sky_radiance, transmittance_coords,
    transmittance_ray,
};
use void_scenery::clouds::{
    SHAPE_PERIOD, SHAPE_SIZE, build_cloud_noise, cloud_shell_intervals, sample_cloud_noise,
};
use void_scenery::tables::{build_irradiance_table, build_multiple_scattering_table, march_sky};
use void_scenery::{DEFAULT_STARS, OrbitView, generate_stars};

const RADIUS: f64 = 6_371_000.0;

#[test]
fn airless_tables_and_sky_have_finite_vacuum_limits() {
    let mut p = earth_like_atmosphere(1_737_400.0);
    p.rayleigh_scattering = [0.0; 3];
    p.ozone_absorption = [0.0; 3];
    p.mie_scattering = 0.0;
    p.mie_extinction = 0.0;
    let trans = build_transmittance_table(&p);
    let multiple = build_multiple_scattering_table(&p, &trans, 8, 4);
    let irradiance = build_irradiance_table(&p, &trans, &multiple, 8, 4);
    assert!(
        trans
            .iter()
            .chain(&multiple)
            .chain(&irradiance)
            .all(|v| v.is_finite())
    );
    // Constants whatever the radius, so the game shares one set among all airless bodies. The
    // multiple-scattering table holds ground bounce here, but it is only drawn times the zero
    // scattering of the air.
    for texel in trans.as_chunks::<4>().0 {
        assert_eq!(&texel[..3], &[1.0; 3]);
    }
    for texel in irradiance.as_chunks::<4>().0 {
        assert_eq!(&texel[..3], &[0.0; 3]);
    }
    let sky = march_sky(
        &p,
        &trans,
        Some(&multiple),
        p.bottom_radius + 100.0,
        DVec3::Z,
        DVec3::Z,
        4,
    );
    assert_eq!(sky.radiance, [0.0; 3]);
    assert_eq!(sky.transmittance, [1.0; 3]);
}

#[test]
fn the_daytime_sky_is_blue_and_dims_toward_the_ground() {
    let p = earth_like_atmosphere(RADIUS);
    let trans = build_transmittance_table(&p);
    assert!(
        trans
            .iter()
            .all(|t| t.is_finite() && (0.0..=1.0).contains(t))
    );
    let multiple = build_multiple_scattering_table(&p, &trans, 32, 10);
    let sun = DVec3::new(0.5, 0.0, 0.866);
    let zenith = march_sky(
        &p,
        &trans,
        Some(&multiple),
        RADIUS + 10.0,
        DVec3::Z,
        sun,
        32,
    );
    assert!(
        zenith.radiance[2] > zenith.radiance[1] && zenith.radiance[1] > zenith.radiance[0],
        "the zenith is blue: {:?}",
        zenith.radiance
    );
    // Looking straight up, the light that gets through is reddened: blue scatters most.
    let t = zenith.transmittance;
    assert!(t[0] > t[1] && t[1] > t[2] && t[2] > 0.5, "{t:?}");
    // The march converges to the fine reference integral of single scattering.
    let reference = sky_radiance(&p, 10.0, DVec3::Z, sun, 2000);
    let single = march_sky(&p, &trans, None, RADIUS + 10.0, DVec3::Z, sun, 256);
    for c in 0..3 {
        assert!(
            (single.radiance[c] / reference[c] - 1.0).abs() < 0.02,
            "channel {c}: {} vs {}",
            single.radiance[c],
            reference[c]
        );
    }
    let irradiance = build_irradiance_table(&p, &trans, &multiple, 32, 8);
    assert!(irradiance.iter().all(|v| v.is_finite() && *v >= 0.0));
}

#[test]
fn transmittance_coordinates_round_trip() {
    let p = earth_like_atmosphere(RADIUS);
    for i in 0..=10 {
        for j in 1..=10 {
            let (x, y) = (i as f64 / 10.0, j as f64 / 10.0);
            let (r, mu) = transmittance_ray(&p, x, y);
            let (bx, by) = transmittance_coords(&p, r, mu);
            assert!((bx - x).abs() < 1e-9 && (by - y).abs() < 1e-9, "({x}, {y})");
        }
    }
}

#[test]
fn cloud_noise_tiles_and_a_ray_down_to_the_ground_crosses_the_shell_once() {
    let shape = build_cloud_noise(SHAPE_SIZE, false);
    for k in 0..20 {
        let point = DVec3::new(k as f64 * 371.0, -(k as f64) * 113.0, k as f64 * 57.0);
        for c in 0..3 {
            let v = sample_cloud_noise(&shape, SHAPE_SIZE, point, SHAPE_PERIOD, c);
            let w = sample_cloud_noise(
                &shape,
                SHAPE_SIZE,
                point + DVec3::new(SHAPE_PERIOD, -SHAPE_PERIOD, 2.0 * SHAPE_PERIOD),
                SHAPE_PERIOD,
                c,
            );
            assert!((0.0..=1.0).contains(&v) && (v - w).abs() < 1e-9);
        }
    }
    let (inner, outer) = (RADIUS + 6500.0, RADIUS + 13000.0);
    let from = DVec3::Z * (RADIUS + 20_000.0);
    // Stopped by the ground 20 km below; without it the ray would cross the far side's shell too.
    let down = cloud_shell_intervals(from, -DVec3::Z, inner, outer, 20_000.0);
    assert_eq!(down.len(), 1);
    assert!((down[0].0 - 7000.0).abs() < 1e-6 && (down[0].1 - 13_500.0).abs() < 1e-6);
    assert!(cloud_shell_intervals(from, DVec3::Z, inner, outer, f64::INFINITY).is_empty());
}

#[test]
fn stars_lie_on_the_sky_sphere_with_finite_colours() {
    let (positions, colors) = generate_stars(&DEFAULT_STARS);
    assert_eq!(positions.len(), DEFAULT_STARS.count);
    assert_eq!(colors.len(), DEFAULT_STARS.count);
    let lengths: Vec<f32> = positions
        .iter()
        .map(|p| (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt())
        .collect();
    let (shortest, longest) = lengths
        .iter()
        .fold((f32::INFINITY, 0.0_f32), |(a, b), &l| (a.min(l), b.max(l)));
    assert!(longest / shortest < 1.0 + 1e-5, "{shortest} .. {longest}");
    // Colours carry brightness, so bright stars exceed 1; none is negative or missing.
    assert!(colors.iter().flatten().all(|c| c.is_finite() && *c >= 0.0));
    assert!(colors.iter().any(|c| c.iter().any(|&v| v > 0.0)));
}

#[test]
fn the_orbit_camera_keeps_an_orthonormal_basis_and_its_radius() {
    let unit = |x: f64, y: f64, z: f64| DVec3::new(x, y, z).normalize();
    let mut view = OrbitView::new(unit(0.3, -0.5, 0.8), RADIUS + 20e3, 0.4, RADIUS * 40.0);
    let check = |view: &OrbitView| {
        let (right, up, back) = view.basis();
        assert!((right.cross(up) - back).length() < 1e-12);
        for a in [right, up, back] {
            assert!((a.length() - 1.0).abs() < 1e-12);
        }
    };
    check(&view);
    view.pan_screen(120.0, -40.0, 1.0, 900.0, 20e3);
    check(&view);
    view.orbit_around_center(0.3, -0.2);
    check(&view);
    view.turn(0.7, 1.1);
    check(&view);
    view.set_radius(RADIUS + 3e3);
    check(&view);
    assert!((view.pose().position.length() - (RADIUS + 3e3)).abs() < 1e-6);
    view.place(unit(0.0, 0.0, 1.0), RADIUS + 400e3, 0.5, 0.0);
    check(&view);
    assert!((view.pose().position - DVec3::Z * (RADIUS + 400e3)).length() < 1e-6);
    // Straight down when untilted.
    assert!((view.pose().forward + DVec3::Z).length() < 1e-12);
    view.set_radius(RADIUS * 100.0);
    assert!(
        (view.pose().position.length() - RADIUS * 40.0).abs() < 1e-3,
        "capped"
    );
}

#[test]
fn small_body_transmittance_inverse_has_exact_shell_endpoints() {
    use void_scenery::atmosphere::transmittance_to_top;
    // Deimos caused the top-row inverse to reconstruct 106200.00000000001 m.
    // Also cover tiny asteroid/comet scales with real scattering and vacuum coefficients.
    for radius in [242.22, 1700.0, 6200.0, 11080.0] {
        for vacuum in [false, true] {
            let mut p = earth_like_atmosphere(radius);
            if vacuum {
                p.rayleigh_scattering = [0.0; 3];
                p.mie_scattering = 0.0;
                p.mie_extinction = 0.0;
                p.ozone_absorption = [0.0; 3];
            }
            for x in [0.0, 0.25, 0.5, 0.75, 1.0] {
                assert_eq!(transmittance_ray(&p, x, 0.0).0, p.bottom_radius);
                assert_eq!(transmittance_ray(&p, x, 1.0).0, p.top_radius);
            }
            let table = build_transmittance_table(&p);
            assert!(
                table
                    .iter()
                    .all(|x| x.is_finite() && (0.0..=1.0).contains(x))
            );
            if vacuum {
                assert!(table.iter().all(|x| *x == 1.0));
            }
            // Invalid physical caller state still fails; no tolerance was added to the gate.
            assert!(
                std::panic::catch_unwind(|| transmittance_to_top(&p, p.top_radius + 1e-6, 1.0))
                    .is_err()
            );
        }
    }
    let p = earth_like_atmosphere(6200.0);
    for (x, y) in [(-0.1, 0.5), (0.5, 1.1), (f64::NAN, 0.0)] {
        assert!(std::panic::catch_unwind(|| transmittance_ray(&p, x, y)).is_err());
    }
}
