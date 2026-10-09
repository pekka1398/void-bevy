use glam::DVec3;
use std::time::{Duration, Instant};
use void_fleet_flight::{
    checkpoint::FlightCheckpoint,
    session::{Action, FlightSession, InitialWorld, Outcome, world_mark},
};
use void_orbit::{ManeuverSpec, NavigationOperation, NavigationRequest, ReferenceMode};

fn fixture() -> FlightSession {
    let planet = void_landing::earth_size();
    let craft = void_assembly::demo_craft();
    let mut session = FlightSession::new(InitialWorld::new(
        &planet,
        &craft,
        void_vessels::flat_site(&planet),
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
    session.execute(Action::EndFrame {
        paused: true,
        rate: 0,
    });
    session
}
fn request(s: &FlightSession) -> NavigationRequest {
    NavigationRequest {
        operation: NavigationOperation::Capture,
        target_body: s.sim().home,
        reference_body: s.sim().home,
        earliest_departure: 100.,
        latest_departure: 100.,
        min_flight_seconds: 100.,
        max_flight_seconds: 12_000.,
        periapsis_altitude_m: 400_000.,
    }
}
fn drain(s: &mut FlightSession, paused: bool) -> Option<Outcome> {
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut result = None;
    while s.navigation_running() {
        if let Some(outcome) = s.poll_navigation(paused) {
            result = Some(outcome);
        }
        assert!(
            Instant::now() < deadline,
            "navigation worker failed to stop: {}",
            s.navigation_status()
        );
        if s.navigation_running() {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    result
}
#[test]
fn background_plan_is_reviewable_without_fuel_and_replays_without_resolving() {
    let mut s = fixture();
    let journal = std::env::temp_dir().join(format!(
        "void-navigation-job-{}-{}.jsonl",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    s.begin_stream(&journal);
    let id = s.sim().selected.clone();
    s.execute(Action::AddManeuver {
        spec: ManeuverSpec {
            start_time: 0.047,
            reference_body: s.sim().home,
            reference_mode: ReferenceMode::Auto,
            prograde: 2.,
            normal: 0.,
            radial: 0.,
        },
    });
    let previous_generation = s.sim().plans[&id].plan.generation;
    let previous_count = s.sim().plans[&id].plan.count();
    let before = s.sim().fleet.snapshot(&id).mass_kg;
    let req = request(&s);
    s.request_navigation(req, true).unwrap();
    assert!(
        s.sim().plans[&id].plan.count() == previous_count,
        "request cannot synchronously mutate plan"
    );
    assert_eq!(drain(&mut s, true), Some(Outcome::Applied));
    assert_eq!(s.sim().fleet.snapshot(&id).mass_kg, before);
    let p = &s.sim().plans[&id].plan;
    assert!(p.complete() && p.trajectory.count() > 2);
    assert!(p.generation > previous_generation);
    assert_eq!(p.count(), previous_count + 1);
    assert!(s.sim().fleet.ephemeris.end_time() >= p.computed_until());
    let recording = s.recording();
    assert!(
        recording
            .entries
            .iter()
            .any(|e| matches!(e.action, Action::CommitNavigation { .. }))
    );
    assert!(
        !recording
            .entries
            .iter()
            .any(|e| matches!(e.action, Action::GenerateNavigation { .. }))
    );
    let expected_actions = recording
        .entries
        .iter()
        .map(|e| serde_json::to_value(&e.action).unwrap())
        .collect::<Vec<_>>();
    let replay = FlightSession::from_recording(recording);
    assert_eq!(world_mark(s.sim()), world_mark(replay.sim()));
    let checkpoint = FlightCheckpoint::capture(s.sim(), s.recording_initial().clone());
    let loaded = FlightSession::from_checkpoint(
        serde_json::from_value(serde_json::to_value(checkpoint).unwrap()).unwrap(),
    );
    assert_eq!(world_mark(s.sim()), world_mark(loaded.sim()));
    s.finish_stream();
    let decoded = void_fleet_flight::session::Recording::read(&journal);
    assert_eq!(
        expected_actions,
        decoded
            .entries
            .iter()
            .map(|e| serde_json::to_value(&e.action).unwrap())
            .collect::<Vec<_>>()
    );
    let durable_replay = FlightSession::from_recording(decoded);
    assert_eq!(world_mark(s.sim()), world_mark(durable_replay.sim()));
    std::fs::remove_file(journal).unwrap();
}
#[test]
fn explicit_cancel_resume_and_edits_discard_results_without_mutation() {
    for mode in 0..3 {
        let mut s = fixture();
        let before = world_mark(s.sim());
        s.request_navigation(request(&s), true).unwrap();
        match mode {
            0 => s.cancel_navigation("pilot cancelled"),
            1 => {
                s.poll_navigation(false);
            }
            _ => {
                s.execute(Action::Select {
                    vessel: s.sim().selected.clone(),
                });
            }
        }
        assert_eq!(drain(&mut s, true), None);
        assert_eq!(world_mark(s.sim()), before);
    }
}
#[test]
fn latest_request_replaces_cancelled_job_and_unpaused_request_is_refused() {
    let mut s = fixture();
    assert!(
        s.request_navigation(request(&s), false)
            .unwrap_err()
            .contains("Pause")
    );
    let mut bad = request(&s);
    bad.target_body = usize::MAX;
    s.request_navigation(bad, true).unwrap();
    s.request_navigation(request(&s), true).unwrap();
    assert_eq!(drain(&mut s, true), Some(Outcome::Applied));
    assert_eq!(s.sim().plans[&s.sim().selected].plan.count(), 1);
}
#[test]
fn journalled_result_rejects_stale_plan_and_forged_anchor() {
    let mut s = fixture();
    s.request_navigation(request(&s), true).unwrap();
    assert_eq!(drain(&mut s, true), Some(Outcome::Applied));
    let action = s
        .recording()
        .entries
        .into_iter()
        .find_map(|e| matches!(e.action, Action::CommitNavigation { .. }).then_some(e.action))
        .unwrap();
    let mut stale = fixture();
    stale.execute(Action::AddManeuver {
        spec: ManeuverSpec {
            start_time: 30.,
            reference_body: stale.sim().home,
            reference_mode: ReferenceMode::Auto,
            prograde: 1.,
            radial: 0.,
            normal: 0.,
        },
    });
    assert!(
        matches!(stale.execute(action.clone()), Outcome::Refused(reason) if reason.contains("stale"))
    );
    let mut huge_horizon = serde_json::to_value(&action).unwrap();
    huge_horizon["prepared"]["ephemeris_end"] = serde_json::json!(1e300);
    let mut target = fixture();
    assert!(
        matches!(target.execute(serde_json::from_value(huge_horizon).unwrap()), Outcome::Refused(reason) if reason.contains("memory budget"))
    );
    let mut json = serde_json::to_value(action).unwrap();
    json["prepared"]["plan"]["anchor"]["y"][0] = serde_json::json!(123.);
    let forged: Action = serde_json::from_value(json).unwrap();
    let mut target = fixture();
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| target.execute(forged))).is_err()
    );
}
#[test]
fn coupled_background_plan_adopts_existing_owner_and_roundtrips() {
    let planet = void_landing::aurelia();
    let craft = void_assembly::demo_craft();
    let mut initial = InitialWorld::new(&planet, &craft, void_vessels::flat_site(&planet), false);
    initial.world = void_fleet_flight::world::stellar_neighborhood(&planet);
    initial.launch_body = "Sol/aurelia".into();
    let mut s = FlightSession::new(initial).with_recording();
    let Outcome::Spawned(id) = s.execute(Action::LaunchOrbit {
        craft,
        offset: DVec3::ZERO,
    }) else {
        panic!("launch")
    };
    s.execute(Action::Select { vessel: id });
    s.execute(Action::Stage);
    s.execute(Action::EndFrame {
        paused: true,
        rate: 0,
    });
    let owner = s.sim().coupled_world.as_ref().unwrap().clone();
    let mass = s.sim().fleet.snapshot(&s.sim().selected).mass_kg;
    s.request_navigation(request(&s), true).unwrap();
    assert_eq!(drain(&mut s, true), Some(Outcome::Applied));
    assert!(std::rc::Rc::ptr_eq(
        &owner,
        s.sim().coupled_world.as_ref().unwrap()
    ));
    assert_eq!(s.sim().fleet.snapshot(&s.sim().selected).mass_kg, mass);
    let cp = FlightCheckpoint::capture(s.sim(), s.recording_initial().clone());
    let loaded = FlightSession::from_checkpoint(
        serde_json::from_value(serde_json::to_value(cp).unwrap()).unwrap(),
    );
    assert_eq!(world_mark(s.sim()), world_mark(loaded.sim()));
    let replay = FlightSession::from_recording(s.recording());
    assert_eq!(world_mark(s.sim()), world_mark(replay.sim()));
}

#[test]
fn explicit_reference_is_not_replaced_by_ui_auto_policy() {
    let mut session = fixture();
    let mut invalid = request(&session);
    invalid.reference_body = usize::MAX;
    session.request_navigation(invalid, true).unwrap();
    assert!(matches!(
        drain(&mut session, true),
        Some(Outcome::Refused(_))
    ));
    assert!(session.sim().plans.is_empty());
}
