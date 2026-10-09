//! Body and wing forces, as the lab's `Aero.ts`: engineering coefficients evaluated at each
//! element's own airflow, not CFD or a voxel flow solution.

use glam::{DQuat, DVec3};

use crate::{
    Air, finite, finite_vec, inverse, length, positive, rotate, smooth, validate_air,
    validate_rotation,
};

#[derive(Clone, Debug, PartialEq)]
pub struct BodyAero {
    pub axis: DVec3,
    pub front_area: f64,
    pub rear_area: f64,
    pub side_area: f64,
    pub wet_area: f64,
    pub length_meters: f64,
    pub front_cd: f64,
    pub rear_cd: f64,
    pub side_cd: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlSurface {
    None,
    Elevator,
    Aileron,
    Rudder,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WingAero {
    pub chord: DVec3,
    pub normal: DVec3,
    pub area: f64,
    pub aspect_ratio: f64,
    pub chord_meters: f64,
    pub sweep_radians: f64,
    pub incidence_radians: f64,
    pub zero_lift_radians: f64,
    pub stall_radians: f64,
    pub cd0: f64,
    pub efficiency: f64,
    pub pitching_moment: f64,
    pub control: ControlSurface,
    pub control_sign: f64,
    pub max_deflection_radians: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum AeroShape {
    Body(BodyAero),
    Wing(WingAero),
}

/// Control surface commands, each in [−1, 1].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Controls {
    pub elevator: f64,
    pub aileron: f64,
    pub rudder: f64,
}

pub const NEUTRAL: Controls = Controls {
    elevator: 0.0,
    aileron: 0.0,
    rudder: 0.0,
};

impl Controls {
    fn deflection(&self, surface: ControlSurface) -> f64 {
        match surface {
            ControlSurface::None => unreachable!("a fixed surface has no command"),
            ControlSurface::Elevator => self.elevator,
            ControlSurface::Aileron => self.aileron,
            ControlSurface::Rudder => self.rudder,
        }
    }
}

/// The rigid body the elements belong to: centre of mass, its velocity, attitude (local to world)
/// and angular velocity in world axes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AeroState {
    pub center: DVec3,
    pub velocity: DVec3,
    pub rotation: DQuat,
    pub angular_velocity: DVec3,
}

/// An aerodynamic element at `point`, in body axes relative to the centre of mass.
#[derive(Clone, Debug, PartialEq)]
pub struct AeroElement {
    pub id: String,
    pub point: DVec3,
    pub shape: AeroShape,
}

/// One element's load, in world axes; `point` is where it acts.
#[derive(Clone, Debug, PartialEq)]
pub struct ElementForce {
    pub id: String,
    pub point: DVec3,
    pub force: DVec3,
    pub moment: DVec3,
    pub drag: DVec3,
    pub lift: DVec3,
    pub speed: f64,
    pub q_pa: f64,
    pub mach: f64,
    pub alpha_radians: f64,
    pub stall: f64,
    pub cl: f64,
    pub cd: f64,
}

/// Total force and torque about the centre of mass, world axes, with the free-stream values.
#[derive(Clone, Debug, PartialEq)]
pub struct AeroForces {
    pub force: DVec3,
    pub torque: DVec3,
    pub elements: Vec<ElementForce>,
    pub q_pa: f64,
    pub speed: f64,
    pub mach: f64,
}

fn unit_vector(v: DVec3, label: &str) {
    finite_vec(v, label);
    assert!(
        (length(v) - 1.0).abs() <= 1e-8,
        "{label} must be a unit vector"
    );
}

pub fn validate_shape(shape: &AeroShape) {
    match shape {
        AeroShape::Body(s) => {
            unit_vector(s.axis, "Body axis");
            for (value, key) in [
                (s.front_area, "frontArea"),
                (s.rear_area, "rearArea"),
                (s.side_area, "sideArea"),
                (s.wet_area, "wetArea"),
                (s.front_cd, "frontCd"),
                (s.rear_cd, "rearCd"),
                (s.side_cd, "sideCd"),
            ] {
                finite(value, key);
                assert!(value >= 0.0, "negative {key}");
            }
            positive(s.length_meters, "body length");
        }
        AeroShape::Wing(s) => {
            finite_vec(s.chord, "wing chord");
            finite_vec(s.normal, "wing normal");
            for (value, key) in [
                (s.area, "area"),
                (s.aspect_ratio, "aspectRatio"),
                (s.chord_meters, "chordMeters"),
                (s.stall_radians, "stallRadians"),
                (s.efficiency, "efficiency"),
            ] {
                positive(value, key);
            }
            assert!(
                (length(s.chord) - 1.0).abs() <= 1e-8
                    && (length(s.normal) - 1.0).abs() <= 1e-8
                    && s.chord.dot(s.normal).abs() <= 1e-8,
                "Wing axes must be orthonormal"
            );
            for (value, key) in [
                (s.sweep_radians, "sweepRadians"),
                (s.incidence_radians, "incidenceRadians"),
                (s.zero_lift_radians, "zeroLiftRadians"),
                (s.cd0, "cd0"),
                (s.pitching_moment, "pitchingMoment"),
                (s.control_sign, "controlSign"),
                (s.max_deflection_radians, "maxDeflectionRadians"),
            ] {
                finite(value, key);
            }
            assert!(
                !(s.cd0 < 0.0
                    || s.efficiency > 1.0
                    || s.stall_radians >= std::f64::consts::FRAC_PI_2
                    || s.sweep_radians.abs() >= std::f64::consts::FRAC_PI_2
                    || s.max_deflection_radians < 0.0),
                "Invalid wing parameters"
            );
        }
    }
}

/// A wing section's coefficients at angle of attack `alpha` and `mach`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Polar {
    pub cl: f64,
    pub cd: f64,
    pub stall: f64,
}

/// Continuous all-angle engineering polar: finite-span lift slope, smooth separation, induced
/// drag and a bounded transonic drag rise.
pub fn wing_polar(s: &WingAero, alpha: f64, mach: f64) -> Polar {
    finite(alpha, "alpha");
    finite(mach, "Mach");
    assert!(mach >= 0.0, "Negative Mach");
    let a = f64::atan2(
        f64::sin(alpha - s.zero_lift_radians),
        f64::cos(alpha - s.zero_lift_radians),
    );
    let stall = smooth(s.stall_radians * 0.8, s.stall_radians * 1.35, a.abs());
    let slope = 2.0 * std::f64::consts::PI * s.aspect_ratio / (s.aspect_ratio + 2.0 / s.efficiency)
        * f64::cos(s.sweep_radians);
    let cl_attached = slope * f64::sin(a) * f64::cos(a);
    let cl_separated = 1.05 * f64::sin(2.0 * a);
    let compressibility = 1.0 + 0.18 * f64::exp(-f64::powf((mach - 0.85) / 0.28, 2.0));
    let supersonic = 1.0 / (1.0 + 0.0_f64.max(mach * mach - 1.0)).sqrt();
    let cl = ((1.0 - stall) * cl_attached + stall * cl_separated) * compressibility * supersonic;
    let wave = 0.12 * smooth(0.65, 1.15, mach) / (1.0 + 0.0_f64.max(mach - 1.15) * 0.3);
    let cd = s.cd0
        + cl * cl / (std::f64::consts::PI * s.efficiency * s.aspect_ratio)
        + 1.8 * stall * f64::powf(f64::sin(a), 2.0)
        + wave;
    Polar { cl, cd, stall }
}

/// Forces on `elements` of a body in `state`. Each element sees its own airflow, the point
/// velocity v + ω × r minus the wind, so rotation is damped by the air itself. No mutable state.
pub fn aerodynamic_forces(
    elements: &[AeroElement],
    state: &AeroState,
    air: &Air,
    wind: DVec3,
    controls: &Controls,
) -> AeroForces {
    for v in [state.center, state.velocity, state.angular_velocity, wind] {
        finite_vec(v, "aerodynamic vector");
    }
    validate_rotation(state.rotation);
    validate_air(air);
    for c in [controls.elevator, controls.aileron, controls.rudder] {
        assert!(
            c.is_finite() && c.abs() <= 1.0,
            "Invalid control surface command"
        );
    }
    let base_speed = finite(length(state.velocity - wind), "airspeed");
    let q_pa = finite(
        0.5 * air.density * f64::powf(base_speed, 2.0),
        "dynamic pressure",
    );
    let mach = finite(
        if air.density > 0.0 {
            base_speed / positive(air.sound_speed, "sound speed")
        } else {
            0.0
        },
        "Mach",
    );
    let mut result = AeroForces {
        force: DVec3::ZERO,
        torque: DVec3::ZERO,
        elements: Vec::with_capacity(elements.len()),
        q_pa,
        speed: base_speed,
        mach,
    };
    let world_to_local = inverse(state.rotation);
    for element in elements {
        finite_vec(element.point, "aerodynamic vector");
        validate_shape(&element.shape);
        let arm = rotate(state.rotation, element.point);
        let point = state.center + arm;
        let v = state.velocity + state.angular_velocity.cross(arm) - wind;
        let speed = length(v);
        let q_pa = 0.5 * air.density * speed * speed;
        let mach = if air.density > 0.0 {
            speed / air.sound_speed
        } else {
            0.0
        };
        let (mut local_force, mut local_moment) = (DVec3::ZERO, DVec3::ZERO);
        let (mut alpha_radians, mut stall, mut cl, mut cd) = (0.0, 0.0, 0.0, 0.0);
        if speed > 0.0 && air.density > 0.0 {
            let local = rotate(world_to_local, v);
            let vhat = local * (1.0 / speed);
            match &element.shape {
                AeroShape::Body(s) => {
                    let axial = local.dot(s.axis);
                    let side = local - s.axis * axial;
                    let side_speed = length(side);
                    let reynolds = air.density * speed * s.length_meters
                        / positive(air.viscosity, "viscosity");
                    let cf = 0.074 / f64::powf(reynolds.max(1.0), 0.2);
                    let cd_axial = (if axial >= 0.0 { s.front_cd } else { s.rear_cd })
                        * (1.0 + 0.7 * f64::exp(-f64::powf((mach - 1.1) / 0.4, 2.0)));
                    let area = if axial >= 0.0 {
                        s.front_area
                    } else {
                        s.rear_area
                    };
                    local_force = s.axis
                        * (-0.5 * air.density * area * cd_axial * axial * axial.abs())
                        + side * (-0.5 * air.density * s.side_area * s.side_cd * side_speed)
                        + vhat * (-q_pa * s.wet_area * cf);
                    cd = -local_force.dot(vhat)
                        / (q_pa * s.front_area.max(s.rear_area).max(s.side_area).max(1e-12));
                    alpha_radians = f64::atan2(side_speed, axial);
                }
                AeroShape::Wing(s) => {
                    let span = s.normal.cross(s.chord);
                    let section_velocity = local - span * local.dot(span);
                    let section_speed = length(section_velocity);
                    local_force = vhat * (-q_pa * s.area * s.cd0);
                    cd = s.cd0;
                    if section_speed > 0.0 {
                        let section_q = 0.5 * air.density * f64::powf(section_speed, 2.0);
                        let deflection = if s.control == ControlSurface::None {
                            0.0
                        } else {
                            controls.deflection(s.control)
                                * s.control_sign
                                * s.max_deflection_radians
                        };
                        alpha_radians = f64::atan2(
                            -section_velocity.dot(s.normal),
                            section_velocity.dot(s.chord),
                        ) + s.incidence_radians
                            + deflection;
                        let polar = wing_polar(s, alpha_radians, mach);
                        (cl, cd, stall) = (polar.cl, polar.cd, polar.stall);
                        let projected_normal = s.normal - vhat * s.normal.dot(vhat);
                        let n = length(projected_normal);
                        // Exactly normal incidence has no perpendicular lift direction;
                        // separated pressure drag remains.
                        let lift = if n > 1e-12 {
                            projected_normal * (section_q * s.area * cl / n)
                        } else {
                            DVec3::ZERO
                        };
                        local_force =
                            lift + vhat * (-s.area * (q_pa * s.cd0 + section_q * (cd - s.cd0)));
                        local_moment = span
                            * (section_q
                                * s.area
                                * s.chord_meters
                                * s.pitching_moment
                                * (1.0 - stall));
                    }
                }
            }
        }
        let force = rotate(state.rotation, local_force);
        let moment = rotate(state.rotation, local_moment);
        let drag = if speed > 0.0 {
            v * (force.dot(v) / (speed * speed))
        } else {
            DVec3::ZERO
        };
        let lift = force - drag;
        result.force += force;
        result.torque += arm.cross(force) + moment;
        result.elements.push(ElementForce {
            id: element.id.clone(),
            point,
            force,
            moment,
            drag,
            lift,
            speed,
            q_pa,
            mach,
            alpha_radians,
            stall,
            cl,
            cd,
        });
    }
    for x in result
        .force
        .to_array()
        .into_iter()
        .chain(result.torque.to_array())
    {
        finite(x, "aerodynamic output");
    }
    result
}

/// A heat-shield disk.
#[derive(Clone, Debug, PartialEq)]
pub struct DiskShield {
    pub id: String,
    pub point: DVec3,
    pub normal: DVec3,
    pub radius_meters: f64,
}

/// Whether authored shield disks (other than `own_id`'s) block the direct air from
/// `toward_incoming_air` at `point`. Not general mesh occlusion.
pub fn shielded(
    point: DVec3,
    toward_incoming_air: DVec3,
    shields: &[DiskShield],
    own_id: &str,
) -> bool {
    for shield in shields {
        if shield.id == own_id {
            continue;
        }
        let denominator = toward_incoming_air.dot(shield.normal);
        if denominator.abs() < 1e-9 {
            continue;
        }
        let t = (shield.point - point).dot(shield.normal) / denominator;
        if t <= 1e-6 {
            continue;
        }
        let hit = point + toward_incoming_air * t - shield.point;
        if length(hit) < shield.radius_meters {
            return true;
        }
    }
    false
}
