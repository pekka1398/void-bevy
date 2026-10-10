use void_assembly::{Module, ModuleState, PartGraph, ResourceId, compile};
use void_modules::thermal::{Input, advance, commit};
use void_testkit::reentry_capsule;
fn inputs(graph: &PartGraph) -> Vec<Input> {
    ["test/pod", "test/shield"]
        .into_iter()
        .map(|id| {
            let part = graph.part(id);
            let (module, parameters) = part
                .definition
                .modules
                .iter()
                .find_map(|m| {
                    if let Module::Thermal { id, parameters } = m {
                        Some((id, parameters))
                    } else {
                        None
                    }
                })
                .unwrap();
            let ModuleState::Thermal { state } = part.modules[module] else {
                unreachable!()
            };
            Input {
                part: id.into(),
                module: module.clone(),
                parameters: parameters.clone(),
                state,
                ablator_kg: part.resource(ResourceId::Ablator),
                environment: void_aero::HeatEnvironment {
                    air: void_aero::Atmosphere::Earth(void_aero::EarthAtmosphere::new(1.0))
                        .sample(30000.0),
                    speed: 7000.0,
                    exposed: true,
                    exposure: 1.0,
                    background_k: 270.0,
                },
            }
        })
        .collect()
}
#[test]
fn ablation_and_connected_conduction_balance_energy_without_trial_mutation() {
    let mut graph = PartGraph::new();
    graph.add(&compile(&reentry_capsule()).unwrap(), "test");
    let mut input = inputs(&graph);
    input[0].state.skin_k = 500.;
    input[0].state.core_k = 500.;
    let before = graph.part("test/shield").resources.clone();
    let updates = advance(&graph, &input, 0.5);
    assert_eq!(
        graph.part("test/shield").resources,
        before,
        "evaluation must not commit resources"
    );
    assert_eq!(
        graph.part("test/shield").modules["thermal"],
        ModuleState::Thermal {
            state: input[1].state
        }
    );
    let residual: f64 = updates
        .iter()
        .map(|u| {
            u.budget.incoming_j - u.budget.radiation_j - u.budget.ablation_j - u.budget.stored_j
        })
        .sum();
    let external: f64 = updates.iter().map(|u| u.budget.core_external_j).sum();
    assert!(
        external.abs() < 1e-8,
        "connected conduction must sum to zero"
    );
    assert!(residual.abs() < 1e-6, "thermal budget residual {residual}");
    assert!(
        updates[1].ablator_kg < 30.0,
        "finite shield must consume material under hypersonic heating"
    );
    commit(&mut graph, updates);
    assert!(graph.part("test/shield").resource(ResourceId::Ablator) < 30.0);
}

#[test]
fn depleted_ablator_stops_protecting_temperature_and_failure_is_irreversible() {
    let mut graph = PartGraph::new();
    graph.add(&compile(&reentry_capsule()).unwrap(), "test");
    graph.set_resource("test/shield", ResourceId::Ablator, 0.0);
    let input = inputs(&graph);
    let updates = advance(&graph, &input, 5.0);
    let shield = &updates[1];
    assert_eq!(shield.ablator_kg, 0.0);
    assert!(shield.state.skin_k > shield.state.core_k);
    assert!(
        shield.state.failed,
        "exhausted shield under strong heating must fail, not clamp temperature"
    );
    commit(&mut graph, updates);
    assert!(graph.part("test/shield").thermally_failed());
    let mut cool = inputs(&graph);
    for p in &mut cool {
        p.environment.air = void_aero::Air::VACUUM;
        p.environment.speed = 0.0;
    }
    let updates = advance(&graph, &cool, 60.0);
    assert!(
        updates[1].state.failed,
        "cooling cannot repair a failed part"
    );
}

#[test]
fn thermally_failed_command_part_cannot_deliver_rcs() {
    let mut graph = PartGraph::new();
    let ids = graph.add(&compile(&reentry_capsule()).unwrap(), "test");
    let mut input = inputs(&graph);
    input[0].state.failed = true;
    let updates = advance(&graph, &input, 0.0);
    commit(&mut graph, updates);
    let allocation = void_modules::rcs::allocate(
        &graph,
        &ids,
        glam::DVec3::ZERO,
        void_modules::rcs::RcsControl {
            enabled: true,
            force: glam::DVec3::X * 80.0,
            torque: glam::DVec3::ZERO,
        },
    );
    assert_eq!(allocation.force, glam::DVec3::ZERO);
    assert!(allocation.nozzles.is_empty());
}
