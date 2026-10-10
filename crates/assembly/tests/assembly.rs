use serde_json::Value;
use std::collections::HashSet;
use void_assembly::*;
fn near(a: f64, b: f64, t: f64) {
    assert!((a - b).abs() <= t, "{a} != {b} (+/- {t})");
}
#[test]
fn demo_craft_summary_and_round_trip() {
    let craft = demo_craft();
    let c = compile(&craft).unwrap();
    let s = c.summary(None);
    near(s.dry_mass_kg, 1090.0, 0.0);
    near(s.fuel_kg, 3500.0, 0.0);
    near(s.mass_kg, 4590.0, 1e-9);
    // Every connection joins two distinct parts of the craft.
    for link in &c.connections {
        assert_ne!(link.a, link.b);
        c.part(&link.a);
        c.part(&link.b);
    }
    assert_eq!(import_craft(&export_craft(&craft).unwrap()).unwrap(), craft);
}
#[test]
fn mating_points_and_normals_with_reversed_and_shuffled_parts() {
    let reversed = add_part(&fresh_craft(), "tank-small", "p1", "bottom", "bottom").unwrap();
    let mut shuffled = demo_craft();
    shuffled.parts.reverse();
    for craft in [demo_craft(), reversed, shuffled] {
        let c = compile(&craft).unwrap();
        for link in &c.connections {
            let a = c.part(&link.a);
            let b = c.part(&link.b);
            let na = node(a.definition, &link.node_a).unwrap();
            let nb = node(b.definition, &link.node_b).unwrap();
            near(
                (a.pose.position + a.pose.rotation * na.position
                    - b.pose.position
                    - b.pose.rotation * nb.position)
                    .length(),
                0.0,
                1e-12,
            );
            near(
                (a.pose.rotation * na.direction + b.pose.rotation * nb.direction).length(),
                0.0,
                1e-12,
            );
        }
    }
}
#[test]
fn bad_data_and_edits_rejected_without_mutating_source() {
    let demo = demo_craft();
    assert!(
        add_part(&demo, "tank-small", "p1", "bottom", "top")
            .unwrap_err()
            .contains("occupied")
    );
    let mut bad = demo.clone();
    bad.parts[1]
        .resources
        .insert(ResourceId::LiquidPropellant, 701.0);
    assert!(compile(&bad).unwrap_err().contains("capacity"));
    bad = demo.clone();
    bad.parts[1]
        .resources
        .insert(ResourceId::LiquidPropellant, f64::NAN);
    assert!(compile(&bad).is_err());
    bad = demo.clone();
    bad.parts[1].attachment = None;
    assert!(compile(&bad).unwrap_err().contains("root"));
    bad = demo.clone();
    bad.parts[1].attachment.as_mut().unwrap().parent_id = "p3".into();
    assert!(compile(&bad).unwrap_err().contains("cycle"));
    bad = demo.clone();
    bad.parts[1].attachment.as_mut().unwrap().parent_id = "missing".into();
    assert!(compile(&bad).unwrap_err().contains("Missing parent"));
    bad = demo.clone();
    bad.parts[1].id = "p1".into();
    assert!(compile(&bad).is_err());
    bad = demo.clone();
    bad.parts[0].stage = Some(0);
    assert!(compile(&bad).is_err());
    bad = demo.clone();
    bad.parts[2].stage = Some(100);
    assert!(compile(&bad).is_err());
    for input in [
        "not json",
        "{\"version\":2}",
        "{\"version\":1,\"name\":\"x\",\"parts\":[]}",
    ] {
        assert!(import_craft(input).is_err());
    }
    let mut json = serde_json::to_value(&demo).unwrap();
    json["parts"][1]["fuelKg"] = Value::from(-1);
    assert!(import_craft(&json.to_string()).is_err());
    let mut missing = serde_json::to_value(&demo).unwrap();
    missing["parts"][0].as_object_mut().unwrap().remove("stage");
    assert!(import_craft(&missing.to_string()).is_err());
    assert_eq!(demo, demo_craft());
    assert!(remove_subtree(&demo, "p1").is_err());
    assert!(remove_subtree(&demo, "missing").is_err());
    assert_eq!(
        remove_subtree(&demo, "p4")
            .unwrap()
            .parts
            .iter()
            .map(|p| p.id.as_str())
            .collect::<Vec<_>>(),
        ["p1", "p2", "p3"]
    );
}
#[test]
fn actual_graph_crossfeed_and_child_side_decoupler() {
    let c = compile(&demo_craft()).unwrap();
    let cuts = HashSet::from(["p4".to_string()]);
    assert_eq!(
        c.components(&cuts),
        [vec!["p1", "p2", "p3"], vec!["p4", "p5", "p6"]]
    );
    assert_eq!(c.fuel_sources("p3", &HashSet::new()), ["p2"]);
    assert_eq!(c.fuel_sources("p6", &HashSet::new()), ["p5"]);
    let mut craft = add_part(&fresh_craft(), "decoupler", "p1", "bottom", "bottom").unwrap();
    craft = add_part(&craft, "tank-small", "p2", "top", "top").unwrap();
    craft = add_part(&craft, "engine-small", "p3", "bottom", "top").unwrap();
    craft.parts[1].stage = Some(1);
    let c = compile(&craft).unwrap();
    assert_eq!(c.decoupler_connection("p2").unwrap().b, "p3");
}
#[test]
fn independent_crossfeed_graph_handles_cycles_subsets_and_blocked_parts() {
    let tank = definition("tank-small").unwrap();
    let mut engine = definition("engine-small").unwrap().clone();
    // An engine is identified by its module, not its display category.
    engine.category = Category::Command;
    let mut blocked = definition("pod").unwrap().clone();
    blocked.crossfeed = false;
    let parts = [
        CrossfeedPart {
            id: "far",
            definition: tank,
        },
        CrossfeedPart {
            id: "engine",
            definition: &engine,
        },
        CrossfeedPart {
            id: "near",
            definition: tank,
        },
        CrossfeedPart {
            id: "blocked",
            definition: &blocked,
        },
        CrossfeedPart {
            id: "isolated",
            definition: tank,
        },
    ];
    let link = |a: &str, b: &str| Connection {
        a: a.into(),
        b: b.into(),
        node_a: "port-a".into(),
        node_b: "port-b".into(),
    };
    // This cyclic graph cannot be represented by the assembly attachment tree.
    let links = vec![
        link("engine", "near"),
        link("near", "far"),
        link("far", "engine"),
        link("near", "blocked"),
        link("blocked", "isolated"),
        link("engine", "outside"),
        link("outside", "isolated"),
    ];
    assert_eq!(crossfeed_tanks(&parts, &links, "engine"), ["far", "near"]);
    let after_separation = vec![links[0].clone(), links[3].clone(), links[4].clone()];
    assert_eq!(
        crossfeed_tanks(&parts, &after_separation, "engine"),
        ["near"]
    );
    let mut disconnected_definition = engine.clone();
    disconnected_definition.crossfeed = false;
    let disconnected_engine = [
        CrossfeedPart {
            id: "engine",
            definition: &disconnected_definition,
        },
        CrossfeedPart {
            id: "near",
            definition: tank,
        },
    ];
    assert!(crossfeed_tanks(&disconnected_engine, &links, "engine").is_empty());
    assert!(std::panic::catch_unwind(|| crossfeed_tanks(&parts, &links, "missing")).is_err());
    assert!(std::panic::catch_unwind(|| crossfeed_tanks(&parts, &links, "near")).is_err());
    let duplicates = [parts[1], parts[1]];
    assert!(std::panic::catch_unwind(|| crossfeed_tanks(&duplicates, &links, "engine")).is_err());
}
