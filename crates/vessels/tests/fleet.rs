use glam::{DMat3, DQuat, DVec3};
use void_assembly::demo_craft;
use void_landing::FrameState;
use void_orbit::{AdvanceOutcome, VesselPropagator};
use void_vessels::*;
fn state(s: &VesselSnapshot) -> FrameState {
    FrameState {
        position: s.position,
        velocity: s.velocity,
    }
}
fn reference_errors(scene: &mut LabScene) -> (f64, f64) {
    let mut prop = VesselPropagator::new(&scene.fleet.ephemeris, scene.fleet.options.tolerances);
    let mut p: f64 = 0.0;
    let mut v: f64 = 0.0;
    let time = scene.fleet.time();
    for (id, run) in &mut scene.references {
        assert_eq!(
            prop.advance(&mut scene.fleet.ephemeris, run, time, 100_000, None, None),
            AdvanceOutcome::Reached
        );
        let actual = scene.fleet.snapshot(id);
        p = p.max((actual.position - run.state().position).length());
        v = v.max((actual.velocity - run.state().velocity).length());
    }
    (p, v)
}
fn momentum(f: &Fleet) -> DVec3 {
    f.vessel_ids().iter().fold(DVec3::ZERO, |sum, id| {
        let s = f.snapshot(id);
        sum + s.velocity * s.mass_kg
    })
}
fn angular(f: &Fleet) -> DVec3 {
    let ids = f.vessel_ids();
    let ss: Vec<_> = ids.iter().map(|id| f.snapshot(id)).collect();
    let rr: Vec<_> = ids.iter().map(|id| f.relative(id, &ids[0])).collect();
    let mass = ss.iter().map(|s| s.mass_kg).sum::<f64>();
    let c = ss
        .iter()
        .zip(&rr)
        .fold(DVec3::ZERO, |sum, (s, r)| sum + r.position * s.mass_kg)
        / mass;
    let v = ss
        .iter()
        .zip(&rr)
        .fold(DVec3::ZERO, |sum, (s, r)| sum + r.velocity * s.mass_kg)
        / mass;
    ss.iter().zip(&rr).fold(DVec3::ZERO, |sum, (s, r)| {
        let rot = DMat3::from_quat(s.rotation);
        let i = DMat3::from_cols_array(&f.inertia(&s.id)).transpose();
        sum + rot * i * rot.transpose() * s.angular_velocity
            + (r.position - c).cross(r.velocity - v) * s.mass_kg
    })
}
#[test]
fn bubble_handoff_and_ten_minute_coast_match_independent_orbits() {
    let mut scene = create_lab_scene(Scenario::Coast);
    let before = scene.fleet.snapshot("v1");
    assert_eq!(before.mode, VesselMode::Bubble);
    assert_eq!(scene.fleet.bubble_count(), 1);
    scene.fleet.advance(600.0);
    let (p, v) = reference_errors(&mut scene);
    assert!(p < 0.02, "coast position {p}");
    assert!(v < 2e-5, "coast velocity {v}");
    assert!(scene.fleet.scene_snapshots().iter().all(|s| !s.asleep));
    let previous = scene.fleet.snapshot("v1");
    assert!(scene.fleet.advance_on_rails(0.0));
    let after = scene.fleet.snapshot("v1");
    assert_eq!(after.mode, VesselMode::Orbit);
    assert!((after.position - previous.position).length() < 1e-5);
    assert!((after.velocity - previous.velocity).length() < 1e-6);
}
#[test]
fn encounter_enters_and_leaves_one_shared_scene() {
    let mut s = create_lab_scene(Scenario::Encounter);
    assert_eq!(s.fleet.bubble_count(), 0);
    s.fleet.advance(700.0);
    assert_eq!(s.fleet.bubble_count(), 0);
    let events: Vec<_> = s
        .fleet
        .events
        .iter()
        .filter(|e| e.vessel == "v1" && e.from.is_some())
        .collect();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].to, Some(VesselMode::Bubble));
    assert!(
        (events[0].time - 196.0).abs() < 3.0,
        "entry {}",
        events[0].time
    );
    assert_eq!(events[1].to, Some(VesselMode::Orbit));
    assert!(
        (events[1].time - 652.0).abs() < 3.0,
        "exit {}",
        events[1].time
    );
    let (p, v) = reference_errors(&mut s);
    assert!(p < 0.03, "encounter error {p}");
    assert!(v < 3e-5, "encounter velocity {v}");
}
#[test]
fn spinning_separation_keeps_parts_poses_fuel_and_momentum() {
    let mut s = create_lab_scene(Scenario::Separate);
    let f = &mut s.fleet;
    let p = momentum(f);
    let l = angular(f);
    let before = f.part_snapshots("v1");
    let child = f.decouple("v1/p4");
    assert_eq!(child, "v2");
    assert!(
        (momentum(f) - p).length() < 0.1,
        "separation linear momentum"
    );
    assert!(
        (angular(f) - l).length() < 0.01,
        "separation angular momentum"
    );
    assert_eq!(f.snapshot("v1").mass_kg + f.snapshot("v2").mass_kg, 4590.0);
    assert_eq!(f.fuel("v1/p2"), 700.0);
    assert_eq!(f.fuel("v1/p5"), 2800.0);
    let after: Vec<_> = f
        .vessel_ids()
        .iter()
        .flat_map(|id| f.part_snapshots(id))
        .collect();
    assert_eq!(after.len(), before.len());
    for p in before {
        let a = after.iter().find(|a| a.id == p.id).unwrap();
        assert!((a.position - p.position).length() < 5e-5);
        assert!(a.rotation.angle_between(p.rotation) < 1e-6);
    }
    assert_eq!(f.control("v2").throttle, 0.0);
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f.set_sas("v2", true))).is_err()
    );
}
#[test]
fn collision_and_join_preserve_part_graph_and_both_momenta() {
    let mut s = create_lab_scene(Scenario::Join);
    while s.fleet.node_gap("v1/p2", "bottom", "v2/p2", "bottom") > 0.08 && s.fleet.time() < 60.0 {
        s.fleet.advance(1.0 / 60.0);
    }
    let f = &mut s.fleet;
    let p = momentum(f);
    let l = angular(f);
    let before: Vec<_> = f
        .vessel_ids()
        .iter()
        .flat_map(|id| f.part_snapshots(id))
        .collect();
    let gap = f.node_gap("v1/p2", "bottom", "v2/p2", "bottom");
    assert!(gap < 0.25, "join gap {gap}");
    f.join("v1/p2", "bottom", "v2/p2", "bottom");
    assert_eq!(f.vessel_ids(), ["v1"]);
    assert_eq!(f.snapshot("v1").mass_kg, 2240.0);
    assert!((momentum(f) - p).length() < 0.1);
    assert!((angular(f) - l).length() < 0.005);
    let after = f.part_snapshots("v1");
    assert_eq!(after.len(), 4);
    for p in before {
        let a = after.iter().find(|a| a.id == p.id).unwrap();
        assert!((p.position - a.position).length() < 5e-5);
        assert!(a.rotation.angle_between(p.rotation) < 1e-6);
    }
    assert!(!f.free_nodes("v1").iter().any(|n| n.node == "bottom"));
    f.advance(0.0);
    assert_eq!(f.snapshot("v1").mode, VesselMode::Orbit);
}
#[test]
fn orbit_and_bubble_burn_use_same_propellant_and_thrust() {
    let mut orbit = create_lab_scene(Scenario::Separate);
    let initial = orbit.fleet.snapshot("v1");
    let mut bubble = create_lab_scene(Scenario::Separate);
    let companion = FrameState {
        position: initial.position + DVec3::X * 300.0,
        velocity: initial.velocity,
    };
    bubble.fleet.launch(
        &pod_tank("companion"),
        companion,
        DQuat::IDENTITY,
        DVec3::ZERO,
    );
    bubble.fleet.advance(0.0);
    for s in [&mut orbit, &mut bubble] {
        s.fleet.stage("v1");
        s.fleet.set_control(
            "v1",
            VesselControl {
                throttle: 1.0,
                turn: DVec3::ZERO,
            },
        );
        s.fleet.advance(10.0);
    }
    let a = orbit.fleet.snapshot("v1");
    let b = bubble.fleet.snapshot("v1");
    assert!((a.mass_kg - b.mass_kg).abs() < 1e-8);
    assert!(
        (a.position - b.position).length() < 0.1,
        "burn error {}",
        (a.position - b.position).length()
    );
    assert!((a.velocity - b.velocity).length() < 0.01);
    assert_eq!(orbit.fleet.fuel("v1/p2"), 700.0);
    assert!(orbit.fleet.fuel("v1/p5") < 2800.0);
    assert!(orbit.fleet.rails_blocker().is_some());
    // Lit booster drains exactly to zero, then only the staged upper engine burns.
    orbit.fleet.advance(100.0);
    assert_eq!(orbit.fleet.fuel("v1/p5"), 0.0);
    assert_eq!(orbit.fleet.thrust("v1").flow_kg_per_second, 0.0);
    let split = orbit.fleet.stage("v1");
    assert_eq!(split.len(), 1);
    assert_eq!(orbit.fleet.control(&split[0]).throttle, 0.0);
    assert!(orbit.fleet.thrust("v1").force.length() > 0.0);
    assert!(orbit.fleet.stages_left("v1").is_empty());
}
#[test]
fn sas_stops_spin_and_rails_stops_for_new_encounter() {
    let mut s = create_lab_scene(Scenario::Sas);
    s.fleet.set_sas("v1", true);
    s.fleet.advance(30.0);
    assert!(s.fleet.snapshot("v1").angular_velocity.length() < 0.001);
    assert_ne!(s.fleet.sas_phase("v1"), void_sas::SasPhase::Off);
    let mut s = create_lab_scene(Scenario::Encounter);
    assert!(!s.fleet.advance_on_rails(700.0));
    assert!((s.fleet.time() - 190.0).abs() < 20.0);
    s.fleet.advance(0.0);
    assert_eq!(s.fleet.bubble_count(), 1);
    let mut s = create_lab_scene(Scenario::Coast);
    assert!(s.fleet.advance_on_rails(3000.0));
    assert_eq!(s.fleet.bubble_count(), 0);
    let (p, _) = reference_errors(&mut s);
    assert!(p < 0.03);
    s.fleet.advance(0.0);
    assert_eq!(s.fleet.bubble_count(), 1);
}
#[test]
fn landed_scenes_sleep_and_remain_fixed_during_rails() {
    let mut s = create_lab_scene(Scenario::Launch);
    assert_eq!(s.fleet.ground_count(), 2);
    let scenes = s.fleet.scene_snapshots();
    assert_eq!(scenes.iter().map(|s| s.members.len()).sum::<usize>(), 3);
    assert!(s.fleet.rails_blocker().is_some());
    for id in s.fleet.vessel_ids() {
        s.fleet.set_sas(&id, true);
    }
    s.fleet.advance(30.0);
    assert!(
        s.fleet.rails_blocker().is_none(),
        "{:?}",
        s.fleet.rails_blocker()
    );
    let p = s.fleet.body_fixed_state("v1", s.body_index);
    assert!(s.fleet.advance_on_rails(86400.0));
    let after = s.fleet.body_fixed_state("v1", s.body_index);
    assert_eq!(p.position, after.position);
    assert_eq!(p.velocity, after.velocity);
    assert!(!s.fleet.terrain_tiles().is_empty());
    let t = &s.fleet.terrain_tiles()[0];
    let (v, i) = s.fleet.terrain_geometry(t.scene, &t.tile);
    assert!(!v.is_empty() && !i.is_empty());
}
#[test]
fn fuel_recentring_keeps_live_parts_and_nodes_on_the_physics_owner() {
    for bubble in [false, true] {
        let mut s = create_lab_scene(Scenario::Separate);
        if bubble {
            let a = s.fleet.snapshot("v1");
            s.fleet.launch(
                &pod_tank("nearby"),
                FrameState {
                    position: a.position + DVec3::X * 300.0,
                    velocity: a.velocity,
                },
                DQuat::IDENTITY,
                DVec3::ZERO,
            );
            s.fleet.advance(0.0);
        }
        s.fleet.stage("v1");
        s.fleet.set_control(
            "v1",
            VesselControl {
                throttle: 1.0,
                turn: DVec3::ZERO,
            },
        );
        s.fleet.advance(5.0);
        let snapshot = s.fleet.snapshot("v1");
        let ps = s.fleet.part_snapshots("v1");
        let c = ps.iter().fold(DVec3::ZERO, |sum, p| {
            sum + (p.position - snapshot.position) * (p.definition.dry_mass_kg + p.fuel_kg)
        }) / snapshot.mass_kg;
        assert!(c.length() < 5e-5, "COM error {c}");
        for p in ps {
            let n = &p.definition.nodes[0];
            let actual = s.fleet.node_frame(&p.id, &n.id);
            let expected = p.position + p.rotation * n.position;
            assert!((actual.0 - expected).length() < 5e-5);
        }
    }
}
#[test]
fn invalid_controls_and_launch_are_rejected() {
    let mut s = create_lab_scene(Scenario::Separate);
    let mut craft = demo_craft();
    craft.parts[5].stage = None;
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| s.fleet.launch(
            &craft,
            state(&s.fleet.snapshot("v1")),
            DQuat::IDENTITY,
            DVec3::ZERO
        )))
        .is_err()
    );
    assert_eq!(s.fleet.vessel_ids(), ["v1"]);
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| s.fleet.set_control(
            "v1",
            VesselControl {
                throttle: f64::NAN,
                turn: DVec3::ZERO
            }
        )))
        .is_err()
    );
    assert_eq!(s.fleet.control("v1").throttle, 0.0);
}
#[test]
fn native_fleet_matches_owning_ts_lab_scenarios() {
    let golden: serde_json::Value = serde_json::from_str(include_str!("golden.json")).unwrap();
    let v = |a: &serde_json::Value| {
        DVec3::new(
            a[0].as_f64().unwrap(),
            a[1].as_f64().unwrap(),
            a[2].as_f64().unwrap(),
        )
    };
    for g in golden.as_array().unwrap() {
        let name = g["scenario"].as_str().unwrap();
        let scenario = match name {
            "encounter" => Scenario::Encounter,
            "coast" => Scenario::Coast,
            "separate" => Scenario::Separate,
            "join" => Scenario::Join,
            "sas" => Scenario::Sas,
            _ => panic!("unknown scenario"),
        };
        let mut scene = create_lab_scene(scenario);
        if scenario == Scenario::Separate {
            scene.fleet.decouple("v1/p4");
        }
        if scenario == Scenario::Join {
            while scene.fleet.node_gap("v1/p2", "bottom", "v2/p2", "bottom") > 0.08
                && scene.fleet.time() < 60.0
            {
                scene.fleet.advance(1.0 / 60.0);
            }
            scene.fleet.join("v1/p2", "bottom", "v2/p2", "bottom");
        }
        if scenario == Scenario::Sas {
            scene.fleet.set_sas("v1", true);
        }
        let duration = match scenario {
            Scenario::Encounter => 700.0,
            Scenario::Coast => 600.0,
            Scenario::Sas => 30.0,
            _ => 1.0,
        };
        scene.fleet.advance(duration);
        assert!(
            (scene.fleet.time() - g["time"].as_f64().unwrap()).abs() < 0.02,
            "{name} time"
        );
        for expected in g["states"].as_array().unwrap() {
            let id = expected["id"].as_str().unwrap();
            let actual = scene.fleet.snapshot(id);
            let dp = (actual.position - v(&expected["position"])).length();
            let dv = (actual.velocity - v(&expected["velocity"])).length();
            assert!(dp < 0.03, "{name}/{id} TS position {dp}");
            assert!(dv < 0.002, "{name}/{id} TS velocity {dv}");
            assert_eq!(actual.mass_kg, expected["mass"].as_f64().unwrap());
            let mode = match actual.mode {
                VesselMode::Orbit => "orbit",
                VesselMode::Bubble => "bubble",
                VesselMode::Ground => "ground",
            };
            assert_eq!(mode, expected["mode"].as_str().unwrap());
            assert_eq!(
                serde_json::to_value(&actual.part_ids).unwrap(),
                expected["parts"]
            );
            let q = &expected["rotation"];
            let q = DQuat::from_xyzw(
                q[0].as_f64().unwrap(),
                q[1].as_f64().unwrap(),
                q[2].as_f64().unwrap(),
                q[3].as_f64().unwrap(),
            );
            assert!(
                actual.rotation.angle_between(q) < 0.002,
                "{name}/{id} attitude"
            );
            assert!(
                (actual.angular_velocity - v(&expected["angularVelocity"])).length() < 0.002,
                "{name}/{id} spin"
            );
            assert!(
                (scene.fleet.relative(id, "v1").position - v(&expected["relative"])).length()
                    < 0.03
            );
        }
    }
}
#[test]
fn ground_launch_coasts_back_into_contact_and_rails_catches_descent() {
    let mut s = create_lab_scene(Scenario::Launch);
    s.fleet.advance(15.0);
    s.fleet.stage("v1");
    s.fleet.set_control(
        "v1",
        VesselControl {
            throttle: 1.0,
            turn: DVec3::ZERO,
        },
    );
    s.fleet.advance(10.0);
    s.fleet.set_control("v1", VesselControl::default());
    assert_eq!(s.fleet.snapshot("v1").mode, VesselMode::Orbit);
    let mut peak: f64 = 0.0;
    for _ in 0..600 {
        s.fleet.advance(1.0);
        peak = peak.max(s.fleet.clearance("v1", s.body_index));
        if s.fleet.snapshot("v1").mode == VesselMode::Ground {
            break;
        }
    }
    assert!(peak > 5000.0);
    assert_eq!(s.fleet.snapshot("v1").mode, VesselMode::Ground);
    let modes: Vec<_> = s
        .fleet
        .events
        .iter()
        .filter(|e| e.vessel == "v1")
        .map(|e| e.to)
        .collect();
    assert_eq!(
        modes,
        [
            Some(VesselMode::Orbit),
            Some(VesselMode::Ground),
            Some(VesselMode::Orbit),
            Some(VesselMode::Ground)
        ]
    );
    let s = create_lab_scene(Scenario::Launch);
    let frame = void_landing::PlanetFrame::new(&s.fleet.ephemeris, s.body_index);
    let d = s
        .fleet
        .body_fixed_state("v1", s.body_index)
        .position
        .normalize();
    let r = s.planet.terrain.radius_meters;
    let initial = frame.to_inertial(
        &s.fleet.ephemeris,
        s.fleet.time(),
        FrameState {
            position: d * (r + s.planet.terrain.height(d) + 1000.0),
            velocity: DVec3::ZERO,
        },
    );
    // A separate fleet excludes the still-awake ground vessels which correctly block rails.
    let tiles = void_landing::ContactWorldOptions {
        step_seconds: 1.0 / 60.0,
        tile_level: void_landing::level_for_tile_size(r, 300.0),
        tile_resolution: 33,
        tile_reach_meters: 300.0,
        tile_keep_meters: 600.0,
        recenter_meters: 5000.0,
        sleeping: true,
    };
    let time = s.fleet.time();
    let environment = s.fleet.environment().clone();
    let mut f = Fleet::new(
        s.fleet.ephemeris,
        environment,
        time,
        vec![GroundSpec {
            body_index: s.body_index,
            tiles,
            band_enter_meters: 200.0,
            band_exit_meters: 400.0,
        }],
        FleetOptions::default(),
    );
    let id = f.launch(&pod_tank("descent"), initial, DQuat::IDENTITY, DVec3::ZERO);
    assert!(!f.advance_on_rails(100.0));
    let clearance = f.clearance(&id, s.body_index);
    assert!(
        (150.0..400.0).contains(&clearance),
        "rails height {clearance}"
    );
    f.advance(0.0);
    assert_eq!(f.snapshot(&id).mode, VesselMode::Ground);
}
#[test]
fn connected_engines_share_tanks_and_drain_proportionally() {
    let mut craft = void_assembly::add_part(
        &void_assembly::fresh_craft(),
        "tank-small",
        "p1",
        "bottom",
        "top",
    )
    .unwrap();
    craft = void_assembly::add_part(&craft, "engine-small", "p2", "bottom", "top").unwrap();
    craft = void_assembly::add_part(&craft, "tank-small", "p3", "bottom", "top").unwrap();
    craft = void_assembly::add_part(&craft, "engine-small", "p4", "bottom", "top").unwrap();
    craft.parts[3].fuel_kg = 350.0;
    craft.parts[2].stage = Some(0);
    craft.parts[4].stage = Some(0);
    let s = create_lab_scene(Scenario::Separate);
    let initial = state(&s.fleet.snapshot("v1"));
    let environment = s.fleet.environment().clone();
    let mut fleet = Fleet::new(
        s.fleet.ephemeris,
        environment,
        0.0,
        vec![],
        FleetOptions::default(),
    );
    let id = fleet.launch(&craft, initial, DQuat::IDENTITY, DVec3::ZERO);
    fleet.stage(&id);
    fleet.set_control(
        &id,
        VesselControl {
            throttle: 1.0,
            turn: DVec3::ZERO,
        },
    );
    let p = fleet.thrust(&id);
    assert_eq!(p.groups.len(), 1);
    assert_eq!(p.groups[0].engines.len(), 2);
    assert_eq!(p.force, DVec3::Y * 60000.0);
    fleet.advance(10.0);
    assert!((fleet.fuel("v1/p2") - 2.0 * fleet.fuel("v1/p4")).abs() < 1e-9);
    let dt = fleet.thrust(&id).seconds_to_flameout;
    fleet.advance(dt + 1.0);
    assert_eq!(fleet.fuel("v1/p2"), 0.0);
    assert_eq!(fleet.fuel("v1/p4"), 0.0);
    assert_eq!(fleet.thrust(&id).force, DVec3::ZERO);
}
#[test]
fn vessel_frames_follow_their_physics_owner() {
    let mut s = create_lab_scene(Scenario::Launch);
    let (_, surface) = s.fleet.body_frames(s.body_index);
    let check_landed = |f: &Fleet| {
        let origin = f.origin_frame();
        for id in f.vessel_ids() {
            let snap = f.snapshot(&id);
            let frame = f.vessel_frame(&id);
            let scene = snap.scene.expect("landed vessels are in a ground scene");
            let (contact, floating) = f.scene_frames(scene);
            assert_eq!(contact, surface);
            assert_eq!(f.frame_tree().parent(frame), Some(contact));
            assert_eq!(f.frame_tree().parent(floating), Some(contact));
            let frames = f.frames();
            let out = frames.transform(frame, origin);
            let turn = out.rotation() * snap.rotation.conjugate();
            assert!(turn.xyz().length() < 1e-12, "{id}: {turn:?}");
            // The inertial snapshot, carried back down the tree, lands on the scene's own state.
            let fixed = frames
                .transform(origin, surface)
                .apply_state(void_frames::State {
                    position: snap.position,
                    velocity: snap.velocity,
                });
            let direct = f.body_fixed_state(&id, s.body_index);
            assert!((fixed.position - direct.position).length() < 1e-8, "{id}");
            assert!((fixed.velocity - direct.velocity).length() < 1e-9, "{id}");
            for part in f.part_snapshots(&id) {
                // Parts sit within a few metres of their vessel's parts frame.
                assert!((part.position - out.apply_point(DVec3::ZERO)).length() < 20.0);
            }
        }
    };
    check_landed(&s.fleet);
    s.fleet.advance(1.0);
    check_landed(&s.fleet);
    let (ephemeris, _) = void_landing::planet_ephemeris(&void_landing::pebble());
    let environment = s.fleet.environment().clone();
    let restored = Fleet::from_checkpoint(ephemeris, environment, s.fleet.checkpoint(), None);
    check_landed(&restored);
    for id in s.fleet.vessel_ids() {
        assert_eq!(
            s.fleet.part_snapshots(&id)[0].position,
            restored.part_snapshots(&id)[0].position
        );
    }

    let mut s = create_lab_scene(Scenario::Join);
    while s.fleet.bubble_count() == 0 && s.fleet.time() < 60.0 {
        s.fleet.advance(1.0 / 60.0);
    }
    let f = &mut s.fleet;
    let scene = f.snapshot("v1").scene.expect("the pair shares a bubble");
    let (bubble, floating) = f.scene_frames(scene);
    assert_eq!(f.frame_tree().parent(bubble), Some(f.origin_frame()));
    let v2 = f.vessel_frame("v2");
    assert_eq!(f.frame_tree().parent(v2), Some(bubble));
    while f.node_gap("v1/p2", "bottom", "v2/p2", "bottom") > 0.08 && f.time() < 60.0 {
        f.advance(1.0 / 60.0);
    }
    f.join("v1/p2", "bottom", "v2/p2", "bottom");
    assert!(!f.frame_tree().contains(v2));
    f.advance(0.0);
    assert_eq!(f.snapshot("v1").mode, VesselMode::Orbit);
    assert_eq!(
        f.frame_tree().parent(f.vessel_frame("v1")),
        Some(f.origin_frame())
    );
    assert!(!f.frame_tree().contains(bubble) && !f.frame_tree().contains(floating));
}

/// Terrain belongs to the world's environment: a ground needs it there, and a checkpoint only
/// restores into an environment with the terrain it was saved with.
#[test]
fn grounds_and_checkpoints_need_the_environments_terrain() {
    let panics =
        |f: &mut dyn FnMut()| std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).is_err();
    let s = create_lab_scene(Scenario::Launch);
    let ground = GroundSpec {
        body_index: s.body_index,
        tiles: void_landing::ContactWorldOptions {
            step_seconds: 1.0 / 60.0,
            tile_level: 8,
            tile_resolution: 33,
            tile_reach_meters: 300.0,
            tile_keep_meters: 600.0,
            recenter_meters: 5000.0,
            sleeping: true,
        },
        band_enter_meters: 200.0,
        band_exit_meters: 400.0,
    };
    let fresh = || void_landing::planet_ephemeris(&void_landing::pebble()).0;
    assert!(panics(&mut || {
        let e = fresh();
        let gravity_only = std::sync::Arc::new(Environment::new(&e));
        Fleet::new(
            e,
            gravity_only,
            0.0,
            vec![ground.clone()],
            FleetOptions::default(),
        );
    }));
    let saved = s.fleet.checkpoint();
    assert!(panics(&mut || {
        let e = fresh();
        let gravity_only = std::sync::Arc::new(Environment::new(&e));
        Fleet::from_checkpoint(e, gravity_only, saved.clone(), None);
    }));
    let other = std::sync::Arc::new(void_terrain::Terrain::from_config(
        &void_terrain::TerrainConfig::Layered(void_terrain::LayeredOptions {
            radius_meters: s.planet.terrain.radius_meters,
            ..void_terrain::DEFAULT_LAYERED
        }),
    ));
    assert!(panics(&mut || {
        let e = fresh();
        let elsewhere = std::sync::Arc::new(
            Environment::new(&e).with(s.body_index, BodyEnvironment::airless(other.clone())),
        );
        Fleet::from_checkpoint(e, elsewhere, saved.clone(), None);
    }));
    let e = fresh();
    let same = s.fleet.environment().clone();
    Fleet::from_checkpoint(e, same, saved, None);
}

/// The part graph is the fleet's record of its parts: separation takes a connection out of it,
/// each vessel is one of its connected groups, and a checkpoint restores every part's state and
/// pose and the connections in their order.
#[test]
fn the_part_graph_is_the_record_and_a_checkpoint_restores_it() {
    let mut s = create_lab_scene(Scenario::Separate);
    let f = &mut s.fleet;
    let connections = f.parts().connections().len();
    // The demo's first stage lights its lower engine; the second releases the decoupler and
    // lights the upper engine.
    assert!(f.stage("v1").is_empty());
    assert!(f.parts().part("v1/p6").lit);
    f.set_control(
        "v1",
        VesselControl {
            throttle: 1.0,
            ..f.control("v1")
        },
    );
    f.advance(0.5);
    assert_eq!(f.stage("v1"), ["v2"]);
    let decoupler = f.parts().part("v1/p4");
    assert!(decoupler.staged && !decoupler.lit);
    assert!(f.parts().part("v1/p3").lit);
    assert_eq!(f.parts().connections().len(), connections - 1);
    let members = |f: &Fleet, id: &str| -> Vec<String> {
        f.part_snapshots(id).into_iter().map(|p| p.id).collect()
    };
    let all: Vec<String> = f
        .vessel_ids()
        .iter()
        .flat_map(|id| members(f, id))
        .collect();
    let groups = f.parts().components(&all);
    assert_eq!(groups.len(), 2);
    for id in f.vessel_ids() {
        let mut own = members(f, &id);
        own.sort();
        assert!(
            groups.iter().any(|g| {
                let mut g = g.clone();
                g.sort();
                g == own
            }),
            "{id} is not one connected group"
        );
    }
    f.advance(0.5);
    let (ephemeris, _) = void_landing::planet_ephemeris(&s.planet);
    let restored = Fleet::from_checkpoint(
        ephemeris,
        s.fleet.environment().clone(),
        s.fleet.checkpoint(),
        None,
    );
    let (a, b): (Vec<_>, Vec<_>) = (
        s.fleet.parts().parts().collect(),
        restored.parts().parts().collect(),
    );
    assert_eq!(a.len(), b.len());
    for (a, b) in a.iter().zip(&b) {
        assert_eq!(a.id, b.id);
        assert_eq!(a.definition.id, b.definition.id);
        assert_eq!(a.fuel_kg.to_bits(), b.fuel_kg.to_bits(), "{}", a.id);
        assert_eq!(
            (a.stage, a.staged, a.lit, a.pose),
            (b.stage, b.staged, b.lit, b.pose),
            "{}",
            a.id
        );
    }
    assert_eq!(
        s.fleet.parts().connections(),
        restored.parts().connections()
    );
}

/// Every part is a frame under its vessel's parts frame, at its pose; separation and docking move
/// the parts' frames to their new vessel, and a restored fleet has them again.
#[test]
fn parts_are_frames_under_their_vessel() {
    let check = |f: &Fleet| {
        let frames = f.frames();
        for id in f.vessel_ids() {
            let vessel = f.vessel_frame(&id);
            for p in f.part_snapshots(&id) {
                let frame = f.part_frame(&p.id);
                assert_eq!(f.frame_tree().parent(frame), Some(vessel), "{}", p.id);
                let local = frames.transform(frame, vessel);
                assert_eq!(local.apply_point(DVec3::ZERO), p.local_position, "{}", p.id);
                assert!(
                    local.rotation().dot(p.local_rotation).abs() > 1.0 - 1e-15,
                    "{}",
                    p.id
                );
                for n in &p.definition.nodes {
                    let (at, out) = f.node_in(&p.id, &n.id, frame);
                    assert!((at - n.position).length() < 1e-15, "{} {}", p.id, n.id);
                    assert!((out - n.direction).length() < 1e-15, "{} {}", p.id, n.id);
                }
            }
        }
    };
    let mut s = create_lab_scene(Scenario::Join);
    check(&s.fleet);
    while s.fleet.node_gap("v1/p2", "bottom", "v2/p2", "bottom") > 0.08 && s.fleet.time() < 60.0 {
        s.fleet.advance(1.0 / 60.0);
    }
    let f = &mut s.fleet;
    check(f);
    // The gap through the two parts' common ancestor agrees with the inertial one to the
    // inertial coordinates' rounding.
    let inertial = (f.node_frame("v1/p2", "bottom").0 - f.node_frame("v2/p2", "bottom").0).length();
    let gap = f.node_gap("v1/p2", "bottom", "v2/p2", "bottom");
    assert!((gap - inertial).abs() < 1e-3, "{gap} {inertial}");
    f.join("v1/p2", "bottom", "v2/p2", "bottom");
    check(f);
    assert_eq!(f.part_snapshots("v1").len(), 4);

    let mut s = create_lab_scene(Scenario::Separate);
    s.fleet.decouple("v1/p4");
    check(&s.fleet);
    assert_eq!(
        s.fleet.frame_tree().parent(s.fleet.part_frame("v1/p5")),
        Some(s.fleet.vessel_frame("v2"))
    );
    let (ephemeris, _) = void_landing::planet_ephemeris(&s.planet);
    let restored = Fleet::from_checkpoint(
        ephemeris,
        s.fleet.environment().clone(),
        s.fleet.checkpoint(),
        None,
    );
    check(&restored);
}
