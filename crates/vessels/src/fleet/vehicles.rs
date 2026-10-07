//! Wheel mechanics in the existing contact owner, with reciprocal support wrenches.
use super::*;
use void_assembly::{ModuleState, VehicleControl, WheelContact};

#[derive(Default)]
pub(super) struct WheelLoads {
    /// Contact-frame force and torque about each body's COM.
    pub bodies: HashMap<RigidBodyHandle, (DVec3, DVec3)>,
    pub updates: Vec<(String, String, ModuleState)>,
}
impl Fleet {
    pub fn has_wheels(&self, vessel: &str) -> bool {
        self.vessel(vessel).members.iter().any(|id| {
            self.parts
                .part(id)
                .definition
                .modules
                .iter()
                .any(|m| matches!(m, Module::Wheel { .. }))
        })
    }
    pub fn vehicle_control(&self, vessel: &str) -> Option<VehicleControl> {
        self.vessel(vessel).members.iter().find_map(|pid| {
            self.parts.part(pid).modules.values().find_map(|m| match m {
                ModuleState::Wheel { control, .. } => Some(*control),
                _ => None,
            })
        })
    }
    pub fn set_vehicle_control(
        &mut self,
        vessel: &str,
        control: VehicleControl,
    ) -> Result<(), String> {
        control.validate();
        if !self.has_wheels(vessel) {
            return Err("selected vessel has no wheels".into());
        }
        let updates: Vec<_> = self
            .vessel(vessel)
            .members
            .iter()
            .flat_map(|pid| {
                self.parts
                    .part(pid)
                    .modules
                    .iter()
                    .filter_map(move |(mid, m)| match m {
                        ModuleState::Wheel { state, .. } => Some((
                            pid.clone(),
                            mid.clone(),
                            ModuleState::Wheel {
                                state: *state,
                                control,
                            },
                        )),
                        _ => None,
                    })
            })
            .collect();
        for (pid, mid, state) in updates {
            self.parts.set_module_state(&pid, &mid, state);
        }
        Ok(())
    }
    pub(super) fn wheel_loads(&self, scene: u64) -> WheelLoads {
        let s = &self.scenes[&scene];
        let world = &s.world;
        let dt = self.options.step_seconds;
        let mut loads = WheelLoads::default();
        for id in &s.members {
            let v = self.vessel(id);
            let Owner::Scene { body, push, .. } = v.owner else {
                unreachable!()
            };
            let b = world.body(body);
            let q = quat64(*b.rotation());
            let translation = world.position(body);
            let com = world.origin + vec64(b.center_of_mass());
            let accepted = world.state(&*self.ephemeris, body, push).velocity;
            let half_correction = accepted - vec64(b.linvel());
            for pid in &v.members {
                let part = self.parts.part(pid);
                for module in &part.definition.modules {
                    let Module::Wheel {
                        id: mid,
                        parameters: d,
                    } = module
                    else {
                        continue;
                    };
                    let ModuleState::Wheel { state, control } = part.modules[mid] else {
                        panic!("wheel state mismatch")
                    };
                    let rotation = q * part.pose.rotation;
                    let down = rotation * d.suspension_direction;
                    let origin = translation
                        + q * (part.pose.position + part.pose.rotation * d.suspension_origin);
                    let steer = DQuat::from_axis_angle(-down, control.steer * d.max_steer_radians);
                    let rolling = steer * (rotation * d.forward);
                    let hit = world.suspension_ray(
                        body,
                        origin,
                        down,
                        d.rest_length_meters + d.travel_meters + d.radius_meters,
                    );
                    let contact = hit.and_then(|h| {
                        let length = h.distance_meters - d.radius_meters;
                        if length < 0.0 {
                            return None;
                        }
                        let projected = rolling - h.normal * rolling.dot(h.normal);
                        if projected.length_squared() < 1e-12 {
                            return None;
                        }
                        let forward = projected.normalize();
                        let side = h.normal.cross(forward);
                        let support_correction = h.body.map_or(DVec3::ZERO, |support| {
                            s.members
                                .iter()
                                .find_map(|sid| match self.vessel(sid).owner {
                                    Owner::Scene { body: b, push, .. } if b == support => Some(
                                        world.state(&*self.ephemeris, b, push).velocity
                                            - vec64(world.body(b).linvel()),
                                    ),
                                    _ => None,
                                })
                                .unwrap_or(DVec3::ZERO)
                        });
                        let effective = |axis| {
                            world.inverse_point_mass(body, h.point, axis)
                                + h.body.map_or(0.0, |support| {
                                    world.inverse_point_mass(support, h.point, axis)
                                })
                        };
                        Some(WheelContact {
                            suspension_length_meters: length,
                            normal: h.normal,
                            forward,
                            relative_point_velocity: world.point_velocity(body, h.point)
                                + half_correction
                                - h.point_velocity
                                - support_correction
                                + loads
                                    .bodies
                                    .get(&body)
                                    .map_or(DVec3::ZERO, |(force, torque)| {
                                        world.impulse_point_velocity_delta(
                                            body,
                                            *force * dt,
                                            *torque * dt,
                                            h.point,
                                        )
                                    })
                                - h.body
                                    .and_then(|support| {
                                        loads.bodies.get(&support).map(|(force, torque)| {
                                            world.impulse_point_velocity_delta(
                                                support,
                                                *force * dt,
                                                *torque * dt,
                                                h.point,
                                            )
                                        })
                                    })
                                    .unwrap_or(DVec3::ZERO),
                            inverse_mass_normal: effective(h.normal),
                            inverse_mass_forward: effective(forward),
                            inverse_mass_side: effective(side),
                        })
                    });
                    let (next, force, axle_torque) =
                        void_assembly::step_wheel(d, state, control, contact, dt);
                    // Positive spin is about normal cross forward. In the air use authored axle.
                    let axle =
                        contact.map_or((-down).cross(rolling), |c| c.normal.cross(c.forward));
                    let point = hit.map_or(origin, |h| h.point);
                    let torque = (point - com).cross(force) + axle * axle_torque;
                    let entry = loads.bodies.entry(body).or_default();
                    entry.0 += force;
                    entry.1 += torque;
                    if let Some(support) = hit
                        .and_then(|h| h.body)
                        .filter(|b| world.body(*b).is_dynamic())
                    {
                        let support_com =
                            world.origin + vec64(world.body(support).center_of_mass());
                        let entry = loads.bodies.entry(support).or_default();
                        entry.0 -= force;
                        entry.1 += (point - support_com).cross(-force);
                    }
                    loads.updates.push((
                        pid.clone(),
                        mid.clone(),
                        ModuleState::Wheel {
                            state: next,
                            control,
                        },
                    ));
                }
            }
        }
        loads
    }
}
