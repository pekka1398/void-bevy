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
            assert_eq!(part.fuel_kg, p.instance.fuel_kg);
            assert_eq!(part.stage, p.instance.stage);
            assert!(!part.staged && !part.lit);
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
    lit.lit = true;
    assert!(panics(|| {
        graph.clone().insert(lit.clone());
    }));
}
