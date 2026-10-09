use glam::DVec3;
use void_assembly::demo_craft;
use void_fleet_flight::{
    checkpoint::FlightCheckpoint,
    session::{Action, FlightSession, InitialWorld, Outcome, world_mark},
};
use void_orbit::{ManeuverSpec, ReferenceMode};
use void_vessels::flat_site;
fn fixture() -> FlightSession {
    let planet = void_landing::earth_size();
    let craft = demo_craft();
    let mut session = FlightSession::new(InitialWorld::new(
        &planet,
        &craft,
        flat_site(&planet),
        false,
    ))
    .with_recording();
    let Outcome::Spawned(id) = session.execute(Action::LaunchOrbit {
        craft,
        offset: DVec3::ZERO,
    }) else {
        panic!("launch")
    };
    session.execute(Action::Select { vessel: id });
    session.execute(Action::Stage);
    session
}
fn add(session: &mut FlightSession, start: f64, dv: f64) {
    assert_eq!(
        session.execute(Action::AddManeuver {
            spec: ManeuverSpec {
                start_time: start,
                reference_body: session.sim().home,
                reference_mode: ReferenceMode::Auto,
                prograde: dv,
                normal: 0.0,
                radial: 0.0
            }
        }),
        Outcome::Applied
    );
}
#[test]
fn per_ship_plan_edits_execution_and_checkpoint_are_deterministic() {
    let mut original = fixture();
    let id = original.sim().selected.clone();
    add(&mut original, 0.047, 2.0);
    add(&mut original, 2.0, 0.2);
    let expected_burn = original.sim().plans[&id].plan.burns()[0];
    assert_eq!(original.execute(Action::ExecuteManeuver), Outcome::Applied);
    original.execute(Action::Advance {
        seconds: 0.067,
        rails: false,
    });
    assert!(original.sim().plans[&id].executing);
    // Selecting another ship must leave this vessel's plan and burn active.
    original.execute(Action::Select {
        vessel: "v1".into(),
    });
    let saved = FlightCheckpoint::capture(original.sim(), original.recording_initial().clone());
    let decoded: FlightCheckpoint =
        serde_json::from_slice(&serde_json::to_vec(&saved).unwrap()).unwrap();
    let mut loaded = FlightSession::from_checkpoint(decoded).with_recording();
    for action in [
        Action::Advance {
            seconds: 0.3,
            rails: false,
        },
        Action::Select { vessel: id.clone() },
    ] {
        assert_eq!(original.execute(action.clone()), loaded.execute(action));
        assert_eq!(world_mark(original.sim()), world_mark(loaded.sim()));
    }
    let plan = &original.sim().plans[&id];
    assert!(!plan.executing);
    assert_eq!(plan.plan.completed_count, 1);
    assert_eq!(plan.plan.count(), 1);
    assert!(
        (original.sim().fleet.snapshot(&id).mass_kg - expected_burn.mass_after_kg).abs() < 1e-8
    );
    let record = loaded.recording();
    let replay = FlightSession::from_recording(record).with_recording();
    assert_eq!(world_mark(loaded.sim()), world_mark(replay.sim()));
}
#[test]
fn rejected_plan_and_manual_abort_do_not_complete_a_burn() {
    let mut session = fixture();
    add(&mut session, 0.05, 1.0);
    assert_eq!(session.execute(Action::ExecuteManeuver), Outcome::Applied);
    assert!(matches!(
        session.execute(Action::RemoveManeuver { index: 0 }),
        Outcome::Refused(_)
    ));
    session.execute(Action::Advance {
        seconds: 0.067,
        rails: false,
    });
    session.execute(Action::Control {
        throttle: 0.0,
        turn: DVec3::ZERO,
    });
    let p = &session.sim().plans[&session.sim().selected];
    assert!(!p.executing);
    assert_eq!(p.plan.completed_count, 0);
    assert!(p.message.contains("manual control"));
    let before = world_mark(session.sim());
    let replay = FlightSession::from_recording(session.recording()).with_recording();
    assert_eq!(before, world_mark(replay.sim()));
}

fn navigation_request(session: &FlightSession) -> void_orbit::NavigationRequest {
    void_orbit::NavigationRequest {
        operation: void_orbit::NavigationOperation::Capture,
        target_body: session.sim().home,
        reference_body: session.sim().home,
        earliest_departure: 100.0,
        latest_departure: 100.0,
        min_flight_seconds: 100.0,
        max_flight_seconds: 12_000.0,
        periapsis_altitude_m: 400_000.0,
    }
}

#[test]
fn navigation_refusals_preserve_existing_plan_and_are_replayed() {
    let mut session = fixture();
    let id = session.sim().selected.clone();
    add(&mut session, 0.047, 2.0);
    let before = world_mark(session.sim());
    let mut request = navigation_request(&session);
    request.target_body = usize::MAX;
    assert!(matches!(
        session.execute(Action::GenerateNavigation { request }),
        Outcome::Refused(_)
    ));
    assert_eq!(before, world_mark(session.sim()));
    // An invalid pre-existing plan is refused explicitly, not removed or repaired.
    add(&mut session, 2.0, 1e8);
    let before = world_mark(session.sim());
    let request = navigation_request(&session);
    assert!(
        matches!(session.execute(Action::GenerateNavigation { request }), Outcome::Refused(reason) if reason.contains("existing plan"))
    );
    assert_eq!(before, world_mark(session.sim()));
    let replay = FlightSession::from_recording(session.recording()).with_recording();
    assert_eq!(world_mark(session.sim()), world_mark(replay.sim()));
    assert_eq!(session.sim().plans[&id].plan.count(), 2);
}

#[test]
fn navigation_cannot_edit_an_armed_burn() {
    let mut session = fixture();
    add(&mut session, 0.047, 2.0);
    assert_eq!(session.execute(Action::ExecuteManeuver), Outcome::Applied);
    let before = world_mark(session.sim());
    let request = navigation_request(&session);
    assert!(
        matches!(session.execute(Action::GenerateNavigation { request }), Outcome::Refused(reason) if reason.contains("executing"))
    );
    assert_eq!(before, world_mark(session.sim()));
}

#[test]
fn navigation_appends_after_existing_burn_without_spending_fuel_and_roundtrips() {
    let mut session = fixture();
    let id = session.sim().selected.clone();
    add(&mut session, 0.047, 10.0);
    let first = session.sim().plans[&id].plan.maneuver(0);
    let before_reference = world_mark(session.sim());
    let home = session.sim().home;
    assert_eq!(session.navigation_reference(&id).unwrap(), home);
    assert_eq!(before_reference, world_mark(session.sim()));
    let mass = session.sim().fleet.snapshot(&id).mass_kg;
    let request = navigation_request(&session);
    assert_eq!(
        session.execute(Action::GenerateNavigation { request }),
        Outcome::Applied
    );
    let plan = &session.sim().plans[&id];
    assert_eq!(plan.plan.count(), 2);
    assert_eq!(plan.selected, 1);
    assert_eq!(
        serde_json::to_value(first).unwrap(),
        serde_json::to_value(plan.plan.maneuver(0)).unwrap()
    );
    assert!(!plan.executing);
    assert!(plan.message.contains("bound orbit verified"));
    assert!(
        plan.plan.complete(),
        "generated plan must be reviewable without advancing live time"
    );
    assert!(plan.plan.trajectory.count() > 2);
    assert!(plan.plan.computed_until() > plan.plan.burns().last().unwrap().end_time);
    assert_eq!(session.sim().fleet.snapshot(&id).mass_kg, mass);
    // Switching ships keeps the generated plan on its original owner.
    session.execute(Action::Select {
        vessel: "v1".into(),
    });
    assert!(!session.sim().plans.contains_key("v1"));
    assert_eq!(session.sim().plans[&id].plan.count(), 2);
    let saved = FlightCheckpoint::capture(session.sim(), session.recording_initial().clone());
    let loaded = FlightSession::from_checkpoint(
        serde_json::from_value(serde_json::to_value(saved).unwrap()).unwrap(),
    );
    assert_eq!(world_mark(session.sim()), world_mark(loaded.sim()));
    let replay = FlightSession::from_recording(session.recording()).with_recording();
    assert_eq!(world_mark(session.sim()), world_mark(replay.sim()));
}
