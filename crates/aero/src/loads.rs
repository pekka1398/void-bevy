//! A vehicle's forces and heating together, as the lab's `Loads.ts`.

use glam::DVec3;
use void_math::pow;

use crate::{
    AeroForces, AeroState, Air, Controls, HeatEnvironment, HeatLoad, ThermalBudget, Vehicle,
    VehicleResources, advance_thermal, aero_elements, aerodynamic_forces, clamp, heat_load,
    inverse, length, mass_properties, rotate, shield_disks, shielded,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PartHeat {
    pub env: HeatEnvironment,
    pub load: HeatLoad,
}

/// Forces on the vehicle and the heat each part takes (in part order).
#[derive(Clone, Debug, PartialEq)]
pub struct VehicleLoads {
    pub aero: AeroForces,
    pub heat: Vec<PartHeat>,
}

impl VehicleLoads {
    /// The highest stagnation flux on any part, W/m² (0 with no heating).
    pub fn max_flux_wm2(&self) -> f64 {
        self.heat.iter().fold(0.0, |m, h| m.max(h.load.flux_wm2))
    }
}

/// Forces and heat loads for `state`; `background_k` is the radiative background.
#[allow(clippy::too_many_arguments)]
pub fn evaluate_vehicle(
    vehicle: &Vehicle,
    resources: &VehicleResources,
    state: &AeroState,
    air: &Air,
    wind: DVec3,
    controls: &Controls,
    background_k: f64,
) -> VehicleLoads {
    let center = mass_properties(vehicle, resources).center;
    let aero = aerodynamic_forces(&aero_elements(vehicle, center), state, air, wind, controls);
    let disks = shield_disks(vehicle);
    let heat = vehicle
        .parts
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let arm = rotate(state.rotation, p.position - center);
            let v = state.velocity + state.angular_velocity.cross(arm) - wind;
            let speed = length(v);
            let local_direction = if speed > 0.0 {
                rotate(inverse(state.rotation), v * (1.0 / speed))
            } else {
                DVec3::ZERO
            };
            let exposed = speed == 0.0 || !shielded(p.position, local_direction, &disks, &p.id);
            let exposure = match p.heating_normal {
                Some(n) => pow(
                    clamp(rotate(p.rotation, n).dot(local_direction), 0.0, 1.0),
                    2.0,
                ),
                None => 1.0,
            };
            let env = HeatEnvironment {
                air: *air,
                speed,
                exposed,
                exposure,
                background_k,
            };
            PartHeat {
                env,
                load: heat_load(&p.thermal, &resources.thermal[i], &env),
            }
        })
        .collect();
    VehicleLoads { aero, heat }
}

/// Advance every part's heat by `dt` under `loads`, with conduction along the thermal links.
/// Returns each part's budget and the first failure: a part overheated or over its force limit.
pub fn advance_heat(
    vehicle: &Vehicle,
    resources: &mut VehicleResources,
    loads: &VehicleLoads,
    dt: f64,
) -> (Vec<ThermalBudget>, Option<String>) {
    let mut core_heat = vec![0.0; vehicle.parts.len()];
    for link in &vehicle.thermal_links {
        let (a, b) = (vehicle.part_index(&link.a), vehicle.part_index(&link.b));
        let w = link.conductance_wk * (resources.thermal[a].core_k - resources.thermal[b].core_k);
        core_heat[a] -= w;
        core_heat[b] += w;
    }
    let mut failure = None;
    let budgets = vehicle
        .parts
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let thermal = &mut resources.thermal[i];
            let budget = advance_thermal(&p.thermal, thermal, &loads.heat[i].env, dt, core_heat[i]);
            if thermal.failed && failure.is_none() {
                failure = Some(format!("{} overheated", p.id));
            }
            if let Some(force) = loads.aero.elements.iter().find(|e| e.id == p.id)
                && length(force.force) > p.max_force_n
                && failure.is_none()
            {
                failure = Some(format!("{} over its aerodynamic load limit", p.id));
            }
            budget
        })
        .collect();
    (budgets, failure)
}
