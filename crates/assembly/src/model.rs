use glam::{DQuat, DVec3};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;
use void_math::hypot;

pub const G0: f64 = 9.80665;
pub type ModelResult<T> = Result<T, String>;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AttachNode {
    pub id: String,
    pub position: DVec3,
    pub direction: DVec3,
    pub size: u32,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum Module {
    Command,
    Tank {
        #[serde(rename = "capacityKg")]
        capacity_kg: f64,
    },
    Engine {
        #[serde(rename = "thrustNewtons")]
        thrust_newtons: f64,
        #[serde(rename = "ispSeconds")]
        isp_seconds: f64,
        direction: DVec3,
    },
    Decoupler {
        #[serde(rename = "nodeId")]
        node_id: String,
        #[serde(rename = "impulseNs")]
        impulse_ns: f64,
    },
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    Command,
    Tank,
    Engine,
    Decoupler,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Shape {
    Cylinder,
    Cone,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PartDefinition {
    pub id: String,
    pub name: String,
    pub description: String,
    pub category: Category,
    pub dry_mass_kg: f64,
    pub height: f64,
    pub radius: f64,
    pub shape: Shape,
    pub color: String,
    pub crossfeed: bool,
    pub nodes: Vec<AttachNode>,
    pub modules: Vec<Module>,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Attachment {
    pub parent_id: String,
    pub parent_node_id: String,
    pub node_id: String,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PartInstance {
    pub id: String,
    pub definition_id: String,
    pub fuel_kg: f64,
    #[serde(deserialize_with = "explicit_option")]
    pub stage: Option<u32>,
    #[serde(deserialize_with = "explicit_option")]
    pub attachment: Option<Attachment>,
}
// Missing nullable fields are invalid data; explicit JSON null is legal.
fn explicit_option<'de, D, T>(d: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::deserialize(d)
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Craft {
    pub version: u32,
    pub name: String,
    pub parts: Vec<PartInstance>,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PartPose {
    pub position: DVec3,
    pub rotation: DQuat,
}
#[derive(Clone, Debug)]
pub struct PlacedPart {
    pub instance: PartInstance,
    pub definition: &'static PartDefinition,
    pub pose: PartPose,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Connection {
    pub a: String,
    pub node_a: String,
    pub b: String,
    pub node_b: String,
}
#[derive(Clone, Debug)]
pub struct CompiledCraft {
    pub craft: Craft,
    pub parts: Vec<PlacedPart>,
    pub connections: Vec<Connection>,
    pub root_id: String,
}
#[derive(Clone, Debug)]
pub struct FreeNode {
    pub part_id: String,
    pub node: &'static AttachNode,
    pub pose: PartPose,
}
#[derive(Clone, Copy, Debug)]
pub struct CraftSummary {
    pub dry_mass_kg: f64,
    pub fuel_kg: f64,
    pub mass_kg: f64,
    pub center: DVec3,
}

pub fn catalog() -> &'static [PartDefinition] {
    static CATALOG: OnceLock<Vec<PartDefinition>> = OnceLock::new();
    CATALOG.get_or_init(|| {
        serde_json::from_str(include_str!("../data/catalog.json"))
            .expect("invalid authored assembly catalog")
    })
}
pub fn definition(id: &str) -> ModelResult<&'static PartDefinition> {
    catalog()
        .iter()
        .find(|p| p.id == id)
        .ok_or_else(|| format!("Unknown part definition: {id}"))
}
pub fn node<'a>(part: &'a PartDefinition, id: &str) -> ModelResult<&'a AttachNode> {
    part.nodes
        .iter()
        .find(|n| n.id == id)
        .ok_or_else(|| format!("{} has no node {id}", part.id))
}
pub fn tank_capacity(part: &PartDefinition) -> f64 {
    part.modules
        .iter()
        .map(|m| match m {
            Module::Tank { capacity_kg } => *capacity_kg,
            _ => 0.0,
        })
        .sum()
}
pub fn actionable(part: &PartDefinition) -> bool {
    part.modules
        .iter()
        .any(|m| matches!(m, Module::Engine { .. } | Module::Decoupler { .. }))
}
pub fn part_inertia_per_kg(part: &PartDefinition) -> DVec3 {
    let side = (3.0 * part.radius * part.radius + part.height * part.height) / 12.0;
    DVec3::new(side, part.radius * part.radius / 2.0, side)
}
/// Same operation order as the lab's quaternion matrix multiplication.
pub fn rotate(q: DQuat, v: DVec3) -> DVec3 {
    let (x, y, z, w) = (q.x, q.y, q.z, q.w);
    DVec3::new(
        (1.0 - 2.0 * (y * y + z * z)) * v.x
            + 2.0 * (x * y - z * w) * v.y
            + 2.0 * (x * z + y * w) * v.z,
        2.0 * (x * y + z * w) * v.x
            + (1.0 - 2.0 * (x * x + z * z)) * v.y
            + 2.0 * (y * z - x * w) * v.z,
        2.0 * (x * z - y * w) * v.x
            + 2.0 * (y * z + x * w) * v.y
            + (1.0 - 2.0 * (x * x + y * y)) * v.z,
    )
}
fn align(from: DVec3, to: DVec3) -> DQuat {
    let a = from / hypot(from.to_array());
    let b = to / hypot(to.to_array());
    if a.dot(b) < -0.999999999 {
        let axis = a.cross(if a.x.abs() < 0.9 { DVec3::X } else { DVec3::Y });
        let axis = axis / hypot(axis.to_array());
        return DQuat::from_xyzw(axis.x, axis.y, axis.z, 0.0);
    }
    let axis = a.cross(b);
    let w = 1.0 + a.dot(b);
    let norm = hypot([axis.x, axis.y, axis.z, w]);
    DQuat::from_xyzw(axis.x / norm, axis.y / norm, axis.z / norm, w / norm)
}
/// Validate untrusted craft data and derive every part pose from its paired stack nodes.
pub fn compile(craft: &Craft) -> ModelResult<CompiledCraft> {
    if craft.version != 1 || craft.name.trim().is_empty() || craft.parts.is_empty() {
        return Err("Craft requires version 1, a name and at least one part".into());
    }
    if craft.parts.len() > 100 {
        return Err("This lab supports at most 100 parts".into());
    }
    let mut indices = HashMap::new();
    for (i, p) in craft.parts.iter().enumerate() {
        if p.id.is_empty()
            || !p
                .id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
            || indices.insert(p.id.clone(), i).is_some()
        {
            return Err(format!("Invalid or duplicate part id: {}", p.id));
        }
        let d = definition(&p.definition_id)?;
        if !p.fuel_kg.is_finite() || p.fuel_kg < 0.0 || p.fuel_kg > tank_capacity(d) {
            return Err(format!("{}: fuel outside capacity", p.id));
        }
        if p.stage.is_some_and(|s| s > 99 || !actionable(d)) {
            return Err(format!("{}: invalid stage", p.id));
        }
    }
    let roots: Vec<_> = craft
        .parts
        .iter()
        .filter(|p| p.attachment.is_none())
        .collect();
    if roots.len() != 1
        || !definition(&roots[0].definition_id)?
            .modules
            .iter()
            .any(|m| matches!(m, Module::Command))
    {
        return Err("Craft requires exactly one command root".into());
    }
    let root_id = roots[0].id.clone();
    struct Placer<'a> {
        craft: &'a Craft,
        indices: HashMap<String, usize>,
        parts: Vec<Option<PlacedPart>>,
        visiting: Vec<bool>,
        occupied: HashSet<(String, String)>,
        connections: Vec<Connection>,
    }
    impl Placer<'_> {
        fn claim(&mut self, id: &str, n: &str) -> ModelResult<()> {
            if !self.occupied.insert((id.into(), n.into())) {
                return Err(format!("Node already occupied: {id}:{n}"));
            }
            Ok(())
        }
        fn place(&mut self, i: usize) -> ModelResult<PlacedPart> {
            if let Some(p) = &self.parts[i] {
                return Ok(p.clone());
            }
            if self.visiting[i] {
                return Err("Attachment cycle".into());
            }
            self.visiting[i] = true;
            let p = self.craft.parts[i].clone();
            let d = definition(&p.definition_id)?;
            let mut pose = PartPose {
                position: DVec3::ZERO,
                rotation: DQuat::IDENTITY,
            };
            if let Some(a) = &p.attachment {
                let pi = *self
                    .indices
                    .get(&a.parent_id)
                    .ok_or_else(|| format!("Missing parent {}", a.parent_id))?;
                let parent = self.place(pi)?;
                let pn = node(parent.definition, &a.parent_node_id)?;
                let cn = node(d, &a.node_id)?;
                if pn.size != cn.size {
                    return Err("Incompatible node sizes".into());
                }
                self.claim(&parent.instance.id, &pn.id)?;
                self.claim(&p.id, &cn.id)?;
                let rotation = align(cn.direction, -rotate(parent.pose.rotation, pn.direction));
                pose = PartPose {
                    rotation,
                    position: parent.pose.position + rotate(parent.pose.rotation, pn.position)
                        - rotate(rotation, cn.position),
                };
                self.connections.push(Connection {
                    a: parent.instance.id.clone(),
                    node_a: pn.id.clone(),
                    b: p.id.clone(),
                    node_b: cn.id.clone(),
                });
            }
            let placed = PlacedPart {
                instance: p,
                definition: d,
                pose,
            };
            self.parts[i] = Some(placed.clone());
            self.visiting[i] = false;
            Ok(placed)
        }
    }
    let mut placer = Placer {
        craft,
        indices,
        parts: vec![None; craft.parts.len()],
        visiting: vec![false; craft.parts.len()],
        occupied: HashSet::new(),
        connections: vec![],
    };
    let parts = (0..craft.parts.len())
        .map(|i| placer.place(i))
        .collect::<ModelResult<Vec<_>>>()?;
    Ok(CompiledCraft {
        craft: craft.clone(),
        parts,
        connections: placer.connections,
        root_id,
    })
}
impl CompiledCraft {
    pub fn part(&self, id: &str) -> &PlacedPart {
        self.parts
            .iter()
            .find(|p| p.instance.id == id)
            .unwrap_or_else(|| panic!("Unknown part {id}"))
    }
    pub fn free_nodes(&self) -> Vec<FreeNode> {
        self.parts
            .iter()
            .flat_map(|p| {
                p.definition
                    .nodes
                    .iter()
                    .filter(|n| {
                        !self.connections.iter().any(|c| {
                            c.a == p.instance.id && c.node_a == n.id
                                || c.b == p.instance.id && c.node_b == n.id
                        })
                    })
                    .map(|n| FreeNode {
                        part_id: p.instance.id.clone(),
                        node: n,
                        pose: PartPose {
                            position: p.pose.position + rotate(p.pose.rotation, n.position),
                            rotation: p.pose.rotation,
                        },
                    })
            })
            .collect()
    }
    pub fn components(&self, cuts: &HashSet<String>) -> Vec<Vec<String>> {
        let mut remaining: HashSet<_> = self.parts.iter().map(|p| p.instance.id.clone()).collect();
        let mut out = vec![];
        for p in &self.parts {
            if !remaining.remove(&p.instance.id) {
                continue;
            }
            let mut group = vec![p.instance.id.clone()];
            let mut i = 0;
            while i < group.len() {
                for c in &self.connections {
                    if cuts.contains(&c.b) {
                        continue;
                    }
                    let next = if c.a == group[i] {
                        Some(&c.b)
                    } else if c.b == group[i] {
                        Some(&c.a)
                    } else {
                        None
                    };
                    if let Some(n) = next
                        && remaining.remove(n)
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
    pub fn fuel_sources(&self, engine_id: &str, cuts: &HashSet<String>) -> Vec<String> {
        let parts: Vec<_> = self
            .parts
            .iter()
            .map(|p| CrossfeedPart {
                id: &p.instance.id,
                definition: p.definition,
            })
            .collect();
        let connections: Vec<_> = self
            .connections
            .iter()
            .filter(|c| !cuts.contains(&c.b))
            .cloned()
            .collect();
        crossfeed_tanks(&parts, &connections, engine_id)
    }

    pub fn decoupler_connection(&self, id: &str) -> ModelResult<&Connection> {
        let p = self
            .parts
            .iter()
            .find(|p| p.instance.id == id)
            .ok_or_else(|| format!("Unknown decoupler {id}"))?;
        let n = p
            .definition
            .modules
            .iter()
            .find_map(|m| {
                if let Module::Decoupler { node_id, .. } = m {
                    Some(node_id)
                } else {
                    None
                }
            })
            .ok_or_else(|| format!("Unknown decoupler {id}"))?;
        self.connections
            .iter()
            .find(|c| c.a == id && c.node_a == *n || c.b == id && c.node_b == *n)
            .ok_or_else(|| format!("{id}: decoupler {n} node is not connected"))
    }
    pub fn summary(&self, fuel: Option<&HashMap<String, f64>>) -> CraftSummary {
        let mut out = CraftSummary {
            dry_mass_kg: 0.0,
            fuel_kg: 0.0,
            mass_kg: 0.0,
            center: DVec3::ZERO,
        };
        for p in &self.parts {
            let f = fuel.map_or(p.instance.fuel_kg, |fs| {
                *fs.get(&p.instance.id).expect("missing fuel state")
            });
            assert!(
                f.is_finite() && f >= 0.0 && f <= tank_capacity(p.definition),
                "invalid fuel state"
            );
            out.dry_mass_kg += p.definition.dry_mass_kg;
            out.fuel_kg += f;
            out.center += p.pose.position * (p.definition.dry_mass_kg + f);
        }
        out.mass_kg = out.dry_mass_kg + out.fuel_kg;
        out.center /= out.mass_kg;
        out
    }
}
/// A borrowed part in a fuel graph; no compiled craft, pose, or attachment tree is required.
#[derive(Clone, Copy, Debug)]
pub struct CrossfeedPart<'a> {
    pub id: &'a str,
    pub definition: &'a PartDefinition,
}

/// Returns reachable tanks in input part order, matching TS `crossfeedTanks`.
/// Both ends of each traversed connection must allow crossfeed. Connections to
/// parts outside this graph are ignored, so callers may pass a subset of a vessel.
/// Pass only active connections after separation. Cycles are supported.
///
/// Panics for duplicate part IDs or an engine ID without an engine module.
pub fn crossfeed_tanks(
    parts: &[CrossfeedPart<'_>],
    connections: &[Connection],
    engine_id: &str,
) -> Vec<String> {
    let mut by_id = HashMap::new();
    for p in parts {
        assert!(
            by_id.insert(p.id, p.definition).is_none(),
            "Duplicate part {}",
            p.id
        );
    }
    assert!(
        by_id
            .get(engine_id)
            .is_some_and(|d| d.modules.iter().any(|m| matches!(m, Module::Engine { .. }))),
        "Unknown engine {engine_id}"
    );
    let mut visited = HashSet::from([engine_id]);
    let mut queue = vec![engine_id];
    let mut i = 0;
    while i < queue.len() {
        for c in connections {
            let (Some(a), Some(b)) = (by_id.get(c.a.as_str()), by_id.get(c.b.as_str())) else {
                continue;
            };
            if !a.crossfeed || !b.crossfeed {
                continue;
            }
            let next = if c.a == queue[i] {
                Some(c.b.as_str())
            } else if c.b == queue[i] {
                Some(c.a.as_str())
            } else {
                None
            };
            if let Some(n) = next
                && visited.insert(n)
            {
                queue.push(n);
            }
        }
        i += 1;
    }
    parts
        .iter()
        .filter(|p| visited.contains(p.id) && tank_capacity(p.definition) > 0.0)
        .map(|p| p.id.to_string())
        .collect()
}

pub fn add_part(
    craft: &Craft,
    definition_id: &str,
    parent_id: &str,
    parent_node_id: &str,
    node_id: &str,
) -> ModelResult<Craft> {
    compile(craft)?;
    let d = definition(definition_id)?;
    let mut i = 1;
    while craft.parts.iter().any(|p| p.id == format!("p{i}")) {
        i += 1;
    }
    let mut next = craft.clone();
    next.parts.push(PartInstance {
        id: format!("p{i}"),
        definition_id: definition_id.into(),
        fuel_kg: tank_capacity(d),
        stage: actionable(d).then_some(0),
        attachment: Some(Attachment {
            parent_id: parent_id.into(),
            parent_node_id: parent_node_id.into(),
            node_id: node_id.into(),
        }),
    });
    compile(&next)?;
    Ok(next)
}
pub fn remove_subtree(craft: &Craft, id: &str) -> ModelResult<Craft> {
    let c = compile(craft)?;
    if id == c.root_id {
        return Err("Keep the command root; use New to clear the craft".into());
    }
    if !craft.parts.iter().any(|p| p.id == id) {
        return Err(format!("Unknown part {id}"));
    }
    let mut removed = HashSet::from([id.to_string()]);
    loop {
        let old = removed.len();
        for p in &craft.parts {
            if p.attachment
                .as_ref()
                .is_some_and(|a| removed.contains(&a.parent_id))
            {
                removed.insert(p.id.clone());
            }
        }
        if removed.len() == old {
            break;
        }
    }
    let mut next = craft.clone();
    next.parts.retain(|p| !removed.contains(&p.id));
    Ok(next)
}
pub fn fresh_craft() -> Craft {
    Craft {
        version: 1,
        name: "Untitled rocket".into(),
        parts: vec![PartInstance {
            id: "p1".into(),
            definition_id: "pod".into(),
            fuel_kg: 0.0,
            stage: None,
            attachment: None,
        }],
    }
}
pub fn demo_craft() -> Craft {
    let mut c = fresh_craft();
    c.name = "Two-stage test rocket".into();
    for (d, p) in [
        ("tank-small", "p1"),
        ("engine-small", "p2"),
        ("decoupler", "p3"),
        ("tank-large", "p4"),
        ("engine-large", "p5"),
    ] {
        c = add_part(&c, d, p, "bottom", "top").expect("authored demo");
    }
    c.parts[2].stage = Some(1);
    c.parts[3].stage = Some(1);
    c
}
pub fn import_craft(text: &str) -> ModelResult<Craft> {
    let c: Craft = serde_json::from_str(text).map_err(|e| e.to_string())?;
    compile(&c)?;
    Ok(c)
}
pub fn export_craft(craft: &Craft) -> ModelResult<String> {
    compile(craft)?;
    serde_json::to_string_pretty(craft).map_err(|e| e.to_string())
}
