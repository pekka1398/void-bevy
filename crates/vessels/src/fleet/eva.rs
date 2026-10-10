//! Crew transfer and dynamic EVA in the existing PartGraph and contact owners.
use super::*;
use void_assembly::{CrewRecord, EvaControl, ModuleState, ResourceId, SeatDefinition};
use void_frames::State;
#[derive(Clone, Debug)]
pub struct CrewSeat {
    pub part: String,
    pub module: String,
    pub occupant: Option<CrewRecord>,
    pub parameters: SeatDefinition,
    pub packed_suit_thermal: Option<void_assembly::PartThermalState>,
}
impl Fleet {
    pub fn crew_seats(&self, vessel: &str) -> Vec<CrewSeat> {
        self.vessel(vessel)
            .members
            .iter()
            .flat_map(|pid| {
                let part = self.parts.part(pid);
                part.definition.modules.iter().filter_map(move |m| match m {
                    Module::Seat { id, parameters } => {
                        let ModuleState::Seat {
                            occupant,
                            packed_suit_thermal,
                        } = part.modules.get(id).expect("seat state")
                        else {
                            panic!("seat module mismatch")
                        };
                        Some(CrewSeat {
                            part: pid.clone(),
                            module: id.clone(),
                            occupant: occupant.clone(),
                            parameters: parameters.clone(),
                            packed_suit_thermal: *packed_suit_thermal,
                        })
                    }
                    _ => None,
                })
            })
            .collect()
    }
    pub fn eva_crew(&self, vessel: &str) -> Option<CrewRecord> {
        self.vessel(vessel).members.iter().find_map(|pid| {
            self.parts.part(pid).modules.values().find_map(|m| match m {
                ModuleState::Crew { crew, .. } => Some(crew.clone()),
                _ => None,
            })
        })
    }
    pub fn eva_control(&self, vessel: &str) -> Option<EvaControl> {
        self.vessel(vessel).members.iter().find_map(|pid| {
            self.parts.part(pid).modules.values().find_map(|m| match m {
                ModuleState::Crew { control, .. } => Some(*control),
                _ => None,
            })
        })
    }
    pub fn set_eva_control(&mut self, vessel: &str, control: EvaControl) -> Result<(), String> {
        control.validate();
        if !self.has_command(vessel) {
            return Err("EVA command capability unavailable or thermally failed".into());
        }
        let changed = self.eva_control(vessel) != Some(control);
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
                        ModuleState::Crew { crew, grounded, .. } => Some((
                            pid.clone(),
                            mid.clone(),
                            ModuleState::Crew {
                                crew: crew.clone(),
                                control,
                                grounded: *grounded,
                            },
                        )),
                        _ => None,
                    })
            })
            .collect();
        if updates.is_empty() {
            return Err("selected vessel is not EVA crew".into());
        }
        for (pid, mid, state) in updates {
            self.parts.set_module_state(&pid, &mid, state);
        }
        if changed && let Owner::Scene { scene, body, .. } = owner {
            self.scenes.get_mut(&scene).unwrap().world.world.bodies[body].wake_up(true);
        }
        Ok(())
    }
    fn rebuild_crew_owner(&mut self, id: &str, state: State, q: DQuat, w: DVec3) {
        let mut v = self.vessels.remove(id).expect("crew transaction vessel");
        let scene = match v.owner {
            Owner::Scene { scene, .. } => Some(scene),
            _ => None,
        };
        self.remove_scene_body(&v, true);
        self.recentre(&v.members);
        if let Some(scene) = scene {
            let local = self.scene_local(scene, state);
            let inv = self.axes(scene).conjugate();
            let local_w = inv * w - self.scenes[&scene].world.frame.spin();
            self.add_scene_body(&mut v, scene, local, inv * q, local_w, DVec3::ZERO);
        } else {
            v.owner = Owner::Orbit {
                run: Box::new(PropagationRun::new(VesselState {
                    time: self.time,
                    position: state.position,
                    velocity: state.velocity,
                    mass_kg: self.mass(&v.members),
                })),
                rotation: q,
                angular_velocity: w,
            };
        }
        self.put(v);
    }
    pub fn eva_exit(&mut self, part: &str, module: &str) -> Result<String, String> {
        let carrier = self.vessel_of_part(part);
        let previous = self.enter_vessel(&carrier);
        let result = self.eva_exit_local(part, module);
        self.restore_view(previous);
        result
    }
    fn eva_exit_local(&mut self, part: &str, module: &str) -> Result<String, String> {
        let vessel = self.vessel_of_part(part);
        let seat = self
            .crew_seats(&vessel)
            .into_iter()
            .find(|s| s.part == part && s.module == module)
            .ok_or("requested module is not a crew seat")?;
        let crew = seat.occupant.ok_or("seat has no crew")?;
        if self.parts.part(part).thermally_failed() {
            return Err("crew seat thermally failed".into());
        }
        let old = self.snapshot(&vessel);
        let old_i = self.inertia(&vessel);
        let r = DMat3::from_quat(old.rotation);
        let old_angular = r * old_i * r.transpose() * old.angular_velocity;
        let seat_pose = self.parts.part(part).pose;
        let hatch = old.position
            + old.rotation
                * (seat_pose.position + seat_pose.rotation * seat.parameters.hatch_position);
        let seat_q = old.rotation * seat_pose.rotation;
        let outward = seat.parameters.exit_direction - DVec3::Y * seat.parameters.exit_direction.y;
        // A vertical hatch preserves authored seat heading; side hatches face their outward normal.
        let suit_q = if outward.length_squared() > 1e-12 {
            seat_q * DQuat::from_rotation_arc(DVec3::Z, outward.normalize())
        } else {
            seat_q
        };
        let suit_def = void_assembly::definition("eva-suit").expect("EVA suit definition");
        if crew.suit_dry_mass_kg != suit_def.dry_mass_kg
            || crew.body_mass_kg != CrewRecord::pilot().body_mass_kg
        {
            return Err("crew dry masses do not match authored EVA suit".into());
        }
        if let Owner::Scene { scene, .. } = self.vessel(&vessel).owner {
            let inv = self.axes(scene).conjugate();
            let local = self.scene_local(
                scene,
                State {
                    position: hatch,
                    velocity: old.velocity,
                },
            );
            if self.scenes[&scene].world.box_overlaps(
                local.position,
                inv * suit_q,
                void_assembly::part_box_size(suit_def) / 2.0,
                None,
            ) {
                return Err("hatch endpoint obstructed by live collision geometry".into());
            }
        }
        let pack = self.parts.part(part).resource(ResourceId::Monopropellant);
        let mut craft = void_assembly::eva_suit();
        craft.parts[0]
            .resources
            .insert(ResourceId::Monopropellant, pack);
        let eva_mass = crew.carried_dry_mass_kg() + pack;
        let mother_mass = old.mass_kg - eva_mass;
        assert!(
            mother_mass > 0.0,
            "crew transaction removes entire carrier mass"
        );
        let offset = hatch - old.position;
        let eva_v = old.velocity + old.angular_velocity.cross(offset);
        let mother_offset = -offset * (eva_mass / mother_mass);
        let mother_v = old.velocity - (eva_v - old.velocity) * (eva_mass / mother_mass);
        // Clear manual carrier inputs at the handoff. Brakes remain physical, not a velocity reset.
        self.controls
            .insert(vessel.clone(), VesselControl::default());
        self.rcs_controls
            .insert(vessel.clone(), RcsControl::default());
        if let Some(previous) = self.vehicle_control(&vessel) {
            self.set_vehicle_control(
                &vessel,
                void_assembly::VehicleControl {
                    drive: 0.0,
                    steer: previous.steer,
                    brake: 1.0,
                },
            )
            .expect("occupied carrier wheel control");
        }
        self.parts.set_module_state(
            part,
            module,
            ModuleState::Seat {
                occupant: None,
                packed_suit_thermal: None,
            },
        );
        self.parts
            .set_resource(part, ResourceId::Monopropellant, 0.0);
        self.recentre(&self.vessel(&vessel).members.clone());
        let actor = self.launch(
            &craft,
            State {
                position: hatch,
                velocity: eva_v,
            },
            suit_q,
            old.angular_velocity,
        );
        let suit = self.vessel(&actor).root.clone();
        self.parts.set_module_state(
            &suit,
            "thermal1",
            ModuleState::Thermal {
                state: seat
                    .packed_suit_thermal
                    .expect("occupied seat has packed suit thermal state"),
            },
        );
        self.parts.set_module_state(
            &suit,
            "crew1",
            ModuleState::Crew {
                crew,
                control: EvaControl::default(),
                grounded: false,
            },
        );
        let ri = DMat3::from_quat(suit_q);
        let suit_i = self.inertia(&actor);
        let remaining = old_angular
            - ri * suit_i * ri.transpose() * old.angular_velocity
            - mother_offset.cross(mother_v - old.velocity) * mother_mass
            - offset.cross(eva_v - old.velocity) * eva_mass;
        let mother_i = self.inertia(&vessel);
        let mother_w = (r * mother_i * r.transpose()).inverse() * remaining;
        self.cancel_guidance(&vessel, "crew exited");
        self.sas.remove(&vessel);
        self.rebuild_crew_owner(
            &vessel,
            State {
                position: old.position + mother_offset,
                velocity: mother_v,
            },
            old.rotation,
            mother_w,
        );
        if let Some(scene) = old.scene {
            self.move_to(&actor, scene);
        }
        self.validate_crew_identities();
        Ok(actor)
    }
    pub fn board_eva(&mut self, actor: &str, part: &str, module: &str) -> Result<String, String> {
        let carrier = self.vessel_of_part(part);
        if self.vessel(actor).system != self.vessel(&carrier).system {
            return Err("EVA and hatch belong to different stellar systems".into());
        }
        let previous = self.enter_vessel(&carrier);
        let result = self.board_eva_local(actor, part, module);
        self.restore_view(previous);
        result
    }
    fn board_eva_local(&mut self, actor: &str, part: &str, module: &str) -> Result<String, String> {
        if !self.has_command(actor) {
            return Err("EVA command capability unavailable or thermally failed".into());
        }
        let crew = self
            .eva_crew(actor)
            .ok_or("selected vessel is not EVA crew")?;
        let carrier = self.vessel_of_part(part);
        if actor == carrier {
            return Err("crew already belongs to carrier".into());
        }
        let seat = self
            .crew_seats(&carrier)
            .into_iter()
            .find(|s| s.part == part && s.module == module)
            .ok_or("requested module is not a crew seat")?;
        if self.parts.part(part).thermally_failed() {
            return Err("crew seat thermally failed".into());
        }
        if seat.occupant.is_some() {
            return Err("seat occupied".into());
        }
        if self.vessel(actor).members.len() != 1 {
            return Err("EVA boarding requires an isolated suit".into());
        }
        let sa = self.snapshot(actor);
        let sb = self.snapshot(&carrier);
        let pose = self.parts.part(part).pose;
        let hatch_offset =
            sb.rotation * (pose.position + pose.rotation * seat.parameters.hatch_position);
        let relative = self.relative(actor, &carrier);
        if (relative.position - hatch_offset).length() > seat.parameters.boarding_radius_meters {
            return Err("crew outside hatch boarding range".into());
        }
        if (relative.velocity - sb.angular_velocity.cross(hatch_offset)).length()
            > seat.parameters.boarding_speed_meters_per_second
        {
            return Err("crew hatch-relative speed exceeds boarding limit".into());
        }
        match (&self.vessel(actor).owner, &self.vessel(&carrier).owner) {
            (Owner::Scene { scene: a, .. }, Owner::Scene { scene: b, .. }) if a == b => {}
            (Owner::Orbit { .. }, Owner::Orbit { .. }) => {}
            _ => return Err("crew and hatch need a shared contact owner".into()),
        }
        if self.parts.part(part).resource(ResourceId::Monopropellant) != 0.0 {
            return Err("empty seat already contains backpack propellant".into());
        }
        let suit = self.vessel(actor).root.clone();
        let pack = self.parts.part(&suit).resource(ResourceId::Monopropellant);
        let mass = sa.mass_kg + sb.mass_kg;
        let d = relative.position;
        let centre_offset = d * (sa.mass_kg / mass);
        let relative_v = relative.velocity;
        let velocity = sb.velocity + relative_v * (sa.mass_kg / mass);
        let aoffset = d - centre_offset;
        let boffset = -centre_offset;
        let ai = self.inertia(actor);
        let ar = DMat3::from_quat(sa.rotation);
        let bi = self.inertia(&carrier);
        let br = DMat3::from_quat(sb.rotation);
        let angular = ar * ai * ar.transpose() * sa.angular_velocity
            + br * bi * br.transpose() * sb.angular_velocity
            + aoffset.cross(relative_v * (sb.mass_kg / mass)) * sa.mass_kg
            + boffset.cross(-relative_v * (sa.mass_kg / mass)) * sb.mass_kg;
        let ModuleState::Thermal {
            state: packed_suit_thermal,
        } = self.parts.part(&suit).modules["thermal1"]
        else {
            panic!("suit thermal state")
        };
        self.parts.set_module_state(
            part,
            module,
            ModuleState::Seat {
                occupant: Some(crew),
                packed_suit_thermal: Some(packed_suit_thermal),
            },
        );
        self.parts
            .set_resource(part, ResourceId::Monopropellant, pack);
        let actor_v = self.vessels.remove(actor).unwrap();
        self.remove_scene_body(&actor_v, false);
        for pid in &actor_v.members {
            let frame = self.part_frames.remove(pid).expect("EVA part frame");
            self.frames.tree.remove(frame);
            self.dynamic
                .retain(|_, d| !matches!(d,Dynamic::Part(p) if p==pid));
            self.parts.remove_isolated(pid);
        }
        self.forget_vessel_frame(actor);
        self.order.retain(|v| v != actor);
        self.controls.remove(actor);
        self.rcs_controls.remove(actor);
        self.sas.remove(actor);
        self.guidance.remove(actor);
        self.gate.remove_vessel(actor);
        self.recentre(&self.vessel(&carrier).members.clone());
        let new_i = self.inertia(&carrier);
        let w = (br * new_i * br.transpose()).inverse() * angular;
        self.cancel_guidance(&carrier, "crew boarded");
        self.sas.remove(&carrier);
        self.rebuild_crew_owner(
            &carrier,
            State {
                position: sb.position + centre_offset,
                velocity,
            },
            sb.rotation,
            w,
        );
        self.validate_crew_identities();
        Ok(carrier)
    }
    pub fn validate_crew_identities(&self) {
        let mut ids = HashSet::new();
        for p in self.parts.parts() {
            for m in p.modules.values() {
                let crew = match m {
                    ModuleState::Seat {
                        occupant: Some(c), ..
                    }
                    | ModuleState::Crew { crew: c, .. } => Some(c),
                    _ => None,
                };
                if let Some(c) = crew {
                    assert!(
                        c.validate() && ids.insert(c.id.clone()),
                        "invalid or duplicate crew identity"
                    );
                }
            }
        }
    }
}

impl Fleet {
    /// Ground support probe from the finite body, on actual terrain/carrier colliders.
    fn eva_ground_probe(
        &self,
        v: &Vessel,
        d: &void_assembly::EvaDefinition,
    ) -> Option<void_landing::ContactRayHit> {
        let Owner::Scene { scene, body, .. } = v.owner else {
            return None;
        };
        let world = &self.scenes[&scene].world;
        // The support direction is local effective gravity, never inertial absolute +Y.
        let state = self.scene_centre(v);
        let acceleration =
            world
                .frame
                .acceleration(&*self.ephemeris, self.time, state.position, state.velocity);
        if acceleration.length_squared() < 1e-12 {
            return None;
        }
        let down = acceleration.normalize();
        let part = self.parts.part(&v.root);
        let half = void_assembly::part_box_size(part.definition).y / 2.0;
        let hit = world.suspension_ray(
            body,
            state.position,
            down,
            half + d.ground_probe_margin_meters,
        );
        hit.filter(|hit| {
            world.body_in_contact_with(body, hit.collider, hit.normal)
                && world.support_normal_load(body, hit.collider, hit.normal) > 0.0
        })
    }
    pub(super) fn add_eva_loads(&self, scene: u64, loads: &mut super::vehicles::WheelLoads) {
        let s = &self.scenes[&scene];
        let world = &s.world;
        for vessel in &s.members {
            let v = self.vessel(vessel);
            let Owner::Scene { body, push, .. } = v.owner else {
                unreachable!()
            };
            let b = world.body(body);
            let q = quat64(*b.rotation());
            let w = vec64(b.angvel());
            let com = world.origin + vec64(b.center_of_mass());
            let mass = self.mass(&v.members);
            for pid in &v.members {
                let part = self.parts.part(pid);
                for m in &part.definition.modules {
                    let Module::Crew {
                        id: mid,
                        parameters: d,
                    } = m
                    else {
                        continue;
                    };
                    let ModuleState::Crew { crew, control, .. } = &part.modules[mid] else {
                        panic!("EVA state")
                    };
                    let hit = self.eva_ground_probe(v, d);
                    if let Some(h) = hit.filter(|_| !part.thermally_failed()) {
                        if *control != EvaControl::default()
                            || h.body
                                .is_some_and(|support| !world.body(support).is_sleeping())
                        {
                            loads.wake.insert(body);
                            if let Some(support) = h.body.filter(|b| world.body(*b).is_dynamic()) {
                                loads.wake.insert(support);
                            }
                        }
                        let n = h.normal;
                        let up = q * DVec3::Y;
                        let forward = q * DVec3::Z;
                        let tangent = forward - n * forward.dot(n);
                        if tangent.length_squared() > 1e-12 {
                            let forward = tangent.normalize();
                            let side = forward.cross(n);
                            let mut input = forward * control.forward + side * control.strafe;
                            if input.length_squared() > 1.0 {
                                input = input.normalize();
                            }
                            let velocity = world.state(&*self.ephemeris, body, push).velocity;
                            let half_correction = velocity - vec64(b.linvel());
                            let support_correction = h.body.map_or(DVec3::ZERO, |support| {
                                s.members
                                    .iter()
                                    .find_map(|id| match self.vessel(id).owner {
                                        Owner::Scene { body: b, push, .. } if b == support => Some(
                                            world.state(&*self.ephemeris, b, push).velocity
                                                - vec64(world.body(b).linvel()),
                                        ),
                                        _ => None,
                                    })
                                    .unwrap_or(DVec3::ZERO)
                            });
                            let relative = world.point_velocity(body, h.point) + half_correction
                                - h.point_velocity
                                - support_correction;
                            let relative = relative - n * relative.dot(n);
                            let normal_load = world.support_normal_load(body, h.collider, n);
                            let foot_half_width =
                                void_assembly::part_box_size(part.definition).x / 2.0;
                            let body_half_height =
                                void_assembly::part_box_size(part.definition).y / 2.0;
                            let torque_budget =
                                d.upright_torque_limit_nm.min(normal_load * foot_half_width);
                            let max_force = (mass
                                * d.walking_acceleration_meters_per_second_squared)
                                .min(normal_load * d.traction_coefficient)
                                .min(0.45 * torque_budget / body_half_height);
                            let request = (input * d.walking_speed_meters_per_second - relative)
                                * (mass / 0.2);
                            let force = if request.length() > max_force && max_force > 0.0 {
                                request.normalize() * max_force
                            } else if max_force == 0.0 {
                                DVec3::ZERO
                            } else {
                                request
                            };
                            let tilt = up.cross(n);
                            let yaw = w.dot(n);
                            let request_torque = tilt * d.upright_stiffness_nm
                                - (w - n * yaw) * d.upright_damping_nm_seconds
                                + n * (-control.yaw * d.yaw_rate_radians_per_second - yaw)
                                    * d.upright_damping_nm_seconds
                                - (h.point - com).cross(force);
                            let limit = torque_budget;
                            let balance = if request_torque.length() > limit && limit > 0.0 {
                                request_torque.normalize() * limit
                            } else if limit == 0.0 {
                                DVec3::ZERO
                            } else {
                                request_torque
                            };
                            let torque = (h.point - com).cross(force) + balance;
                            let entry = loads.bodies.entry(body).or_default();
                            entry.0 += force;
                            entry.1 += torque;
                            if let Some(support) = h.body.filter(|b| world.body(*b).is_dynamic()) {
                                let centre =
                                    world.origin + vec64(world.body(support).center_of_mass());
                                let entry = loads.bodies.entry(support).or_default();
                                entry.0 -= force;
                                entry.1 += (h.point - centre).cross(-force) - balance;
                            }
                        }
                    }
                    loads.updates.push((
                        pid.clone(),
                        mid.clone(),
                        ModuleState::Crew {
                            crew: crew.clone(),
                            control: *control,
                            grounded: hit.is_some(),
                        },
                    ));
                }
            }
        }
    }
    pub fn eva_jump(&mut self, vessel: &str) -> Result<(), String> {
        let previous = self.enter_vessel(vessel);
        let result = self.eva_jump_local(vessel);
        self.restore_view(previous);
        result
    }
    fn eva_jump_local(&mut self, vessel: &str) -> Result<(), String> {
        if !self.has_command(vessel) {
            return Err("EVA command capability unavailable or thermally failed".into());
        }
        let v = self.vessel(vessel);
        let d = v
            .members
            .iter()
            .find_map(|pid| {
                self.parts
                    .part(pid)
                    .definition
                    .modules
                    .iter()
                    .find_map(|m| match m {
                        Module::Crew { parameters, .. } => Some(parameters.clone()),
                        _ => None,
                    })
            })
            .ok_or("selected vessel is not EVA crew")?;
        let is_grounded = v.members.iter().any(|pid| {
            self.parts
                .part(pid)
                .modules
                .values()
                .any(|m| matches!(m, ModuleState::Crew { grounded: true, .. }))
        });
        if !is_grounded {
            return Err("EVA jump requires accepted grounded state".into());
        }
        let hit = self
            .eva_ground_probe(v, &d)
            .ok_or("EVA jump requires actual native support contact")?;
        let Owner::Scene { scene, body, .. } = v.owner else {
            unreachable!()
        };
        let impulse = hit.normal * self.mass(&v.members) * d.jump_speed_meters_per_second;
        let world = &mut self.scenes.get_mut(&scene).unwrap().world;
        let centre = world.origin + vec64(world.body(body).center_of_mass());
        world.apply_instantaneous_impulse(body, impulse, (hit.point - centre).cross(impulse));
        if let Some(support) = hit.body.filter(|h| world.body(*h).is_dynamic()) {
            let centre = world.origin + vec64(world.body(support).center_of_mass());
            world.apply_instantaneous_impulse(
                support,
                -impulse,
                (hit.point - centre).cross(-impulse),
            );
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
                        ModuleState::Crew { crew, control, .. } => Some((
                            pid.clone(),
                            mid.clone(),
                            ModuleState::Crew {
                                crew: crew.clone(),
                                control: *control,
                                grounded: false,
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
}
