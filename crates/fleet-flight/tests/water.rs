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
