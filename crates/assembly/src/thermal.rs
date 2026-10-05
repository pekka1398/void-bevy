//! Authored thermal module and its accepted state. Ablator mass lives in typed resources;
//! temperatures and irreversible failure live on this stable module ID.
use glam::DVec3;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThermalDefinition {
    pub skin_capacity_jk: f64,
    pub core_capacity_jk: f64,
    pub conductance_wk: f64,
    pub connection_conductance_wk: f64,
    pub radiating_area: f64,
    pub heating_area: f64,
    pub convection_area: f64,
    pub emissivity: f64,
    pub nose_radius: f64,
    pub heating_factor: f64,
    pub max_skin_k: f64,
    pub max_core_k: f64,
    pub initial_temperature_k: f64,
    pub background_k: f64,
    /// Outward normal in part axes. None heats an unshielded body at its stagnation estimate.
    #[serde(deserialize_with = "crate::model::explicit_option")]
    pub normal: Option<DVec3>,
    #[serde(deserialize_with = "crate::model::explicit_option")]
    pub ablation: Option<AblationDefinition>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AblationDefinition {
    pub activation_k: f64,
    pub latent_j_kg: f64,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartThermalState {
    pub skin_k: f64,
    pub core_k: f64,
    pub failed: bool,
}
impl ThermalDefinition {
    pub fn validate(&self) -> Result<(), String> {
        if [
            self.skin_capacity_jk,
            self.core_capacity_jk,
            self.nose_radius,
            self.max_skin_k,
            self.max_core_k,
            self.initial_temperature_k,
            self.background_k,
        ]
        .iter()
        .any(|v| !v.is_finite() || *v <= 0.0)
            || [
                self.conductance_wk,
                self.connection_conductance_wk,
                self.radiating_area,
                self.heating_area,
                self.convection_area,
                self.emissivity,
                self.heating_factor,
            ]
            .iter()
            .any(|v| !v.is_finite() || *v < 0.0)
            || self.emissivity > 1.0
            || self.initial_temperature_k > self.max_skin_k.min(self.max_core_k)
            || self
                .normal
                .is_some_and(|n| !n.is_finite() || (n.length() - 1.0).abs() > 1e-9)
            || self.ablation.as_ref().is_some_and(|a| {
                !a.activation_k.is_finite()
                    || a.activation_k <= 0.0
                    || !a.latent_j_kg.is_finite()
                    || a.latent_j_kg <= 0.0
            })
        {
            return Err("invalid thermal parameters".into());
        }
        Ok(())
    }
    pub fn initial(&self) -> PartThermalState {
        PartThermalState {
            skin_k: self.initial_temperature_k,
            core_k: self.initial_temperature_k,
            failed: false,
        }
    }
}
