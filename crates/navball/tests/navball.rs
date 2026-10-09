//! The navball's geometry: compass, horizon, attitudes up to the poles. The drawing is checked by
//! eye in the example.

use glam::DVec3;
use void_navball::{
    NavballInput, NavballPainter, heading_pitch, horizon_axes, horizon_direction, navball_basis,
    to_ball,
};

fn angle_difference(a: f64, b: f64) -> f64 {
    (((a - b + 540.0) % 360.0) - 180.0).abs()
}

// ENU at the equator on the prime meridian (+x): east +y, north +z, up +x; the pole is +z.
const EAST: DVec3 = DVec3::Y;
const NORTH: DVec3 = DVec3::Z;
const UP: DVec3 = DVec3::X;
const PRIME_MERIDIAN: DVec3 = DVec3::X;

fn standing() -> NavballInput {
    NavballInput {
        nose: UP,
        top: NORTH,
        up: UP,
        pole: NORTH,
        prime_meridian: PRIME_MERIDIAN,
        velocity: DVec3::ZERO,
    }
}

#[test]
fn standing_rocket_and_compass() {
    let b = navball_basis(&standing());
    assert!(
        (to_ball(&b, UP) - DVec3::Z).length() < 1e-12,
        "zenith at the centre"
    );
    assert!(
        (to_ball(&b, NORTH) - DVec3::Y).length() < 1e-12,
        "north at the top"
    );
    assert!(
        (to_ball(&b, EAST) - DVec3::X).length() < 1e-12,
        "east at the right"
    );
    assert!((b.north - NORTH).length() < 1e-12 && (b.east - EAST).length() < 1e-12);

    let mut worst = 0.0_f64;
    for heading in (0..360).step_by(15) {
        for pitch in (-85..=85).step_by(17) {
            let (h, p) = heading_pitch(&b, horizon_direction(&b, heading as f64, pitch as f64));
            worst = worst
                .max(angle_difference(h, heading as f64))
                .max((p - pitch as f64).abs());
        }
    }
    println!("heading and pitch round trip: largest error {worst:.1e} deg");
    assert!(worst < 1e-9);
    let (east_heading, east_pitch) = heading_pitch(&b, EAST);
    let (ne_heading, _) = heading_pitch(&b, horizon_direction(&b, 45.0, 30.0));
    assert!(
        angle_difference(east_heading, 90.0) < 1e-9
            && east_pitch.abs() < 1e-9
            && angle_difference(ne_heading, 45.0) < 1e-9
    );
}

#[test]
fn level_flight_and_climb() {
    // Flying east, level, top up: prograde along the nose at the centre, retrograde behind the ball.
    let b = navball_basis(&NavballInput {
        nose: EAST,
        top: UP,
        velocity: EAST,
        ..standing()
    });
    assert!((to_ball(&b, EAST) - DVec3::Z).length() < 1e-12);
    assert!(to_ball(&b, -EAST).z < 0.0);
    assert!((to_ball(&b, UP) - DVec3::Y).length() < 1e-12);
    // A climb shows as a prograde marker above the reticle.
    let marker = to_ball(&b, horizon_direction(&b, 90.0, 10.0));
    println!(
        "10° climb: marker at y {:.4} (sin 10° = {:.4})",
        marker.y,
        (10.0_f64).to_radians().sin()
    );
    assert!(marker.y > 0.0 && marker.x.abs() < 1e-12);
}

#[test]
fn bad_input_panics() {
    let panics = |input: NavballInput| {
        std::panic::catch_unwind(|| {
            navball_basis(&input);
        })
        .is_err()
    };
    assert!(panics(NavballInput {
        prime_meridian: DVec3::new(1.0, 0.0, 0.1).normalize(),
        ..standing()
    }));
    assert!(panics(NavballInput {
        top: DVec3::new(0.1, 0.0, 0.99_f64.sqrt()),
        ..standing()
    }));
    assert!(panics(NavballInput {
        up: DVec3::new(2.0, 0.0, 0.0),
        ..standing()
    }));
}

#[test]
fn horizons_up_to_the_poles() {
    // Every horizon, from the equator to exactly at either pole, is a right-handed frame.
    let mut worst = 0.0_f64;
    let offsets = [1.0, 0.1, 1e-6, 1e-11, 1e-13, 0.0];
    for sign in [1.0, -1.0] {
        for offset in offsets {
            let u = DVec3::new(offset, 0.3 * offset, sign).normalize();
            let (n, e) = horizon_axes(u, NORTH, PRIME_MERIDIAN);
            worst = worst
                .max((e.cross(n) - u).length())
                .max((n.length() - 1.0).abs())
                .max((e.length() - 1.0).abs());
        }
    }
    println!(
        "{} latitudes down to exactly ±90°: largest error {worst:.1e}",
        offsets.len() * 2
    );
    assert!(worst < 1e-9);
    // Exactly at a pole the ball uses grid north, along the prime meridian.
    for u in [NORTH, -NORTH] {
        let b = navball_basis(&NavballInput {
            nose: PRIME_MERIDIAN,
            top: u,
            up: u,
            ..standing()
        });
        let (h, p) = heading_pitch(&b, PRIME_MERIDIAN);
        assert!((b.north - PRIME_MERIDIAN).length() < 1e-15);
        assert!(angle_difference(h, 0.0) < 1e-9 && p.abs() < 1e-9);
    }
}

#[test]
fn painter_draws_sky_above_ground_below() {
    let mut painter = NavballPainter::new(150.0, 1.0);
    // Level flight east with the top up: sky above the centre, ground below, nothing outside.
    let readout = painter.draw(&NavballInput {
        nose: EAST,
        top: UP,
        velocity: EAST * 50.0,
        ..standing()
    });
    let pixel = |x: usize, y: usize| {
        let i = (y * painter.size + x) * 4;
        [
            painter.rgba[i],
            painter.rgba[i + 1],
            painter.rgba[i + 2],
            painter.rgba[i + 3],
        ]
    };
    let (sky, ground, corner) = (pixel(20, 50), pixel(20, 100), pixel(2, 2));
    println!(
        "HDG {:.1} pitch {:.1}: sky {sky:?}, ground {ground:?}, corner {corner:?}",
        readout.heading, readout.pitch
    );
    assert!(sky[2] > sky[0] && ground[0] > ground[2] && corner[3] == 0);
    assert!(angle_difference(readout.heading, 90.0) < 1e-9 && readout.pitch.abs() < 1e-9);
    assert!(painter.labels.iter().any(|l| l.text == "E"));
}

#[test]
fn the_basis_is_orthonormal_for_any_attitude() {
    for k in 0..200 {
        let a = k as f64 * 0.731;
        let nose = DVec3::new(a.cos(), (1.7 * a).sin(), (0.3 * a).cos()).normalize();
        let top = nose.any_orthonormal_vector();
        let top = (top * (0.9 * a).cos() + nose.cross(top) * (0.9 * a).sin()).normalize();
        let up = DVec3::new((0.2 * a).sin(), 0.4, (1.3 * a).cos()).normalize();
        let b = navball_basis(&NavballInput {
            nose,
            top,
            up,
            ..standing()
        });
        assert!((b.right.cross(b.top) - b.nose).length() < 1e-12, "case {k}");
        for (axis, other) in [(b.right, b.top), (b.top, b.nose), (b.north, b.east)] {
            assert!((axis.length() - 1.0).abs() < 1e-12 && axis.dot(other).abs() < 1e-12);
        }
        assert!(
            (to_ball(&b, nose) - DVec3::Z).length() < 1e-12,
            "the nose is at the centre"
        );
    }
}
