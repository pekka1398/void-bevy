use void_fleet_flight::{
    session::{FlightSession, InitialWorld},
    world::stellar_neighborhood,
};
use void_frames::{FrameSource, SystemId};

#[test]
fn authored_neighborhood_launches_normal_craft_and_directly_restores_celestial_state() {
    let planet = void_landing::aurelia();
    let craft = void_assembly::demo_craft();
    let mut initial = InitialWorld::new(&planet, &craft, void_vessels::flat_site(&planet), true);
    initial.world = stellar_neighborhood(&planet);
    initial.launch_body = "Sol/aurelia".into();
    let mut session = FlightSession::new(initial);
    assert_eq!(session.sim().fleet.ephemeris.system_count(), 3);
    let a = session
        .sim()
        .fleet
        .ephemeris
        .system_state(SystemId(0), 0.0)
        .0;
    let b = session
        .sim()
        .fleet
        .ephemeris
        .system_state(SystemId(1), 0.0)
        .0;
    assert!((b.relative(&a).length() / void_multiscale::LIGHT_YEAR - 4.24).abs() < 1e-10);
    for _ in 0..30 {
        session.execute(void_fleet_flight::session::Action::Advance {
            seconds: 1.0 / 60.0,
            rails: false,
        });
    }
    let saved = void_fleet_flight::checkpoint::FlightCheckpoint::capture(
        session.sim(),
        session.recording_initial().clone(),
    );
    let wire = serde_json::to_string(&saved).unwrap();
    let decoded: void_fleet_flight::checkpoint::FlightCheckpoint =
        serde_json::from_str(&wire).unwrap();
    let restored = decoded.restore();
    assert_eq!(restored.fleet.ephemeris.system_count(), 3);
    assert_eq!(
        restored.coupled_world.as_ref().unwrap().borrow().steps,
        session.sim().coupled_world.as_ref().unwrap().borrow().steps
    );
    assert_eq!(
        restored.fleet.snapshot(&restored.selected).position,
        session
            .sim()
            .fleet
            .snapshot(&session.sim().selected)
            .position
    );
}

#[test]
fn stellar_checkpoint_refuses_missing_state_or_conflicting_world_bound() {
    let planet = void_landing::aurelia();
    let mut initial = InitialWorld::new(
        &planet,
        &void_assembly::demo_craft(),
        void_vessels::flat_site(&planet),
        true,
    );
    initial.world = stellar_neighborhood(&planet);
    initial.launch_body = "Sol/aurelia".into();
    let session = FlightSession::new(initial);
    let saved = void_fleet_flight::checkpoint::FlightCheckpoint::capture(
        session.sim(),
        session.recording_initial().clone(),
    );
    for (key, replacement) in [
        ("coupled_world", serde_json::Value::Null),
        ("ephemeris_end", serde_json::json!(9e8)),
    ] {
        let mut altered = serde_json::to_value(&saved).unwrap();
        altered[key] = replacement;
        let altered: void_fleet_flight::checkpoint::FlightCheckpoint =
            serde_json::from_value(altered).unwrap();
        assert!(
            std::panic::catch_unwind(|| altered.restore()).is_err(),
            "{key}"
        );
    }
}
