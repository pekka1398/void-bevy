use glam::DVec3;
use void_assembly::demo_craft;
use void_fleet_flight::{
    checkpoint::FlightCheckpoint,
    session::{Action, FlightSession, InitialWorld, world_mark},
};
use void_testkit::flat_site;

fn make(air: bool) -> FlightSession {
    let planet = void_testkit::earth_size();
    FlightSession::new(InitialWorld::new(
        &planet,
        &demo_craft(),
        flat_site(&planet),
        air,
    ))
    .with_recording()
}
fn compare_and_continue(original: &mut FlightSession) {
    let saved = FlightCheckpoint::capture(original.sim(), original.recording_initial().clone());
    let bytes = serde_json::to_vec(&saved).unwrap();
    let decoded: FlightCheckpoint = serde_json::from_slice(&bytes).unwrap();
    let mut restored = FlightSession::from_checkpoint(decoded).with_recording();
    assert_eq!(world_mark(original.sim()), world_mark(restored.sim()));
    for action in [
        Action::Advance {
            seconds: 0.113,
            rails: false,
        },
        Action::Control {
            throttle: 0.3,
            turn: DVec3::ZERO,
        },
        Action::Advance {
            seconds: 1.0,
            rails: false,
        },
        Action::Stage,
        Action::Advance {
            seconds: 0.037,
            rails: false,
        },
    ] {
        let a = original.execute(action.clone());
        let b = restored.execute(action);
        assert_eq!(a, b);
        assert_eq!(world_mark(original.sim()), world_mark(restored.sim()));
    }
    // A recording that starts from the loaded checkpoint must not replay its old launch history.
    let record = restored.recording();
    assert!(record.base.is_some());
    let after = FlightSession::from_recording(record).with_recording();
    assert_eq!(world_mark(original.sim()), world_mark(after.sim()));
}
#[test]
fn awake_ground_pending_fuel_and_sas_continue_exactly_after_direct_restore() {
    for air in [false, true] {
        let mut original = make(air);
        original.execute(Action::Sas { enabled: true });
        original.execute(Action::Control {
            throttle: 0.7,
            turn: DVec3::ZERO,
        });
        original.execute(Action::Stage);
        original.execute(Action::Advance {
            seconds: 0.031,
            rails: false,
        });
        assert!(original.sim().fleet.pending_seconds() > 0.0);
        compare_and_continue(&mut original);
    }
}
#[test]
fn mixed_ground_orbit_and_bubble_owner_caches_survive_direct_restore() {
    let mut original = make(true);
    for offset in [DVec3::ZERO, DVec3::Y * 30.0, DVec3::Y * 10_000.0] {
        original.execute(Action::LaunchOrbitAt {
            body: {
                let sim = original.sim();
                sim.fleet.ephemeris.bodies()[sim.home].id.clone()
            },
            craft: demo_craft(),
            offset,
        });
    }
    original.execute(Action::Select {
        vessel: "v2".into(),
    });
    original.execute(Action::Sas { enabled: true });
    original.execute(Action::Stage);
    original.execute(Action::Control {
        throttle: 0.2,
        turn: DVec3::ZERO,
    });
    original.execute(Action::Advance {
        seconds: 0.37,
        rails: false,
    });
    assert!(original.sim().fleet.bubble_count() > 0);
    compare_and_continue(&mut original);
}
#[test]
fn sleeping_save_load_can_resume_a_day_on_rails() {
    let planet = void_testkit::aurelia();
    let craft = void_testkit::pod_tank("Resting checkpoint");
    let mut original = FlightSession::new(InitialWorld::new(
        &planet,
        &craft,
        flat_site(&planet),
        false,
    ))
    .with_recording();
    original.execute(Action::Advance {
        seconds: 30.0,
        rails: false,
    });
    assert!(original.sim().fleet.rails_blocker().is_none());
    let mut restored = FlightSession::from_checkpoint(FlightCheckpoint::capture(
        original.sim(),
        original.recording_initial().clone(),
    ))
    .with_recording();
    for s in [&mut original, &mut restored] {
        s.execute(Action::Advance {
            seconds: 86400.0,
            rails: true,
        });
    }
    assert_eq!(world_mark(original.sim()), world_mark(restored.sim()));
    for s in [&mut original, &mut restored] {
        s.execute(Action::Advance {
            seconds: 1.0,
            rails: false,
        });
    }
    assert_eq!(world_mark(original.sim()), world_mark(restored.sim()));
}
#[test]
fn corrupted_native_cache_and_graph_are_rejected() {
    let original = make(false);
    let saved = FlightCheckpoint::capture(original.sim(), original.recording_initial().clone());
    let mut value = serde_json::to_value(saved).unwrap();
    value["fleet"]["scenes"][0]["world"]["physics"] = serde_json::json!([1, 2, 3]);
    let bad: FlightCheckpoint = serde_json::from_value(value).unwrap();
    assert!(std::panic::catch_unwind(|| bad.restore()).is_err());
    let saved = FlightCheckpoint::capture(original.sim(), original.recording_initial().clone());
    let mut value = serde_json::to_value(saved).unwrap();
    value["fleet"]["order"] = serde_json::json!([]);
    let bad: FlightCheckpoint = serde_json::from_value(value).unwrap();
    assert!(std::panic::catch_unwind(|| bad.restore()).is_err());
}

#[test]
#[should_panic(expected = "world checkpoint: incompatible model")]
fn pre_frame_tree_checkpoint_is_rejected_before_restoring_owners() {
    let s = make(false);
    let saved = FlightCheckpoint::capture(s.sim(), s.recording_initial().clone());
    let mut value = serde_json::to_value(saved).unwrap();
    value["model_version"] = serde_json::json!(4);
    let old: FlightCheckpoint = serde_json::from_value(value).unwrap();
    old.restore();
}

#[test]
#[should_panic(expected = "world checkpoint: incompatible model")]
fn pre_part_graph_model_is_rejected_before_restoring_owners() {
    let original = make(false);
    let saved = FlightCheckpoint::capture(original.sim(), original.recording_initial().clone());
    let mut value = serde_json::to_value(saved).unwrap();
    value["model_version"] = serde_json::json!(7);
    let saved: FlightCheckpoint = serde_json::from_value(value).unwrap();
    saved.restore();
}

#[test]
fn buffered_checkpoint_writes_complete_files_and_can_overwrite_and_resume() {
    let path = std::env::temp_dir().join(format!(
        "void-buffered-checkpoint-{}.json",
        std::process::id()
    ));
    let mut original = make(false);
    for advance in [0.0, 0.137] {
        if advance > 0.0 {
            original.execute(Action::Stage);
            original.execute(Action::Advance {
                seconds: advance,
                rails: false,
            });
        }
        let saved = FlightCheckpoint::capture(original.sim(), original.recording_initial().clone());
        saved.write(&path);
        let bytes = std::fs::read(&path).unwrap();
        // Exceed the write buffer so both automatic buffer drains and the final flush are exercised.
        assert!(bytes.len() > 64 * 1024);
        assert_eq!(bytes.last(), Some(&b'\n'));
        let read = FlightCheckpoint::read(&path);
        assert_eq!(
            serde_json::to_value(&read).unwrap(),
            serde_json::to_value(&saved).unwrap()
        );
        let mut restored = FlightSession::from_checkpoint(read);
        assert_eq!(world_mark(restored.sim()), world_mark(original.sim()));
        let action = Action::Advance {
            seconds: 0.031,
            rails: false,
        };
        original.execute(action.clone());
        restored.execute(action);
        assert_eq!(world_mark(restored.sim()), world_mark(original.sim()));
    }
    std::fs::remove_file(path).unwrap();
}

/// A save taken while the main rocket climbs through the air on SAS restores. Its orbital run
/// steps 1/60 s at a time, and those steps must end on the fleet's clock: a run left a few
/// 1e-14 s short every advance failed the restore's clock check after 50 s of ascent.
#[test]
fn a_save_mid_ascent_restores_and_flies_on() {
    let planet = void_testkit::aurelia();
    let craft = void_assembly::flight_rocket();
    let mut original =
        FlightSession::new(InitialWorld::new(&planet, &craft, flat_site(&planet), true))
            .with_recording();
    original.execute(Action::Sas { enabled: true });
    original.execute(Action::Control {
        throttle: 1.0,
        turn: DVec3::ZERO,
    });
    original.execute(Action::Stage);
    for _ in 0..200 {
        original.execute(Action::Advance {
            seconds: 0.25,
            rails: false,
        });
    }
    let ship = original.sim().fleet.snapshot(&original.sim().selected);
    assert_eq!(
        ship.mode,
        void_vessels::VesselMode::Orbit,
        "{:?}",
        ship.mode
    );
    let mut restored = FlightSession::from_checkpoint(FlightCheckpoint::capture(
        original.sim(),
        original.recording_initial().clone(),
    ))
    .with_recording();
    for s in [&mut original, &mut restored] {
        for _ in 0..20 {
            s.execute(Action::Advance {
                seconds: 0.25,
                rails: false,
            });
        }
    }
    assert_eq!(world_mark(original.sim()), world_mark(restored.sim()));
}

#[test]
#[should_panic(expected = "world checkpoint: incompatible model")]
fn pre_clock_fix_model_eight_is_rejected_before_restoring_owners() {
    let original = make(false);
    let saved = FlightCheckpoint::capture(original.sim(), original.recording_initial().clone());
    let mut value = serde_json::to_value(saved).unwrap();
    value["model_version"] = serde_json::json!(8);
    let saved: FlightCheckpoint = serde_json::from_value(value).unwrap();
    saved.restore();
}
