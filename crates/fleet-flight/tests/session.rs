use glam::DVec3;
use void_assembly::{demo_craft, export_craft};
use void_fleet_flight::session::{Action, FlightSession, InitialWorld, Outcome, world_mark};
use void_landing::earth_size;
use void_vessels::flat_site;

fn session(air: bool) -> FlightSession {
    let planet = earth_size();
    FlightSession::new(InitialWorld::new(
        &planet,
        &demo_craft(),
        flat_site(&planet),
        air,
    ))
    .with_recording()
}
fn exercise(s: &mut FlightSession) {
    s.execute(Action::Sas { enabled: true });
    s.execute(Action::Control {
        throttle: 0.6,
        turn: DVec3::ZERO,
    });
    s.execute(Action::Stage);
    s.execute(Action::Advance {
        seconds: 0.031,
        rails: false,
    });
    s.mark();
    let before = s.sim().fleet.time();
    let refused = s.execute(Action::Advance {
        seconds: 100.0,
        rails: true,
    });
    assert!(matches!(refused, Outcome::Refused(_)));
    assert_eq!(s.sim().fleet.time(), before);
    s.execute(Action::Stage);
    s.execute(Action::Control {
        throttle: 0.0,
        turn: DVec3::ZERO,
    });
    s.execute(Action::LaunchOrbit {
        craft: demo_craft(),
        offset: DVec3::ZERO,
    });
    s.execute(Action::LaunchOrbit {
        craft: demo_craft(),
        offset: DVec3::Y * 30.0,
    });
    s.execute(Action::Select {
        vessel: "v3".into(),
    });
    s.execute(Action::Sas { enabled: true });
    s.execute(Action::Control {
        throttle: 0.2,
        turn: DVec3::Z * 0.1,
    });
    s.execute(Action::Stage);
    s.execute(Action::Advance {
        seconds: 0.12,
        rails: false,
    });
    s.mark();
}
#[test]
fn a_live_multivessel_world_round_trips_and_continues_after_reload() {
    for air in [false, true] {
        let mut original = session(air);
        exercise(&mut original);
        let path =
            std::env::temp_dir().join(format!("void-fleet-save-{}-{air}.json", std::process::id()));
        original.save(&path);
        let mut restored = FlightSession::load(&path).with_recording();
        assert_eq!(world_mark(restored.sim()), world_mark(original.sim()));
        // Future controls, pending substep, SAS and fuel must continue as the unsaved world does.
        for s in [&mut original, &mut restored] {
            s.execute(Action::Advance {
                seconds: 0.4,
                rails: false,
            });
            s.execute(Action::Control {
                throttle: 0.0,
                turn: DVec3::ZERO,
            });
            s.execute(Action::Select {
                vessel: "v1".into(),
            });
            s.execute(Action::Advance {
                seconds: 0.037,
                rails: false,
            });
        }
        assert_eq!(world_mark(restored.sim()), world_mark(original.sim()));
        restored.save(&path); // Atomically replace an existing save.
        let again = FlightSession::load(&path).with_recording();
        assert_eq!(world_mark(again.sim()), world_mark(restored.sim()));
        std::fs::remove_file(path).unwrap();
    }
}
#[test]
fn changed_controls_are_rejected_by_the_recorded_world_marks() {
    let mut s = session(false);
    exercise(&mut s);
    let mut record = s.recording();
    record
        .entries
        .iter_mut()
        .find_map(|e| match &mut e.action {
            Action::Control { throttle, .. } => Some(throttle),
            _ => None,
        })
        .map(|throttle| *throttle = 0.1)
        .unwrap();
    let failed =
        std::panic::catch_unwind(|| FlightSession::from_recording(record).with_recording());
    assert!(failed.is_err());
}
#[test]
fn unsupported_versions_missing_marks_and_changed_catalog_fail_explicitly() {
    let mut s = session(false);
    let record = s.recording();
    for kind in 0..4 {
        let mut bad = record.clone();
        match kind {
            0 => bad.format_version += 1,
            1 => bad.model_version += 1,
            2 => bad.marks.clear(),
            3 => bad.catalog = serde_json::json!([]),
            _ => unreachable!(),
        }
        assert!(std::panic::catch_unwind(|| bad.validate()).is_err());
    }
    assert!(!export_craft(&record.initial.craft).unwrap().is_empty());
}
#[test]
fn an_empty_checkpoint_and_long_sleeping_rails_are_reconstructable() {
    let planet = void_landing::aurelia();
    let craft = void_vessels::pod_tank("Sleeping save");
    let mut s = FlightSession::new(InitialWorld::new(
        &planet,
        &craft,
        flat_site(&planet),
        false,
    ))
    .with_recording();
    let initial = s.recording();
    assert_eq!(
        world_mark(s.sim()),
        world_mark(
            FlightSession::from_recording(initial)
                .with_recording()
                .sim()
        )
    );
    s.execute(Action::Advance {
        seconds: 30.0,
        rails: false,
    });
    assert!(
        s.sim().fleet.rails_blocker().is_none(),
        "{:?}",
        s.sim().fleet.rails_blocker()
    );
    assert_eq!(
        s.execute(Action::Advance {
            seconds: 86400.0,
            rails: true
        }),
        Outcome::Advanced(true)
    );
    let record = s.recording();
    assert_eq!(
        world_mark(s.sim()),
        world_mark(FlightSession::from_recording(record).with_recording().sim())
    );
}

#[test]
fn incremental_window_playback_matches_headless_reload_and_continues() {
    use void_fleet_flight::session::Playback;
    let mut s = session(true);
    exercise(&mut s);
    let recorded = s.recording();
    let (mut playback, mut replayed) = Playback::new(recorded.clone());
    let mut frames = 0;
    while playback.next_frame(&mut replayed) {
        frames += 1;
    }
    assert_eq!(
        frames,
        recorded
            .entries
            .iter()
            .filter(|e| matches!(e.action, Action::Advance { .. }))
            .count()
    );
    assert_eq!(world_mark(s.sim()), world_mark(replayed.sim()));
    for live in [&mut s, &mut replayed] {
        live.execute(Action::Advance {
            seconds: 0.03,
            rails: false,
        });
    }
    assert_eq!(world_mark(s.sim()), world_mark(replayed.sim()));
}

#[test]
fn recording_is_opt_in_and_stopping_releases_history_without_stopping_the_world() {
    let planet = earth_size();
    let initial = InitialWorld::new(&planet, &demo_craft(), flat_site(&planet), false);
    let mut live = FlightSession::new(initial);
    for _ in 0..2000 {
        live.execute(Action::EndFrame {
            paused: true,
            rate: 0,
        });
        live.mark();
    }
    assert_eq!(live.retained_counts(), (0, 0));
    let saved = void_fleet_flight::checkpoint::FlightCheckpoint::capture(
        live.sim(),
        live.recording_initial().clone(),
    );
    let restored = FlightSession::from_checkpoint(saved);
    assert_eq!(restored.retained_counts(), (0, 0));
    let path = std::env::temp_dir().join(format!("void-opt-in-{}.jsonl", std::process::id()));
    live.begin_stream(&path);
    live.execute(Action::Stage);
    live.execute(Action::Advance {
        seconds: 0.031,
        rails: false,
    });
    live.execute(Action::EndFrame {
        paused: false,
        rate: 0,
    });
    assert_eq!(live.retained_counts().0, 3);
    let recorded_mark = world_mark(live.sim());
    live.finish_stream();
    assert_eq!(live.retained_counts(), (0, 0));
    let recording = void_fleet_flight::session::Recording::read(&path);
    assert_eq!(
        recording.entries.len(),
        3,
        "unrecorded earlier frames must not be included"
    );
    let verified = FlightSession::from_recording(recording.clone());
    assert_eq!(world_mark(verified.sim()), recorded_mark);
    assert_eq!(verified.retained_counts(), (0, 0));
    let (mut playback, mut replay) = void_fleet_flight::session::Playback::new(recording);
    while playback.next_frame(&mut replay) {}
    assert_eq!(replay.retained_counts(), (0, 0));
    for _ in 0..2000 {
        live.execute(Action::EndFrame {
            paused: true,
            rate: 0,
        });
        live.mark();
    }
    live.execute(Action::Advance {
        seconds: 0.031,
        rails: false,
    });
    assert!(live.sim().fleet.time() > verified.sim().fleet.time());
    assert_eq!(live.retained_counts(), (0, 0));
    // Starting a second stream captures the current world, not prior commands.
    let second = path.with_extension("second.jsonl");
    live.begin_stream(&second);
    live.execute(Action::EndFrame {
        paused: true,
        rate: 0,
    });
    live.finish_stream();
    let second_record = void_fleet_flight::session::Recording::read(&second);
    assert_eq!(second_record.entries.len(), 1);
    assert_eq!(
        world_mark(FlightSession::from_recording(second_record).sim()),
        world_mark(live.sim())
    );
    std::fs::remove_file(path).unwrap();
    std::fs::remove_file(second).unwrap();
}

#[test]
#[should_panic(expected = "session: incompatible simulation model")]
fn pre_frame_tree_recording_is_rejected_before_replay() {
    let mut s = session(false);
    let mut recording = s.recording();
    recording.model_version = 4;
    FlightSession::from_recording(recording);
}
