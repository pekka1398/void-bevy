//! Stable crew identity. Consumable backpack propellant stays in typed PartGraph resources.
use glam::DVec3;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CrewRecord {
    pub id: String,
    pub name: String,
    pub body_mass_kg: f64,
    pub suit_dry_mass_kg: f64,
}
impl CrewRecord {
    pub fn pilot() -> Self {
        Self {
            id: "unassigned".into(),
            name: "Pilot".into(),
            body_mass_kg: 80.0,
            suit_dry_mass_kg: 20.0,
        }
    }
    pub fn validate(&self) -> bool {
        !self.id.is_empty()
            && !self.name.is_empty()
            && self.body_mass_kg.is_finite()
            && self.body_mass_kg > 0.0
            && self.suit_dry_mass_kg.is_finite()
            && self.suit_dry_mass_kg > 0.0
    }
    pub fn carried_dry_mass_kg(&self) -> f64 {
        self.body_mass_kg + self.suit_dry_mass_kg
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SeatDefinition {
    /// Suit centre at the hatch endpoint in seat-part axes, not a terrain teleport target.
    pub hatch_position: DVec3,
    pub exit_direction: DVec3,
    pub boarding_radius_meters: f64,
    pub boarding_speed_meters_per_second: f64,
    pub initial_crew: bool,
}
impl SeatDefinition {
    pub fn validate(&self) -> Result<(), String> {
        if !self.hatch_position.is_finite()
            || !self.exit_direction.is_finite()
            || (self.exit_direction.length_squared() - 1.0).abs() > 1e-9
            || !self.boarding_radius_meters.is_finite()
            || self.boarding_radius_meters <= 0.0
            || !self.boarding_speed_meters_per_second.is_finite()
            || self.boarding_speed_meters_per_second <= 0.0
        {
            return Err("invalid crew seat/hatch geometry or boarding limits".into());
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EvaControl {
    pub forward: f64,
    /// Positive means player's right in the forward/top frame.
    pub strafe: f64,
    pub yaw: f64,
}
impl Default for EvaControl {
    fn default() -> Self {
        Self {
            forward: 0.0,
            strafe: 0.0,
            yaw: 0.0,
        }
    }
}
impl EvaControl {
    pub fn validate(self) {
        assert!(
            [self.forward, self.strafe, self.yaw]
                .iter()
                .all(|v| v.is_finite() && v.abs() <= 1.0),
            "invalid EVA control"
        );
    }
}

/// Initial crew payload only; accepted live mass always reads PartGraph module state.
pub fn initial_crew_mass_kg(d: &crate::PartDefinition) -> f64 {
    d.modules
        .iter()
        .map(|m| match m {
            crate::Module::Seat { parameters, .. } if parameters.initial_crew => {
                CrewRecord::pilot().carried_dry_mass_kg()
            }
            crate::Module::Crew { .. } => CrewRecord::pilot().body_mass_kg,
            _ => 0.0,
        })
        .sum()
}

/// Finite grounded locomotion actuators; no position or velocity is prescribed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EvaDefinition {
    pub walking_speed_meters_per_second: f64,
    pub walking_acceleration_meters_per_second_squared: f64,
    pub traction_coefficient: f64,
    pub upright_stiffness_nm: f64,
    pub upright_damping_nm_seconds: f64,
    pub upright_torque_limit_nm: f64,
    pub yaw_rate_radians_per_second: f64,
    pub jump_speed_meters_per_second: f64,
    pub ground_probe_margin_meters: f64,
}
impl EvaDefinition {
    pub fn validate(&self) -> Result<(), String> {
        if [
            self.walking_speed_meters_per_second,
            self.walking_acceleration_meters_per_second_squared,
            self.traction_coefficient,
            self.upright_stiffness_nm,
            self.upright_damping_nm_seconds,
            self.upright_torque_limit_nm,
            self.yaw_rate_radians_per_second,
            self.jump_speed_meters_per_second,
            self.ground_probe_margin_meters,
        ]
        .iter()
        .any(|v| !v.is_finite() || *v <= 0.0)
        {
            return Err("EVA actuators require finite positive parameters".into());
        }
        Ok(())
    }
}
/// Packed suit remembers its accepted thermal state while stowed. Temperatures are not reset
/// by boarding or exiting, and consumable mass remains on the seat's typed resource tank.
pub fn eva_suit_thermal_definition() -> &'static crate::ThermalDefinition {
    crate::definition("eva-suit")
        .expect("authored EVA suit")
        .modules
        .iter()
        .find_map(|m| match m {
            crate::Module::Thermal { parameters, .. } => Some(parameters),
            _ => None,
        })
        .expect("EVA suit thermal module")
}
