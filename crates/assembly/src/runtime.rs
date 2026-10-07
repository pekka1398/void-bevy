use crate::{
    CompiledCraft, Craft, G0, ModelResult, Module, PartPose, PlacedPart, Shape, compile, node,
    part_box_size, part_inertia_per_kg, rotate,
};
use glam::{DQuat, DVec3};
use rapier3d::math::{Pose, Rotation, Vector};
use rapier3d::prelude::*;
use std::collections::{HashMap, HashSet};

pub const STEP_SECONDS: f64 = 1.0 / 60.0;
pub const LAB_GRAVITY: f64 = 9.81;
#[derive(Clone, Copy, Debug, Default)]
pub struct FlightInput {
    pub throttle: f64,
    pub turn: DVec3,
}
#[derive(Debug)]
pub struct FlightGroup {
    pub ids: Vec<String>,
    pub body: RigidBodyHandle,
    colliders: HashMap<String, ColliderHandle>,
}
/// Flat local range, as the TS assembly lab. Each separated component remains live.
/// Uses that lab's native Rapier stepping, not landing's orbital contact frame.
pub struct AssemblyFlight {
    pub compiled: CompiledCraft,
    pub world: PhysicsWorld,
    pub fuel: HashMap<String, f64>,
    pub lit: HashSet<String>,
    pub firing: HashMap<String, f64>,
    pub cuts: HashSet<String>,
    pub stages: Vec<u32>,
    pub groups: Vec<FlightGroup>,
    pub next_stage: usize,
    pub time: f64,
}
fn v32(v: DVec3) -> Vector {
    Vector::new(v.x as f32, v.y as f32, v.z as f32)
}
fn v64(v: Vector) -> DVec3 {
    DVec3::new(v.x as f64, v.y as f64, v.z as f64)
}
fn q32(q: DQuat) -> Rotation {
    Rotation::from_xyzw(q.x as f32, q.y as f32, q.z as f32, q.w as f32)
}
fn q64(q: Rotation) -> DQuat {
    DQuat::from_xyzw(q.x as f64, q.y as f64, q.z as f64, q.w as f64)
}
fn mass_properties(p: &PlacedPart, fuel: f64) -> MassProperties {
    let mass = p.definition.dry_mass_kg + crate::initial_crew_mass_kg(p.definition) + fuel;
    MassProperties::with_principal_inertia_frame(
        Vector::ZERO,
        mass as f32,
        v32(part_inertia_per_kg(p.definition) * mass),
        Rotation::IDENTITY,
    )
}
impl AssemblyFlight {
    pub fn new(craft: &Craft, gravity: f64) -> ModelResult<Self> {
        assert!(gravity.is_finite() && gravity >= 0.0, "invalid lab gravity");
        let compiled = compile(craft)?;
        for p in &compiled.parts {
            if p.definition.modules.iter().any(|m| {
                matches!(
                    m,
                    Module::Parachute { .. }
                        | Module::Tank {
                            resource: crate::ResourceId::Monopropellant,
                            ..
                        }
                        | Module::Engine {
                            resource: crate::ResourceId::Monopropellant,
                            ..
                        }
                )
            }) || p
                .definition
                .modules
                .iter()
                .filter(|m| matches!(m, Module::Engine { .. }))
                .count()
                > 1
            {
                return Err("legacy assembly flight supports one liquid engine per part; use void-part-state-lab for modules/resources".into());
            }
        }
        for p in &compiled.parts {
            if p.definition
                .modules
                .iter()
                .any(|m| matches!(m, Module::Engine { .. } | Module::Decoupler { .. }))
                && p.instance.stage.is_none()
            {
                return Err(format!("{}: assign a stage before launch", p.instance.id));
            }
            if p.definition
                .modules
                .iter()
                .any(|m| matches!(m, Module::Decoupler { .. }))
            {
                compiled.decoupler_connection(&p.instance.id)?;
            }
        }
        if !compiled.parts.iter().any(|p| {
            p.definition
                .modules
                .iter()
                .any(|m| matches!(m, Module::Engine { .. }))
        }) {
            return Err("Add an engine before launch".into());
        }
        let mut stages: Vec<_> = compiled
            .parts
            .iter()
            .filter_map(|p| p.instance.stage)
            .collect();
        stages.sort_unstable();
        stages.dedup();
        let fuel = compiled
            .parts
            .iter()
            .map(|p| (p.instance.id.clone(), p.instance.resource_mass()))
            .collect();
        let mut world = PhysicsWorld {
            gravity: Vector::new(0.0, -gravity as f32, 0.0),
            ..PhysicsWorld::default()
        };
        world.integration_parameters.dt = STEP_SECONDS as f32;
        world.colliders.insert(
            ColliderBuilder::cuboid(100000.0, 0.1, 100000.0)
                .translation(Vector::new(0.0, -0.1, 0.0))
                .friction(0.8),
        );
        let low = compiled
            .parts
            .iter()
            .map(|p| p.pose.position.y - p.definition.height / 2.0)
            .fold(f64::INFINITY, f64::min);
        let ids = compiled
            .parts
            .iter()
            .map(|p| p.instance.id.clone())
            .collect();
        let mut out = Self {
            compiled,
            world,
            fuel,
            stages,
            lit: HashSet::new(),
            firing: HashMap::new(),
            cuts: HashSet::new(),
            groups: vec![],
            next_stage: 0,
            time: 0.0,
        };
        let group = out.create_group(
            ids,
            DVec3::new(0.0, -low + 0.04, 0.0),
            DQuat::IDENTITY,
            DVec3::ZERO,
            DVec3::ZERO,
        );
        out.groups.push(group);
        Ok(out)
    }
    fn create_group(
        &mut self,
        ids: Vec<String>,
        position: DVec3,
        rotation: DQuat,
        velocity: DVec3,
        spin: DVec3,
    ) -> FlightGroup {
        let body = self.world.bodies.insert(
            RigidBodyBuilder::dynamic()
                .pose(Pose::from_parts(v32(position), q32(rotation)))
                .linvel(v32(velocity))
                .angvel(v32(spin))
                .angular_damping(0.7)
                .ccd_enabled(true),
        );
        let mut colliders = HashMap::new();
        for id in &ids {
            let p = self.compiled.part(id);
            let d = p.definition;
            let builder = match d.shape {
                Shape::Box => {
                    let h = part_box_size(d) / 2.0;
                    ColliderBuilder::cuboid(h.x as f32, h.y as f32, h.z as f32)
                }
                Shape::Cone => ColliderBuilder::cone(d.height as f32 / 2.0, d.radius as f32),
                Shape::Cylinder => {
                    ColliderBuilder::cylinder(d.height as f32 / 2.0, d.radius as f32)
                }
            };
            let builder = builder
                .position(Pose::from_parts(v32(p.pose.position), q32(p.pose.rotation)))
                .friction(0.8)
                .restitution(0.0)
                .mass_properties(mass_properties(p, self.fuel[id]));
            let h = self
                .world
                .colliders
                .insert_with_parent(builder, body, &mut self.world.bodies);
            colliders.insert(id.clone(), h);
        }
        self.world.bodies[body].recompute_mass_properties_from_colliders(&self.world.colliders);
        FlightGroup {
            ids,
            body,
            colliders,
        }
    }
    fn group(&self, id: &str) -> &FlightGroup {
        self.groups
            .iter()
            .find(|g| g.ids.iter().any(|s| s == id))
            .unwrap_or_else(|| panic!("No physical group for {id}"))
    }
    pub fn controlled_handle(&self) -> RigidBodyHandle {
        self.group(&self.compiled.root_id).body
    }
    pub fn controlled_body(&self) -> &RigidBody {
        &self.world.bodies[self.controlled_handle()]
    }
    pub fn controlled_part_ids(&self) -> &[String] {
        &self.group(&self.compiled.root_id).ids
    }
    pub fn part_pose(&self, id: &str) -> PartPose {
        let p = self.compiled.part(id);
        let body = &self.world.bodies[self.group(id).body];
        let q = q64(*body.rotation());
        PartPose {
            position: v64(body.translation()) + rotate(q, p.pose.position),
            rotation: q * p.pose.rotation,
        }
    }
    pub fn controlled_center(&self) -> DVec3 {
        v64(self.controlled_body().center_of_mass())
    }
    pub fn controlled_velocity(&self) -> DVec3 {
        v64(self.controlled_body().linvel())
    }
    /// Preserve v + omega × (new COM − old COM), then apply equal/opposite impulses at the node.
    /// A decoupler can cut its child edge; this is not hardcoded to the parent edge.
    fn decouple(&mut self, id: &str) {
        let p = self.compiled.part(id);
        let (node_id, impulse) = p
            .definition
            .modules
            .iter()
            .find_map(|m| {
                if let Module::Decoupler {
                    node_id,
                    impulse_ns,
                    ..
                } = m
                {
                    Some((node_id.clone(), *impulse_ns))
                } else {
                    None
                }
            })
            .expect("expected decoupler");
        let c = self
            .compiled
            .decoupler_connection(id)
            .expect("launch validated explosive node")
            .clone();
        assert!(
            !self.cuts.contains(&c.b),
            "Connection already separated: {}",
            c.b
        );
        let old = self.group(id);
        let old_ids = old.ids.clone();
        let old_handle = old.body;
        let body = &self.world.bodies[old_handle];
        let position = v64(body.translation());
        let rotation = q64(*body.rotation());
        let old_center = v64(body.center_of_mass());
        let velocity = v64(body.linvel());
        let spin = v64(body.angvel());
        let pose = self.part_pose(id);
        let n = node(p.definition, &node_id).expect("authored node");
        let point = pose.position + rotate(pose.rotation, n.position);
        let normal = rotate(pose.rotation, n.direction);
        self.cuts.insert(c.b);
        let subsets: Vec<_> = self
            .compiled
            .components(&self.cuts)
            .into_iter()
            .filter(|ids| ids.iter().any(|p| old_ids.contains(p)))
            .collect();
        assert_eq!(
            subsets.len(),
            2,
            "Decoupling must split one tree into two groups"
        );
        let mut replacements = vec![];
        for ids in subsets {
            let g = self.create_group(ids, position, rotation, velocity, spin);
            let center = v64(self.world.bodies[g.body].center_of_mass());
            self.world.bodies[g.body]
                .set_linvel(v32(velocity + spin.cross(center - old_center)), true);
            replacements.push(g);
        }
        self.world
            .remove_body(old_handle)
            .expect("old group exists");
        self.groups.retain(|g| g.body != old_handle);
        self.groups.extend(replacements);
        let own = self.group(id).body;
        let other = self
            .groups
            .iter()
            .find(|g| g.body != own && g.ids.iter().any(|p| old_ids.contains(p)))
            .expect("other separated group")
            .body;
        self.world.bodies[own].apply_impulse_at_point(v32(-normal * impulse), v32(point), true);
        self.world.bodies[other].apply_impulse_at_point(v32(normal * impulse), v32(point), true);
    }
    pub fn stage(&mut self) -> Option<u32> {
        let stage = *self.stages.get(self.next_stage)?;
        let ids: Vec<_> = self
            .compiled
            .parts
            .iter()
            .filter(|p| p.instance.stage == Some(stage))
            .map(|p| p.instance.id.clone())
            .collect();
        for id in &ids {
            if self
                .compiled
                .part(id)
                .definition
                .modules
                .iter()
                .any(|m| matches!(m, Module::Decoupler { .. }))
            {
                self.decouple(id);
            }
        }
        for id in ids {
            if self
                .compiled
                .part(&id)
                .definition
                .modules
                .iter()
                .any(|m| matches!(m, Module::Engine { .. }))
            {
                self.lit.insert(id);
            }
        }
        self.next_stage += 1;
        Some(stage)
    }
    pub fn step(&mut self, input: FlightInput) {
        assert!(
            (0.0..=1.0).contains(&input.throttle) && input.turn.is_finite(),
            "Invalid flight input"
        );
        self.firing.clear();
        let mut burns = vec![];
        // Ignition order, never HashSet iteration order.
        let lit: Vec<_> = self.stages[..self.next_stage]
            .iter()
            .flat_map(|s| {
                self.compiled
                    .parts
                    .iter()
                    .filter(move |p| p.instance.stage == Some(*s))
            })
            .filter(|p| self.lit.contains(&p.instance.id))
            .map(|p| p.instance.id.clone())
            .collect();
        for id in lit {
            let p = self.compiled.part(&id);
            let (thrust, isp, direction) = p
                .definition
                .modules
                .iter()
                .find_map(|m| {
                    if let Module::Engine {
                        thrust_newtons,
                        isp_seconds,
                        direction,
                        ..
                    } = m
                    {
                        Some((*thrust_newtons, *isp_seconds, *direction))
                    } else {
                        None
                    }
                })
                .expect("expected engine");
            let sources = self.compiled.fuel_sources(&id, &self.cuts);
            let available: f64 = sources.iter().map(|s| self.fuel[s]).sum();
            let requested = thrust * input.throttle / (isp * G0) * STEP_SECONDS;
            let burned = available.min(requested);
            let fraction = if requested > 0.0 {
                burned / requested
            } else {
                0.0
            };
            if available > 0.0 && burned > 0.0 {
                for tank in sources {
                    let f = self.fuel.get_mut(&tank).expect("tank fuel");
                    *f = (*f * (1.0 - burned / available)).max(0.0);
                }
            }
            self.firing.insert(id.clone(), input.throttle * fraction);
            burns.push((
                id,
                thrust * input.throttle * fraction * STEP_SECONDS,
                direction,
            ));
        }
        for group in &self.groups {
            for id in &group.ids {
                self.world.colliders[group.colliders[id]]
                    .set_mass_properties(mass_properties(self.compiled.part(id), self.fuel[id]));
            }
            self.world.bodies[group.body]
                .recompute_mass_properties_from_colliders(&self.world.colliders);
        }
        for (id, impulse, direction) in burns {
            let pose = self.part_pose(&id);
            let h = self.group(&id).body;
            self.world.bodies[h].apply_impulse_at_point(
                v32(rotate(pose.rotation, direction) * impulse),
                v32(pose.position),
                true,
            );
        }
        let h = self.controlled_handle();
        let body = &self.world.bodies[h];
        let torque = rotate(
            q64(*body.rotation()),
            input.turn * (body.mass() as f64 * STEP_SECONDS * 2.0),
        );
        self.world.bodies[h].apply_torque_impulse(v32(torque), true);
        self.world.step();
        self.time += STEP_SECONDS;
    }
}
