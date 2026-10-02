use glam::{DQuat, DVec3};
use serde_json::Value;
use std::collections::HashSet;
use void_assembly::*;
fn near(a: f64, b: f64, t: f64) {
    assert!((a - b).abs() <= t, "{a} != {b} (+/- {t})");
}
fn vec(v: &Value) -> DVec3 {
    DVec3::new(
        v[0].as_f64().unwrap(),
        v[1].as_f64().unwrap(),
        v[2].as_f64().unwrap(),
    )
}
fn idle() -> FlightInput {
    FlightInput::default()
}
fn full() -> FlightInput {
    FlightInput {
        throttle: 1.0,
        ..idle()
    }
}
#[test]
fn golden_model_and_catalog() {
    let golden: Value = serde_json::from_str(include_str!("golden/assembly.json")).unwrap();
    for g in golden.as_array().unwrap() {
        let craft: Craft = serde_json::from_value(g["craft"].clone()).unwrap();
        let c = compile(&craft).unwrap();
        assert_eq!(c.root_id, g["rootId"]);
        assert_eq!(
            serde_json::to_value(&c.connections).unwrap(),
            g["connections"]
        );
        let s = c.summary(None);
        near(s.mass_kg, g["summary"]["massKg"].as_f64().unwrap(), 0.0);
        near(s.fuel_kg, g["summary"]["fuelKg"].as_f64().unwrap(), 0.0);
        near(
            (s.center - vec(&g["summary"]["center"])).length(),
            0.0,
            1e-12,
        );
        for (p, e) in c.parts.iter().zip(g["parts"].as_array().unwrap()) {
            assert_eq!(p.instance.id, e["id"]);
            near((p.pose.position - vec(&e["position"])).length(), 0.0, 1e-12);
            let a = e["rotation"].as_array().unwrap();
            let q = DQuat::from_xyzw(
                a[0].as_f64().unwrap(),
                a[1].as_f64().unwrap(),
                a[2].as_f64().unwrap(),
                a[3].as_f64().unwrap(),
            );
            near((p.pose.rotation - q).length(), 0.0, 1e-15);
            assert_eq!(part_inertia_per_kg(p.definition), vec(&e["inertia"]));
            if p.definition.category == Category::Engine {
                assert_eq!(
                    serde_json::to_value(c.fuel_sources(&p.instance.id, &HashSet::new())).unwrap(),
                    g["fuelSources"][&p.instance.id]
                );
            }
        }
        for (n, e) in c.free_nodes().iter().zip(g["free"].as_array().unwrap()) {
            assert_eq!(n.part_id, e["partId"]);
            assert_eq!(n.node.id, e["nodeId"]);
            near((n.pose.position - vec(&e["position"])).length(), 0.0, 1e-12);
        }
        assert_eq!(c.free_nodes().len(), g["free"].as_array().unwrap().len());
        assert_eq!(import_craft(&export_craft(&craft).unwrap()).unwrap(), craft);
    }
    let c = compile(&demo_craft()).unwrap();
    near(c.summary(None).dry_mass_kg, 1090.0, 0.0);
    near(c.summary(None).fuel_kg, 3500.0, 0.0);
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
                (a.pose.position + rotate(a.pose.rotation, na.position)
                    - b.pose.position
                    - rotate(b.pose.rotation, nb.position))
                .length(),
                0.0,
                1e-12,
            );
            near(
                (rotate(a.pose.rotation, na.direction) + rotate(b.pose.rotation, nb.direction))
                    .length(),
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
    bad.parts[1].fuel_kg = 701.0;
    assert!(compile(&bad).unwrap_err().contains("capacity"));
    bad = demo.clone();
    bad.parts[1].fuel_kg = f64::NAN;
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
    let mut f = AssemblyFlight::new(&craft, 0.0).unwrap();
    f.stage();
    f.stage();
    assert_eq!(f.groups.len(), 2);
    assert_eq!(f.controlled_part_ids(), ["p1", "p2"]);
}
#[test]
fn physical_mass_center_and_pad_rest() {
    let mut f = AssemblyFlight::new(&demo_craft(), LAB_GRAVITY).unwrap();
    near(f.controlled_body().mass() as f64, 4590.0, 0.01);
    let expected = f.compiled.summary(None).center + f.part_pose("p1").position;
    near((f.controlled_center() - expected).length(), 0.0, 1e-4);
    for _ in 0..600 {
        f.step(idle());
    }
    assert!(f.part_pose("p6").position.y > 0.5);
    near(f.controlled_velocity().y, 0.0, 0.01);
    near(f.compiled.summary(Some(&f.fuel)).fuel_kg, 3500.0, 0.0);
}
#[test]
fn first_stage_lifts_at_isp_rate_and_leaves_upper_tank_untouched() {
    let mut f = AssemblyFlight::new(&demo_craft(), LAB_GRAVITY).unwrap();
    let y = f.part_pose("p1").position.y;
    assert_eq!(f.stage(), Some(0));
    for _ in 0..120 {
        f.step(full());
    }
    near(
        f.fuel["p5"],
        2800.0 - 90000.0 / (310.0 * G0) * 120.0 * STEP_SECONDS,
        1e-5,
    );
    assert_eq!(f.fuel["p2"], 700.0);
    assert!(f.part_pose("p1").position.y > y + 10.0);
    assert!(f.controlled_velocity().y > 10.0);
}
fn momentum(f: &AssemblyFlight) -> (DVec3, DVec3) {
    let mut linear = DVec3::ZERO;
    let mut angular = DVec3::ZERO;
    for g in &f.groups {
        let b = &f.world.bodies[g.body];
        let cv = |v: rapier3d::math::Vector| DVec3::new(v.x as f64, v.y as f64, v.z as f64);
        let v = cv(b.linvel());
        let w = cv(b.angvel());
        let com = cv(b.center_of_mass());
        for id in &g.ids {
            let p = f.compiled.part(id);
            let pose = f.part_pose(id);
            let m = p.definition.dry_mass_kg + f.fuel[id];
            let pv = (v + w.cross(pose.position - com)) * m;
            linear += pv;
            angular += pose.position.cross(pv)
                + rotate(
                    pose.rotation,
                    part_inertia_per_kg(p.definition) * m * rotate(pose.rotation.conjugate(), w),
                );
        }
    }
    (linear, angular)
}
#[test]
fn rotating_separation_preserves_every_pose_and_linear_and_angular_momentum() {
    let mut f = AssemblyFlight::new(&demo_craft(), 0.0).unwrap();
    f.stage();
    let h = f.controlled_handle();
    f.world.bodies[h].set_linvel(rapier3d::math::Vector::new(12.0, 20.0, -3.0), true);
    f.world.bodies[h].set_angvel(rapier3d::math::Vector::new(0.3, -0.2, 0.4), true);
    let poses: Vec<_> = f
        .compiled
        .parts
        .iter()
        .map(|p| (p.instance.id.clone(), f.part_pose(&p.instance.id)))
        .collect();
    let (p, l) = momentum(&f);
    assert_eq!(f.stage(), Some(1));
    assert_eq!(f.groups.len(), 2);
    assert_eq!(f.controlled_part_ids(), ["p1", "p2", "p3"]);
    let (pa, la) = momentum(&f);
    near((pa - p).length(), 0.0, 0.04);
    near((la - l).length(), 0.0, 0.3);
    for (id, pose) in poses {
        near(
            (f.part_pose(&id).position - pose.position).length(),
            0.0,
            1e-5,
        );
        near(
            (f.part_pose(&id).rotation - pose.rotation).length(),
            0.0,
            1e-6,
        );
    }
    assert!(f.lit.contains("p3"));
    assert_eq!(f.stage(), None);
    f.step(full());
    assert!(f.fuel["p2"] < 700.0 && f.fuel["p5"] < 2800.0);
}
#[test]
fn custom_imported_craft_fuel_exhaustion_clips_impulse_and_mass() {
    let c = add_part(&fresh_craft(), "tank-small", "p1", "bottom", "top").unwrap();
    let mut c = add_part(&c, "engine-small", "p2", "bottom", "top").unwrap();
    c.parts[1].fuel_kg = 0.01;
    let mut f =
        AssemblyFlight::new(&import_craft(&export_craft(&c).unwrap()).unwrap(), 0.0).unwrap();
    f.stage();
    f.step(full());
    assert_eq!(f.fuel["p2"], 0.0);
    assert!(f.firing["p3"] > 0.0 && f.firing["p3"] < 1.0);
    near(f.controlled_body().mass() as f64, 500.0, 0.001);
    let v = f.controlled_velocity();
    f.step(full());
    assert_eq!(f.firing["p3"], 0.0);
    near((v - f.controlled_velocity()).length(), 0.0, 1e-5);
}
#[test]
fn invalid_launch_and_input_fail_explicitly() {
    let mut c = demo_craft();
    c.parts[2].stage = None;
    assert!(
        AssemblyFlight::new(&c, LAB_GRAVITY)
            .err()
            .unwrap()
            .contains("assign a stage")
    );
    let c = add_part(&fresh_craft(), "engine-small", "p1", "bottom", "top").unwrap();
    let c = add_part(&c, "decoupler", "p2", "bottom", "bottom").unwrap();
    assert!(
        AssemblyFlight::new(&c, LAB_GRAVITY)
            .err()
            .unwrap()
            .contains("not connected")
    );
    assert!(AssemblyFlight::new(&fresh_craft(), LAB_GRAVITY).is_err());
    let mut f = AssemblyFlight::new(&demo_craft(), 0.0).unwrap();
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f.step(FlightInput {
            throttle: f64::NAN,
            ..idle()
        })))
        .is_err()
    );
}
#[test]
fn nonconsecutive_and_same_stage_execution() {
    let mut c = demo_craft();
    c.parts[5].stage = Some(3);
    c.parts[2].stage = Some(7);
    c.parts[3].stage = Some(7);
    let mut f = AssemblyFlight::new(&c, 0.0).unwrap();
    assert_eq!(f.stages, [3, 7]);
    assert_eq!(f.stage(), Some(3));
    assert_eq!(f.stage(), Some(7));
    assert!(f.lit.contains("p3"));
    assert_eq!(f.groups.len(), 2);
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
