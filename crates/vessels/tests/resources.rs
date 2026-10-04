use glam::{DQuat, DVec3};
use void_assembly::*;
use void_modules::Conditions;
use void_vessels::*;
fn dual_graph() -> (PartGraph, Vec<String>) {
    let mut c = fresh_craft();
    c.parts[0].definition_id = "dual-resource-pod".into();
    c.parts[0].resources = full_resources(definition("dual-resource-pod").unwrap());
    c.parts[0].stage = Some(0);
    let mut g = PartGraph::new();
    let ids = g.add(&compile(&c).unwrap(), "v1");
    g.stage_part(&ids[0]);
    (g, ids)
}
#[test]
fn independent_resources_and_shared_consumers_never_double_spend() {
    let (mut g, ids) = dual_graph();
    let p = propulsion(&g, &ids, 1.0, DVec3::ZERO, &Conditions::VACUUM);
    assert_eq!(p.groups.len(), 2);
    assert_eq!(p.force, DVec3::Y * 200.0);
    let f = 100.0 / (100.0 * G0);
    assert_eq!(p.flow_kg_per_second, 2.0 * f);
    let dt = 5.0;
    burn(&mut g, &p.groups, dt);
    assert!(
        (g.part(&ids[0]).resource(ResourceId::LiquidPropellant) - (20.0 - f * dt)).abs() < 1e-12
    );
    assert!((g.part(&ids[0]).resource(ResourceId::Monopropellant) - (10.0 - f * dt)).abs() < 1e-12);
    g.set_resource(&ids[0], ResourceId::Monopropellant, 0.0);
    let p = propulsion(&g, &ids, 1.0, DVec3::ZERO, &Conditions::VACUUM);
    assert_eq!(p.groups.len(), 1);
    assert_eq!(p.force, DVec3::Y * 100.0);
    g.set_resource(&ids[0], ResourceId::LiquidPropellant, 0.01);
    let p = propulsion(&g, &ids, 1.0, DVec3::ZERO, &Conditions::VACUUM);
    let (force, _, used) = step_thrust(&p, 1.0, DVec3::ZERO);
    assert_eq!(used, 0.01);
    assert!((force.y - 100.0 * 0.01 / f).abs() < 1e-12);
    assert_eq!(burn(&mut g, &p.groups, 1.0), used);
    assert_eq!(g.part(&ids[0]).resource_mass(), 0.0);
}
#[test]
fn same_resource_engines_share_one_pool_and_flame_out_together() {
    let mut d = definition("dual-resource-pod").unwrap().clone();
    if let Module::Engine { resource, .. } = &mut d.modules[4] {
        *resource = ResourceId::LiquidPropellant;
    }
    let d: &'static PartDefinition = Box::leak(Box::new(d));
    let mut g = PartGraph::new();
    g.insert(Part {
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
    });
    g.stage_part("p");
    let ids = vec!["p".into()];
    let p = propulsion(&g, &ids, 1.0, DVec3::ZERO, &Conditions::VACUUM);
    assert_eq!(p.groups.len(), 1);
    assert_eq!(p.groups[0].engines.len(), 2);
    let dt = p.seconds_to_flameout;
    let (_, _, used) = step_thrust(&p, dt + 1.0, DVec3::ZERO);
    assert!((used - 20.0).abs() < 1e-12);
    burn(&mut g, &p.groups, dt + 1.0);
    assert_eq!(g.part("p").resource(ResourceId::LiquidPropellant), 0.0);
    assert_eq!(g.part("p").resource(ResourceId::Monopropellant), 10.0);
}

#[test]
fn typed_supply_stays_with_connected_vessel_members_and_closed_parts_block_it() {
    let (mut g, a) = dual_graph();
    let other = g.part(&a[0]).clone();
    let mut other = other;
    other.id = "v2/p1".into();
    g.insert(other);
    let b = "v2/p1".to_string();
    assert_eq!(g.resource_tanks(&a, &a[0], ResourceId::Monopropellant), a);
    g.connect(Connection {
        a: a[0].clone(),
        node_a: "bottom".into(),
        b: b.clone(),
        node_b: "top".into(),
    });
    let all = vec![a[0].clone(), b.clone()];
    assert_eq!(
        g.resource_tanks(&all, &a[0], ResourceId::Monopropellant),
        all
    );
    assert_eq!(
        g.resource_tanks(&a, &a[0], ResourceId::Monopropellant),
        a,
        "cross-vessel supply is forbidden even when another edge is passed"
    );
    g.disconnect(&a[0], "bottom");
    assert_eq!(g.resource_tanks(&all, &a[0], ResourceId::Monopropellant), a);
    let mut blocked = definition("dual-resource-pod").unwrap().clone();
    blocked.crossfeed = false;
    let blocked = Box::leak(Box::new(blocked));
    let mut part = g.part(&b).clone();
    part.id = "blocked".into();
    part.definition = blocked;
    g.insert(part);
    g.connect(Connection {
        a: a[0].clone(),
        node_a: "bottom".into(),
        b: "blocked".into(),
        node_b: "top".into(),
    });
    let members = vec![a[0].clone(), "blocked".into()];
    assert_eq!(
        g.resource_tanks(&members, &a[0], ResourceId::LiquidPropellant),
        a
    );
}

#[test]
fn active_vacuum_parachute_refuses_rails_without_committing_trial_state() {
    let p = void_landing::earth_size();
    let (e, home) = void_landing::planet_ephemeris(&p);
    let env = void_landing::planet_environment(&p, &e, home, true);
    let frame = void_landing::PlanetFrame::new(&e, home);
    let state = frame.to_inertial(
        &e,
        0.0,
        void_landing::FrameState {
            position: DVec3::X * (p.terrain.radius_meters + 130000.0),
            velocity: DVec3::Y * 1e8,
        },
    );
    let mut f = Fleet::new(e, env, 0.0, vec![], FleetOptions::default());
    let mut c = fresh_craft();
    c.parts[0].definition_id = "parachute-pod".into();
    let id = f.launch(&c, state, DQuat::IDENTITY, DVec3::ZERO);
    f.parachute_command(
        "v1/p1",
        "parachute1",
        void_modules::parachute::Command::Deploy,
    );
    let before = serde_json::to_string(&f.checkpoint()).unwrap();
    assert!(f.rails_blocker().unwrap().contains("parachute"));
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f.advance_on_rails(1e6))).is_err()
    );
    assert_eq!(before, serde_json::to_string(&f.checkpoint()).unwrap());
    assert_eq!(f.snapshot(&id).position, state.position);
}

#[test]
fn multiple_engines_on_one_part_have_independent_stages_and_states() {
    let p = void_landing::earth_size();
    let (e, home) = void_landing::planet_ephemeris(&p);
    let env = void_landing::planet_environment(&p, &e, home, false);
    let frame = void_landing::PlanetFrame::new(&e, home);
    let state = frame.to_inertial(
        &e,
        0.0,
        void_landing::FrameState {
            position: DVec3::X * (p.terrain.radius_meters + 500000.0),
            velocity: DVec3::Y * 7500.0,
        },
    );
    let mut c = fresh_craft();
    c.parts[0].definition_id = "dual-resource-pod".into();
    c.parts[0].resources = full_resources(definition("dual-resource-pod").unwrap());
    c.parts[0].stage = Some(0);
    c.parts[0].module_stages.insert("engine2".into(), Some(1));
    let mut f = Fleet::new(e, env, 0.0, vec![], FleetOptions::default());
    let id = f.launch(&c, state, DQuat::IDENTITY, DVec3::ZERO);
    assert_eq!(f.stages_left(&id), vec![0, 1]);
    f.stage(&id);
    let part = f.parts().part("v1/p1");
    assert!(part.engine_enabled("engine1"));
    assert!(!part.engine_enabled("engine2"));
    assert!(!part.staged());
    f.set_control(
        &id,
        VesselControl {
            throttle: 1.0,
            turn: DVec3::ZERO,
        },
    );
    f.advance(0.25);
    let part = f.parts().part("v1/p1");
    assert_eq!(part.resource(ResourceId::Monopropellant), 10.0);
    assert!(part.resource(ResourceId::LiquidPropellant) < 20.0);
    assert_eq!(f.stages_left(&id), vec![1]);
    f.stage(&id);
    let part = f.parts().part("v1/p1");
    assert!(part.engine_enabled("engine1") && part.engine_enabled("engine2") && part.staged());
    f.advance(0.25);
    assert!(f.parts().part("v1/p1").resource(ResourceId::Monopropellant) < 10.0);
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f.set_module_stage(
            "v1/p1",
            "engine1",
            Some(5)
        )))
        .is_err()
    );
}

#[test]
fn saved_state_maps_reject_duplicate_raw_json_keys() {
    let p = void_landing::earth_size();
    let (e, home) = void_landing::planet_ephemeris(&p);
    let env = void_landing::planet_environment(&p, &e, home, false);
    let mut f = Fleet::new(e, env, 0.0, vec![], FleetOptions::default());
    let mut c = fresh_craft();
    c.parts[0].definition_id = "dual-resource-pod".into();
    c.parts[0].resources = full_resources(definition("dual-resource-pod").unwrap());
    c.parts[0].stage = Some(0);
    f.launch(
        &c,
        void_landing::FrameState {
            position: DVec3::X * 7000000.0,
            velocity: DVec3::Y * 7000.0,
        },
        DQuat::IDENTITY,
        DVec3::ZERO,
    );
    let raw = serde_json::to_string(&f.checkpoint()).unwrap();
    for (field, duplicate) in [
        ("resources", "\"liquidPropellant\":0,"),
        ("modules", "\"command1\":{\"kind\":\"Passive\"},"),
        ("module_stages", "\"engine1\":null,"),
    ] {
        let changed = raw.replacen(
            &format!("\"{field}\":{{"),
            &format!("\"{field}\":{{{duplicate}"),
            1,
        );
        assert_ne!(raw, changed);
        assert!(
            serde_json::from_str::<FleetCheckpoint>(&changed)
                .unwrap_err()
                .to_string()
                .contains("duplicate")
        );
    }
}

#[test]
fn module_stages_work_without_a_legacy_part_stage_and_parachutes_can_stage() {
    let p = void_landing::earth_size();
    let (e, home) = void_landing::planet_ephemeris(&p);
    let env = void_landing::planet_environment(&p, &e, home, false);
    let mut fleet = Fleet::new(e, env, 0.0, vec![], FleetOptions::default());
    let mut craft = fresh_craft();
    craft.parts[0].definition_id = "dual-resource-pod".into();
    craft.parts[0].resources = full_resources(definition("dual-resource-pod").unwrap());
    craft.parts[0]
        .module_stages
        .insert("engine1".into(), Some(0));
    craft.parts[0]
        .module_stages
        .insert("engine2".into(), Some(1));
    let initial = void_landing::FrameState {
        position: DVec3::X * 7000000.0,
        velocity: DVec3::Y * 7000.0,
    };
    let id = fleet.launch(&craft, initial, DQuat::IDENTITY, DVec3::ZERO);
    fleet.stage(&id);
    assert!(fleet.parts().part("v1/p1").engine_enabled("engine1"));
    assert!(!fleet.parts().part("v1/p1").engine_enabled("engine2"));
    craft.parts[0].definition_id = "parachute-pod".into();
    craft.parts[0].resources = full_resources(definition("parachute-pod").unwrap());
    craft.parts[0].module_stages.clear();
    craft.parts[0].stage = Some(0);
    let id = fleet.launch(&craft, initial, DQuat::IDENTITY, DVec3::ZERO);
    fleet.stage(&id);
    let ModuleState::Parachute { state } = fleet.parts().part("v2/p1").modules["parachute1"] else {
        panic!()
    };
    assert_eq!(state.phase, ParachutePhase::Armed);
}
