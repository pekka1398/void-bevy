//! The part graph of flying vessels: every part with its own state, and the connections between
//! parts. A vessel is one connected group of it; separation and docking are graph operations.
//! See `docs/part-graph.md`.
use crate::{
    AttachNode, CompiledCraft, Connection, CrossfeedPart, Module, PartDefinition, PartPose,
    crossfeed_tanks, node,
};
use glam::DVec3;
use std::collections::{BTreeMap, HashSet};

/// One flying part.
#[derive(Clone, Debug)]
pub struct Part {
    pub id: String,
    pub definition: &'static PartDefinition,
    /// Propellant in the part's tanks; zero without one.
    pub fuel_kg: f64,
    pub stage: Option<u32>,
    /// Its stage has fired: an engine lit, a decoupler released.
    pub staged: bool,
    /// An engine that is burning when throttled; only engines can be lit.
    pub lit: bool,
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
}

impl Part {
    pub fn mass_kg(&self) -> f64 {
        self.definition.dry_mass_kg + self.fuel_kg
    }
    pub fn is_command(&self) -> bool {
        self.definition
            .modules
            .iter()
            .any(|m| matches!(m, Module::Command))
    }
    pub fn engine(&self) -> Option<EngineRating> {
        self.definition.modules.iter().find_map(|m| match m {
            Module::Engine {
                thrust_newtons,
                isp_seconds,
                nozzle_exit_area_m2,
                direction,
            } => Some(EngineRating {
                thrust_newtons: *thrust_newtons,
                isp_seconds: *isp_seconds,
                nozzle_exit_area_m2: *nozzle_exit_area_m2,
                direction: *direction,
            }),
            _ => None,
        })
    }
    /// The decoupler's node and separation impulse.
    pub fn decoupler(&self) -> Option<(&'static str, f64)> {
        self.definition.modules.iter().find_map(|m| match m {
            Module::Decoupler {
                node_id,
                impulse_ns,
            } => Some((node_id.as_str(), *impulse_ns)),
            _ => None,
        })
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
                    fuel_kg: p.instance.fuel_kg,
                    stage: p.instance.stage,
                    staged: false,
                    lit: false,
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
        assert!(
            part.fuel_kg.is_finite() && part.fuel_kg >= 0.0,
            "part graph: {} fuel {}",
            part.id,
            part.fuel_kg
        );
        assert!(
            !part.lit || part.engine().is_some(),
            "part graph: {} is lit but has no engine",
            part.id
        );
        let id = part.id.clone();
        assert!(
            self.parts.insert(id.clone(), part).is_none(),
            "part graph: duplicate part {id}"
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

    pub fn contains(&self, id: &str) -> bool {
        self.parts.contains_key(id)
    }
    pub fn part(&self, id: &str) -> &Part {
        self.parts
            .get(id)
            .unwrap_or_else(|| panic!("part graph: unknown part {id}"))
    }
    pub fn part_mut(&mut self, id: &str) -> &mut Part {
        self.parts
            .get_mut(id)
            .unwrap_or_else(|| panic!("part graph: unknown part {id}"))
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
        let parts: Vec<_> = members
            .iter()
            .map(|id| CrossfeedPart {
                id,
                definition: self.part(id).definition,
            })
            .collect();
        crossfeed_tanks(&parts, &self.connections, engine)
    }

    /// Summed in `ids` order.
    pub fn mass(&self, ids: &[String]) -> f64 {
        ids.iter().map(|id| self.part(id).mass_kg()).sum()
    }
}
