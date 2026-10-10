mod common;
use common::solar_world;
use void_fleet_flight::{
    checkpoint::FlightCheckpoint,
    presentation::{Presentation, Toggle, ViewCommand},
    session::{Action, FlightSession, InitialWorld, world_mark},
};

#[test]
fn visual_controls_preserve_physics_and_roundtrip_checkpoint_and_journal() {
    let planet = void_testkit::aurelia();
    let mut initial = InitialWorld::new(
        &planet,
        &void_testkit::pod_tank("UI witness"),
        void_testkit::flat_site(&planet),
        true,
    );
    initial.world = solar_world(&planet);
    let mut session = FlightSession::new(initial.clone()).with_recording();
    let physical_mark = |sim: &void_fleet_flight::FleetFlight| {
        let mut mark = world_mark(sim);
        mark.as_object_mut().unwrap().remove("presentation");
        mark
    };
    let before = physical_mark(session.sim());
    for setting in [
        Toggle::VisualAir,
        Toggle::VisualClouds,
        Toggle::VisualOcean,
        Toggle::VisualStars,
    ] {
        session.execute(Action::View {
            command: ViewCommand::Toggle { setting },
        });
    }
    let view = &session.sim().presentation;
    assert!(!view.visual_air && !view.visual_clouds && !view.visual_ocean && !view.visual_stars);
    assert_eq!(physical_mark(session.sim()), before);
    let checkpoint = FlightCheckpoint::capture(session.sim(), initial);
    let encoded = serde_json::to_value(&checkpoint).unwrap();
    let decoded: FlightCheckpoint = serde_json::from_value(encoded).unwrap();
    assert_eq!(world_mark(&decoded.restore()), world_mark(session.sim()));
    session.mark();
    let recording = session.recording();
    let replayed = FlightSession::from_recording(recording);
    assert_eq!(world_mark(replayed.sim()), world_mark(session.sim()));
}

#[test]
fn legacy_presentation_missing_visual_fields_is_rejected() {
    let mut encoded = serde_json::to_value(Presentation::new(
        glam::DVec3::X * 7_000_000.0,
        glam::DVec3::ZERO,
        0.0,
    ))
    .unwrap();
    encoded.as_object_mut().unwrap().remove("visual_air");
    assert!(serde_json::from_value::<Presentation>(encoded).is_err());
}
