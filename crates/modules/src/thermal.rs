//! Accepted thermal evolution using aero's skin/core/ablation solver. Inputs are sampled by
//! Fleet, this function is pure; resource and module commits remain the caller's responsibility.
use std::collections::BTreeMap;
use void_aero::{Ablator, HeatEnvironment, ThermalBudget, ThermalSpec, ThermalState};
use void_assembly::{ModuleState, PartGraph, PartThermalState, ResourceId, ThermalDefinition};
#[derive(Clone)]
pub struct Input {
    pub part: String,
    pub module: String,
    pub parameters: ThermalDefinition,
    pub state: PartThermalState,
    pub ablator_kg: f64,
    pub environment: HeatEnvironment,
}
pub struct Update {
    pub part: String,
    pub module: String,
    pub state: PartThermalState,
    pub ablator_kg: f64,
    pub budget: ThermalBudget,
}
fn spec(p: &ThermalDefinition, mass: f64) -> ThermalSpec {
    ThermalSpec {
        skin_capacity_jk: p.skin_capacity_jk,
        core_capacity_jk: p.core_capacity_jk,
        conductance_wk: p.conductance_wk,
        radiating_area: p.radiating_area,
        heating_area: p.heating_area,
        convection_area: p.convection_area,
        emissivity: p.emissivity,
        nose_radius: p.nose_radius,
        heating_factor: p.heating_factor,
        max_skin_k: p.max_skin_k,
        max_core_k: p.max_core_k,
        ablator: p.ablation.as_ref().map(|a| Ablator {
            mass_kg: mass,
            activation_k: a.activation_k,
            latent_j_kg: a.latent_j_kg,
        }),
    }
}
/// Conservative core conduction over connected parts, sampled from the same previous states.
/// Adaptive outer steps bound link exchange; aero independently bounds skin/core exchanges.
pub fn advance(graph: &PartGraph, inputs: &[Input], seconds: f64) -> Vec<Update> {
    assert!(
        seconds.is_finite() && seconds >= 0.0,
        "thermal: invalid accepted time"
    );
    for input in inputs {
        input.parameters.validate().expect("thermal parameters");
        assert!(
            input.ablator_kg.is_finite() && input.ablator_kg >= 0.0,
            "thermal ablator mass"
        );
    }
    let indices: BTreeMap<_, _> = inputs
        .iter()
        .enumerate()
        .map(|(i, p)| (p.part.as_str(), i))
        .collect();
    assert_eq!(
        indices.len(),
        inputs.len(),
        "thermal: multiple modules per part"
    );
    let mut states: Vec<_> = inputs
        .iter()
        .map(|p| ThermalState {
            skin_k: p.state.skin_k,
            core_k: p.state.core_k,
            ablator_kg: p.ablator_kg,
            failed: p.state.failed,
        })
        .collect();
    let specs: Vec<_> = inputs
        .iter()
        .map(|p| spec(&p.parameters, p.ablator_kg))
        .collect();
    let mut links = Vec::new();
    let mut conductances = vec![0.0; inputs.len()];
    for c in graph.connections() {
        if let (Some(&a), Some(&b)) = (indices.get(c.a.as_str()), indices.get(c.b.as_str())) {
            let g = inputs[a]
                .parameters
                .connection_conductance_wk
                .min(inputs[b].parameters.connection_conductance_wk);
            links.push((a, b, g));
            conductances[a] += g;
            conductances[b] += g;
        }
    }
    let max_step = inputs
        .iter()
        .zip(&conductances)
        .fold(f64::INFINITY, |dt, (p, g)| {
            dt.min(if *g > 0.0 {
                0.1 * p.parameters.core_capacity_jk / g
            } else {
                f64::INFINITY
            })
        });
    let mut budgets = vec![ThermalBudget::default(); inputs.len()];
    let mut left = seconds;
    while left > 1e-12 {
        let dt = left
            .min(max_step)
            .min(if seconds <= 1.0 { 0.05 } else { f64::INFINITY });
        let mut core = vec![0.0; inputs.len()];
        for &(a, b, g) in &links {
            let w = g * (states[a].core_k - states[b].core_k);
            core[a] -= w;
            core[b] += w;
        }
        for i in 0..inputs.len() {
            let p = &inputs[i];
            let b = void_aero::advance_thermal_adaptive(
                &specs[i],
                &mut states[i],
                &p.environment,
                dt,
                core[i],
            );
            budgets[i].incoming_j += b.incoming_j;
            budgets[i].radiation_j += b.radiation_j;
            budgets[i].ablation_j += b.ablation_j;
            budgets[i].core_external_j += b.core_external_j;
            budgets[i].stored_j += b.stored_j;
        }
        left -= dt;
    }
    inputs
        .iter()
        .zip(states)
        .zip(budgets)
        .map(|((p, s), budget)| Update {
            part: p.part.clone(),
            module: p.module.clone(),
            state: PartThermalState {
                skin_k: s.skin_k,
                core_k: s.core_k,
                failed: s.failed,
            },
            ablator_kg: s.ablator_kg,
            budget,
        })
        .collect()
}
pub fn commit(graph: &mut PartGraph, updates: Vec<Update>) {
    for u in updates {
        if graph
            .part(&u.part)
            .resources
            .contains_key(&ResourceId::Ablator)
        {
            graph.set_resource(&u.part, ResourceId::Ablator, u.ablator_kg);
        }
        graph.set_module_state(&u.part, &u.module, ModuleState::Thermal { state: u.state });
    }
}
