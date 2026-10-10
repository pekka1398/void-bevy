use glam::DVec3;
use void_orbit::{FrameSpec, Trajectory};
use void_view::plot::{BodyPlots, PlotPath};
#[test]
fn surface_plot_keeps_a_ground_point_fixed_and_cache_rewinds() {
    let planet = void_testkit::earth_size();
    let (mut eph, body) = void_testkit::planet_ephemeris(&planet);
    eph.extend_to(3600.0);
    let radius = planet.terrain.radius_meters + 100.0;
    let omega = eph.bodies()[body].rotation.rate();
    let mut trajectory = Trajectory::new();
    for i in 0..=1024 {
        let t = 3600.0 * i as f64 / 1024.0;
        let axes = eph.bodies()[body].rotation.body_axes(t);
        let p = axes[0] * radius;
        let v = axes[1] * radius * omega;
        trajectory.append(t, &[p.x, p.y, p.z, v.x, v.y, v.z]);
    }
    let mut surface = PlotPath::default();
    let spec = FrameSpec::BodySurface { body };
    let origin = trajectory.position(0);
    surface.update(&eph, &trajectory, spec, 0, 0.0, origin, body);
    assert!(surface.points.len() > 100);
    assert!(
        surface.points.iter().all(|p| p.length() < 1e-3),
        "ground point must stay fixed in surface plot"
    );
    let original = surface.points.clone();
    surface.update(
        &eph,
        &trajectory,
        spec,
        0,
        1800.0,
        trajectory.sample(1800.0).0,
        body,
    );
    surface.update(&eph, &trajectory, spec, 0, 0.0, origin, body);
    assert_eq!(
        surface.points, original,
        "loading an earlier time must rebuild the cache"
    );
    surface.update(
        &eph,
        &trajectory,
        FrameSpec::BodyInertial { body },
        0,
        0.0,
        origin,
        body,
    );
    assert!(
        surface.points.last().unwrap().length() > 1e6,
        "same ground point moves in an inertial plot"
    );
    let mut bodies = BodyPlots::default();
    assert!(
        bodies.update(&eph, spec, 0.0, DVec3::ZERO)[body].is_empty(),
        "centred body has no trail"
    );
}

#[test]
fn numerical_nodes_and_apsides_share_plot_placement() {
    let planet = void_testkit::earth_size();
    let (mut eph, body) = void_testkit::planet_ephemeris(&planet);
    eph.extend_to(3600.0);
    let mut trajectory = Trajectory::new();
    let r = planet.terrain.radius_meters + 400000.0;
    let axes = eph.bodies()[body].rotation.equatorial_basis();
    let tilt = 0.4_f64;
    for i in 0..=512 {
        let t = 3600.0 * i as f64 / 512.0;
        let theta = t / 3600.0 * std::f64::consts::TAU * 2.0 - 0.3;
        let radial =
            axes[0] * theta.cos() + (axes[1] * tilt.cos() + axes[2] * tilt.sin()) * theta.sin();
        let tangent =
            -axes[0] * theta.sin() + (axes[1] * tilt.cos() + axes[2] * tilt.sin()) * theta.cos();
        let p = radial * r;
        let v = tangent * r * std::f64::consts::TAU * 2.0 / 3600.0;
        trajectory.append(t, &[p.x, p.y, p.z, v.x, v.y, v.z]);
    }
    for spec in [
        FrameSpec::BodyInertial { body },
        FrameSpec::BodySurface { body },
    ] {
        let now = 10.0;
        let origin = trajectory.sample(now).0;
        let mut path = PlotPath::default();
        path.update(&eph, &trajectory, spec, 0, now, origin, body);
        assert_eq!(path.nodes.len(), 4);
        let mut evaluator = void_orbit::FrameEvaluator::new(&eph, spec);
        let here = evaluator.evaluate(&eph, now);
        let focus = void_orbit::to_frame(&here, origin);
        for (node, placed) in &path.nodes {
            let at = evaluator.evaluate(&eph, node.time);
            let expected = void_view::frame_to_ecliptic(
                &here.axes,
                void_orbit::to_frame(&at, node.position) - focus,
            );
            assert!((*placed - expected).length() < 1e-5);
            assert!(node.in_frame.z.abs() < 0.1);
            if spec == (FrameSpec::BodyInertial { body }) {
                assert!((node.apparent_inclination_radians.unwrap() - tilt).abs() < 1e-6);
            }
        }
    }
}

#[test]
fn all_four_plot_modes_transform_each_future_sample_and_body_consistently() {
    let system = void_orbit::build_system(&void_orbit::SystemSpec::from_json(include_str!(
        "../../../orbit/systems/sol.json"
    )));
    let mut eph = void_orbit::Ephemeris::new(
        &system,
        void_orbit::EphemerisOptions {
            step_seconds: 60.0,
            chunk_steps: 32,
        },
    );
    eph.extend_to(3600.0);
    let mut trajectory = Trajectory::new();
    for i in 0..=60 {
        let t = i as f64 * 60.0;
        let p = eph.body_position(4, t) + DVec3::new(1e7, 1e7, 1e7);
        let velocity = void_frames::BodyStates::body_state(&eph, void_frames::BodyId(4), t).1;
        trajectory.append(t, &[p.x, p.y, p.z, velocity.x, velocity.y, velocity.z]);
    }
    let now = 120.0;
    let origin = trajectory.sample(now).0;
    for spec in [
        FrameSpec::Barycentric,
        FrameSpec::BodyInertial { body: 3 },
        FrameSpec::BodySurface { body: 3 },
        FrameSpec::TwoBodyRotating {
            primary: 3,
            secondary: 4,
        },
    ] {
        let mut evaluator = void_orbit::FrameEvaluator::new(&eph, spec);
        let here = evaluator.evaluate(&eph, now);
        let focus = void_orbit::to_frame(&here, origin);
        let mut path = PlotPath::default();
        path.update(&eph, &trajectory, spec, 0, now, origin, 3);
        let future = evaluator.evaluate(&eph, 3600.0);
        let expected = void_view::frame_to_ecliptic(
            &here.axes,
            void_orbit::to_frame(&future, trajectory.sample(3600.0).0) - focus,
        );
        assert!((*path.points.last().unwrap() - expected).length() < 1e-3);
        let mut plots = BodyPlots::default();
        let bodies = plots.update(&eph, spec, now, origin);
        if !spec.centred_on(4) {
            let expected = void_view::frame_to_ecliptic(
                &here.axes,
                void_orbit::to_frame(&future, eph.body_position(4, 3600.0)) - focus,
            );
            assert!((*bodies[4].last().unwrap() - expected).length() < 1e-3);
        }
    }
}
