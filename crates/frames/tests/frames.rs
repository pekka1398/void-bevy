use std::f64::consts::{FRAC_PI_2, TAU};

use glam::{DQuat, DVec3};
use void_frames::{BodyId, BodyStates, FrameTree, Motion, Spin, State};

/// Bodies on circular orbits about the barycentre: (radius, period, phase at t = 0).
struct Circles(Vec<(f64, f64, f64)>);

impl BodyStates for Circles {
    fn body_state(&self, body: BodyId, t: f64) -> (DVec3, DVec3) {
        let (r, period, phase) = self.0[body.0];
        let w = TAU / period;
        let a = phase + w * t;
        (DVec3::new(r * a.cos(), r * a.sin(), 0.0), DVec3::new(-r * w * a.sin(), r * w * a.cos(), 0.0))
    }
}

/// Bodies that stay put.
struct Still(Vec<DVec3>);

impl BodyStates for Still {
    fn body_state(&self, body: BodyId, _t: f64) -> (DVec3, DVec3) {
        (self.0[body.0], DVec3::ZERO)
    }
}

const EARTH: Spin = Spin {
    period_seconds: 86_164.1,
    obliquity_radians: 0.4091,
    pole_longitude_radians: -FRAC_PI_2,
    angle_at_epoch_radians: 1.2,
};
const MOON: Spin = Spin {
    period_seconds: 2_360_591.5,
    obliquity_radians: 0.1,
    pole_longitude_radians: 0.7,
    angle_at_epoch_radians: 0.3,
};
const AU: f64 = 1.495_978_707e11;
const EARTH_RADIUS: f64 = 6.371e6;

fn v3(json: &serde_json::Value) -> DVec3 {
    let a: Vec<f64> = json.as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
    DVec3::new(a[0], a[1], a[2])
}

/// Frame axes (as columns in the parent) from a rotation.
fn columns(q: DQuat) -> [DVec3; 3] {
    [q * DVec3::X, q * DVec3::Y, q * DVec3::Z]
}

#[test]
fn matches_the_orbit_lab() {
    let golden: serde_json::Value =
        serde_json::from_str(include_str!("golden/frames.json")).expect("golden/frames.json");
    let origin = v3(&golden["origin"]);
    let points: Vec<DVec3> = golden["points"].as_array().unwrap().iter().map(v3).collect();
    let (mut axes_error, mut point_error) = (0.0_f64, 0.0_f64);
    for case in golden["cases"].as_array().unwrap() {
        let s = &case["spin"];
        let spin = Spin {
            period_seconds: s["periodSeconds"].as_f64().unwrap(),
            obliquity_radians: s["obliquityRadians"].as_f64().unwrap(),
            pole_longitude_radians: s["poleLongitudeRadians"].as_f64().unwrap(),
            angle_at_epoch_radians: s["angleAtEpochRadians"].as_f64().unwrap(),
        };
        let mut tree = FrameTree::new();
        let (inertial, surface) = tree.add_body(BodyId(0), spin);
        let bodies = Still(vec![origin]);

        let equatorial = columns(tree.at(0.0, &bodies).transform(inertial, FrameTree::ROOT).rotation());
        for (axis, expected) in equatorial.iter().zip(case["equatorial"].as_array().unwrap()) {
            axes_error = axes_error.max((*axis - v3(expected)).length());
        }
        for o in case["orientations"].as_array().unwrap() {
            let snapshot = tree.at(o["t"].as_f64().unwrap(), &bodies);
            let axes = columns(snapshot.transform(surface, FrameTree::ROOT).rotation());
            for (axis, expected) in axes.iter().zip(o["axes"].as_array().unwrap()) {
                axes_error = axes_error.max((*axis - v3(expected)).length());
            }
            let into = snapshot.transform(FrameTree::ROOT, surface);
            for (p, expected) in points.iter().zip(o["toFrame"].as_array().unwrap()) {
                let expected = v3(expected);
                point_error = point_error.max((into.apply_point(*p) - expected).length() / expected.length());
            }
        }
    }
    println!("orbit lab: axes within {axes_error:.1e}, points within {point_error:.1e} (relative)");
    assert!(axes_error < 1e-12, "axes differ by {axes_error:e}");
    assert!(point_error < 1e-12, "points differ by {point_error:e} relative");
}

#[test]
fn common_ancestor_keeps_metre_scale_precision_at_one_au() {
    let mut tree = FrameTree::new();
    let (_, surface) = tree.add_body(BodyId(0), EARTH);
    let tile_at = DVec3::new(0.31, -0.72, 0.62).normalize() * EARTH_RADIUS;
    let tile_turn = DQuat::from_euler(glam::EulerRot::ZXY, 0.4, -1.1, 0.25);
    let tile = tree.add_fixed(surface, Motion::fixed(tile_at, tile_turn));
    let vessel = tree.add_free(surface);
    let offset = DVec3::new(0.7, -0.4, 0.55);
    let bodies = Circles(vec![(AU, 3.155_76e7, 0.3)]);

    let (mut via_ancestor, mut via_root) = (0.0_f64, 0.0_f64);
    for step in 0..50 {
        let t = 1.0e5 + 7_919.0 * f64::from(step);
        tree.set_free(vessel, t, Motion::fixed(tile_at + offset, DQuat::IDENTITY));
        let snapshot = tree.at(t, &bodies);
        assert_eq!(snapshot.common_ancestor(vessel, tile), surface);
        let expected = tile_turn.inverse() * offset;
        via_ancestor = via_ancestor.max((snapshot.transform(vessel, tile).apply_point(DVec3::ZERO) - expected).length());
        via_root = via_root.max((snapshot.transform_via_root(vessel, tile).apply_point(DVec3::ZERO) - expected).length());
    }
    println!("1 m apart at 1 AU: {via_ancestor:.1e} m through the common ancestor, {via_root:.1e} m through the root");
    // A point 6.4e6 m from the centre is spaced 9e-10 m in f64; nothing finer is possible.
    assert!(via_ancestor < 1e-8, "through the common ancestor: {via_ancestor:e} m");
    assert!(via_root > 1e-7, "the root path should show 1 AU rounding, got {via_root:e} m");
}

#[test]
fn ground_point_moves_with_the_spin() {
    let mut tree = FrameTree::new();
    let (inertial, surface) = tree.add_body(BodyId(0), EARTH);
    let bodies = Still(vec![DVec3::new(AU, 0.0, 0.0)]);
    let ground = DVec3::new(4.1e6, -2.3e6, 4.3e6);
    let s = tree.at(5_000.0, &bodies).transform(surface, inertial).apply_state(State { position: ground, velocity: DVec3::ZERO });
    let expected = DVec3::new(0.0, 0.0, EARTH.rate()).cross(s.position);
    let error = (s.velocity - expected).length();
    println!("ground point: |v| {:.3} m/s, error {error:.1e} m/s", s.velocity.length());
    assert!(error < 1e-12, "ground velocity off by {error:e}");
}

/// Velocities from `transform` against finite differences of positions, between two moving,
/// spinning bodies. This checks `then` and `inverse` together.
#[test]
fn velocities_match_finite_differences() {
    let mut tree = FrameTree::new();
    let (_, earth) = tree.add_body(BodyId(0), EARTH);
    let (_, moon) = tree.add_body(BodyId(1), MOON);
    let pad = tree.add_fixed(moon, Motion::fixed(DVec3::new(1.2e6, 8.0e5, -3.0e5), DQuat::from_rotation_y(0.6)));
    let bodies = Circles(vec![(1.0e8, 2.0e6, 0.0), (4.8e8, 2.4e6, 1.0)]);
    let point = State { position: DVec3::new(30.0, -12.0, 5.0), velocity: DVec3::new(1.5, 0.2, -0.7) };
    let (t, h) = (2.0e5, 1e-2);

    let at = |t: f64| tree.at(t, &bodies).transform(pad, earth);
    let here = at(t).apply_state(point);
    // The point moves with its own velocity in the pad frame while the frames move.
    let ahead = at(t + h).apply_point(point.position + point.velocity * h);
    let behind = at(t - h).apply_point(point.position - point.velocity * h);
    let numeric = (ahead - behind) / (2.0 * h);
    let error = (here.velocity - numeric).length() / here.velocity.length();
    println!("moon pad in earth's surface frame: |v| {:.1} m/s, finite difference within {error:.1e}", here.velocity.length());
    assert!(error < 1e-7, "velocity differs from the finite difference by {error:e}");
}

#[test]
fn transforms_compose_and_invert() {
    let mut tree = FrameTree::new();
    let (earth_inertial, earth) = tree.add_body(BodyId(0), EARTH);
    let (_, moon) = tree.add_body(BodyId(1), MOON);
    let bodies = Circles(vec![(1.0e8, 2.0e6, 0.0), (4.8e8, 2.4e6, 1.0)]);
    let snapshot = tree.at(7.7e5, &bodies);
    let state = State { position: DVec3::new(1.7e6, -2.0e5, 9.0e5), velocity: DVec3::new(-120.0, 33.0, 8.0) };

    let back = snapshot.transform(moon, earth).apply_state(snapshot.transform(earth, moon).apply_state(state));
    let round_trip = (back.position - state.position).length().max((back.velocity - state.velocity).length());
    let direct = snapshot.transform(earth, earth_inertial).apply_state(state);
    let chained = snapshot.transform(earth, moon).to_motion().then(&snapshot.transform(moon, earth_inertial).to_motion()).apply_state(state);
    let chain = (direct.position - chained.position).length().max((direct.velocity - chained.velocity).length());
    println!("round trip through the moon {round_trip:.1e}, chain against direct {chain:.1e}");
    assert!(round_trip < 1e-6, "round trip off by {round_trip:e}");
    assert!(chain < 1e-6, "chain off by {chain:e}");
}

#[test]
fn spin_angle_keeps_precision_at_large_times() {
    let spin = Spin { period_seconds: 86_400.0, obliquity_radians: 0.0, pole_longitude_radians: 0.0, angle_at_epoch_radians: 0.0 };
    // 1e9 s is 11,574 turns and 6,400 s; both are exact, so the exact angle is known.
    let t = 1.0e9 + 0.5;
    let exact = TAU * (6_400.5 / 86_400.0);
    let remainder = (spin.angle(t) - exact).abs();
    let naive = (TAU * t / spin.period_seconds).rem_euclid(TAU) - exact;
    println!("angle at t = 1e9 s: remainder {remainder:.1e} rad, naive {:.1e} rad", naive.abs());
    assert!(remainder < 1e-15, "remainder angle off by {remainder:e}");
}

#[test]
#[should_panic(expected = "used before it was written")]
fn unwritten_free_frame_panics() {
    let mut tree = FrameTree::new();
    let free = tree.add_free(FrameTree::ROOT);
    let bodies = Still(vec![]);
    tree.at(0.0, &bodies).transform(free, FrameTree::ROOT);
}

#[test]
#[should_panic(expected = "written at t = 1")]
fn stale_free_frame_panics() {
    let mut tree = FrameTree::new();
    let free = tree.add_free(FrameTree::ROOT);
    tree.set_free(free, 1.0, Motion::IDENTITY);
    let bodies = Still(vec![]);
    tree.at(2.0, &bodies).transform(free, FrameTree::ROOT);
}

#[test]
#[should_panic(expected = "rotation not unit")]
fn non_unit_rotation_panics() {
    Motion::fixed(DVec3::ZERO, DQuat::from_xyzw(0.0, 0.0, 0.0, 2.0));
}

#[test]
#[should_panic(expected = "spin period")]
fn invalid_spin_panics() {
    FrameTree::new().add_body(BodyId(0), Spin { period_seconds: 0.0, ..EARTH });
}
