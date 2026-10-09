//! The part graph of flying vessels: every part with its own state, and the connections between
//! parts. A vessel is one connected group of it; separation and docking are graph operations.
//! See `docs/part-graph.md`.
use crate::{
    AttachNode, CompiledCraft, Connection, Module, PartDefinition, PartPose, ResourceId, Resources,
    node, validate_resources,
};
use glam::DVec3;
use std::collections::{BTreeMap, HashSet};

/// One flying part.
#[derive(Clone, Debug)]
pub struct Part {
    pub id: String,
    pub definition: &'static PartDefinition,
    /// Propellant in the part's tanks; zero without one.
    pub resources: Resources,
    pub modules: BTreeMap<String, ModuleState>,
    pub stage: Option<u32>,
    pub module_stages: BTreeMap<String, Option<u32>>,
    /// In the parts frame of the vessel it belongs to.
    pub pose: PartPose,
}

/// An engine module's rating.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EngineRating {
    pub thrust_newtons: f64,
    pub isp_seconds: f64,
    /// Back pressure costs this area times the ambient pressure of the vacuum thrust.
    pub nozzle_exit_area_m2: f64,
    /// Thrust direction in the part's axes.
    pub direction: DVec3,
    pub resource: ResourceId,
    pub jet: Option<crate::JetDefinition>,
}

impl Part {
    pub fn mass_kg(&self) -> f64 {
        self.definition.dry_mass_kg + self.resource_mass() + self.crew_mass_kg()
    }
    pub fn crew_mass_kg(&self) -> f64 {
        self.modules
            .values()
            .map(|m| match m {
                ModuleState::Seat {
                    occupant: Some(crew),
                    ..
                } => crew.carried_dry_mass_kg(),
                ModuleState::Crew { crew, .. } => crew.body_mass_kg,
                _ => 0.0,
            })
            .sum()
    }
    pub fn is_command(&self) -> bool {
        self.definition
            .modules
            .iter()
            .any(|m| matches!(m, Module::Command { .. }))
    }
    pub fn engines(&self) -> impl Iterator<Item = (&str, EngineRating)> {
        self.definition.modules.iter().filter_map(|m| match m {
            Module::Engine {
                id,
                thrust_newtons,
                isp_seconds,
                nozzle_exit_area_m2,
                direction,
                resource,
                jet,
            } => Some((
                id.as_str(),
                EngineRating {
                    thrust_newtons: *thrust_newtons,
                    isp_seconds: *isp_seconds,
                    nozzle_exit_area_m2: *nozzle_exit_area_m2,
                    direction: *direction,
                    resource: *resource,
                    jet: *jet,
                },
            )),
            _ => None,
        })
    }
    /// Only for callers that require a single engine; rejects ambiguous definitions.
    pub fn engine(&self) -> Option<EngineRating> {
        let mut es = self.engines();
        let first = es.next();
        assert!(
            es.next().is_none(),
            "single-engine API used on multi-engine part"
        );
        first.map(|(_, e)| e)
    }
    pub fn engine_enabled(&self, id: &str) -> bool {
        match self.modules.get(id).expect("unknown engine module") {
            ModuleState::Engine { enabled, .. } => *enabled,
            _ => panic!("not an engine module"),
        }
    }
    /// The decoupler's node and separation impulse.
    pub fn decoupler(&self) -> Option<(&'static str, f64)> {
        let mut modules = self.definition.modules.iter().filter_map(|m| match m {
            Module::Decoupler {
                node_id,
                impulse_ns,
                ..
            } => Some((node_id.as_str(), *impulse_ns)),
            _ => None,
        });
        let first = modules.next();
        assert!(
            modules.next().is_none(),
            "single-decoupler API used on multi-decoupler part"
        );
        first
    }
}

/// Parts by ID and the connections between them, in the order they were made.
#[derive(Clone, Debug, Default)]
pub struct PartGraph {
    parts: BTreeMap<String, Part>,
    connections: Vec<Connection>,
}

impl PartGraph {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a craft's parts as `{prefix}/{part}` with the craft's poses, fuel and stages, and its
    /// connections. Returns the new IDs in the craft's part order.
    pub fn add(&mut self, craft: &CompiledCraft, prefix: &str) -> Vec<String> {
        let ids: Vec<String> = craft
            .parts
            .iter()
            .map(|p| {
                let id = format!("{prefix}/{}", p.instance.id);
                self.insert(Part {
                    id: id.clone(),
                    definition: p.definition,
                    resources: p.instance.resources.clone(),
                    modules: initial_modules(p.definition)
                        .into_iter()
                        .map(|(mid, mut state)| {
                            match &mut state {
                                ModuleState::Seat {
                                    occupant: Some(crew),
                                    ..
                                }
                                | ModuleState::Crew { crew, .. } => {
                                    crew.id = format!("{prefix}/{}/{}", p.instance.id, mid);
                                }
                                _ => {}
                            }
                            (mid, state)
                        })
                        .collect(),
                    stage: p.instance.stage,
                    module_stages: p
                        .definition
                        .modules
                        .iter()
                        .filter(|m| {
                            matches!(
                                m,
                                Module::Engine { .. }
                                    | Module::Decoupler { .. }
                                    | Module::Parachute { .. }
                            )
                        })
                        .map(|m| {
                            (
                                m.id().to_string(),
                                p.instance
                                    .module_stages
                                    .get(m.id())
                                    .copied()
                                    .unwrap_or(p.instance.stage),
                            )
                        })
                        .collect(),
                    pose: p.pose,
                });
                id
            })
            .collect();
        for link in &craft.connections {
            self.connections.push(Connection {
                a: format!("{prefix}/{}", link.a),
                b: format!("{prefix}/{}", link.b),
                ..link.clone()
            });
        }
        ids
    }

    /// A part restored as it was saved.
    pub fn insert(&mut self, part: Part) {
        crate::validate_definition(part.definition).expect("part graph: invalid definition");
        validate_resources(part.definition, &part.resources)
            .unwrap_or_else(|e| panic!("part graph: {} {e}", part.id));
        check_modules(&part);
        let expected: Vec<_> = part
            .definition
            .modules
            .iter()
            .filter(|m| {
                matches!(
                    m,
                    Module::Engine { .. } | Module::Decoupler { .. } | Module::Parachute { .. }
                )
            })
            .map(|m| m.id())
            .collect();
        assert!(
            expected.len() == part.module_stages.len()
                && expected
                    .iter()
                    .all(|id| part.module_stages.contains_key(*id))
                && part
                    .module_stages
                    .values()
                    .all(|s| s.is_none_or(|s| s <= 99)),
            "part graph: invalid module stage mapping"
        );
        Self::check_pose(part.pose);
        let id = part.id.clone();
        match self.parts.entry(id.clone()) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(part);
            }
            std::collections::btree_map::Entry::Occupied(_) => {
                panic!("part graph: duplicate part {id}")
            }
        }
    }

    fn check_pose(pose: PartPose) {
        assert!(
            pose.position.is_finite()
                && pose.rotation.is_finite()
                && (pose.rotation.length() - 1.0).abs() < 1e-6,
            "part graph: invalid pose {pose:?}"
        );
    }

    /// Connections restored as they were saved, in their saved order.
    pub fn restore_connections(&mut self, connections: Vec<Connection>) {
        assert!(
            self.connections.is_empty(),
            "part graph: connections already restored"
        );
        for c in connections {
            self.check_connection(&c);
            self.connections.push(c);
        }
    }

    /// Remove an isolated part after its identity/resources have transferred in an owner transaction.
    pub fn remove_isolated(&mut self, id: &str) -> Part {
        assert!(
            self.connections.iter().all(|c| c.a != id && c.b != id),
            "cannot remove connected part"
        );
        self.parts.remove(id).expect("unknown isolated part")
    }
    pub fn contains(&self, id: &str) -> bool {
        self.parts.contains_key(id)
    }
    pub fn part(&self, id: &str) -> &Part {
        self.parts
            .get(id)
            .unwrap_or_else(|| panic!("part graph: unknown part {id}"))
    }
    /// Validates before changing a quantity; identities and definitions stay immutable.
    pub fn set_resource(&mut self, id: &str, resource: ResourceId, amount: f64) {
        let part = self.part(id);
        let mut quantities = part.resources.clone();
        assert!(
            quantities.contains_key(&resource),
            "part graph: {id} has no {resource:?} tank"
        );
        quantities.insert(resource, amount);
        validate_resources(part.definition, &quantities)
            .unwrap_or_else(|e| panic!("part graph: {id} {e}"));
        self.parts.get_mut(id).expect("checked part").resources = quantities;
    }
    /// Legacy observation/control: liquid propellant only, never total resource mass.
    pub fn set_fuel(&mut self, id: &str, amount: f64) {
        self.set_resource(id, ResourceId::LiquidPropellant, amount);
    }

    /// Definition-aware module state update validates before mutating.
    pub fn set_module_state(&mut self, id: &str, module: &str, state: ModuleState) {
        let mut part = self.part(id).clone();
        assert!(
            part.modules.contains_key(module),
            "part graph: unknown module {module}"
        );
        check_transition(&part.modules[module], &state);
        part.modules.insert(module.to_string(), state);
        check_modules(&part);
        self.parts.get_mut(id).expect("checked part").modules = part.modules;
    }
    pub fn set_pose(&mut self, id: &str, pose: PartPose) {
        self.part(id);
        Self::check_pose(pose);
        self.parts.get_mut(id).expect("checked part").pose = pose;
    }

    /// Consume one addressed module's action. Repeated staging is explicitly idempotent.
    pub fn stage_module(&mut self, id: &str, module: &str) {
        let old = self
            .part(id)
            .modules
            .get(module)
            .expect("unknown staged module")
            .clone();
        let state = match old {
            ModuleState::Engine {
                activated: true, ..
            } => return,
            ModuleState::Engine { .. } => ModuleState::Engine {
                activated: true,
                enabled: true,
            },
            ModuleState::Decoupler { .. } => ModuleState::Decoupler { activated: true },
            ModuleState::Parachute { state } => {
                if state.phase != crate::ParachutePhase::Stowed {
                    return;
                }
                ModuleState::Parachute {
                    state: crate::ParachuteState {
                        phase: crate::ParachutePhase::Armed,
                        elapsed_seconds: 0.0,
                    },
                }
            }
            ModuleState::Seat { .. }
            | ModuleState::Crew { .. }
            | ModuleState::Wheel { .. }
            | ModuleState::Thermal { .. }
            | ModuleState::Rcs { .. }
            | ModuleState::DockingPort { .. }
            | ModuleState::Passive => {
                panic!("passive module has no stage action")
            }
        };
        self.set_module_state(id, module, state);
    }
    /// A whole-part action activates all its actionable modules.
    pub fn stage_part(&mut self, id: &str) {
        let ids: Vec<_> = self.part(id).module_stages.keys().cloned().collect();
        for module in ids {
            self.stage_module(id, &module);
        }
    }
    pub fn set_module_stage(&mut self, id: &str, module: &str, stage: Option<u32>) {
        assert!(stage.is_none_or(|s| s <= 99), "invalid module stage");
        let part = self.part(id);
        assert!(
            part.module_stages.contains_key(module),
            "unknown action module"
        );
        assert!(
            !part.module_activated(module),
            "cannot change consumed module stage"
        );
        self.parts
            .get_mut(id)
            .expect("checked part")
            .module_stages
            .insert(module.to_string(), stage);
    }

    /// Every part, by ID.
    pub fn parts(&self) -> impl Iterator<Item = &Part> {
        self.parts.values()
    }
    pub fn connections(&self) -> &[Connection] {
        &self.connections
    }

    /// The connection at a part's node, if any.
    pub fn connection_at(&self, part: &str, node: &str) -> Option<&Connection> {
        self.connections
            .iter()
            .find(|c| (c.a == part && c.node_a == node) || (c.b == part && c.node_b == node))
    }

    /// Panics unless `c` could be made: two different parts, both nodes existing, free and the
    /// same size.
    pub fn check_connection(&self, c: &Connection) {
        assert_ne!(c.a, c.b, "part graph: {} connected to itself", c.a);
        let na = node(self.part(&c.a).definition, &c.node_a)
            .unwrap_or_else(|e| panic!("part graph: {e}"));
        let nb = node(self.part(&c.b).definition, &c.node_b)
            .unwrap_or_else(|e| panic!("part graph: {e}"));
        assert_eq!(na.size, nb.size, "part graph: node sizes differ");
        for (p, n) in [(&c.a, &c.node_a), (&c.b, &c.node_b)] {
            assert!(
                self.connection_at(p, n).is_none(),
                "part graph: {p} {n} is occupied"
            );
        }
    }

    /// Docking: both nodes must exist, be free and be the same size.
    pub fn connect(&mut self, connection: Connection) {
        self.check_connection(&connection);
        self.connections.push(connection);
    }

    /// Separation at a part's node; the other connections keep their order.
    pub fn disconnect(&mut self, part: &str, node: &str) -> Connection {
        let i = self
            .connections
            .iter()
            .position(|c| (c.a == part && c.node_a == node) || (c.b == part && c.node_b == node))
            .unwrap_or_else(|| panic!("part graph: {part} {node} is not connected"));
        self.connections.remove(i)
    }

    /// The connected groups of `ids`, each in the order it is reached from the first of `ids`
    /// not yet grouped, following connections in the order they were made.
    pub fn components(&self, ids: &[String]) -> Vec<Vec<String>> {
        let mut left: HashSet<&str> = ids.iter().map(String::as_str).collect();
        let mut out = vec![];
        for id in ids {
            if !left.remove(id.as_str()) {
                continue;
            }
            let mut group = vec![id.clone()];
            let mut i = 0;
            while i < group.len() {
                for c in &self.connections {
                    let next = if c.a == group[i] {
                        Some(&c.b)
                    } else if c.b == group[i] {
                        Some(&c.a)
                    } else {
                        None
                    };
                    if let Some(n) = next
                        && left.remove(n.as_str())
                    {
                        group.push(n.clone());
                    }
                }
                i += 1;
            }
            out.push(group);
        }
        out
    }

    /// Nodes of `ids` with nothing attached, in part then node order.
    pub fn free_nodes(&self, ids: &[String]) -> Vec<(String, &'static AttachNode)> {
        ids.iter()
            .flat_map(|p| {
                self.part(p)
                    .definition
                    .nodes
                    .iter()
                    .filter(move |n| self.connection_at(p, &n.id).is_none())
                    .map(move |n| (p.clone(), n))
            })
            .collect()
    }

    /// The tanks an engine draws from, in `members` order (`crossfeed_tanks`).
    pub fn crossfeed_tanks(&self, members: &[String], engine: &str) -> Vec<String> {
        self.resource_tanks(members, engine, ResourceId::LiquidPropellant)
    }

    pub fn resource_tanks(
        &self,
        members: &[String],
        consumer: &str,
        resource: ResourceId,
    ) -> Vec<String> {
        assert!(
            members.iter().any(|id| id == consumer),
            "resource consumer is outside vessel"
        );
        let mut visited = HashSet::from([consumer.to_string()]);
        let mut queue = vec![consumer.to_string()];
        let mut i = 0;
        while i < queue.len() {
            for c in &self.connections {
                if !members.contains(&c.a)
                    || !members.contains(&c.b)
                    || !self.part(&c.a).definition.crossfeed
                    || !self.part(&c.b).definition.crossfeed
                {
                    continue;
                }
                let next = if c.a == queue[i] {
                    Some(&c.b)
                } else if c.b == queue[i] {
                    Some(&c.a)
                } else {
                    None
                };
                if let Some(n) = next
                    && visited.insert(n.clone())
                {
                    queue.push(n.clone());
                }
            }
            i += 1;
        }
        members
            .iter()
            .filter(|id| visited.contains(*id) && self.part(id).resources.contains_key(&resource))
            .cloned()
            .collect()
    }

    /// Summed in `ids` order.
    pub fn mass(&self, ids: &[String]) -> f64 {
        ids.iter().map(|id| self.part(id).mass_kg()).sum()
    }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum ModuleState {
    Seat {
        occupant: Option<crate::CrewRecord>,
        packed_suit_thermal: Option<crate::PartThermalState>,
    },
    Crew {
        crew: crate::CrewRecord,
        control: crate::EvaControl,
        grounded: bool,
    },
    Wheel {
        state: crate::WheelState,
        control: crate::VehicleControl,
    },
    Thermal {
        state: crate::PartThermalState,
    },
    Parachute {
        state: crate::ParachuteState,
    },
    Rcs {
        enabled: bool,
    },
    DockingPort {
        armed: bool,
    },
    Passive,
    Engine {
        activated: bool,
        enabled: bool,
    },
    Decoupler {
        activated: bool,
    },
}
pub fn initial_modules(definition: &PartDefinition) -> BTreeMap<String, ModuleState> {
    definition
        .modules
        .iter()
        .map(|m| {
            (
                m.id().to_string(),
                match m {
                    Module::Seat { parameters, .. } => ModuleState::Seat {
                        occupant: parameters.initial_crew.then(crate::CrewRecord::pilot),
                        packed_suit_thermal: parameters
                            .initial_crew
                            .then(|| crate::eva_suit_thermal_definition().initial()),
                    },
                    Module::Crew { .. } => ModuleState::Crew {
                        crew: crate::CrewRecord::pilot(),
                        control: crate::EvaControl::default(),
                        grounded: false,
                    },
                    Module::Wheel { parameters, .. } => ModuleState::Wheel {
                        state: parameters.initial(),
                        control: crate::VehicleControl::default(),
                    },
                    Module::Thermal { parameters, .. } => ModuleState::Thermal {
                        state: parameters.initial(),
                    },
                    Module::Rcs { .. } => ModuleState::Rcs { enabled: true },
                    Module::DockingPort { .. } => ModuleState::DockingPort { armed: true },
                    Module::Parachute { .. } => ModuleState::Parachute {
                        state: crate::ParachuteState::STOWED,
                    },
                    Module::Engine { .. } => ModuleState::Engine {
                        activated: false,
                        enabled: false,
                    },
                    Module::Decoupler { .. } => ModuleState::Decoupler { activated: false },
                    _ => ModuleState::Passive,
                },
            )
        })
        .collect()
}
fn check_modules(part: &Part) {
    assert_eq!(
        part.modules.len(),
        part.definition.modules.len(),
        "part graph: module count mismatch"
    );
    let mut ids = HashSet::new();
    for m in &part.definition.modules {
        assert!(
            !m.id().is_empty() && ids.insert(m.id()),
            "part graph: empty/duplicate module ID"
        );
        let state = part
            .modules
            .get(m.id())
            .expect("part graph: missing module state");
        assert!(
            match (m, state) {
                (
                    Module::Seat { .. },
                    ModuleState::Seat {
                        occupant,
                        packed_suit_thermal,
                    },
                ) => {
                    let d = crate::eva_suit_thermal_definition();
                    occupant.is_some() == packed_suit_thermal.is_some()
                        && occupant.as_ref().is_none_or(|c| c.validate())
                        && packed_suit_thermal.is_none_or(|s| {
                            s.skin_k.is_finite()
                                && s.skin_k > 0.0
                                && s.core_k.is_finite()
                                && s.core_k > 0.0
                                && (s.failed
                                    || (s.skin_k <= d.max_skin_k && s.core_k <= d.max_core_k))
                        })
                }
                (Module::Crew { .. }, ModuleState::Crew { crew, control, .. }) =>
                    crew.validate()
                        && [control.forward, control.strafe, control.yaw]
                            .iter()
                            .all(|v| v.is_finite() && v.abs() <= 1.0),
                (Module::Wheel { parameters, .. }, ModuleState::Wheel { state, control }) => {
                    state.steer_radians.is_finite()
                        && state.steer_radians.abs() <= parameters.max_steer_radians
                        && state.spin_radians.is_finite()
                        && state.spin_radians_per_second.is_finite()
                        && state.suspension_length_meters.is_finite()
                        && state.suspension_length_meters >= 0.0
                        && state.suspension_length_meters
                            <= parameters.rest_length_meters + parameters.travel_meters
                        && control.drive.is_finite()
                        && control.drive.abs() <= 1.0
                        && control.steer.is_finite()
                        && control.steer.abs() <= 1.0
                        && control.brake.is_finite()
                        && (0.0..=1.0).contains(&control.brake)
                }
                (Module::Thermal { parameters, .. }, ModuleState::Thermal { state }) =>
                    state.skin_k.is_finite()
                        && state.skin_k > 0.0
                        && state.core_k.is_finite()
                        && state.core_k > 0.0
                        && (state.failed
                            || (state.skin_k <= parameters.max_skin_k
                                && state.core_k <= parameters.max_core_k)),
                (Module::Rcs { .. }, ModuleState::Rcs { .. })
                | (Module::DockingPort { .. }, ModuleState::DockingPort { .. }) => true,
                (Module::Engine { .. }, ModuleState::Engine { activated, enabled }) =>
                    !enabled || *activated,
                (Module::Decoupler { .. }, ModuleState::Decoupler { .. }) => true,
                (
                    Module::Command { .. } | Module::Tank { .. } | Module::LiftingSurface { .. },
                    ModuleState::Passive,
                ) => true,
                (Module::Parachute { parameters: p, .. }, ModuleState::Parachute { state: s }) =>
                    s.elapsed_seconds.is_finite()
                        && s.elapsed_seconds >= 0.0
                        && match s.phase {
                            crate::ParachutePhase::SemiDeploying =>
                                s.elapsed_seconds < p.semi_seconds,
                            crate::ParachutePhase::FullDeploying =>
                                s.elapsed_seconds < p.full_seconds,
                            _ => s.elapsed_seconds == 0.0,
                        },
                _ => false,
            },
            "part graph: module definition/state mismatch"
        );
    }
}
impl Part {
    pub fn thermally_failed(&self) -> bool {
        self.modules
            .values()
            .any(|m| matches!(m, ModuleState::Thermal { state } if state.failed))
    }
    pub fn resource(&self, r: ResourceId) -> f64 {
        self.resources.get(&r).copied().unwrap_or(0.0)
    }
    pub fn resource_mass(&self) -> f64 {
        self.resources.values().sum()
    }
    pub fn fuel_kg(&self) -> f64 {
        self.resource(ResourceId::LiquidPropellant)
    }
    pub fn lit(&self) -> bool {
        self.modules
            .values()
            .any(|m| matches!(m, ModuleState::Engine { enabled: true, .. }))
    }
    pub fn staged(&self) -> bool {
        let mut actions = self.modules.values().filter_map(|m| match m {
            ModuleState::Engine { activated, .. } | ModuleState::Decoupler { activated } => {
                Some(*activated)
            }
            _ => None,
        });
        let Some(first) = actions.next() else {
            return false;
        };
        first && actions.all(|a| a)
    }
}

fn check_transition(old: &ModuleState, new: &ModuleState) {
    use crate::ParachutePhase as P;
    let valid = match (old, new) {
        (ModuleState::Engine { activated: a, .. }, ModuleState::Engine { activated: b, .. })
        | (ModuleState::Decoupler { activated: a }, ModuleState::Decoupler { activated: b }) => {
            !a || *b
        }
        (ModuleState::Thermal { state: a }, ModuleState::Thermal { state: b }) => {
            !a.failed || b.failed
        }
        (
            ModuleState::Seat {
                occupant: a,
                packed_suit_thermal: ta,
            },
            ModuleState::Seat {
                occupant: b,
                packed_suit_thermal: tb,
            },
        ) => match (a, b, ta, tb) {
            (Some(a), Some(b), Some(ta), Some(tb)) => a.id == b.id && (!ta.failed || tb.failed),
            _ => a.is_none() || b.is_none(),
        },
        (ModuleState::Crew { .. }, ModuleState::Crew { .. })
        | (ModuleState::Wheel { .. }, ModuleState::Wheel { .. })
        | (ModuleState::Rcs { .. }, ModuleState::Rcs { .. })
        | (ModuleState::DockingPort { .. }, ModuleState::DockingPort { .. })
        | (ModuleState::Passive, ModuleState::Passive) => true,
        (ModuleState::Parachute { state: a }, ModuleState::Parachute { state: b }) => {
            if a.phase == b.phase {
                b.elapsed_seconds >= a.elapsed_seconds
            } else {
                matches!(
                    (a.phase, b.phase),
                    (P::Stowed, P::Armed)
                        | (P::Armed, P::SemiDeploying | P::Cut)
                        | (P::SemiDeploying, P::Semi | P::Cut)
                        | (P::Semi, P::FullDeploying | P::Cut)
                        | (P::FullDeploying, P::Full | P::Cut)
                        | (P::Full, P::Cut)
                )
            }
        }
        _ => false,
    };
    assert!(
        valid,
        "part graph: illegal module transition {old:?} -> {new:?}"
    );
}
impl Part {
    pub fn module_activated(&self, id: &str) -> bool {
        match self.modules.get(id).expect("unknown module") {
            ModuleState::Engine { activated, .. } | ModuleState::Decoupler { activated } => {
                *activated
            }
            ModuleState::Parachute { state } => state.phase != crate::ParachutePhase::Stowed,
            ModuleState::Seat { .. }
            | ModuleState::Crew { .. }
            | ModuleState::Wheel { .. }
            | ModuleState::Thermal { .. }
            | ModuleState::Rcs { .. }
            | ModuleState::DockingPort { .. }
            | ModuleState::Passive => false,
        }
    }
    pub fn decoupler_module(&self, id: &str) -> (&'static str, f64) {
        match self
            .definition
            .modules
            .iter()
            .find(|m| m.id() == id)
            .expect("unknown decoupler")
        {
            Module::Decoupler {
                node_id,
                impulse_ns,
                ..
            } => (node_id.as_str(), *impulse_ns),
            _ => panic!("not decoupler"),
        }
    }
}
pub fn default_module_stages(
    definition: &PartDefinition,
    stage: Option<u32>,
) -> BTreeMap<String, Option<u32>> {
    definition
        .modules
        .iter()
        .filter(|m| {
            matches!(
                m,
                Module::Engine { .. } | Module::Decoupler { .. } | Module::Parachute { .. }
            )
        })
        .map(|m| (m.id().to_string(), stage))
        .collect()
}
