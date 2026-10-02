use glam::DVec3;
use void_orbit::*;
use void_orbit_lab::{
    scene::{PathKind, SceneView},
    *,
};
fn settle(lab: &mut OrbitLab) {
    for _ in 0..50 {
        lab.sim.extend_prediction(4000);
        lab.sim.extend_plan(4000);
    }
}
#[test]
fn both_systems_and_all_frames_have_finite_paths() {
    for system in [SystemPreset::Sol, SystemPreset::Binary] {
        let mut lab = OrbitLab::new(system);
        lab.paused = true;
        lab.sim.advance(120.0, 20000);
        settle(&mut lab);
        let moon = lab.target.unwrap();
        let mut scene = SceneView::default();
        for frame in [
            FrameSpec::Barycentric,
            FrameSpec::BodyInertial { body: lab.home },
            FrameSpec::BodySurface { body: lab.home },
            FrameSpec::TwoBodyRotating {
                primary: lab.home,
                secondary: moon,
            },
        ] {
            lab.set_frame(frame);
            let plotted = scene.update(&mut lab);
            assert_eq!(plotted.bodies.len(), lab.sim.system.bodies.len());
            assert!(plotted.paths.iter().any(|p| p.kind == PathKind::Prediction));
            assert!(plotted.paths.iter().any(|p| p.kind == PathKind::History));
            for path in &plotted.paths {
                for &(_, p) in &path.points {
                    assert!(p.is_finite());
                }
            }
            assert!(
                plotted
                    .markers
                    .iter()
                    .any(|m| m.focus == Some(Focus::Vessel))
            );
            if let FrameSpec::TwoBodyRotating { primary, secondary } = frame {
                let separation =
                    plotted.bodies[secondary].position - plotted.bodies[primary].position;
                assert!(separation.x > 0.0);
                assert!(separation.y.abs() < 1e-4 && separation.z.abs() < 1e-4);
            }
        }
    }
}
#[test]
fn burn_target_endpoints_and_execution_follow_the_same_plan() {
    let mut lab = OrbitLab::new(SystemPreset::Sol);
    lab.paused = true;
    lab.add_burn().unwrap();
    lab.edit_burn(|b| {
        b.prograde = 40.0;
        b.radial = 5.0;
    })
    .unwrap();
    settle(&mut lab);
    let mut scene = SceneView::default();
    let plotted = scene.update(&mut lab);
    let plan = plotted
        .paths
        .iter()
        .find(|p| p.kind == PathKind::Plan)
        .unwrap();
    let target = plotted
        .paths
        .iter()
        .find(|p| p.kind == PathKind::Target)
        .unwrap();
    assert_eq!(
        plan.points.first().unwrap().0,
        target.points.first().unwrap().0
    );
    let tail = lab.sim.plan.trajectory.last_time();
    let expected = lab
        .sim
        .plan
        .trajectory
        .position(lab.sim.plan.trajectory.count() - 1);
    let target_body = lab.target.unwrap();
    let expected_gap = (expected - lab.sim.ephemeris.body_position(target_body, tail)).length()
        - lab.sim.system.bodies[target_body].radius_meters;
    assert_eq!(plotted.target_gap.unwrap(), expected_gap);
    assert_eq!(plotted.markers.iter().filter(|m| m.ring).count(), 2);
    assert!(plotted.markers.iter().any(|m| m.text.starts_with("Burn 1")));
    // Target-centred plotting suppresses its zero-length trail but retains its endpoint.
    lab.set_frame(FrameSpec::BodyInertial { body: target_body });
    let plotted = scene.update(&mut lab);
    assert!(!plotted.paths.iter().any(|p| p.kind == PathKind::Target));
    assert_eq!(plotted.markers.iter().filter(|m| m.ring).count(), 2);
    lab.target = None;
    let plotted = scene.update(&mut lab);
    assert_eq!(plotted.markers.iter().filter(|m| m.ring).count(), 1);
    // Warp stops before ignition, then executes the same finite burn, never teleporting.
    lab.warp_to_burn().unwrap();
    for _ in 0..100 {
        lab.tick(0.05);
        if lab.warp_target.is_none() {
            break;
        }
    }
    assert!(lab.warp_target.is_none());
    assert_eq!(lab.warp, 0);
    assert!((lab.sim.time - 570.0).abs() < 1e-8);
    lab.warp = 1;
    for _ in 0..600 {
        lab.tick(0.05);
        if lab.sim.plan.completed_count == 1 {
            break;
        }
    }
    assert_eq!(lab.sim.plan.completed_count, 1);
    assert_eq!(lab.sim.plan.count(), 0);
    assert!(lab.selected_burn.is_none());
    assert!(lab.sim.fuel_kg() < 30000.0);
}
#[test]
fn resetting_switching_planes_retention_and_blocked_burns() {
    let mut lab = OrbitLab::new(SystemPreset::Binary);
    lab.sim.advance(100.0, 20000);
    lab.set_spans(DAY, 3600.0);
    assert_eq!(lab.sim.retention_seconds(), 2.0 * DAY);
    lab.set_start_plane(StartPlane::Equatorial {
        inclination_radians: 0.0,
    });
    assert_eq!(lab.sim.time, 100.0);
    let planet = &lab.sim.system.bodies[lab.home];
    let mut p = vec![DVec3::ZERO; lab.sim.system.bodies.len()];
    let mut v = p.clone();
    lab.sim
        .ephemeris
        .states_at(lab.sim.time, &mut p, Some(&mut v));
    let state = lab.sim.vessel();
    let pole = (state.position - p[lab.home])
        .cross(state.velocity - v[lab.home])
        .normalize();
    assert!((pole - planet.rotation.axis()).length() < 1e-10);
    lab.add_burn().unwrap();
    lab.edit_burn(|b| b.prograde = 1e6).unwrap();
    assert!(lab.sim.plan.status(0).is_err());
    assert!(lab.warp_to_burn().is_err());
    lab.remove_burn().unwrap();
    assert!(lab.selected_burn.is_none());
    lab.add_burn().unwrap();
    lab.edit_burn(|b| b.prograde = 40.0).unwrap();
    lab.sim.advance(601.0, 20000);
    assert!(lab.sim.executing_burn().is_some());
    assert!(lab.edit_burn(|b| b.normal = 1.0).is_err());
    assert!(lab.remove_burn().is_err());
    lab.reset_vessel();
    assert_eq!(lab.sim.plan.count(), 0);
    assert_eq!(lab.sim.fuel_kg(), 30000.0);
}
#[test]
fn plotted_geometry_and_fade_match_actual_ts_scene_view() {
    let golden: serde_json::Value = serde_json::from_str(include_str!("scene.json")).unwrap();
    let mut lab = OrbitLab::new(SystemPreset::Sol);
    lab.sim.advance(120.0, 20000);
    lab.add_burn().unwrap();
    lab.edit_burn(|b| {
        b.prograde = 40.0;
        b.radial = 5.0;
    })
    .unwrap();
    settle(&mut lab);
    for case in golden["cases"].as_array().unwrap() {
        lab.set_frame(match case["frame"]["kind"].as_str().unwrap() {
            "barycentric" => FrameSpec::Barycentric,
            "body-inertial" => FrameSpec::BodyInertial { body: lab.home },
            "body-surface" => FrameSpec::BodySurface { body: lab.home },
            "two-body-rotating" => FrameSpec::TwoBodyRotating {
                primary: lab.home,
                secondary: lab.target.unwrap(),
            },
            _ => unreachable!(),
        });
        let scene = SceneView::default().update(&mut lab);
        for (name, kind) in [
            ("history", PathKind::History),
            ("prediction", PathKind::Prediction),
            ("plan", PathKind::Plan),
            ("target", PathKind::Target),
        ] {
            let path = scene.paths.iter().find(|p| p.kind == kind).unwrap();
            let expected = &case[name];
            assert_eq!(
                path.points.len(),
                expected["count"].as_u64().unwrap() as usize,
                "{name} {:?}",
                lab.frame
            );
            for (k, index) in expected["indices"].as_array().unwrap().iter().enumerate() {
                let (t, position) = path.points[index.as_u64().unwrap() as usize];
                let p = &expected["points"][k];
                let expected_position = DVec3::new(
                    p[0].as_f64().unwrap(),
                    p[1].as_f64().unwrap(),
                    p[2].as_f64().unwrap(),
                );
                // TS render vertices are f32 kilometres; round the Rust vertex at the same boundary.
                let actual = (position * 1e-3).as_vec3().as_dvec3();
                assert!(
                    (actual - expected_position).length() < 1e-3,
                    "{name} vertex {k}: {} km",
                    (actual - expected_position).length()
                );
                let (hex, shade) = path.shade(t);
                let rgb: bevy::color::LinearRgba =
                    bevy::color::Srgba::hex(hex.trim_start_matches('#'))
                        .unwrap()
                        .into();
                let color = [rgb.red, rgb.green, rgb.blue];
                for (axis, value) in color.iter().enumerate() {
                    let difference = (*value as f64 * shade
                        - expected["colors"][k][axis].as_f64().unwrap())
                    .abs();
                    assert!(difference < 1e-6, "{name} colour {difference}");
                }
            }
        }
        for (index, body_path) in case["bodyPaths"].as_array().unwrap().iter().enumerate() {
            let visible = scene
                .paths
                .iter()
                .any(|p| p.kind == PathKind::Body && p.color == lab.sim.system.bodies[index].color);
            assert_eq!(visible, body_path["visible"].as_bool().unwrap());
        }
    }
}
