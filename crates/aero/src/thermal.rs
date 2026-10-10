//! Part heating: a skin and a core per part, internal conduction,
//! radiation, low-speed convection, Sutton–Graves stagnation heating at high Mach with a hot-wall
//! correction, and an ablator spent as a finite latent-heat reserve.

use crate::{Air, finite, positive, smooth, validate_air};

pub const STEFAN_BOLTZMANN: f64 = 5.670374419e-8;
/// Sutton–Graves constant for Earth air, SI.
pub const EARTH_SUTTON_GRAVES: f64 = 1.7415e-4;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ablator {
    pub mass_kg: f64,
    pub activation_k: f64,
    pub latent_j_kg: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ThermalSpec {
    pub skin_capacity_jk: f64,
    pub core_capacity_jk: f64,
    pub conductance_wk: f64,
    pub radiating_area: f64,
    pub heating_area: f64,
    pub convection_area: f64,
    pub emissivity: f64,
    pub nose_radius: f64,
    pub heating_factor: f64,
    pub max_skin_k: f64,
    pub max_core_k: f64,
    pub ablator: Option<Ablator>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ThermalState {
    pub skin_k: f64,
    pub core_k: f64,
    pub ablator_kg: f64,
    pub failed: bool,
}

/// What a part's skin sees: the air, its airspeed, whether a shield blocks the direct flow, how
/// squarely it faces the flow (0–1) and the radiative background temperature.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HeatEnvironment {
    pub air: Air,
    pub speed: f64,
    pub exposed: bool,
    pub exposure: f64,
    pub background_k: f64,
}

/// Heat flows into the skin (aerodynamic, convection) and out of it (radiation, conduction to the
/// core), W, and the stagnation flux, W/m².
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HeatLoad {
    pub aerodynamic_w: f64,
    pub convection_w: f64,
    pub radiation_w: f64,
    pub conduction_w: f64,
    pub flux_wm2: f64,
}

/// Energy over one advance, J: in, radiated, spent on ablation, added to the core from outside,
/// and the change stored in skin and core.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ThermalBudget {
    pub incoming_j: f64,
    pub radiation_j: f64,
    pub ablation_j: f64,
    pub core_external_j: f64,
    pub stored_j: f64,
}

pub fn thermal_state(spec: &ThermalSpec, temperature_k: f64) -> ThermalState {
    positive(temperature_k, "initial temperature");
    ThermalState {
        skin_k: temperature_k,
        core_k: temperature_k,
        ablator_kg: spec.ablator.map_or(0.0, |a| a.mass_kg),
        failed: false,
    }
}

/// Cold-wall stagnation-point heating of a blunt body, W/m² (SI): k √(ρ/Rn) V³.
pub fn stagnation_flux(density: f64, speed: f64, nose_radius: f64) -> f64 {
    assert!(
        density >= 0.0 && speed >= 0.0,
        "Negative heating flow input"
    );
    finite(density, "heating density");
    finite(speed, "heating speed");
    positive(nose_radius, "nose radius");
    EARTH_SUTTON_GRAVES * (density / nose_radius).sqrt() * f64::powf(speed, 3.0)
}

fn film_coefficient(air: &Air, speed: f64) -> f64 {
    (5.0 + 12.0 * speed.sqrt()) * f64::powf(air.density / 1.225, 0.6)
}

pub fn heat_load(spec: &ThermalSpec, state: &ThermalState, env: &HeatEnvironment) -> HeatLoad {
    let (air, speed) = (&env.air, env.speed);
    validate_air(air);
    positive(env.background_k, "radiative background");
    finite(env.exposure, "heat exposure");
    assert!(
        (0.0..=1.0).contains(&env.exposure),
        "Heat exposure outside [0,1]"
    );
    finite(state.ablator_kg, "remaining ablator");
    assert!(state.ablator_kg >= 0.0, "Negative remaining ablator");
    positive(state.skin_k, "skin temperature");
    positive(state.core_k, "core temperature");
    finite(speed, "heat speed");
    assert!(speed >= 0.0, "Negative heat speed");
    let mach = if air.density > 0.0 {
        speed / positive(air.sound_speed, "heat sound speed")
    } else {
        0.0
    };
    let hypersonic_weight = smooth(3.0, 5.0, mach);
    let recovery_k = air.temperature_k + 0.9 * speed * speed / (2.0 * 1005.0);
    let h = if air.density > 0.0 {
        film_coefficient(air, speed)
    } else {
        0.0
    };
    let exposure = if env.exposed {
        env.exposure.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let hot_wall = if speed > 0.0 {
        (1.0 - 0.0_f64.max(state.skin_k - air.temperature_k) / (speed * speed / (2.0 * 1005.0)))
            .clamp(0.0, 1.0)
    } else {
        0.0
    };
    let flux_wm2 = stagnation_flux(air.density, speed, spec.nose_radius)
        * spec.heating_factor
        * exposure
        * hot_wall
        * hypersonic_weight;
    HeatLoad {
        flux_wm2,
        aerodynamic_w: flux_wm2 * spec.heating_area,
        convection_w: exposure
            * h
            * spec.convection_area
            * (recovery_k - state.skin_k)
            * (1.0 - hypersonic_weight),
        radiation_w: spec.emissivity
            * STEFAN_BOLTZMANN
            * spec.radiating_area
            * (f64::powf(state.skin_k, 4.0) - f64::powf(env.background_k, 4.0)),
        conduction_w: spec.conductance_wk * (state.skin_k - state.core_k),
    }
}

/// Advance `state` by `dt` in substeps short enough for the stiffest exchange. Heat beyond what
/// reaches the ablator's activation temperature is spent on its latent reserve until that runs
/// out; then the skin heats again (no temperature clamp protects it). `external_core_w` is heat
/// into the core from other parts.
pub fn advance_thermal(
    spec: &ThermalSpec,
    state: &mut ThermalState,
    env: &HeatEnvironment,
    dt: f64,
    external_core_w: f64,
) -> ThermalBudget {
    advance_with_limit(spec, state, env, dt, external_core_w, 0.05)
}
/// The same solver without the fixed 50 ms accuracy cap. Stable adaptive exchange bounds
/// remain; used for accepted Fleet cooling/rails intervals rather than millions of idle steps.
pub fn advance_thermal_adaptive(
    spec: &ThermalSpec,
    state: &mut ThermalState,
    env: &HeatEnvironment,
    dt: f64,
    external_core_w: f64,
) -> ThermalBudget {
    advance_with_limit(spec, state, env, dt, external_core_w, f64::INFINITY)
}
fn advance_with_limit(
    spec: &ThermalSpec,
    state: &mut ThermalState,
    env: &HeatEnvironment,
    dt: f64,
    external_core_w: f64,
    max_step: f64,
) -> ThermalBudget {
    positive(dt, "thermal step");
    finite(external_core_w, "core heat");
    let start_energy = state.skin_k * spec.skin_capacity_jk + state.core_k * spec.core_capacity_jk;
    let mut budget = ThermalBudget {
        core_external_j: external_core_w * dt,
        ..ThermalBudget::default()
    };
    let mut left = dt;
    while left > 1e-12 {
        let radiation_slope = 4.0
            * spec.emissivity
            * STEFAN_BOLTZMANN
            * spec.radiating_area
            * f64::powf(state.skin_k, 3.0);
        let h = if env.air.density > 0.0 {
            film_coefficient(&env.air, env.speed) * spec.convection_area
        } else {
            0.0
        };
        let step = left
            .min(max_step)
            .min(
                0.2 * spec.skin_capacity_jk
                    / 1.0_f64.max(spec.conductance_wk + radiation_slope + h),
            )
            .min(0.2 * spec.core_capacity_jk / 1.0_f64.max(spec.conductance_wk));
        let load = heat_load(spec, state, env);
        let mut skin_j =
            (load.aerodynamic_w + load.convection_w - load.radiation_w - load.conduction_w) * step;
        if let Some(ablator) = spec.ablator
            && state.ablator_kg > 0.0
            && skin_j > 0.0
        {
            let to_activation =
                0.0_f64.max((ablator.activation_k - state.skin_k) * spec.skin_capacity_jk);
            let available = 0.0_f64.max(skin_j - to_activation);
            let used_kg = state.ablator_kg.min(available / ablator.latent_j_kg);
            let consumed_j = used_kg * ablator.latent_j_kg;
            state.ablator_kg -= used_kg;
            skin_j -= consumed_j;
            budget.ablation_j += consumed_j;
        }
        state.skin_k += skin_j / spec.skin_capacity_jk;
        state.core_k += (load.conduction_w + external_core_w) * step / spec.core_capacity_jk;
        positive(state.skin_k, "thermal skin result");
        positive(state.core_k, "thermal core result");
        if state.skin_k > spec.max_skin_k || state.core_k > spec.max_core_k {
            state.failed = true;
        }
        budget.incoming_j += (load.aerodynamic_w + load.convection_w) * step;
        budget.radiation_j += load.radiation_w * step;
        left -= step;
    }
    budget.stored_j =
        state.skin_k * spec.skin_capacity_jk + state.core_k * spec.core_capacity_jk - start_energy;
    budget
}
