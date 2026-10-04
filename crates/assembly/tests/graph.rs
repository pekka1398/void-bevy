//! The flying part graph agrees with the compiled craft it was made from, and separation and
//! docking are its own operations.
use std::collections::HashSet;
use std::panic::{AssertUnwindSafe, catch_unwind};

use void_assembly::{Category, Connection, PartGraph, compile, demo_craft, flight_rocket};

fn panics(f: impl FnOnce()) -> bool {
    catch_unwind(AssertUnwindSafe(f)).is_err()
}

#[test]
fn a_launched_craft_is_its_compiled_graph() {
    for craft in [demo_craft(), flight_rocket()] {
        let c = compile(&craft).unwrap();
        let mut graph = PartGraph::new();
        let ids = graph.add(&c, "v1");
        let local = |id: &str| id.strip_prefix("v1/").unwrap().to_string();
        assert_eq!(
            ids.iter().map(|id| local(id)).collect::<Vec<_>>(),
            c.parts
                .iter()
                .map(|p| p.instance.id.clone())
                .collect::<Vec<_>>()
        );
        for (id, p) in ids.iter().zip(&c.parts) {
            let part = graph.part(id);
            assert_eq!(part.pose, p.pose);
            assert_eq!(part.fuel_kg(), p.instance.resource_mass());
            assert_eq!(part.stage, p.instance.stage);
            assert!(!part.staged() && !part.lit());
            assert_eq!(
                part.engine().is_some(),
                p.definition.category == Category::Engine
            );
        }
        assert_eq!(graph.mass(&ids), c.summary(None).mass_kg, "{}", craft.name);
        let whole = graph.components(&ids);
        assert_eq!(whole.len(), 1);
        let compiled = c.components(&HashSet::new());
        assert_eq!(
            whole[0].iter().map(|id| local(id)).collect::<Vec<_>>(),
            compiled[0]
        );
        let free: Vec<_> = graph
            .free_nodes(&ids)
            .into_iter()
            .map(|(p, n)| (local(&p), n.id.clone()))
            .collect();
        let want: Vec<_> = c
            .free_nodes()
            .into_iter()
            .map(|n| (n.part_id, n.node.id.clone()))
            .collect();
        assert_eq!(free, want);
        for p in c
            .parts
            .iter()
            .filter(|p| p.definition.category == Category::Engine)
        {
            let tanks: Vec<_> = graph
                .crossfeed_tanks(&ids, &format!("v1/{}", p.instance.id))
                .iter()
                .map(|id| local(id))
                .collect();
            assert_eq!(tanks, c.fuel_sources(&p.instance.id, &HashSet::new()));
        }
    }
}

#[test]
fn separation_and_docking_are_graph_operations() {
    let c = compile(&demo_craft()).unwrap();
    let mut graph = PartGraph::new();
    let a = graph.add(&c, "v1");
    let b = graph.add(&c, "v2");
    let all: Vec<String> = a.iter().chain(&b).cloned().collect();
    assert_eq!(graph.components(&all), [a.clone(), b.clone()]);
    // The demo's decoupler splits the stack in two at its node.
    let decoupler = a
        .iter()
        .find(|id| graph.part(id).decoupler().is_some())
        .expect("the demo has a decoupler")
        .clone();
    let (node, _) = graph.part(&decoupler).decoupler().unwrap();
    let cut = graph.disconnect(&decoupler, node);
    let halves = graph.components(&a);
    assert_eq!(halves.len(), 2);
    assert!(halves[0].contains(&"v1/p1".to_string()));
    assert_eq!(
        halves.iter().map(Vec::len).sum::<usize>(),
        a.len(),
        "{halves:?}"
    );
    // Each engine now reaches only the tanks on its own side.
    for half in &halves {
        for engine in half.iter().filter(|id| graph.part(id).engine().is_some()) {
            let reach = graph.crossfeed_tanks(&a, engine);
            assert!(
                !reach.is_empty() && reach.iter().all(|t| half.contains(t)),
                "{reach:?}"
            );
        }
    }
    // Docking the free bottom of the second stack's pod to the first's freed node joins them.
    graph.connect(cut.clone());
    assert_eq!(graph.components(&a).len(), 1);
    assert!(panics(|| {
        graph.clone().connect(cut.clone());
    }));
    graph.disconnect(&cut.a, &cut.node_a);
    let free = graph.free_nodes(&b);
    let (part, n) = free.first().expect("a free node on the second stack");
    let (other, m) = graph
        .free_nodes(&halves[1])
        .into_iter()
        .find(|(_, m)| m.size == n.size)
        .expect("a free node of the same size");
    graph.connect(Connection {
        a: part.clone(),
        node_a: n.id.clone(),
        b: other.clone(),
        node_b: m.id.clone(),
    });
    let joined: Vec<String> = halves[1].iter().chain(&b).cloned().collect();
    assert_eq!(graph.components(&joined).len(), 1);
}

#[test]
fn invalid_graph_operations_panic() {
    let c = compile(&demo_craft()).unwrap();
    let mut graph = PartGraph::new();
    graph.add(&c, "v1");
    assert!(panics(|| {
        graph.clone().add(&c, "v1");
    }));
    assert!(panics(|| {
        graph.part("v9/p1");
    }));
    assert!(panics(|| {
        graph.clone().disconnect("v1/p1", "top");
    }));
    let occupied = graph.connections()[0].clone();
    assert!(panics(|| {
        graph.clone().connect(occupied.clone());
    }));
    assert!(panics(|| {
        graph.clone().connect(Connection {
            a: "v1/p1".into(),
            node_a: "nowhere".into(),
            b: "v1/p5".into(),
            node_b: "bottom".into(),
        });
    }));
    assert!(panics(|| {
        graph.clone().connect(Connection {
            a: "v1/p5".into(),
            node_a: "bottom".into(),
            b: "v1/p5".into(),
            node_b: "bottom".into(),
        });
    }));
    let mut lit = graph.part("v1/p1").clone();
    lit.id = "v2/p1".into();
    lit.modules.insert(
        "command1".into(),
        void_assembly::ModuleState::Engine {
            activated: true,
            enabled: true,
        },
    );
    assert!(panics(|| {
        graph.clone().insert(lit.clone());
    }));
}

/// Every engine in the catalog rates its own nozzle: the back pressure the flight's air takes
/// from its vacuum thrust.
#[test]
fn every_catalog_engine_rates_its_nozzle() {
    let engines: Vec<_> = void_assembly::catalog()
        .iter()
        .filter(|d| d.category == Category::Engine)
        .collect();
    assert_eq!(engines.len(), 4);
    for d in engines {
        let mut graph = PartGraph::new();
        graph.insert(void_assembly::Part {
            id: "e".into(),
            definition: d,
            resources: void_assembly::Resources::new(),
            modules: void_assembly::initial_modules(d),
            stage: None,
            module_stages: void_assembly::default_module_stages(d, None),
            pose: void_assembly::PartPose {
                position: glam::DVec3::ZERO,
                rotation: glam::DQuat::IDENTITY,
            },
        });
        let rating = graph.part("e").engine().expect("an engine module");
        assert!(
            rating.nozzle_exit_area_m2.is_finite() && rating.nozzle_exit_area_m2 > 0.0,
            "{}",
            d.id
        );
        // At one atmosphere every engine keeps some thrust.
        assert!(
            rating.nozzle_exit_area_m2 * 101_325.0 < rating.thrust_newtons,
            "{}",
            d.id
        );
    }
}

#[test]
fn insertion_rejects_invalid_fuel_pose_and_ignition() {
    let mut source = PartGraph::new();
    source.add(&compile(&demo_craft()).unwrap(), "v1");
    let tank = source.part("v1/p2").clone();
    for fuel in [
        -1.0,
        f64::NAN,
        f64::INFINITY,
        void_assembly::tank_capacity(tank.definition) + 1.0,
    ] {
        let mut part = tank.clone();
        part.resources
            .insert(void_assembly::ResourceId::LiquidPropellant, fuel);
        assert!(panics(|| PartGraph::new().insert(part)));
    }
    for pose in [
        void_assembly::PartPose {
            position: glam::DVec3::NAN,
            ..tank.pose
        },
        void_assembly::PartPose {
            rotation: glam::DQuat::from_xyzw(0.0, 0.0, 0.0, 0.0),
            ..tank.pose
        },
        void_assembly::PartPose {
            rotation: glam::DQuat::from_xyzw(0.0, 0.0, 0.0, 2.0),
            ..tank.pose
        },
    ] {
        let mut part = tank.clone();
        part.pose = pose;
        assert!(panics(|| PartGraph::new().insert(part)));
    }
    let mut engine = source.part("v1/p3").clone();
    engine.modules.insert(
        "engine1".into(),
        void_assembly::ModuleState::Engine {
            activated: false,
            enabled: true,
        },
    );
    assert!(panics(|| PartGraph::new().insert(engine.clone())));
    engine.modules.insert(
        "engine1".into(),
        void_assembly::ModuleState::Engine {
            activated: true,
            enabled: true,
        },
    );
    PartGraph::new().insert(engine);
    let mut no_tank = source.part("v1/p1").clone();
    no_tank
        .resources
        .insert(void_assembly::ResourceId::LiquidPropellant, 1.0);
    assert!(panics(|| PartGraph::new().insert(no_tank)));
}

#[test]
fn runtime_edits_preserve_identity_and_reject_invalid_state_before_mutating() {
    let mut graph = PartGraph::new();
    graph.add(&compile(&demo_craft()).unwrap(), "v1");
    let before = graph.part("v1/p2").clone();
    assert!(panics(|| graph.set_fuel("v1/p2", -1.0)));
    assert!(panics(|| graph.set_pose(
        "v1/p2",
        void_assembly::PartPose {
            position: glam::DVec3::NAN,
            ..before.pose
        }
    )));
    let mut duplicate = before.clone();
    duplicate
        .resources
        .insert(void_assembly::ResourceId::LiquidPropellant, 0.0);
    assert!(panics(|| graph.insert(duplicate)));
    assert_eq!(graph.part("v1/p2").fuel_kg(), before.fuel_kg());
    assert_eq!(graph.part("v1/p2").pose, before.pose);
    graph.set_fuel("v1/p2", 0.0);
    graph.set_pose(
        "v1/p2",
        void_assembly::PartPose {
            position: glam::DVec3::X,
            ..before.pose
        },
    );
    assert_eq!(graph.part("v1/p2").id, before.id);
    assert!(std::ptr::eq(
        graph.part("v1/p2").definition,
        before.definition
    ));
    graph.stage_part("v1/p3");
    assert!(graph.part("v1/p3").lit() && graph.part("v1/p3").staged());
    graph.stage_part("v1/p4");
    assert!(!graph.part("v1/p4").lit() && graph.part("v1/p4").staged());
}
