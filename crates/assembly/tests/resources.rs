use glam::{DQuat, DVec3};
use void_assembly::*;
fn part(d: &'static PartDefinition) -> Part {
    Part {
        id: "p".into(),
        definition: d,
        resources: full_resources(d),
        modules: initial_modules(d),
        stage: Some(0),
        module_stages: default_module_stages(d, Some(0)),
        pose: PartPose {
            position: DVec3::ZERO,
            rotation: DQuat::IDENTITY,
        },
    }
}
#[test]
fn module_ids_resources_and_states_are_validated_before_mutation() {
    let d = definition("dual-resource-pod").unwrap();
    let mut graph = PartGraph::new();
    graph.insert(part(d));
    assert_eq!(graph.part("p").resource_mass(), 30.0);
    graph.set_module_state(
        "p",
        "engine1",
        ModuleState::Engine {
            activated: true,
            enabled: true,
        },
    );
    assert!(graph.part("p").engine_enabled("engine1"));
    assert!(!graph.part("p").engine_enabled("engine2"));
    let before = graph.part("p").resources.clone();
    for q in [-1.0, f64::NAN, f64::INFINITY, 11.0] {
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| graph.set_resource(
                "p",
                ResourceId::Monopropellant,
                q
            )))
            .is_err()
        );
        assert_eq!(graph.part("p").resources, before);
    }
    let mut bad = part(d);
    bad.modules.remove("engine2");
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| PartGraph::new().insert(bad)))
            .is_err()
    );
    let mut bad = part(d);
    bad.resources.remove(&ResourceId::LiquidPropellant);
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| PartGraph::new().insert(bad)))
            .is_err()
    );
    let mut bad = part(d);
    bad.modules.insert("engine1".into(), ModuleState::Passive);
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| PartGraph::new().insert(bad)))
            .is_err()
    );
}
#[test]
fn legacy_conversion_is_explicit_and_preserves_authored_mass() {
    let old: serde_json::Value =
        serde_json::from_str(include_str!("golden/assembly.json")).unwrap();
    for g in old.as_array().unwrap() {
        assert!(import_craft(&g["craft"].to_string()).is_err());
        let c = migrate_legacy_craft(g["craft"].clone()).unwrap();
        assert_eq!(
            compile(&c).unwrap().summary(None).mass_kg,
            g["summary"]["massKg"].as_f64().unwrap()
        );
        assert_eq!(import_craft(&export_craft(&c).unwrap()).unwrap(), c);
    }
}
#[test]
fn catalog_rejects_duplicate_module_identity_and_bad_ratings() {
    let mut d = definition("dual-resource-pod").unwrap().clone();
    d.modules.push(d.modules[0].clone());
    assert!(validate_definition(&d).is_err());
    let mut d = definition("dual-resource-pod").unwrap().clone();
    if let Module::Engine { isp_seconds, .. } = &mut d.modules[3] {
        *isp_seconds = f64::NAN;
    }
    assert!(validate_definition(&d).is_err());
}

#[test]
fn illegal_module_transitions_and_unknown_stage_overrides_are_rejected() {
    let d = definition("dual-resource-pod").unwrap();
    let mut g = PartGraph::new();
    g.insert(part(d));
    g.stage_module("p", "engine1");
    let before = g.part("p").modules.clone();
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| g.set_module_state(
            "p",
            "engine1",
            ModuleState::Engine {
                activated: false,
                enabled: false
            }
        )))
        .is_err()
    );
    assert_eq!(g.part("p").modules, before);
    let mut c = fresh_craft();
    c.parts[0].module_stages.insert("missing".into(), Some(1));
    assert!(compile(&c).is_err());
    let mut g = PartGraph::new();
    let d = definition("parachute-pod").unwrap();
    g.insert(part(d));
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| g.set_module_state(
            "p",
            "parachute1",
            ModuleState::Parachute {
                state: ParachuteState {
                    phase: ParachutePhase::Full,
                    elapsed_seconds: 0.0
                }
            }
        )))
        .is_err()
    );
    assert_eq!(
        g.part("p").modules["parachute1"],
        ModuleState::Parachute {
            state: ParachuteState::STOWED
        }
    );
}

#[test]
fn authored_state_maps_reject_duplicate_raw_json_keys() {
    let mut c = fresh_craft();
    c.parts[0].definition_id = "dual-resource-pod".into();
    c.parts[0].resources = full_resources(definition("dual-resource-pod").unwrap());
    c.parts[0].module_stages.insert("engine1".into(), Some(0));
    let raw = export_craft(&c).unwrap();
    for changed in [
        raw.replacen(
            "\"liquidPropellant\":",
            "\"liquidPropellant\":0,\"liquidPropellant\":",
            1,
        ),
        raw.replacen("\"engine1\":", "\"engine1\":1,\"engine1\":", 1),
    ] {
        assert!(import_craft(&changed).unwrap_err().contains("duplicate"));
    }
}
