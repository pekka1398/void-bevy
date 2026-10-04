//! Explicit migration; normal craft loading never guesses a legacy schema.
fn main() {
    let paths: Vec<_> = std::env::args().skip(1).collect();
    assert_eq!(
        paths.len(),
        2,
        "usage: migrate-craft INPUT.json OUTPUT.json"
    );
    let value =
        serde_json::from_str(&std::fs::read_to_string(&paths[0]).expect("read legacy craft"))
            .expect("parse legacy craft");
    let craft = void_assembly::migrate_legacy_craft(value).expect("invalid legacy craft");
    std::fs::write(&paths[1], void_assembly::export_craft(&craft).unwrap())
        .expect("write migrated craft");
}
