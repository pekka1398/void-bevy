use glam::DVec3;
use void_frames::SplitPosition;

#[test]
fn durable_split_cells_never_use_lossy_json_numbers() {
    let position = SplitPosition::new(DVec3::new(0.01, -0.5, 2.0), [1_i128 << 100, -19, 0]);
    let wire = serde_json::to_string(&position).unwrap();
    let decoded: SplitPosition = serde_json::from_str(&wire).unwrap();
    assert_eq!(decoded, position);
    let mut invalid = serde_json::to_value(position).unwrap();
    invalid["cell"][0] = serde_json::json!(12);
    assert!(serde_json::from_value::<SplitPosition>(invalid).is_err());
}

#[test]
fn malformed_split_state_is_rejected_instead_of_normalized() {
    for wire in [
        r#"{"cell":["0","0","0"],"offset":[2147483648.0,0.0,0.0]}"#,
        r#"{"cell":["00","0","0"],"offset":[0.0,0.0,0.0]}"#,
        r#"{"cell":["0","0","0"],"offset":[0.0,0.0,0.0],"extra":1}"#,
    ] {
        assert!(
            serde_json::from_str::<SplitPosition>(wire).is_err(),
            "{wire}"
        );
    }
}
