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
        let mut restored = FlightSession::load(&path);
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
        let again = FlightSession::load(&path);
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
    let failed = std::panic::catch_unwind(|| FlightSession::from_recording(record));
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
    ));
    let initial = s.recording();
    assert_eq!(
        world_mark(s.sim()),
        world_mark(FlightSession::from_recording(initial).sim())
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
        world_mark(FlightSession::from_recording(record).sim())
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
