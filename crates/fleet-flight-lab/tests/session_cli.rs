//! Separate-process checks of the actual lab executable's no-window verification path.
use glam::DVec3;
use std::process::Command;
use void_assembly::demo_craft;
use void_fleet_flight::session::{Action, FlightSession, InitialWorld};
use void_vessels::flat_site;

#[test]
fn saved_fleet_verifies_in_a_fresh_process_and_changed_inputs_fail() {
    let planet = void_landing::earth_size();
    let craft = demo_craft();
    let mut session =
        FlightSession::new(InitialWorld::new(&planet, &craft, flat_site(&planet), true));
    session.execute(Action::Sas { enabled: true });
    session.execute(Action::Control {
        throttle: 0.7,
        turn: DVec3::ZERO,
    });
    session.execute(Action::Stage);
    session.execute(Action::Advance {
        seconds: 0.113,
        rails: false,
    });
    session.execute(Action::Stage);
    session.execute(Action::LaunchOrbit {
        craft: craft.clone(),
        offset: DVec3::ZERO,
    });
    session.execute(Action::LaunchOrbit {
        craft,
        offset: DVec3::Y * 30.0,
    });
    session.execute(Action::Advance {
        seconds: 0.5,
        rails: false,
    });
    let path = std::env::temp_dir().join(format!("void-fleet-cli-{}.json", std::process::id()));
    session.save(&path);
    let profile_path = path.with_extension("profile.json");
    for _ in 0..3 {
        let result = Command::new(env!("CARGO_BIN_EXE_void-fleet-flight-lab"))
            .arg("--verify")
            .arg(&path)
            .arg("--profile")
            .arg(&profile_path)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(String::from_utf8_lossy(&result.stdout).contains("Verified Fleet session"));
    }
    let report: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&profile_path).unwrap()).unwrap();
    assert_eq!(report["metrics"]["headless_verify"]["samples"], 1);
    assert!(
        report["metrics"]["headless_verify"]["p95_ms"]
            .as_f64()
            .unwrap()
            > 0.0
    );
    assert_eq!(report["traceEvents"][0]["name"], "headless_verify");
    assert_eq!(report["traceEvents"][0]["ph"], "X");
    std::fs::remove_file(profile_path).unwrap();
    let mut record = session.recording();
    for entry in &mut record.entries {
        if let Action::Control { throttle, .. } = &mut entry.action {
            *throttle = 0.1;
            break;
        }
    }
    record.write(&path);
    let result = Command::new(env!("CARGO_BIN_EXE_void-fleet-flight-lab"))
        .arg("--verify")
        .arg(&path)
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("world diverged"));
    std::fs::remove_file(path).unwrap();
}

#[test]
fn direct_world_save_loads_in_a_fresh_process_without_the_input_journal() {
    let planet = void_landing::earth_size();
    let craft = demo_craft();
    let mut session =
        FlightSession::new(InitialWorld::new(&planet, &craft, flat_site(&planet), true));
    session.execute(Action::LaunchOrbit {
        craft: craft.clone(),
        offset: DVec3::ZERO,
    });
    session.execute(Action::LaunchOrbit {
        craft,
        offset: DVec3::Y * 30.0,
    });
    session.execute(Action::Select {
        vessel: "v2".into(),
    });
    session.execute(Action::Sas { enabled: true });
    session.execute(Action::Stage);
    session.execute(Action::Control {
        throttle: 0.2,
        turn: DVec3::ZERO,
    });
    session.execute(Action::Advance {
        seconds: 0.213,
        rails: false,
    });
    let path =
        std::env::temp_dir().join(format!("void-direct-save-cli-{}.json", std::process::id()));
    session.save_checkpoint(&path);
    let value: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert!(
        value.get("entries").is_none(),
        "direct save must not contain an input history"
    );
    let result = Command::new(env!("CARGO_BIN_EXE_void-fleet-flight-lab"))
        .arg("--verify-save")
        .arg(&path)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(String::from_utf8_lossy(&result.stdout).contains("Verified Fleet world save"));
    let mut restored = FlightSession::load_checkpoint(&path);
    for live in [&mut session, &mut restored] {
        live.execute(Action::Advance {
            seconds: 0.43,
            rails: false,
        });
    }
    assert_eq!(
        void_fleet_flight::session::world_mark(session.sim()),
        void_fleet_flight::session::world_mark(restored.sim())
    );
    std::fs::remove_file(path).unwrap();
}
