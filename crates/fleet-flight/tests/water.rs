use glam::DVec3;
use void_fleet_flight::session::{Action, FlightSession, InitialWorld, Outcome};
#[test]
fn capsule_splashdown_floats_and_replays() {
    let mut planet = void_landing::earth_size();
    planet.sea_level = Some(1800.);
    let mut session = FlightSession::new(
        InitialWorld::new(&planet, &void_assembly::reentry_capsule(), DVec3::X, true)
            .with_air_dynamics(void_vessels::AirDynamics::ForceAndTorque),
    )
    .with_recording();
    let id = void_fleet_flight::water::splashdown(&mut session);
    for _ in 0..120 {
        assert_eq!(
            session.execute(Action::Advance {
                seconds: 0.25,
                rails: false
            }),
            Outcome::Advanced(true)
        );
    }
    let sim = session.sim();
    let state = sim.fleet.body_fixed_state(&id, sim.home);
    let sea = sim
        .fleet
        .environment()
        .body(sim.home)
        .unwrap()
        .sea_level_meters
        .unwrap();
    let altitude =
        state.position.length() - sim.fleet.environment().bodies()[sim.home].radius_meters - sea;
    println!(
        "water settled altitude {altitude} velocity {:?} force {:?}",
        state.velocity,
        sim.fleet.water_wrench(&id)
    );
    assert!(altitude.abs() < 2.0);
    assert!(state.velocity.length() < 0.2);
    assert!(sim.fleet.rails_blocker().is_some());
    let checkpoint = void_fleet_flight::checkpoint::FlightCheckpoint::capture(
        session.sim(),
        session.recording_initial().clone(),
    );
    let mut restored = FlightSession::from_checkpoint(checkpoint);
    let advance = Action::Advance {
        seconds: 1.,
        rails: false,
    };
    session.execute(advance.clone());
    restored.execute(advance);
    assert_eq!(
        void_fleet_flight::session::world_mark(session.sim()),
        void_fleet_flight::session::world_mark(restored.sim())
    );
    let (mut playback, mut replay) = void_fleet_flight::session::Playback::new(session.recording());
    while playback.next_frame(&mut replay) {}
}

#[test]
fn tilted_fast_capsule_remains_finite_and_damps() {
    let mut planet = void_landing::earth_size();
    planet.sea_level = Some(1800.);
    let mut session = FlightSession::new(
        InitialWorld::new(&planet, &void_assembly::reentry_capsule(), DVec3::X, true)
            .with_air_dynamics(void_vessels::AirDynamics::ForceAndTorque),
    );
    let id = void_fleet_flight::water::splashdown_with(
        &mut session,
        &void_assembly::reentry_capsule(),
        40.,
        0.6,
    );
    for _ in 0..160 {
        session.execute(Action::Advance {
            seconds: 0.25,
            rails: false,
        });
    }
    let sim = session.sim();
    let state = sim.fleet.body_fixed_state(&id, sim.home);
    let height =
        state.position.length() - sim.fleet.environment().bodies()[sim.home].radius_meters - 1800.;
    println!("fast tilt settled {height} {:?}", state.velocity);
    assert!(height.abs() < 2.);
    assert!(state.velocity.length() < 0.2);
}
#[test]
fn detached_dense_engine_sinks_under_same_scene_owner() {
    let mut planet = void_landing::earth_size();
    planet.sea_level = Some(1800.);
    let mut craft=void_assembly::import_craft(r#"{"version":2,"name":"dense separated engine","parts":[{"id":"pod","definitionId":"flight-pod","resources":{},"stage":null,"attachment":null},{"id":"separator","definitionId":"flight-decoupler","resources":{},"stage":0,"attachment":{"parentId":"pod","parentNodeId":"bottom","nodeId":"top"}},{"id":"engine","definitionId":"flight-booster-engine","resources":{},"stage":1,"attachment":{"parentId":"separator","parentNodeId":"bottom","nodeId":"top"}}]}"#).unwrap();
    for i in 1..10 {
        let mut part = craft.parts[2].clone();
        part.id = format!("engine{i}");
        part.attachment.as_mut().unwrap().parent_id = if i == 1 {
            "engine".into()
        } else {
            format!("engine{}", i - 1)
        };
        craft.parts.push(part);
    }
    let mut session = FlightSession::new(
        InitialWorld::new(&planet, &void_assembly::reentry_capsule(), DVec3::X, true)
            .with_air_dynamics(void_vessels::AirDynamics::ForceAndTorque),
    );
    let id = void_fleet_flight::water::splashdown_with(&mut session, &craft, 2., 0.);
    session.execute(Action::Select { vessel: id.clone() });
    let pod_id = id;
    let Outcome::Staged(ids) = session.execute(Action::Stage) else {
        panic!("stage did not separate")
    };
    println!("separated {ids:?}");
    for _ in 0..80 {
        session.execute(Action::Advance {
            seconds: 0.25,
            rails: false,
        });
    }
    let sim = session.sim();
    let part = sim
        .fleet
        .parts()
        .parts()
        .find(|p| p.definition.id == "flight-booster-engine")
        .unwrap()
        .id
        .clone();
    let id = sim.fleet.vessel_of_part(&part);
    let state = sim.fleet.body_fixed_state(&id, sim.home);
    let altitude =
        state.position.length() - sim.fleet.environment().bodies()[sim.home].radius_meters - 1800.;
    assert_eq!(
        sim.fleet.snapshot(&id).mode,
        void_vessels::VesselMode::Ground
    );
    let pod = sim.fleet.body_fixed_state(&pod_id, sim.home);
    assert!(
        (pod.position.length() - sim.fleet.environment().bodies()[sim.home].radius_meters - 1800.)
            .abs()
            < 2.
    );
    println!("engine sunk {altitude}");
    assert!(altitude < -5.);
}
#[test]
fn airless_ground_water_sampling_is_same_in_both_air_modes() {
    let make = |mode| {
        let mut planet = void_landing::earth_size();
        planet.sea_level = Some(1800.);
        let mut initial =
            InitialWorld::new(&planet, &void_assembly::reentry_capsule(), DVec3::X, false)
                .with_air_dynamics(mode);
        initial
            .world
            .bodies
            .get_mut("terra")
            .unwrap()
            .visual
            .color_datum_meters = 1800.;
        let mut session = FlightSession::new(initial);
        let id = void_fleet_flight::water::splashdown_with(
            &mut session,
            &void_assembly::reentry_capsule(),
            8.,
            0.6,
        );
        (session, id)
    };
    let (mut only, a) = make(void_vessels::AirDynamics::ForceOnly);
    let (mut full, b) = make(void_vessels::AirDynamics::ForceAndTorque);
    for _ in 0..40 {
        for session in [&mut only, &mut full] {
            session.execute(Action::Advance {
                seconds: 0.25,
                rails: false,
            });
        }
    }
    let sa = only.sim().fleet.body_fixed_state(&a, only.sim().home);
    let sb = full.sim().fleet.body_fixed_state(&b, full.sim().home);
    println!(
        "water modes dp {} dv {}",
        (sa.position - sb.position).length(),
        (sa.velocity - sb.velocity).length()
    );
    assert!((sa.position - sb.position).length() < 1e-8);
    assert!((sa.velocity - sb.velocity).length() < 1e-8);
    assert!(
        (only.sim().fleet.snapshot(&a).angular_velocity
            - full.sim().fleet.snapshot(&b).angular_velocity)
            .length()
            < 1e-8
    );
}
#[test]
fn fully_submerged_fast_motion_dissipates_and_checkpoint_retimes_exactly() {
    let mut planet = void_landing::earth_size();
    planet.sea_level = Some(1800.);
    let initial = InitialWorld::new(&planet, &void_assembly::reentry_capsule(), DVec3::X, true)
        .with_air_dynamics(void_vessels::AirDynamics::ForceAndTorque);
    let mut session = FlightSession::new(initial.clone());
    let above = void_fleet_flight::water::splashdown(&mut session);
    let sim = session.sim();
    let body = sim.home;
    let direction = sim
        .fleet
        .body_fixed_state(&above, body)
        .position
        .normalize();
    let tangent = direction.any_orthonormal_vector();
    let transform = sim
        .fleet
        .frames()
        .transform(sim.fleet.body_frames(body).1, sim.fleet.origin_frame());
    let state = transform.apply_state(void_frames::State {
        position: direction * (planet.terrain.radius_meters + 1800. - 20.),
        velocity: tangent * 100. - direction * 100.,
    });
    let q = transform.rotation() * glam::DQuat::from_rotation_arc(DVec3::Y, direction);
    let Outcome::Spawned(id) = session.execute(Action::LaunchState {
        craft: void_assembly::reentry_capsule(),
        position: state.position,
        velocity: state.velocity,
        rotation: q,
        angular_velocity: DVec3::ZERO,
    }) else {
        panic!("submerged launch")
    };
    session.execute(Action::Select { vessel: id.clone() });
    let mut previous = 20000.;
    for _ in 0..15 {
        session.execute(Action::Advance {
            seconds: 1. / 60.,
            rails: false,
        });
        let sim = session.sim();
        let speed = sim
            .fleet
            .body_fixed_state(&id, sim.home)
            .velocity
            .length_squared();
        assert!(
            speed.is_finite() && speed <= previous + 1.,
            "energy growth {previous} -> {speed}"
        );
        previous = speed;
    }
    assert!(previous < 100., "stiff drag failed to dissipate {previous}");
    let base = void_fleet_flight::checkpoint::FlightCheckpoint::capture(
        session.sim(),
        session.recording_initial().clone(),
    );
    let mut restored = FlightSession::from_checkpoint(base);
    for seconds in [1. / 60., 0.2, 0.5] {
        let action = Action::Advance {
            seconds,
            rails: false,
        };
        session.execute(action.clone());
        restored.execute(action);
        assert_eq!(
            void_fleet_flight::session::world_mark(session.sim()),
            void_fleet_flight::session::world_mark(restored.sim())
        );
    }
}
