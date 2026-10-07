//! Authored raycast suspension and finite-inertia tire contact. No runtime model fallback.
use glam::{DQuat, DVec3};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WheelDefinition {
    pub suspension_origin: DVec3,
    /// Unit vector from the suspension mount towards the road, in part axes.
    pub suspension_direction: DVec3,
    /// Unit rolling direction at zero steer, perpendicular to suspension_direction.
    pub forward: DVec3,
    pub radius_meters: f64,
    pub rest_length_meters: f64,
    pub travel_meters: f64,
    pub spring_newtons_per_meter: f64,
    pub damping_newton_seconds_per_meter: f64,
    pub wheel_inertia_kg_m2: f64,
    /// Unlimited-energy motor in this first physical milestone; zero is passive landing gear.
    pub drive_torque_nm: f64,
    pub brake_torque_nm: f64,
    pub friction_coefficient: f64,
    pub max_steer_radians: f64,
    /// Finite-rate ideal steering servo; energy budgeting is deferred.
    #[serde(default = "default_steer_rate")]
    pub max_steer_rate_radians_per_second: f64,
}
fn default_steer_rate() -> f64 {
    1.5
}
impl WheelDefinition {
    pub fn validate(&self) -> Result<(), String> {
        if !self.suspension_origin.is_finite()
            || !self.suspension_direction.is_finite()
            || !self.forward.is_finite()
            || (self.suspension_direction.length_squared() - 1.0).abs() > 1e-9
            || (self.forward.length_squared() - 1.0).abs() > 1e-9
            || self.forward.dot(self.suspension_direction).abs() > 1e-9
        {
            return Err("wheel axes must be finite, unit and perpendicular".into());
        }
        let positive = [
            self.radius_meters,
            self.rest_length_meters,
            self.travel_meters,
            self.spring_newtons_per_meter,
            self.damping_newton_seconds_per_meter,
            self.wheel_inertia_kg_m2,
            self.friction_coefficient,
            self.max_steer_rate_radians_per_second,
        ];
        if positive.iter().any(|v| !v.is_finite() || *v <= 0.0)
            || [
                self.drive_torque_nm,
                self.brake_torque_nm,
                self.max_steer_radians,
            ]
            .iter()
            .any(|v| !v.is_finite() || *v < 0.0)
            || self.max_steer_radians >= std::f64::consts::FRAC_PI_2
            || self.travel_meters > self.rest_length_meters
        {
            return Err("invalid wheel dimensions, suspension or actuator parameters".into());
        }
        Ok(())
    }
    pub fn initial(&self) -> WheelState {
        WheelState {
            steer_radians: 0.0,
            spin_radians: 0.0,
            spin_radians_per_second: 0.0,
            suspension_length_meters: self.rest_length_meters + self.travel_meters,
            grounded: false,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WheelState {
    /// Accepted servo angle, required in saved state; positive means player right.
    pub steer_radians: f64,
    pub spin_radians: f64,
    pub spin_radians_per_second: f64,
    pub suspension_length_meters: f64,
    pub grounded: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VehicleControl {
    pub drive: f64,
    /// Positive turns to the player's right (negative rotation about authored up).
    pub steer: f64,
    pub brake: f64,
}
impl Default for VehicleControl {
    fn default() -> Self {
        Self {
            drive: 0.0,
            steer: 0.0,
            brake: 1.0,
        }
    }
}
impl VehicleControl {
    pub fn validate(self) {
        assert!(
            self.drive.is_finite()
                && self.drive.abs() <= 1.0
                && self.steer.is_finite()
                && self.steer.abs() <= 1.0
                && self.brake.is_finite()
                && (0.0..=1.0).contains(&self.brake),
            "invalid vehicle control"
        );
    }
}
/// Contact geometry and velocities in one contact frame, force at the road contact point.
#[derive(Clone, Copy, Debug)]
pub struct WheelContact {
    pub suspension_length_meters: f64,
    pub normal: DVec3,
    pub forward: DVec3,
    pub relative_point_velocity: DVec3,
    /// Inverse effective point mass of chassis plus contacted support in each direction.
    pub inverse_mass_normal: f64,
    pub inverse_mass_forward: f64,
    pub inverse_mass_side: f64,
}
/// Pure accepted-step wheel solve. Brakes oppose rotation; tire friction couples road and spin.
/// The friction circle is the authored Coulomb tire model, not a numerical force clamp.
pub fn step_wheel(
    d: &WheelDefinition,
    state: WheelState,
    control: VehicleControl,
    contact: Option<WheelContact>,
    dt: f64,
) -> (WheelState, DVec3, f64) {
    control.validate();
    assert!(dt.is_finite() && dt > 0.0);
    let mut next = state;
    next.steer_radians = wheel_steer_after(d, state, control, dt);
    let free_omega = state.spin_radians_per_second
        + control.drive * d.drive_torque_nm / d.wheel_inertia_kg_m2 * dt;
    let brake_limit = control.brake * d.brake_torque_nm;
    let brake_delta = brake_limit / d.wheel_inertia_kg_m2 * dt;
    let mut omega = free_omega.signum() * (free_omega.abs() - brake_delta).max(0.0);
    let mut force = DVec3::ZERO;
    next.grounded = false;
    next.suspension_length_meters = d.rest_length_meters + d.travel_meters;
    if let Some(c) = contact {
        assert!(
            c.normal.is_finite()
                && c.forward.is_finite()
                && (c.normal.length_squared() - 1.0).abs() < 1e-9
                && (c.forward.length_squared() - 1.0).abs() < 1e-9
                && c.normal.dot(c.forward).abs() < 1e-9
                && c.suspension_length_meters.is_finite()
                && c.suspension_length_meters >= 0.0
                && c.suspension_length_meters <= d.rest_length_meters + d.travel_meters
                && c.relative_point_velocity.is_finite()
                && [
                    c.inverse_mass_normal,
                    c.inverse_mass_forward,
                    c.inverse_mass_side
                ]
                .iter()
                .all(|m| m.is_finite() && *m > 0.0)
        );
        next.grounded = true;
        next.suspension_length_meters = c.suspension_length_meters;
        let compression = d.rest_length_meters - c.suspension_length_meters;
        // Backward damping uses effective mass, so a heavy damper cannot inject energy.
        let normal_force = ((d.spring_newtons_per_meter * compression
            - d.damping_newton_seconds_per_meter * c.relative_point_velocity.dot(c.normal))
            / (1.0 + d.damping_newton_seconds_per_meter * dt * c.inverse_mass_normal))
            .max(0.0);
        let side = c.normal.cross(c.forward).normalize();
        let road_speed = c.relative_point_velocity.dot(c.forward);
        // Solve brake and road impulse together. A locked tire holds until either brake torque
        // or friction saturates; sequentially stopping spin would lose the road reaction.
        let locked_force = -road_speed / (dt * c.inverse_mass_forward);
        let lock_brake = d.radius_meters * locked_force - d.wheel_inertia_kg_m2 * free_omega / dt;
        let brake_torque = if lock_brake.abs() <= brake_limit {
            lock_brake
        } else {
            lock_brake.signum() * brake_limit
        };
        let braked_omega = free_omega + brake_torque * dt / d.wheel_inertia_kg_m2;
        let slip = braked_omega * d.radius_meters - road_speed;
        let forward_force = slip
            / (dt * (c.inverse_mass_forward + d.radius_meters.powi(2) / d.wheel_inertia_kg_m2));
        let side_force = -c.relative_point_velocity.dot(side) / (dt * c.inverse_mass_side);
        let demand = forward_force.hypot(side_force);
        let friction = d.friction_coefficient * normal_force;
        let scale = if demand > friction && demand > 0.0 {
            friction / demand
        } else {
            1.0
        };
        let longitudinal = forward_force * scale;
        force = c.normal * normal_force + c.forward * longitudinal + side * side_force * scale;
        omega = braked_omega - longitudinal * d.radius_meters / d.wheel_inertia_kg_m2 * dt;
        if lock_brake.abs() <= brake_limit && scale == 1.0 {
            // The solved static brake branch has exactly zero angular velocity.
            omega = 0.0;
        }
    }
    next.spin_radians_per_second = omega;
    next.spin_radians = (state.spin_radians + omega * dt).rem_euclid(std::f64::consts::TAU);
    assert!(
        force.is_finite() && omega.is_finite(),
        "non-finite wheel solve"
    );
    // Applied alongside the road-point force about the chassis COM. This subtracts the
    // spin angular momentum credited to the virtual wheel, including motor/brake reaction.
    let axle_reaction_nm = -d.wheel_inertia_kg_m2 * (omega - state.spin_radians_per_second) / dt;
    (next, force, axle_reaction_nm)
}

/// Pure accepted servo evaluation, shared by contact geometry and airborne rotation.
pub fn wheel_steer_after(
    d: &WheelDefinition,
    state: WheelState,
    control: VehicleControl,
    dt: f64,
) -> f64 {
    control.validate();
    assert!(dt.is_finite() && dt > 0.0);
    let target = control.steer * d.max_steer_radians;
    let remaining = target - state.steer_radians;
    let travel = d.max_steer_rate_radians_per_second * dt;
    if remaining.abs() <= travel {
        target
    } else {
        state.steer_radians + remaining.signum() * travel
    }
}
/// Authored physical rotor axle in part coordinates, independent of the road's normal.
pub fn wheel_axle(d: &WheelDefinition, steer_radians: f64) -> DVec3 {
    let rolling = DQuat::from_axis_angle(d.suspension_direction, steer_radians) * d.forward;
    (-d.suspension_direction).cross(rolling)
}
