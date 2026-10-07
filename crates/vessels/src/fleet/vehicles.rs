//! Wheel mechanics in the existing contact owner, with reciprocal support wrenches.
use super::*;
use void_assembly::{ModuleState, VehicleControl, WheelContact};

#[derive(Default)]
pub(super) struct WheelLoads {
    /// Contact-frame force and torque about each body's COM.
    pub bodies: HashMap<RigidBodyHandle, (DVec3, DVec3)>,
    pub updates: Vec<(String, String, ModuleState)>,
    pub wake: HashSet<RigidBodyHandle>,
}
/// One tire evaluated from the accepted boundary, with immutable live-collider geometry.
struct TireConstraint<'a> {
    part: String,
    module: String,
    body: RigidBodyHandle,
    support: Option<RigidBodyHandle>,
    point: DVec3,
    com: DVec3,
    axle: DVec3,
    definition: &'a void_assembly::WheelDefinition,
    initial: void_assembly::WheelState,
    control: VehicleControl,
    actuator: VehicleControl,
    contact: Option<WheelContact>,
    previous_wrench: (DVec3, DVec3),
    accepted: void_assembly::WheelState,
}
impl Fleet {
    /// Root command owns the selected vessel's operator profile, including docked mixed craft.
    pub fn control_profile(&self, vessel: &str) -> Option<void_assembly::ControlProfile> {
        let v = self.vessel(vessel);
        self.parts
            .part(&v.root)
            .definition
            .modules
            .iter()
            .find_map(|m| match m {
                Module::Command {
                    control_profile, ..
                } => Some(*control_profile),
                _ => None,
            })
    }

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
        if !self.has_command(vessel) {
            return Err("vehicle command unavailable: required healthy crew missing or command thermally failed".into());
        }
        if !self.has_wheels(vessel) {
            return Err("selected vessel has no wheels".into());
        }
        let changed = self.vehicle_control(vessel) != Some(control);
        let owner = self.vessel(vessel).owner.clone();
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
        if changed && let Owner::Scene { scene, body, .. } = owner {
            self.scenes.get_mut(&scene).unwrap().world.world.bodies[body].wake_up(true);
        }
        Ok(())
    }
    pub(super) fn wheel_loads(
        &self,
        scene: u64,
        external_acceleration: &HashMap<RigidBodyHandle, DVec3>,
    ) -> WheelLoads {
        let s = &self.scenes[&scene];
        let world = &s.world;
        let dt = self.options.step_seconds;
        let mut loads = WheelLoads::default();
        let mut tires = Vec::new();
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
                    let steer = DQuat::from_axis_angle(down, control.steer * d.max_steer_radians);
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
                                - h.point_velocity,
                            inverse_mass_normal: effective(h.normal),
                            inverse_mass_forward: effective(forward),
                            inverse_mass_side: effective(side),
                        })
                    });
                    let actuator = if part.thermally_failed() {
                        VehicleControl {
                            drive: 0.0,
                            steer: control.steer,
                            brake: 0.0,
                        }
                    } else if !self.has_command(id) {
                        // A latched mechanical parking brake stays set when the crew leaves.
                        VehicleControl {
                            drive: 0.0,
                            steer: control.steer,
                            brake: control.brake,
                        }
                    } else {
                        control
                    };
                    let moving_support = hit.and_then(|h| h.body).is_some_and(|support| {
                        let b = world.body(support);
                        !b.is_sleeping()
                            && (vec64(b.linvel()) != DVec3::ZERO
                                || vec64(b.angvel()) != DVec3::ZERO)
                    });
                    if actuator.drive != 0.0
                        || state.spin_radians_per_second != 0.0
                        || moving_support
                    {
                        loads.wake.insert(body);
                        if let Some(support) = hit
                            .and_then(|h| h.body)
                            .filter(|b| world.body(*b).is_dynamic())
                        {
                            loads.wake.insert(support);
                        }
                    }
                    let point = hit.map_or(origin, |h| h.point);
                    let support = hit
                        .and_then(|h| h.body)
                        .filter(|b| world.body(*b).is_dynamic());
                    let axle =
                        contact.map_or((-down).cross(rolling), |c| c.normal.cross(c.forward));
                    let acceleration = world.frame.acceleration(
                        &*self.ephemeris,
                        self.time,
                        world.state(&*self.ephemeris, body, push).position,
                        accepted,
                    );
                    let support_acceleration = support.map_or(DVec3::ZERO, |handle| {
                        let sv = s
                            .members
                            .iter()
                            .find_map(|sid| match self.vessel(sid).owner {
                                Owner::Scene { body: h, push, .. } if h == handle => {
                                    Some(world.state(&*self.ephemeris, h, push))
                                }
                                _ => None,
                            })
                            .expect("dynamic support vessel");
                        world.frame.acceleration(
                            &*self.ephemeris,
                            self.time,
                            sv.position,
                            sv.velocity,
                        )
                    });
                    let contact = contact.map(|mut c| {
                        c.relative_point_velocity += (acceleration + external_acceleration[&body]
                            - support_acceleration
                            - support.map_or(DVec3::ZERO, |h| external_acceleration[&h]))
                            * dt;
                        c
                    });
                    tires.push(TireConstraint {
                        part: pid.clone(),
                        module: mid.clone(),
                        body,
                        support,
                        point,
                        com,
                        axle,
                        definition: d,
                        initial: state,
                        control,
                        actuator,
                        contact,
                        previous_wrench: (DVec3::ZERO, DVec3::ZERO),
                        accepted: state,
                    });
                }
            }
        }
        // Projected Gauss-Seidel on frozen geometry. Each visit replaces its previous
        // wrench; only the final batch is applied to Rapier and committed to PartGraph.
        // This couples all support/brake contacts instead of biasing the last wheel.
        for _ in 0..32 {
            for tire in &mut tires {
                let TireConstraint {
                    body,
                    support,
                    point,
                    com,
                    axle,
                    definition: d,
                    initial: state,
                    actuator,
                    contact: base,
                    previous_wrench: previous,
                    accepted: next,
                    ..
                } = tire;
                let predicted = |h| {
                    loads.bodies.get(&h).map_or(DVec3::ZERO, |(f, t)| {
                        world.impulse_point_velocity_delta(h, *f * dt, *t * dt, *point)
                    })
                };
                let contact = base.map(|mut c| {
                    c.relative_point_velocity +=
                        predicted(*body) - support.map_or(DVec3::ZERO, predicted);
                    let side = c.normal.cross(c.forward);
                    // The local wheel kernel handles its own diagonal effective masses.
                    // Keep cross-axis response in the predictor, including its axle reaction.
                    c.relative_point_velocity -= dt
                        * (c.normal * previous.0.dot(c.normal) * c.inverse_mass_normal
                            + c.forward * previous.0.dot(c.forward) * c.inverse_mass_forward
                            + side * previous.0.dot(side) * c.inverse_mass_side);
                    c
                });
                let entry = loads.bodies.entry(*body).or_default();
                entry.0 -= previous.0;
                entry.1 -= previous.1;
                if let Some(h) = support {
                    let entry = loads.bodies.entry(*h).or_default();
                    entry.0 += previous.0;
                    let support_com = world.origin + vec64(world.body(*h).center_of_mass());
                    entry.1 -= (*point - support_com).cross(-previous.0);
                }
                let (updated, force, reaction) =
                    void_assembly::step_wheel(d, *state, *actuator, contact, dt);
                *next = updated;
                let torque = (*point - *com).cross(force) + *axle * reaction;
                let entry = loads.bodies.entry(*body).or_default();
                entry.0 += force;
                entry.1 += torque;
                if let Some(h) = support {
                    let support_com = world.origin + vec64(world.body(*h).center_of_mass());
                    let entry = loads.bodies.entry(*h).or_default();
                    entry.0 -= force;
                    entry.1 += (*point - support_com).cross(-force);
                }
                *previous = (force, torque);
            }
        }
        for tire in tires {
            loads.updates.push((
                tire.part,
                tire.module,
                ModuleState::Wheel {
                    state: tire.accepted,
                    control: tire.control,
                },
            ));
        }
        loads
    }
}
