mod common;
use common::Setup;
use glam::{DQuat, DVec3};
use void_assembly::fresh_craft;
use void_frames::State;
use void_landing::PlanetFrame;
use void_testkit::{earth_size, planet_environment, planet_ephemeris};
use void_vessels::{Fleet, FleetOptions, VesselMode, VesselSnapshot};
fn scene(dt: f64, bubble: bool, air: bool) -> Fleet {
    let p = earth_size();
    let (e, home) = planet_ephemeris(&p);
    let env = planet_environment(&p, &e, home, air);
    let frame = PlanetFrame::new(&e, home);
    let state = frame.to_inertial(
        &e,
        0.0,
        State {
            position: DVec3::X * (p.terrain.radius_meters + 5000.0),
            velocity: DVec3::Y * 80.0,
        },
    );
    let q = env
        .frames()
        .tree
        .at(0.0, &e)
        .transform(env.frames().surface[home], env.frames().origin)
        .rotation()
        * DQuat::from_rotation_z(0.2);
    let mut f = Fleet::new(
        e,
        env,
        0.0,
        vec![],
        FleetOptions {
            air_dynamics: void_vessels::AirDynamics::ForceAndTorque,
            step_seconds: dt,
            ..Default::default()
        },
    );
    let mut c = fresh_craft();
    c.parts[0].definition_id = "aero-stabilizer-pod".into();
    f.launch(&c, state, q, DVec3::Z * 0.3);
    if bubble {
        let mut c = fresh_craft();
        c.parts[0].definition_id = "pod".into();
        f.launch(
            &c,
            State {
                position: state.position + DVec3::Z * 100.0,
                ..state
            },
            q,
            DVec3::ZERO,
        );
    }
    f.advance(0.0);
    assert_eq!(
        f.snapshot("v1").mode,
        if bubble {
            VesselMode::Bubble
        } else {
            VesselMode::Orbit
        }
    );
    f
}
fn run(dt: f64, bubble: bool) -> VesselSnapshot {
    let mut f = scene(dt, bubble, true);
    f.advance(2.0);
    f.snapshot("v1")
}
fn error(a: &VesselSnapshot, b: &VesselSnapshot) -> (f64, f64, f64) {
    (
        (a.position - b.position).length(),
        (a.velocity - b.velocity).length(),
        a.rotation.angle_between(b.rotation).abs(),
    )
}
#[test]
fn aerodynamic_translation_and_attitude_converge_in_both_owners() {
    for bubble in [false, true] {
        let reference = run(1.0 / 960.0, bubble);
        let a = run(1.0 / 30.0, bubble);
        let b = run(1.0 / 60.0, bubble);
        let c = run(1.0 / 120.0, bubble);
        let ea = error(&a, &reference);
        let eb = error(&b, &reference);
        let ec = error(&c, &reference);
        println!("bubble={bubble}: dt 1/30 {ea:?}, 1/60 {eb:?}, 1/120 {ec:?}");
        assert!(
            eb.1 < ea.1 * 0.65 && ec.1 < eb.1 * 0.65,
            "velocity must converge"
        );
        assert!(
            eb.2 < ea.2 * 0.65 && ec.2 < eb.2 * 0.65,
            "attitude must converge"
        );
        assert!(ec.0 < 0.02 && ec.1 < 0.02 && ec.2 < 0.001);
    }
    let orbit = run(1.0 / 120.0, false);
    let bubble = run(1.0 / 120.0, true);
    let e = error(&orbit, &bubble);
    println!("owner difference {e:?}");
    assert!(e.0 < 0.03 && e.1 < 0.02 && e.2 < 0.001);
}
#[test]
fn air_load_blocks_rails_and_vacuum_keeps_free_rotation() {
    let f = scene(1.0 / 60.0, false, true);
    assert!(f.rails_blocker().unwrap().contains("aerodynamic"));
    let mut vacuum = scene(1.0 / 60.0, false, false);
    let before = vacuum.snapshot("v1");
    let wrench = vacuum.aerodynamic_wrench("v1");
    assert_eq!(wrench.force, DVec3::ZERO);
    assert_eq!(wrench.torque, DVec3::ZERO);
    assert!(vacuum.rails_blocker().is_none());
    vacuum.advance(2.0);
    let after = vacuum.snapshot("v1");
    assert!(before.rotation.angle_between(after.rotation) > 0.4);
    assert!((after.angular_velocity - before.angular_velocity).length() < 1e-12);
}

#[test]
fn trial_sampling_is_pure_and_checkpoint_resumes_same_wrench() {
    for bubble in [false, true] {
        let mut f = scene(1.0 / 120.0, bubble, true);
        // The module trial calls do not commit resources, chute phase, or owner state.
        let before = serde_json::to_string(&f.checkpoint()).unwrap();
        for _ in 0..20 {
            let _ = f.aerodynamic_wrench("v1");
        }
        assert_eq!(before, serde_json::to_string(&f.checkpoint()).unwrap());
        f.advance(0.5);
        let saved: void_vessels::FleetCheckpoint =
            serde_json::from_str(&serde_json::to_string(&f.checkpoint()).unwrap()).unwrap();
        let p = earth_size();
        let (mut e, _) = planet_ephemeris(&p);
        e.extend_to(f.time());
        let mut restored = Fleet::from_checkpoint(e, f.environment().clone(), saved);
        let a = f.aerodynamic_wrench("v1");
        let b = restored.aerodynamic_wrench("v1");
        assert_eq!(a, b);
        for _ in 0..30 {
            f.advance(1.0 / 60.0);
            restored.advance(1.0 / 60.0);
        }
        assert_eq!(
            serde_json::to_value(f.checkpoint()).unwrap(),
            serde_json::to_value(restored.checkpoint()).unwrap()
        );
    }
}
#[test]
fn eccentric_chute_torque_enters_orbit_and_bubble_without_spending_trial_state() {
    for bubble in [false, true] {
        let mut f = scene(1.0 / 120.0, bubble, true);
        // Replace the acceptance vessel with a fresh eccentric-chute craft in this same world.
        let state = f.snapshot("v1");
        let mut c = fresh_craft();
        c.parts[0].definition_id = "eccentric-chute-pod".into();
        let id = f.launch(
            &c,
            State {
                position: state.position + DVec3::Z * 5000.0,
                velocity: state.velocity,
            },
            DQuat::IDENTITY,
            DVec3::ZERO,
        );
        if bubble {
            f.launch(
                &fresh_craft(),
                State {
                    position: state.position + DVec3::Z * 5100.0,
                    velocity: state.velocity,
                },
                DQuat::IDENTITY,
                DVec3::ZERO,
            );
        }
        f.parachute_command(
            &format!("{id}/p1"),
            "parachute1",
            void_modules::parachute::Command::Deploy,
        );
        f.advance(0.5);
        let snap = f.snapshot(&id);
        let w = f.aerodynamic_wrench(&id);
        assert_eq!(
            snap.mode,
            if bubble {
                VesselMode::Bubble
            } else {
                VesselMode::Orbit
            }
        );
        assert!(w.torque.length() > 1.0, "{w:?}");
        assert!(snap.angular_velocity.length() > 0.01, "{snap:?}");
        assert!(snap.rotation.is_finite() && snap.velocity.is_finite());
        assert!(f.rails_blocker().is_some());
    }
}

#[test]
fn passive_air_does_not_wake_sleeping_ground_bodies_or_block_idle_rails() {
    let scene = common::scene(Setup::Launch);
    let mut place = scene
        .fleet
        .environment()
        .body(scene.body_index)
        .unwrap()
        .clone();
    place.atmosphere = Some(void_environment::Atmosphere::earth());
    let env = std::sync::Arc::new(
        void_environment::Environment::new(&*scene.fleet.ephemeris).with(scene.body_index, place),
    );
    let ground = void_vessels::GroundSpec {
        body_index: scene.body_index,
        band_enter_meters: 200.0,
        band_exit_meters: 400.0,
        tiles: void_landing::ContactWorldOptions {
            step_seconds: 1.0 / 60.0,
            tile_level: void_landing::level_for_tile_size(
                scene.planet.terrain.radius_meters,
                300.0,
            ),
            tile_resolution: 33,
            tile_reach_meters: 300.0,
            tile_keep_meters: 600.0,
            recenter_meters: 5000.0,
            sleeping: true,
        },
    };
    let mut f = Fleet::new(
        scene.fleet.ephemeris,
        env,
        0.0,
        vec![ground],
        FleetOptions {
            air_dynamics: void_vessels::AirDynamics::ForceAndTorque,
            ..FleetOptions::default()
        },
    );
    f.launch_landed(
        &fresh_craft(),
        scene.body_index,
        void_testkit::flat_site(&scene.planet),
    );
    f.set_sas("v1", true);
    f.advance(30.0);
    f.set_sas("v1", false);
    assert!(f.scene_snapshots().iter().all(|s| s.asleep));
    assert!(f.rails_blocker().is_none());
    f.advance(2.0);
    assert!(f.scene_snapshots().iter().all(|s| s.asleep));
    assert!(f.advance_on_rails(100.0));
    assert!(f.scene_snapshots().iter().all(|s| s.asleep));
}

#[test]
fn sub_ulp_flameout_commits_once_and_representable_small_burn_is_integrated() {
    use void_assembly::{ResourceId, definition};
    use void_vessels::VesselControl;
    for fuel in [1e-14, 0.1] {
        let p = earth_size();
        let (mut e, home) = planet_ephemeris(&p);
        let t = 1024.0;
        e.extend_to(t);
        let env = planet_environment(&p, &e, home, false);
        let state = PlanetFrame::new(&e, home).to_inertial(
            &e,
            t,
            State {
                position: DVec3::X * (p.terrain.radius_meters + 500000.0),
                velocity: DVec3::Y * 7500.0,
            },
        );
        let mut f = Fleet::new(
            e,
            env,
            t,
            vec![],
            FleetOptions {
                air_dynamics: void_vessels::AirDynamics::ForceAndTorque,
                ..FleetOptions::default()
            },
        );
        let mut c = fresh_craft();
        c.parts[0].definition_id = "dual-resource-pod".into();
        c.parts[0].stage = Some(0);
        c.parts[0].resources = std::collections::BTreeMap::from([
            (ResourceId::LiquidPropellant, fuel),
            (ResourceId::Monopropellant, 0.0),
        ]);
        f.launch(&c, state, DQuat::IDENTITY, DVec3::Y * 0.1);
        f.stage("v1");
        f.set_control(
            "v1",
            VesselControl {
                throttle: 1.0,
                turn: DVec3::ZERO,
            },
        );
        f.set_sas("v1", true);
        let rating = f.thrust("v1");
        assert!(rating.flow_kg_per_second > 0.0);
        if fuel < 1e-12 {
            assert_eq!(t + rating.seconds_to_flameout, t);
        }
        let dt = if fuel < 1e-12 { 0.01 } else { 1e-8 };
        f.advance(dt);
        let remaining = f
            .parts()
            .part("v1/p1")
            .resource(ResourceId::LiquidPropellant);
        if fuel < 1e-12 {
            assert_eq!(remaining, 0.0);
            assert_eq!(
                f.snapshot("v1").mass_kg,
                definition("dual-resource-pod").unwrap().dry_mass_kg
            );
        } else {
            let accepted = f.time() - t;
            assert!(accepted > 0.0);
            assert!((fuel - remaining - rating.flow_kg_per_second * accepted).abs() < 1e-16);
        }
        assert!((f.time() - (t + dt)).abs() < 1e-12);
        assert!(f.snapshot("v1").rotation.is_finite());
    }
}

#[test]
fn ideal_guidance_prescribes_trial_and_accepted_attitude_then_releases_without_an_impulse() {
    use void_vessels::{GuidanceStatus, VesselControl};
    let p = earth_size();
    let (e, home) = planet_ephemeris(&p);
    let env = planet_environment(&p, &e, home, true);
    let state = PlanetFrame::new(&e, home).to_inertial(
        &e,
        0.0,
        State {
            position: DVec3::X * (p.terrain.radius_meters + 5000.0),
            velocity: DVec3::Y * 80.0,
        },
    );
    let mut f = Fleet::new(
        e,
        env,
        0.0,
        vec![],
        FleetOptions {
            air_dynamics: void_vessels::AirDynamics::ForceAndTorque,
            ..FleetOptions::default()
        },
    );
    f.launch(
        &void_assembly::demo_craft(),
        state,
        DQuat::IDENTITY,
        DVec3::ZERO,
    );
    f.stage("v1");
    let direction = DVec3::X;
    f.arm_guided_burn(
        "v1",
        0.0,
        0.05,
        void_orbit::AttitudeLaw::Inertial { direction },
    )
    .unwrap();
    f.advance(0.05);
    assert_eq!(f.guidance("v1").unwrap().status, GuidanceStatus::Completed);
    let before = f.snapshot("v1");
    assert!((before.rotation * DVec3::Y - direction).length() < 1e-12);
    assert_eq!(before.angular_velocity, DVec3::ZERO);
    assert!(f.aerodynamic_wrench("v1").torque.length() > 1.0);
    f.set_control(
        "v1",
        VesselControl {
            throttle: 0.0,
            turn: DVec3::ZERO,
        },
    );
    f.advance(0.0);
    let released = f.snapshot("v1");
    assert_eq!(released.rotation, before.rotation);
    assert_eq!(released.angular_velocity, before.angular_velocity);
    f.advance(1.0 / 60.0);
    assert!(f.snapshot("v1").angular_velocity.length() > 1e-6);
}

#[test]
fn force_only_is_the_explicit_default_and_checkpoint_mode_is_required() {
    use void_vessels::AirDynamics;
    assert_eq!(FleetOptions::default().air_dynamics, AirDynamics::ForceOnly);
    let mut f = scene(1.0 / 60.0, false, true);
    let full = f.aerodynamic_wrench("v1");
    assert!(full.torque.length() > 0.0);
    f.options.air_dynamics = AirDynamics::ForceOnly;
    let old = f.aerodynamic_wrench("v1");
    assert_eq!(old.torque, DVec3::ZERO);
    assert_ne!(
        old.force, full.force,
        "rotating point flow belongs only to full mode"
    );
    let mut value = serde_json::to_value(f.checkpoint()).unwrap();
    assert_eq!(value["version"], 13);
    value["options"]
        .as_object_mut()
        .unwrap()
        .remove("air_dynamics");
    assert!(
        serde_json::from_value::<void_vessels::FleetCheckpoint>(value).is_err(),
        "missing physics mode must not default during restore"
    );
}

#[test]
fn torque_free_vessel_acquires_air_torque_on_first_ceiling_entry_leg() {
    let p = earth_size();
    let (e, home) = planet_ephemeris(&p);
    let env = planet_environment(&p, &e, home, true);
    let frame = PlanetFrame::new(&e, home);
    let state = frame.to_inertial(
        &e,
        0.0,
        State {
            position: DVec3::X * (p.terrain.radius_meters + 120_010.0),
            velocity: -DVec3::X * 30_000.0 + DVec3::Y * 80.0,
        },
    );
    let q = env
        .frames()
        .tree
        .at(0.0, &e)
        .transform(env.frames().surface[home], env.frames().origin)
        .rotation()
        * DQuat::from_rotation_z(0.2);
    let mut f = Fleet::new(
        e,
        env,
        0.0,
        vec![],
        FleetOptions {
            air_dynamics: void_vessels::AirDynamics::ForceAndTorque,
            step_seconds: 1.0 / 120.0,
            ..Default::default()
        },
    );
    let mut craft = fresh_craft();
    craft.parts[0].definition_id = "aero-stabilizer-pod".into();
    f.launch(&craft, state, q, DVec3::ZERO);
    f.advance(0.0);
    assert_eq!(f.aerodynamic_wrench("v1").torque, DVec3::ZERO);
    f.advance(1.0);
    let s = f.snapshot("v1");
    assert_eq!(s.mode, VesselMode::Orbit);
    assert!(f.aerodynamic_wrench("v1").torque.length() > 0.01);
    assert!(
        s.angular_velocity.length() > 1e-5,
        "first ceiling-entry leg must integrate aerodynamic torque"
    );
    assert!(s.rotation.angle_between(q) > 1e-6);
}
