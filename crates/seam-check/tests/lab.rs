use std::{fs, process::Command};
use void_seam_check::{Case, Input, cases, run_case};
#[test]
fn all_six_seams_hold_from_multiple_serialized_states() {
    let dir = std::env::temp_dir().join(format!("void-seam-sweep-{}", std::process::id()));
    for case in cases(0x5eed, 3).into_iter().chain(cases(0x1234, 2)) {
        // Check the deserialized input, as the re-run CLI does, not a separately reconstructed seed.
        let restored: Case = serde_json::from_slice(&serde_json::to_vec(&case).unwrap()).unwrap();
        run_case(&restored, &dir).unwrap();
    }
    assert!(
        !dir.exists(),
        "passing cases should not manufacture failure artifacts"
    );
}
#[test]
fn a_failing_case_is_saved_and_reproduced_in_another_process() {
    let dir = std::env::temp_dir().join(format!("void-seam-cli-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("input.json");
    let mut case = cases(99, 1).remove(0);
    let Input::Separate { rotation, .. } = &mut case.input else {
        unreachable!()
    };
    // A real rejected public initial state; no test-only simulator hook.
    *rotation = glam::DQuat::from_xyzw(0.0, 0.0, 0.0, 0.0);
    fs::write(&path, serde_json::to_vec(&case).unwrap()).unwrap();
    let failures = dir.join("failures");
    let run = |path: &std::path::Path| {
        Command::new(env!("CARGO_BIN_EXE_void-seam-check"))
            .arg("--case")
            .arg(path)
            .arg("--failure-dir")
            .arg(&failures)
            .output()
            .unwrap()
    };
    let first = run(&path);
    assert_eq!(first.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&first.stderr).contains("fleet: invalid attitude"));
    let artifact = fs::read_dir(&failures)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let restored = Case::read(&artifact);
    assert_eq!(
        serde_json::to_value(restored).unwrap(),
        serde_json::to_value(case).unwrap()
    );
    let second = run(&artifact);
    assert_eq!(second.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&second.stderr).contains("fleet: invalid attitude"));
    assert_eq!(
        fs::read_dir(&failures).unwrap().count(),
        2,
        "re-run must preserve the original artifact"
    );
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn translating_the_distant_join_changes_no_local_result() {
    let mut a = cases(0x700, 1).remove(2);
    let mut b = a.clone();
    let Input::Join { cells, .. } = &mut a.input else {
        unreachable!()
    };
    *cells = [0; 3];
    let Input::Join { cells, .. } = &mut b.input else {
        unreachable!()
    };
    *cells = [10_i128.pow(24), -10_i128.pow(23), 10_i128.pow(22)];
    assert_eq!(a.check(), b.check());
}
#[test]
fn exported_corpus_replays_through_the_real_cli() {
    let d = std::env::temp_dir().join(format!("void-seam-export-{}", std::process::id()));
    let first = Command::new(env!("CARGO_BIN_EXE_void-seam-check"))
        .args(["--count", "1", "--write-cases"])
        .arg(&d)
        .output()
        .unwrap();
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    assert_eq!(fs::read_dir(&d).unwrap().count(), 6);
    let second = Command::new(env!("CARGO_BIN_EXE_void-seam-check"))
        .arg("--corpus")
        .arg(&d)
        .output()
        .unwrap();
    assert!(
        second.status.success(),
        "{}",
        String::from_utf8_lossy(&second.stderr)
    );
    assert!(String::from_utf8_lossy(&second.stdout).contains("6 passed, 0 failed"));
    fs::remove_dir_all(d).unwrap();
}
