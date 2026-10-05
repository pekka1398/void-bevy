//! Thermal commit shared by Orbit/Bubble/Ground. Trials/HUDs never call this path.
use super::*;
use void_assembly::{ModuleState, ResourceId};
impl Fleet {
    fn thermal_inputs(&self) -> Vec<void_modules::thermal::Input> {
        let at = self.frames();
        let frames = self.environment.frames();
        let mut inputs = Vec::new();
        for id in &self.order {
            let v = self.vessel(id);
            let visible_members: Vec<_> = v
                .members
                .iter()
                .filter(|p| !self.parts.part(p).thermally_failed())
                .cloned()
                .collect();
            for pid in &v.members {
                let part = self.parts.part(pid);
                if !part
                    .definition
                    .modules
                    .iter()
                    .any(|m| matches!(m, Module::Thermal { .. }))
                {
                    continue;
                }
                let transform = at.transform(self.part_frame(pid), self.origin_frame());
                let state = transform.apply_state(State {
                    position: DVec3::ZERO,
                    velocity: DVec3::ZERO,
                });
                let air = (0..self.environment.bodies().len()).find_map(|body| {
                    self.environment
                        .surroundings(&at, frames, self.origin_frame(), state, body)
                        .air
                });
                for module in &part.definition.modules {
                    let Module::Thermal {
                        id: module,
                        parameters,
                    } = module
                    else {
                        continue;
                    };
                    let ModuleState::Thermal { state: thermal } = part.modules[module] else {
                        panic!("thermal module state mismatch");
                    };
                    let speed = air.map_or(0.0, |a| a.airspeed.length());
                    let flow = air.filter(|_| speed > 0.0).map(|a| a.airspeed / speed);
                    let normal = parameters.normal.map(|n| transform.apply_direction(n));
                    let exposure = match (normal, flow) {
                        (Some(n), Some(d)) => n.dot(d).max(0.0),
                        (None, Some(d)) => {
                            // Use the same exposed end areas as the aerodynamic body module.
                            // Buried stack faces do not all receive stagnation heating; failed
                            // neighbours no longer count as thermal protection.
                            let element = void_modules::body::element(
                                &self.parts,
                                &visible_members,
                                pid,
                                DVec3::ZERO,
                            );
                            let void_aero::AeroShape::Body(shape) = element.shape else {
                                unreachable!()
                            };
                            let cosine =
                                transform.apply_direction(DVec3::Y).dot(d).clamp(-1.0, 1.0);
                            let area = std::f64::consts::PI * part.definition.radius.powi(2);
                            let end = if cosine >= 0.0 {
                                shape.front_area
                            } else {
                                shape.rear_area
                            };
                            (cosine.abs() * end / area + (1.0 - cosine * cosine).sqrt()).min(1.0)
                        }
                        _ => 1.0,
                    };
                    // An upstream ablative disk shields a downstream part only inside its projected
                    // disk. Separation/docking change members/connections, not an external cache.
                    let blocked = flow.is_some_and(|direction| {
                        v.members.iter().filter(|other| *other != pid).any(|other| {
                            let shield = self.parts.part(other);
                            if shield.thermally_failed() || !shield.definition.modules.iter().any(|m| matches!(m, Module::Thermal {parameters,..} if parameters.normal.is_some() && parameters.ablation.is_some())) {
                                return false;
                            }
                            let placed = at.transform(self.part_frame(other), self.origin_frame());
                            shield.definition.modules.iter().any(|m| {
                                let Module::Thermal { parameters: p, .. } = m else {
                                    return false;
                                };
                                let Some(n) = p.normal.filter(|_| p.ablation.is_some()) else {
                                    return false;
                                };
                                let n = placed.apply_direction(n);
                                let facing = n.dot(direction);
                                if facing <= 1e-9 {
                                    return false;
                                }
                                let offset = placed.apply_point(DVec3::ZERO) - state.position;
                                let along = offset.dot(n) / facing;
                                along > 0.0
                                    && (direction * along - offset).length()
                                        + part.definition.radius
                                        <= shield.definition.radius
                            })
                        })
                    });
                    inputs.push(void_modules::thermal::Input {
                        part: pid.clone(),
                        module: module.clone(),
                        parameters: parameters.clone(),
                        state: thermal,
                        ablator_kg: part.resource(ResourceId::Ablator),
                        environment: void_aero::HeatEnvironment {
                            air: air.map_or(void_aero::Air::VACUUM, |a| a.air),
                            speed,
                            exposed: !blocked,
                            exposure,
                            background_k: parameters.background_k,
                        },
                    });
                }
            }
        }
        inputs
    }
    pub(super) fn commit_thermal(&mut self, seconds: f64) {
        let inputs = self.thermal_inputs();
        if inputs.is_empty() || seconds == 0.0 {
            return;
        }
        let updates = void_modules::thermal::advance(&self.parts, &inputs, seconds);
        let changed: HashSet<_> = updates
            .iter()
            .filter(|u| u.ablator_kg != self.parts.part(&u.part).resource(ResourceId::Ablator))
            .map(|u| u.part.clone())
            .collect();
        void_modules::thermal::commit(&mut self.parts, updates);
        for id in self.order.clone() {
            if self.command_failed(&id) {
                self.controls.insert(id.clone(), VesselControl::default());
                self.rcs_controls.insert(id.clone(), RcsControl::default());
                self.sas.remove(&id);
                self.cancel_guidance(&id, "command part thermally failed");
            }
            if !self.vessel(&id).members.iter().any(|p| changed.contains(p)) {
                continue;
            }
            let mut v = self.vessels.remove(&id).expect("thermal vessel");
            let mass = self.mass(&v.members);
            match &mut v.owner {
                Owner::Orbit {
                    run,
                    rotation,
                    angular_velocity,
                } => {
                    let centre = self.recentre(&v.members);
                    let d = *rotation * centre;
                    let u = angular_velocity.cross(d);
                    for i in 0..3 {
                        run.y[i] += d[i];
                        run.y[i + 3] += u[i];
                    }
                    run.y[6] = mass;
                    **run = run.restarted();
                }
                Owner::Scene { scene, body, .. } => {
                    let masses: Vec<_> = v.members.iter().map(|p| self.part_mass(p)).collect();
                    let world = &mut self.scenes.get_mut(scene).expect("thermal scene").world;
                    let b = world.body(*body);
                    let old_centre = vec64(b.local_center_of_mass());
                    let velocity = vec64(b.linvel());
                    let rotation = quat64(*b.rotation());
                    let omega = vec64(b.angvel());
                    world.set_piece_masses(*body, &masses);
                    let shift =
                        rotation * (vec64(world.body(*body).local_center_of_mass()) - old_centre);
                    // Lost material leaves with the local rigid-body point velocity. Preserve the
                    // remaining part origins' velocities when the COM moves, just as Orbit does.
                    world.world.bodies[*body]
                        .set_linvel(vec32(velocity + omega.cross(shift)), true);
                }
            }
            self.vessels.insert(id, v);
        }
    }
    /// Incoming aerodynamic heating requires accepted physics steps; quiet cooling can evolve
    /// on rails without freezing temperature or material state.
    pub(super) fn thermal_rails_blocker(&self) -> Option<String> {
        // Enter physics before the next rails chunk can reach any atmosphere. A conservative
        // speed/gravity buffer avoids charging an entire quiet chunk at its hot end state.
        let at = self.frames();
        for id in &self.order {
            let v = self.vessel(id);
            if !v.members.iter().any(|p| {
                self.parts
                    .part(p)
                    .definition
                    .modules
                    .iter()
                    .any(|m| matches!(m, Module::Thermal { .. }))
            }) {
                continue;
            }
            for body in 0..self.environment.bodies().len() {
                let Some(description) = self.environment.body(body) else {
                    continue;
                };
                let Some(air) = &description.atmosphere else {
                    continue;
                };
                let local = at
                    .transform(self.vessel_frame(id), self.body_frames(body).1)
                    .apply_state(State {
                        position: self.centre_of_mass_local(id),
                        velocity: DVec3::ZERO,
                    });
                let radius = local.position.length();
                assert!(
                    radius.is_finite() && radius > 0.0,
                    "thermal: invalid distance from atmosphere centre"
                );
                let gap = radius
                    - self.ephemeris.bodies()[body].radius_meters
                    - description.air_datum_meters
                    - air.ceiling_meters();
                let inward = local.position.dot(local.velocity) / radius;
                let dt = self.options.rails_chunk_seconds;
                let gravity = self.ephemeris.bodies()[body].gm
                    / self.ephemeris.bodies()[body].radius_meters.powi(2);
                let buffer = 50.0 + local.velocity.length() * dt + 2.0 * gravity * dt * dt;
                if gap > 0.0 && gap <= buffer && inward < -1e-6 {
                    return Some(format!(
                        "approaching atmosphere requires thermal physics on {id}"
                    ));
                }
            }
        }
        self.thermal_inputs().iter().find_map(|p| {
            let e = &p.environment;
            (e.exposed && e.exposure > 0.0 && e.air.density > 0.0 && e.speed > 1.0)
                .then(|| format!("thermal airflow requires physics on {}", p.part))
        })
    }
}
