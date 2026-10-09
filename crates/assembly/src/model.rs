use glam::{DQuat, DVec3};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::OnceLock;

pub const G0: f64 = 9.80665;
pub type ModelResult<T> = Result<T, String>;

/// Mass-bearing resources; authored IDs are deliberately a closed vocabulary.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub enum ResourceId {
    LiquidPropellant,
    Monopropellant,
    Ablator,
}
pub type Resources = BTreeMap<ResourceId, f64>;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AttachNode {
    pub id: String,
    pub position: DVec3,
    pub direction: DVec3,
    pub size: u32,
    /// Named capacity slot for arbitrary placement on an authored cuboid surface.
    #[serde(default, skip_serializing_if = "is_false")]
    pub surface: bool,
}
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ControlProfile {
    #[default]
    Flight,
    Aircraft,
    Rover,
    Eva,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum Module {
    Seat {
        id: String,
        parameters: crate::SeatDefinition,
    },
    Crew {
        id: String,
        parameters: crate::EvaDefinition,
    },
    Wheel {
        id: String,
        parameters: crate::WheelDefinition,
    },
    Thermal {
        id: String,
        parameters: crate::ThermalDefinition,
    },
    Rcs {
        id: String,
        resource: ResourceId,
        #[serde(rename = "thrustNewtons")]
        thrust_newtons: f64,
        #[serde(rename = "ispSeconds")]
        isp_seconds: f64,
        direction: DVec3,
        point: DVec3,
    },
    DockingPort {
        id: String,
        #[serde(rename = "nodeId")]
        node_id: String,
        parameters: DockingDefinition,
    },
    LiftingSurface {
        id: String,
        parameters: LiftingSurfaceDefinition,
    },
    Parachute {
        id: String,
        parameters: ParachuteDefinition,
    },
    Command {
        id: String,
        #[serde(default, rename = "controlProfile")]
        control_profile: ControlProfile,
        #[serde(default, rename = "requiresCrew")]
        requires_crew: bool,
        #[serde(default = "default_true", rename = "reactionWheel")]
        reaction_wheel: bool,
    },
    Tank {
        id: String,
        resource: ResourceId,
        #[serde(rename = "capacityKg")]
        capacity_kg: f64,
    },
    Engine {
        id: String,
        resource: ResourceId,
        #[serde(rename = "thrustNewtons")]
        thrust_newtons: f64,
        #[serde(rename = "ispSeconds")]
        isp_seconds: f64,
        /// F = F_vac − area × ambient pressure.
        #[serde(rename = "nozzleExitAreaM2")]
        nozzle_exit_area_m2: f64,
        direction: DVec3,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        jet: Option<JetDefinition>,
    },
    Decoupler {
        id: String,
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
    Structure,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Shape {
    Cylinder,
    Cone,
    Box,
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
    /// Explicit full cuboid dimensions. Absent preserves legacy square-section geometry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub box_size_meters: Option<DVec3>,
    pub color: String,
    pub crossfeed: bool,
    pub nodes: Vec<AttachNode>,
    pub modules: Vec<Module>,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Attachment {
    pub parent_id: String,
    pub parent_node_id: String,
    pub node_id: String,
    /// Twist about the parent socket outward normal after normal alignment.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub twist_radians: f64,
    /// Explicit parent-relative welded pose at the named surface socket.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pose: Option<PartPose>,
}
fn default_true() -> bool {
    true
}
fn is_false(value: &bool) -> bool {
    !*value
}
fn is_zero(value: &f64) -> bool {
    *value == 0.0
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PartInstance {
    pub id: String,
    pub definition_id: String,
    #[serde(deserialize_with = "unique_map")]
    pub resources: Resources,
    /// Explicit per-module overrides of the authored part's default stage. Empty means inherit.
    #[serde(default, deserialize_with = "unique_map")]
    pub module_stages: BTreeMap<String, Option<u32>>,
    #[serde(deserialize_with = "explicit_option")]
    pub stage: Option<u32>,
    #[serde(deserialize_with = "explicit_option")]
    pub attachment: Option<Attachment>,
}
// Missing nullable fields are invalid data; explicit JSON null is legal.
pub(super) fn explicit_option<'de, D, T>(d: D) -> Result<Option<T>, D::Error>
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
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
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
            Module::Tank { capacity_kg, .. } => *capacity_kg,
            _ => 0.0,
        })
        .sum()
}
pub fn actionable(part: &PartDefinition) -> bool {
    part.modules.iter().any(|m| {
        matches!(
            m,
            Module::Engine { .. } | Module::Decoupler { .. } | Module::Parachute { .. }
        )
    })
}
pub fn part_box_size(part: &PartDefinition) -> DVec3 {
    assert_eq!(
        part.shape,
        Shape::Box,
        "box dimensions requested for non-box"
    );
    part.box_size_meters.unwrap_or(DVec3::new(
        2.0 * part.radius,
        part.height,
        2.0 * part.radius,
    ))
}
/// Bounding radius about the authored part origin, shared by contact activation.
pub fn part_bound_radius(part: &PartDefinition) -> f64 {
    if let Some(size) = part.box_size_meters {
        size.length() / 2.0
    } else if part.shape == Shape::Box {
        (part.height / 2.0).hypot(part.radius * 2.0_f64.sqrt())
    } else {
        (part.height / 2.0).hypot(part.radius)
    }
}
pub fn part_inertia_per_kg(part: &PartDefinition) -> DVec3 {
    if part.shape == Shape::Box {
        if let Some(size) = part.box_size_meters {
            return DVec3::new(
                size.y * size.y + size.z * size.z,
                size.x * size.x + size.z * size.z,
                size.x * size.x + size.y * size.y,
            ) / 12.0;
        }
        let side = (4.0 * part.radius.powi(2) + part.height.powi(2)) / 12.0;
        return DVec3::new(side, 2.0 * part.radius.powi(2) / 3.0, side);
    }
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
    let a = from / from.length();
    let b = to / to.length();
    if a.dot(b) < -0.999999999 {
        let axis = a.cross(if a.x.abs() < 0.9 { DVec3::X } else { DVec3::Y });
        let axis = axis / axis.length();
        return DQuat::from_xyzw(axis.x, axis.y, axis.z, 0.0);
    }
    let axis = a.cross(b);
    let w = 1.0 + a.dot(b);
    let norm = glam::DVec4::new(axis.x, axis.y, axis.z, w).length();
    DQuat::from_xyzw(axis.x / norm, axis.y / norm, axis.z / norm, w / norm)
}
/// Validate untrusted craft data and derive every part pose from its paired stack nodes.
pub fn compile(craft: &Craft) -> ModelResult<CompiledCraft> {
    if !matches!(craft.version, 2 | 3) || craft.name.trim().is_empty() || craft.parts.is_empty() {
        return Err("Craft requires version 2 or 3, a name and at least one part".into());
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
        validate_definition(d)?;
        if craft.version == 2
            && (d.box_size_meters.is_some()
                || p.attachment
                    .as_ref()
                    .is_some_and(|a| a.pose.is_some() || a.twist_radians != 0.0))
        {
            return Err("Explicit geometry and attachment poses require craft version 3".into());
        }
        for (id, stage) in &p.module_stages {
            if !d.modules.iter().any(|m| {
                m.id() == id
                    && matches!(
                        m,
                        Module::Engine { .. } | Module::Decoupler { .. } | Module::Parachute { .. }
                    )
            }) || stage.is_some_and(|s| s > 99)
            {
                return Err(format!("{}: invalid module stage {id}", p.id));
            }
        }
        validate_resources(d, &p.resources).map_err(|e| format!("{}: {e}", p.id))?;
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
            .any(|m| matches!(m, Module::Command { .. }))
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
                if !a.twist_radians.is_finite() {
                    return Err("Non-finite attachment twist".into());
                }
                if a.pose.is_some() && a.twist_radians != 0.0 {
                    return Err("Surface pose and node twist are mutually exclusive".into());
                }
                let base = align(cn.direction, -rotate(parent.pose.rotation, pn.direction));
                let rotation = if a.twist_radians == 0.0 {
                    base
                } else {
                    DQuat::from_axis_angle(
                        rotate(parent.pose.rotation, pn.direction).normalize(),
                        a.twist_radians,
                    ) * base
                };
                pose = PartPose {
                    rotation,
                    position: parent.pose.position + rotate(parent.pose.rotation, pn.position)
                        - rotate(rotation, cn.position),
                };
                if let Some(local) = a.pose {
                    if !local.position.is_finite()
                        || !local.rotation.is_finite()
                        || (local.rotation.length() - 1.0).abs() > 1e-9
                    {
                        return Err("Invalid surface attachment pose".into());
                    }
                    if !pn.surface || parent.definition.shape != Shape::Box {
                        return Err(
                            "Surface pose requires an authored cuboid surface socket".into()
                        );
                    }
                    let point = local.position + local.rotation * cn.position;
                    let half = part_box_size(parent.definition) / 2.0;
                    if (point.abs() - half).max_element() > 1e-6 {
                        return Err("Surface attachment point outside parent collider".into());
                    }
                    let distance = (point.abs() - half).abs();
                    let axis = if distance.x <= distance.y && distance.x <= distance.z {
                        0
                    } else if distance.y <= distance.z {
                        1
                    } else {
                        2
                    };
                    if distance[axis] > 1e-6 {
                        return Err(
                            "Surface attachment point is not on parent collider boundary".into(),
                        );
                    }
                    let mut normal = DVec3::ZERO;
                    normal[axis] = point[axis].signum();
                    if (local.rotation * cn.direction).normalize().dot(normal) > -1.0 + 1e-8 {
                        return Err("Surface attachment normal does not face parent surface".into());
                    }
                    pose = PartPose {
                        position: parent.pose.position + parent.pose.rotation * local.position,
                        rotation: parent.pose.rotation * local.rotation,
                    };
                }
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
            let f = fuel.map_or(p.instance.resource_mass(), |fs| {
                *fs.get(&p.instance.id).expect("missing fuel state")
            });
            assert!(
                f.is_finite() && f >= 0.0 && f <= tank_capacity(p.definition),
                "invalid fuel state"
            );
            out.dry_mass_kg += p.definition.dry_mass_kg + crate::initial_crew_mass_kg(p.definition);
            out.fuel_kg += f;
            out.center += p.pose.position
                * (p.definition.dry_mass_kg + crate::initial_crew_mass_kg(p.definition) + f);
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
        resources: full_resources(d),
        module_stages: BTreeMap::new(),
        stage: actionable(d).then_some(0),
        attachment: Some(Attachment {
            parent_id: parent_id.into(),
            parent_node_id: parent_node_id.into(),
            node_id: node_id.into(),
            twist_radians: 0.0,
            pose: None,
        }),
    });
    compile(&next)?;
    Ok(next)
}
/// An authored mount request for a future editor or main-game craft builder.
#[derive(Clone, Debug)]
pub struct SurfaceMount {
    pub definition_id: String,
    pub parent_socket_id: String,
    pub node_id: String,
    pub pose: PartPose,
}
/// Add a uniquely identified part on a named surface capacity slot.
pub fn mount_surface(craft: &Craft, parent_id: &str, mount: &SurfaceMount) -> ModelResult<Craft> {
    if craft.version != 3 {
        return Err("Surface mounting requires craft version 3".into());
    }
    let added = add_part(
        craft,
        &mount.definition_id,
        parent_id,
        &mount.parent_socket_id,
        &mount.node_id,
    )?;
    let id = &added.parts.last().expect("added part").id;
    set_attachment_pose(&added, id, mount.pose)
}
/// Add an authored opposite-handed pair atomically. Each part receives a fresh stable ID.
/// The negative-x counterpart supplies its own mount node and geometry/control definition.
pub fn mount_mirrored_pair(
    craft: &Craft,
    parent_id: &str,
    positive: &SurfaceMount,
    negative_definition_id: &str,
    negative_parent_socket_id: &str,
    negative_node_id: &str,
) -> ModelResult<Craft> {
    let added = mount_surface(craft, parent_id, positive)?;
    mount_surface(
        &added,
        parent_id,
        &SurfaceMount {
            definition_id: negative_definition_id.into(),
            parent_socket_id: negative_parent_socket_id.into(),
            node_id: negative_node_id.into(),
            pose: mirror_pose_x(positive.pose)?,
        },
    )
}
/// Change a welded surface attachment without mutating a live PartGraph or cached poses.
/// Named sockets retain stable graph connection and supply semantics.
pub fn set_attachment_pose(craft: &Craft, id: &str, pose: PartPose) -> ModelResult<Craft> {
    compile(craft)?;
    let mut next = craft.clone();
    let part = next
        .parts
        .iter_mut()
        .find(|p| p.id == id)
        .ok_or("Unknown part")?;
    let attachment = part
        .attachment
        .as_mut()
        .ok_or("Cannot place command root")?;
    attachment.pose = Some(pose);
    attachment.twist_radians = 0.0;
    compile(&next)?;
    Ok(next)
}
/// Rotate a node attachment about its parent socket's normal; inputs are radians.
pub fn set_attachment_twist(craft: &Craft, id: &str, radians: f64) -> ModelResult<Craft> {
    compile(craft)?;
    let mut next = craft.clone();
    let part = next
        .parts
        .iter_mut()
        .find(|p| p.id == id)
        .ok_or("Unknown part")?;
    let attachment = part
        .attachment
        .as_mut()
        .ok_or("Cannot rotate command root")?;
    attachment.pose = None;
    attachment.twist_radians = radians;
    compile(&next)?;
    Ok(next)
}
/// Bilateral pose about the parent-local yz plane. The geometry must itself be mirror symmetric;
/// asymmetric aerodynamic/control definitions need their authored opposite-handed counterpart.
pub fn mirror_pose_x(pose: PartPose) -> ModelResult<PartPose> {
    if !pose.position.is_finite()
        || !pose.rotation.is_finite()
        || (pose.rotation.length() - 1.0).abs() > 1e-9
    {
        return Err("Invalid mirror pose".into());
    }
    let mirror = glam::DMat3::from_diagonal(DVec3::new(-1.0, 1.0, 1.0));
    let rotation = DQuat::from_mat3(&(mirror * glam::DMat3::from_quat(pose.rotation) * mirror));
    Ok(PartPose {
        position: mirror * pose.position,
        rotation,
    })
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
        version: 2,
        name: "Untitled rocket".into(),
        parts: vec![PartInstance {
            id: "p1".into(),
            definition_id: "pod".into(),
            resources: Resources::new(),
            module_stages: BTreeMap::new(),
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

/// The main game's original 7620 kg, 9.6 km/s rocket, in the editor's portable craft format.
pub fn flight_rocket() -> Craft {
    import_craft(include_str!("../data/flight-rocket.json")).expect("authored flight rocket")
}

/// Main-game rocket with an addressed nose port and a separate finite RCS supply.
/// Preserve the original flight fixture and all engine, tank, leg and attachment geometry.
pub fn rcs_flight_rocket() -> Craft {
    let mut craft = flight_rocket();
    craft.name = "VOID two-stage rendezvous rocket".into();
    let pod = craft
        .parts
        .iter_mut()
        .find(|p| p.id == "p1")
        .expect("flight command pod");
    assert_eq!(pod.definition_id, "flight-pod");
    pod.definition_id = "flight-rcs-pod".into();
    pod.resources = full_resources(definition(&pod.definition_id).expect("flight RCS pod"));
    compile(&craft).expect("authored RCS flight rocket");
    craft
}

/// A command pod with a finite ablative disk on its bottom stack node.
pub fn reentry_capsule() -> Craft {
    let craft = Craft {
        version: 2,
        name: "VOID shielded reentry capsule".into(),
        parts: vec![
            PartInstance {
                id: "pod".into(),
                definition_id: "flight-rcs-pod".into(),
                resources: full_resources(definition("flight-rcs-pod").unwrap()),
                module_stages: BTreeMap::new(),
                stage: None,
                attachment: None,
            },
            PartInstance {
                id: "shield".into(),
                definition_id: "heat-shield".into(),
                resources: full_resources(definition("heat-shield").unwrap()),
                module_stages: BTreeMap::new(),
                stage: None,
                attachment: Some(Attachment {
                    parent_id: "pod".into(),
                    parent_node_id: "bottom".into(),
                    node_id: "top".into(),
                    twist_radians: 0.0,
                    pose: None,
                }),
            },
        ],
    };
    compile(&craft).expect("authored reentry capsule");
    craft
}

impl Module {
    pub fn id(&self) -> &str {
        match self {
            Self::Seat { id, .. }
            | Self::Crew { id, .. }
            | Self::Wheel { id, .. }
            | Self::Thermal { id, .. }
            | Self::Rcs { id, .. }
            | Self::DockingPort { id, .. }
            | Self::Command { id, .. }
            | Self::Tank { id, .. }
            | Self::Engine { id, .. }
            | Self::Decoupler { id, .. }
            | Self::Parachute { id, .. }
            | Self::LiftingSurface { id, .. } => id,
        }
    }
}
impl PartInstance {
    pub fn resource_mass(&self) -> f64 {
        self.resources.values().sum()
    }
}
pub fn capacity(part: &PartDefinition, resource: ResourceId) -> f64 {
    part.modules
        .iter()
        .filter_map(|m| match m {
            Module::Tank {
                resource: r,
                capacity_kg,
                ..
            } if *r == resource => Some(*capacity_kg),
            _ => None,
        })
        .sum()
}
pub fn full_resources(part: &PartDefinition) -> Resources {
    let mut out = Resources::new();
    for m in &part.modules {
        if let Module::Tank {
            resource,
            capacity_kg,
            ..
        } = m
        {
            *out.entry(*resource).or_insert(0.0) += capacity_kg;
        }
    }
    out
}
pub fn validate_resources(part: &PartDefinition, resources: &Resources) -> ModelResult<()> {
    let capacities = full_resources(part);
    if capacities.keys().ne(resources.keys()) {
        return Err("resource inventory does not match tank definitions".into());
    }
    for (r, q) in resources {
        if !q.is_finite() || *q < 0.0 || *q > capacities[r] {
            return Err(format!("{r:?}: quantity outside capacity"));
        }
    }
    Ok(())
}
/// Explicit, offline conversion of the historical single-liquid craft schema.
pub fn migrate_legacy_craft(value: serde_json::Value) -> ModelResult<Craft> {
    let mut value = value;
    if value["version"] != 1 {
        return Err("migration requires craft version 1".into());
    }
    value["version"] = 2.into();
    for p in value["parts"].as_array_mut().ok_or("parts must be array")? {
        let amount = p
            .as_object_mut()
            .ok_or("part must be object")?
            .remove("fuelKg")
            .ok_or("missing fuelKg")?;
        let d = definition(p["definitionId"].as_str().ok_or("missing definitionId")?)?;
        p["resources"] = if tank_capacity(d) > 0.0 {
            serde_json::json!({"liquidPropellant":amount})
        } else {
            if amount.as_f64() != Some(0.0) {
                return Err("non-tank legacy fuel".into());
            }
            serde_json::json!({})
        };
    }
    let craft: Craft = serde_json::from_value(value).map_err(|e| e.to_string())?;
    compile(&craft)?;
    Ok(craft)
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ParachuteDefinition {
    /// Aerodynamic attachment point in part axes, metres.
    pub point: DVec3,
    pub semi_area_m2: f64,
    pub full_area_m2: f64,
    pub drag_coefficient: f64,
    pub min_pressure_pa: f64,
    pub max_dynamic_pressure_pa: f64,
    pub full_deploy_altitude_meters: f64,
    pub semi_seconds: f64,
    pub full_seconds: f64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ParachutePhase {
    Stowed,
    Armed,
    SemiDeploying,
    Semi,
    FullDeploying,
    Full,
    Cut,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParachuteState {
    pub phase: ParachutePhase,
    pub elapsed_seconds: f64,
}
impl ParachuteState {
    pub const STOWED: Self = Self {
        phase: ParachutePhase::Stowed,
        elapsed_seconds: 0.0,
    };
}
/// Atmosphere-fed engine rating; thrust and fuel flow track density, no vacuum operation.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JetDefinition {
    pub reference_density_kg_m3: f64,
    pub minimum_density_kg_m3: f64,
}
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SurfaceControl {
    #[default]
    Fixed,
    Pitch,
    Roll,
    Yaw,
}
fn default_control_sign() -> f64 {
    1.0
}
/// Fixed aerodynamic section in part axes; offset is its centre of pressure from part origin.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LiftingSurfaceDefinition {
    pub point: DVec3,
    pub chord: DVec3,
    pub normal: DVec3,
    pub area_m2: f64,
    pub aspect_ratio: f64,
    pub chord_meters: f64,
    pub sweep_radians: f64,
    pub incidence_radians: f64,
    pub zero_lift_radians: f64,
    pub stall_radians: f64,
    pub cd0: f64,
    pub efficiency: f64,
    pub pitching_moment: f64,
    #[serde(default)]
    pub control: SurfaceControl,
    #[serde(default = "default_control_sign")]
    pub control_sign: f64,
    #[serde(default)]
    pub max_deflection_radians: f64,
}
pub fn validate_definition(d: &PartDefinition) -> ModelResult<()> {
    if let Some(size) = d.box_size_meters
        && (d.shape != Shape::Box || !size.is_finite() || size.min_element() <= 0.0)
    {
        return Err("boxSizeMeters requires positive finite Box dimensions".into());
    }
    if d.modules
        .iter()
        .filter(|m| matches!(m, Module::Thermal { .. }))
        .count()
        > 1
    {
        return Err("only one thermal module per part".into());
    }
    if d.modules
        .iter()
        .any(|m| matches!(m, Module::Thermal { parameters, .. } if parameters.ablation.is_some()))
        && capacity(d, ResourceId::Ablator) <= 0.0
    {
        return Err("ablative thermal module requires an ablator tank".into());
    }
    let mut ids = HashSet::new();
    for m in &d.modules {
        if m.id().is_empty() || !ids.insert(m.id()) {
            return Err("empty/duplicate module ID".into());
        }
        match m {
            Module::Crew { parameters, .. } => {
                parameters.validate()?;
                if d.shape != Shape::Box
                    || capacity(d, ResourceId::Monopropellant) <= 0.0
                    || !d.modules.iter().any(|m| {
                        matches!(
                            m,
                            Module::Command {
                                control_profile: ControlProfile::Eva,
                                ..
                            }
                        )
                    })
                {
                    return Err("EVA crew requires authored Box body, backpack tank and EVA command profile".into());
                }
            }
            Module::Seat { parameters, .. } => {
                parameters.validate()?;
                if capacity(d, ResourceId::Monopropellant) <= 0.0 || d.crossfeed {
                    return Err(
                        "crew seat requires isolated authored backpack monopropellant tank".into(),
                    );
                }
            }
            Module::Wheel { parameters, .. } => {
                parameters.validate()?;
            }
            Module::Thermal { parameters, .. } => {
                parameters.validate()?;
            }
            Module::Rcs {
                thrust_newtons,
                isp_seconds,
                direction,
                point,
                ..
            } if !thrust_newtons.is_finite()
                || *thrust_newtons <= 0.0
                || !isp_seconds.is_finite()
                || *isp_seconds <= 0.0
                || !point.is_finite()
                || !direction.is_finite()
                || (direction.length() - 1.0).abs() > 1e-9 =>
            {
                return Err("invalid RCS nozzle".into());
            }
            Module::DockingPort {
                node_id,
                parameters: p,
                ..
            } => {
                node(d, node_id)?;
                if [
                    p.capture_distance_m,
                    p.max_angle_radians,
                    p.max_speed_mps,
                    p.max_spin_radians_per_second,
                ]
                .iter()
                .any(|v| !v.is_finite() || *v <= 0.0)
                    || p.max_angle_radians > std::f64::consts::FRAC_PI_2
                    || !p.separation_impulse_ns.is_finite()
                    || p.separation_impulse_ns < 0.0
                {
                    return Err("invalid docking parameters".into());
                }
            }
            Module::Engine { jet: Some(jet), .. }
                if !jet.reference_density_kg_m3.is_finite()
                    || jet.reference_density_kg_m3 <= 0.0
                    || !jet.minimum_density_kg_m3.is_finite()
                    || jet.minimum_density_kg_m3 < 0.0
                    || jet.minimum_density_kg_m3 >= jet.reference_density_kg_m3 =>
            {
                return Err("Invalid jet density range".into());
            }
            Module::LiftingSurface { parameters: p, .. }
                if !p.control_sign.is_finite()
                    || p.control_sign.abs() != 1.0
                    || !p.max_deflection_radians.is_finite()
                    || !(0.0..=std::f64::consts::FRAC_PI_2).contains(&p.max_deflection_radians)
                    || (p.control == SurfaceControl::Fixed && p.max_deflection_radians != 0.0)
                    || (p.control != SurfaceControl::Fixed && p.max_deflection_radians == 0.0)
                    || !p.point.is_finite()
                    || !p.chord.is_finite()
                    || !p.normal.is_finite()
                    || (p.chord.length() - 1.0).abs() > 1e-8
                    || (p.normal.length() - 1.0).abs() > 1e-8
                    || p.chord.dot(p.normal).abs() > 1e-8
                    || [
                        p.area_m2,
                        p.aspect_ratio,
                        p.chord_meters,
                        p.stall_radians,
                        p.efficiency,
                    ]
                    .iter()
                    .any(|v| !v.is_finite() || *v <= 0.0)
                    || [
                        p.sweep_radians,
                        p.incidence_radians,
                        p.zero_lift_radians,
                        p.cd0,
                        p.pitching_moment,
                    ]
                    .iter()
                    .any(|v| !v.is_finite())
                    || p.cd0 < 0.0
                    || p.efficiency > 1.0
                    || p.stall_radians >= std::f64::consts::FRAC_PI_2
                    || p.sweep_radians.abs() >= std::f64::consts::FRAC_PI_2 =>
            {
                return Err("invalid lifting surface parameters".into());
            }
            Module::Tank { capacity_kg, .. } if !capacity_kg.is_finite() || *capacity_kg <= 0.0 => {
                return Err("invalid tank capacity".into());
            }
            Module::Engine {
                thrust_newtons,
                isp_seconds,
                nozzle_exit_area_m2,
                direction,
                ..
            } if !thrust_newtons.is_finite()
                || *thrust_newtons <= 0.0
                || !isp_seconds.is_finite()
                || *isp_seconds <= 0.0
                || !nozzle_exit_area_m2.is_finite()
                || *nozzle_exit_area_m2 < 0.0
                || !direction.is_finite()
                || (direction.length() - 1.0).abs() > 1e-9 =>
            {
                return Err("invalid engine rating".into());
            }
            Module::Parachute { parameters: p, .. }
                if !p.point.is_finite()
                    || [
                        p.semi_area_m2,
                        p.full_area_m2,
                        p.drag_coefficient,
                        p.min_pressure_pa,
                        p.max_dynamic_pressure_pa,
                        p.full_deploy_altitude_meters,
                        p.semi_seconds,
                        p.full_seconds,
                    ]
                    .iter()
                    .any(|v| !v.is_finite() || *v <= 0.0)
                    || p.full_area_m2 < p.semi_area_m2 =>
            {
                return Err("invalid parachute parameters".into());
            }
            _ => {}
        }
    }
    Ok(())
}

/// Maps in authored/saved state must reject duplicate JSON keys before insertion.
/// Shared publicly with Fleet SavedPart; parsing through serde_json::Value first loses this check.
pub fn unique_map<'de, D, K, V>(deserializer: D) -> Result<BTreeMap<K, V>, D::Error>
where
    D: serde::Deserializer<'de>,
    K: Deserialize<'de> + Ord,
    V: Deserialize<'de>,
{
    struct Unique<K, V>(std::marker::PhantomData<(K, V)>);
    impl<'de, K: Deserialize<'de> + Ord, V: Deserialize<'de>> serde::de::Visitor<'de> for Unique<K, V> {
        type Value = BTreeMap<K, V>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a map with unique keys")
        }
        fn visit_map<A: serde::de::MapAccess<'de>>(
            self,
            mut map: A,
        ) -> Result<Self::Value, A::Error> {
            let mut out = BTreeMap::new();
            while let Some((key, value)) = map.next_entry()? {
                if out.insert(key, value).is_some() {
                    return Err(serde::de::Error::custom("duplicate state map key"));
                }
            }
            Ok(out)
        }
    }
    deserializer.deserialize_map(Unique(std::marker::PhantomData))
}

/// Capture is inelastic: poses remain unchanged and the compound preserves momentum.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DockingDefinition {
    pub capture_distance_m: f64,
    pub max_angle_radians: f64,
    pub max_speed_mps: f64,
    pub max_spin_radians_per_second: f64,
    pub separation_impulse_ns: f64,
}

/// A self-contained near-rendezvous vehicle, using the same authored catalog as other craft.
pub fn rendezvous_pod() -> Craft {
    let mut craft = fresh_craft();
    craft.name = "Rendezvous pod".into();
    craft.parts[0].definition_id = "rcs-pod".into();
    craft.parts[0].resources = full_resources(definition("rcs-pod").expect("authored pod"));
    craft
}

/// Configurable four-wheel main-game acceptance craft, using ordinary surface assembly APIs.
pub fn rover() -> Craft {
    import_craft(include_str!("../data/rover.json")).expect("invalid rover fixture")
}

/// Rover with a dedicated crew seat and isolated backpack tank, ordinary surface assembly.
pub fn crew_rover() -> Craft {
    let mut base = rover();
    base.parts[0].definition_id = "rover-crewed-chassis".into();
    mount_surface(
        &base,
        "chassis",
        &SurfaceMount {
            definition_id: "crew-seat".into(),
            parent_socket_id: "surface-4".into(),
            node_id: "mount".into(),
            pose: PartPose {
                position: DVec3::new(0.0, 0.675, 0.0),
                rotation: DQuat::IDENTITY,
            },
        },
    )
    .expect("invalid crew rover fixture")
}
/// Existing two-stage rocket carrying the same reusable seat/hatch and finite backpack tank.
/// The external seat is an explicit first-round fixture; there is no cabin traversal model.
pub fn crewed_flight_rocket() -> Craft {
    let mut base = rcs_flight_rocket();
    base.version = 3;
    base.name = "VOID crewed two-stage rocket".into();
    base.parts
        .iter_mut()
        .find(|p| p.id == "p1")
        .unwrap()
        .definition_id = "crewed-flight-rcs-pod".into();
    add_part(&base, "crew-seat", "p1", "crew-seat", "side").expect("invalid crewed rocket fixture")
}
pub fn eva_suit() -> Craft {
    import_craft(include_str!("../data/eva-suit.json")).expect("invalid EVA suit")
}
