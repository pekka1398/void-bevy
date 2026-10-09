use void_fleet_flight::{
    checkpoint::FlightCheckpoint,
    session::{Action, FlightSession, InitialWorld, world_mark},
};
fn session() -> FlightSession {
    let p = void_landing::aurelia();
    FlightSession::new(InitialWorld::new(
        &p,
        &void_vessels::pod_tank("checkpoint UI"),
        void_vessels::flat_site(&p),
        true,
    ))
    .with_recording()
}
#[test]
fn rejected_external_checkpoint_preserves_world_and_journal() {
    let mut s = session();
    let before = world_mark(s.sim());
    let counts = s.retained_counts();
    let dir = std::env::temp_dir().join(format!("void-ui-reject-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("save.json");
    assert!(s.load_external_checkpoint(dir.join("missing")).is_err());
    let checkpoint = FlightCheckpoint::capture(s.sim(), s.recording_initial().clone());
    let value = serde_json::to_value(checkpoint).unwrap();
    for variant in 0..4 {
        let mut broken = value.clone();
        match variant {
            0 => broken["model_version"] = 0.into(),
            1 => broken["selected"] = "missing ship".into(),
            2 => broken["mark"] = serde_json::json!({}),
            _ => {
                broken["initial"]["launch_body"] = "missing body".into();
            }
        }
        std::fs::write(&path, serde_json::to_vec(&broken).unwrap()).unwrap();
        assert!(s.load_external_checkpoint(&path).is_err());
        assert_eq!(world_mark(s.sim()), before);
        assert_eq!(s.retained_counts(), counts);
    }
    std::fs::write(&path, b"{ invalid").unwrap();
    assert!(s.load_external_checkpoint(&path).is_err());
    assert_eq!(s.retained_counts(), counts);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn external_load_is_journalled_once_and_replays() {
    let mut s = session();
    let path = std::env::temp_dir().join(format!("void-ui-load-{}.json", std::process::id()));
    FlightCheckpoint::capture(s.sim(), s.recording_initial().clone())
        .write_external(&path)
        .unwrap();
    s.execute(Action::Control {
        throttle: 0.4,
        turn: glam::DVec3::ZERO,
    });
    let count = s.retained_counts().0;
    s.load_external_checkpoint(&path).unwrap();
    assert_eq!(s.retained_counts().0, count + 1);
    s.mark();
    let recorded = s.recording();
    let replay = FlightSession::from_recording(recorded);
    assert_eq!(world_mark(s.sim()), world_mark(replay.sim()));
    std::fs::remove_file(path).unwrap();
}
#[test]
fn failed_save_does_not_remove_unowned_temporary_file() {
    let s = session();
    let dir = std::env::temp_dir().join(format!("void-ui-save-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("save.json");
    let tmp = dir.join(format!(".save.json.{}.ui.tmp", std::process::id()));
    std::fs::write(&tmp, b"another writer").unwrap();
    std::fs::write(&path, b"previous save").unwrap();
    let checkpoint = FlightCheckpoint::capture(s.sim(), s.recording_initial().clone());
    assert!(checkpoint.write_external(&path).is_err());
    assert_eq!(std::fs::read(&tmp).unwrap(), b"another writer");
    assert_eq!(std::fs::read(&path).unwrap(), b"previous save");
    std::fs::remove_dir_all(dir).unwrap();
}
