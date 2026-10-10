//! The craft files under `crafts/` are the code fixtures, so `--craft crafts/<name>.json` flies
//! exactly them.
#[test]
fn craft_files_match_the_code_fixtures() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crafts");
    for (name, craft) in [
        ("aircraft", void_assembly::aircraft()),
        ("rover", void_assembly::crew_rover()),
        ("reentry-capsule", void_assembly::reentry_capsule()),
    ] {
        let path = dir.join(format!("{name}.json"));
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"));
        let file = void_assembly::import_craft(&text).unwrap();
        assert_eq!(
            void_assembly::export_craft(&file).unwrap(),
            void_assembly::export_craft(&craft).unwrap(),
            "{name}"
        );
    }
}
