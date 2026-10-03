use crate::{
    EnvironmentPart, EnvironmentSample, FleetEnvironment, FreeFallFrame, Propulsion,
    PropulsionPart, burn, propulsion, step_thrust,
};
use glam::{DMat3, DQuat, DVec3};
use rapier3d::prelude::RigidBodyHandle;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;
use void_assembly::{
    Connection, Craft, Module, PartDefinition, PartPose, Shape, compile, node, part_inertia_per_kg,
};
use void_landing::{
    BodyShape, ContactBodySpec, ContactFrame, ContactWorld, ContactWorldOptions,
    EncounterPhysicsGate, EncounterRanges, FrameState, Piece, PieceMass, PlanetFrame, SimpleShape,
};
use void_orbit::{
    AdvanceOutcome, CelestialBody, Control, EphemerisSource, ForceControl, PropagationRun,
    Tolerances, VesselPropagator, VesselState, body_orientation,
};
use void_rotation::{Mat3, rotation_step};
use void_sas::{SAS_TUNING, SasPhase, StabilityAssist};
use void_terrain::Terrain;

mod guidance;
pub use guidance::{GuidanceStatus, GuidedBurn};

type Poses = Vec<(String, PartPose)>;
type SceneGroup = (Option<usize>, Vec<String>, Vec<(u64, usize)>);
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct FleetOptions {
    pub step_seconds: f64,
    pub tolerances: Tolerances,
    pub encounter: EncounterRanges,
    pub recenter_meters: f64,
    pub follow_meters: f64,
    pub flight_chunk_seconds: f64,
    pub rails_chunk_seconds: f64,
    pub steering_torque: f64,
}
impl Default for FleetOptions {
    fn default() -> Self {
        Self {
            step_seconds: 1.0 / 60.0,
            tolerances: Tolerances {
                position_meters: 1e-6,
                velocity_meters_per_second: 1e-9,
            },
            encounter: EncounterRanges {
                unpack_meters: 2000.0,
                pack_meters: 2500.0,
            },
            recenter_meters: 5000.0,
            follow_meters: 250.0,
            flight_chunk_seconds: 1.0,
            rails_chunk_seconds: 10.0,
            steering_torque: 6000.0,
        }
    }
}
#[derive(Clone, Debug)]
pub struct GroundSpec {
    pub body_index: usize,
    pub terrain: Arc<Terrain>,
    pub tiles: ContactWorldOptions,
    pub band_enter_meters: f64,
    pub band_exit_meters: f64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VesselMode {
    Orbit,
    Bubble,
    Ground,
}
#[derive(Clone, Debug)]
pub struct VesselSnapshot {
    pub id: String,
    pub name: String,
    pub mode: VesselMode,
    pub scene: Option<u64>,
    pub position: DVec3,
    pub velocity: DVec3,
    pub rotation: DQuat,
    pub angular_velocity: DVec3,
    pub mass_kg: f64,
    pub part_ids: Vec<String>,
}
impl VesselSnapshot {
    fn state(&self) -> FrameState {
        FrameState {
            position: self.position,
            velocity: self.velocity,
        }
    }
}
#[derive(Clone, Debug)]
pub struct PartSnapshot {
    pub id: String,
    pub definition: &'static PartDefinition,
    pub position: DVec3,
    pub rotation: DQuat,
    pub fuel_kg: f64,
    pub stage: Option<u32>,
    pub staged: bool,
    pub lit: bool,
    pub firing: bool,
}
#[derive(Clone, Debug)]
pub struct SceneSnapshot {
    pub id: u64,
    pub kind: VesselMode,
    pub members: Vec<String>,
    pub origin: DVec3,
    pub rotation: DQuat,
    pub asleep: bool,
    pub recenters: u64,
}

/// Actual live collision geometry. Packed orbital ships have no Rapier collider to display.
#[derive(Clone, Debug)]
pub struct VesselColliderMesh {
    pub vessel: String,
    pub position: DVec3,
    pub rotation: DQuat,
    pub mesh: void_landing::BodyColliderMesh,
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct VesselControl {
    pub throttle: f64,
    pub turn: DVec3,
}
#[derive(Clone, Debug)]
pub struct FleetEvent {
    pub time: f64,
    pub vessel: String,
    pub from: Option<VesselMode>,
    pub to: Option<VesselMode>,
    pub scene: Option<u64>,
}
#[derive(Clone, Debug)]
pub struct FreeNode {
    pub part: String,
    pub node: String,
    pub size: u32,
}
#[derive(Clone, Debug)]
pub struct TerrainTile {
    pub scene: u64,
    pub tile: String,
    pub position: DVec3,
    pub rotation: DQuat,
}
struct Ground {
    spec: GroundSpec,
    frame: PlanetFrame,
}
enum SceneFrame {
    Bubble(Box<FreeFallFrame>),
    Ground(Box<PlanetFrame>),
}
impl ContactFrame for SceneFrame {
    fn terrain_body(&self) -> Option<&CelestialBody> {
        match self {
            Self::Bubble(f) => f.terrain_body(),
            Self::Ground(f) => f.terrain_body(),
        }
    }

    fn acceleration(&self, e: &dyn EphemerisSource, t: f64, r: DVec3, v: DVec3) -> DVec3 {
        match self {
            Self::Bubble(f) => f.acceleration(e, t, r, v),
            Self::Ground(f) => f.acceleration(e, t, r, v),
        }
    }
    fn spin(&self) -> DVec3 {
        match self {
            Self::Bubble(f) => f.spin(),
            Self::Ground(f) => f.spin(),
        }
    }
}
struct Scene {
    world: ContactWorld<SceneFrame>,
    ground: Option<usize>,
    members: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
enum Owner {
    Orbit {
        run: Box<PropagationRun>,
        rotation: DQuat,
        angular_velocity: DVec3,
    },
    Scene {
        scene: u64,
        body: RigidBodyHandle,
        push: DVec3,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Vessel {
    id: String,
    name: String,
    root: String,
    poses: Poses,
    owner: Owner,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Sas {
    assist: StabilityAssist,
    ground: Option<usize>,
}
/// Fleet owns the part graph. Every connected vessel has exactly one physics owner.
pub struct Fleet {
    pub ephemeris: Box<dyn EphemerisSource>,
    pub options: FleetOptions,
    pub events: Vec<FleetEvent>,
    propagator: VesselPropagator,
    environment: Option<Arc<dyn FleetEnvironment>>,
    grounds: Vec<Ground>,
    parts: HashMap<String, PropulsionPart>,
    connections: Vec<Connection>,
    vessels: BTreeMap<String, Vessel>,
    order: Vec<String>,
    controls: HashMap<String, VesselControl>,
    guidance: BTreeMap<String, GuidedBurn>,
    lit: HashSet<String>,
    staged: HashSet<String>,
    sas: HashMap<String, Sas>,
    scenes: BTreeMap<u64, Scene>,
    gate: EncounterPhysicsGate,
    time: f64,
    pending: f64,
    next_vessel: u64,
    next_scene: u64,
}
fn rows(a: DMat3) -> Mat3 {
    a.transpose().to_cols_array()
}
fn vec64(v: rapier3d::math::Vector) -> DVec3 {
    DVec3::new(v.x as f64, v.y as f64, v.z as f64)
}
fn vec32(v: DVec3) -> rapier3d::math::Vector {
    rapier3d::math::Vector::new(v.x as f32, v.y as f32, v.z as f32)
}
fn quat64(q: rapier3d::math::Rotation) -> DQuat {
    DQuat::from_xyzw(q.x as f64, q.y as f64, q.z as f64, q.w as f64).normalize()
}
impl Fleet {
    pub fn new(
        mut ephemeris: impl EphemerisSource + 'static,
        time: f64,
        grounds: Vec<GroundSpec>,
        options: FleetOptions,
    ) -> Self {
        assert!(
            time.is_finite()
                && options.step_seconds.is_finite()
                && options.step_seconds > 0.0
                && options.flight_chunk_seconds >= options.step_seconds
                && options.follow_meters > 0.0
                && options.recenter_meters > options.encounter.pack_meters
                && options.rails_chunk_seconds > 0.0
                && options.steering_torque > 0.0,
            "fleet: invalid options"
        );
        assert!(
            [
                options.follow_meters,
                options.recenter_meters,
                options.flight_chunk_seconds,
                options.rails_chunk_seconds,
                options.steering_torque
            ]
            .iter()
            .all(|v| v.is_finite()),
            "fleet: non-finite options"
        );
        let mut seen = HashSet::new();
        let grounds = grounds
            .into_iter()
            .map(|spec| {
                assert!(seen.insert(spec.body_index), "fleet: duplicate ground");
                let frame = PlanetFrame::new(&ephemeris, spec.body_index);
                assert_eq!(
                    spec.terrain.radius_meters, frame.body.radius_meters,
                    "fleet: terrain radius mismatch"
                );
                assert!(
                    spec.band_enter_meters > 0.0 && spec.band_exit_meters > spec.band_enter_meters,
                    "fleet: invalid height band"
                );
                Ground { spec, frame }
            })
            .collect();
        let propagator = VesselPropagator::new(&ephemeris, options.tolerances);
        ephemeris.extend_to(time + options.step_seconds);
        Self {
            ephemeris: Box::new(ephemeris),
            options,
            events: vec![],
            propagator,
            environment: None,
            grounds,
            parts: HashMap::new(),
            connections: vec![],
            vessels: BTreeMap::new(),
            order: vec![],
            controls: HashMap::new(),
            guidance: BTreeMap::new(),
            lit: HashSet::new(),
            staged: HashSet::new(),
            sas: HashMap::new(),
            scenes: BTreeMap::new(),
            gate: EncounterPhysicsGate::new(options.encounter),
            time,
            pending: 0.0,
            next_vessel: 1,
            next_scene: 1,
        }
    }
    pub fn set_environment(&mut self, environment: Option<Arc<dyn FleetEnvironment>>) {
        self.environment = environment;
        for vessel in self.vessels.values_mut() {
            if let Owner::Orbit { run, .. } = &mut vessel.owner {
                **run = run.restarted();
            }
        }
    }
    fn environment_sample(&self, v: &Vessel, time: f64) -> Option<EnvironmentSample> {
        self.environment.as_ref().map(|environment| {
            let parts = self
                .centred(&v.poses)
                .0
                .into_iter()
                .map(|(id, pose)| EnvironmentPart {
                    definition: self.parts[&id].definition,
                    id,
                    pose,
                })
                .collect::<Vec<_>>();
            environment.sample(
                &self.ephemeris,
                time,
                &self.snapshot_of(v),
                &parts,
                &self.connections,
            )
        })
    }
    pub fn time(&self) -> f64 {
        self.time
    }
    /// Substep time already requested but not yet integrated by the fixed-step owners.
    pub fn pending_seconds(&self) -> f64 {
        self.pending
    }
    pub fn connection_snapshots(&self) -> Vec<Connection> {
        self.connections.clone()
    }
    pub fn sas_target(&self, id: &str) -> Option<DQuat> {
        self.vessel(id);
        self.sas.get(id).and_then(|sas| sas.assist.target())
    }
    pub fn vessel_ids(&self) -> Vec<String> {
        self.order.clone()
    }
    pub fn bubble_count(&self) -> usize {
        self.scenes.values().filter(|s| s.ground.is_none()).count()
    }
    pub fn ground_count(&self) -> usize {
        self.scenes.values().filter(|s| s.ground.is_some()).count()
    }
    fn vessel(&self, id: &str) -> &Vessel {
        self.vessels.get(id).expect("fleet: unknown vessel")
    }
    fn mass(&self, poses: &Poses) -> f64 {
        poses.iter().map(|(id, _)| self.part_mass(id)).sum()
    }
    fn part_mass(&self, id: &str) -> f64 {
        let p = &self.parts[id];
        p.definition.dry_mass_kg + p.fuel_kg
    }
    fn centred(&self, poses: &Poses) -> (Poses, DVec3) {
        let c = poses.iter().fold(DVec3::ZERO, |sum, (id, p)| {
            sum + p.position * self.part_mass(id)
        }) / self.mass(poses);
        (
            poses
                .iter()
                .map(|(id, p)| {
                    (
                        id.clone(),
                        PartPose {
                            position: p.position - c,
                            rotation: p.rotation,
                        },
                    )
                })
                .collect(),
            c,
        )
    }
    fn inertia_of(&self, poses: &Poses) -> DMat3 {
        poses.iter().fold(DMat3::ZERO, |sum, (id, p)| {
            let d = part_inertia_per_kg(self.parts[id].definition);
            let r = DMat3::from_quat(p.rotation);
            let v = p.position;
            let outer = DMat3::from_cols(v * v.x, v * v.y, v * v.z);
            sum + (r * DMat3::from_diagonal(d) * r.transpose()
                + DMat3::IDENTITY * v.length_squared()
                - outer)
                * self.part_mass(id)
        })
    }
    pub fn inertia(&self, id: &str) -> Mat3 {
        rows(self.inertia_of(&self.centred(&self.vessel(id).poses).0))
    }
    fn commanded(&self, v: &Vessel) -> bool {
        v.poses.iter().any(|(id, _)| {
            self.parts[id]
                .definition
                .modules
                .iter()
                .any(|m| matches!(m, Module::Command))
        })
    }
    fn propulsion_with_environment(
        &self,
        v: &Vessel,
        sample: Option<&EnvironmentSample>,
    ) -> Propulsion {
        self.propulsion_at(v, sample, self.time)
    }
    fn propulsion_at(
        &self,
        v: &Vessel,
        sample: Option<&EnvironmentSample>,
        time: f64,
    ) -> Propulsion {
        let parts: Vec<_> = v.poses.iter().map(|(id, _)| &self.parts[id]).collect();
        let mut p = propulsion(
            &parts,
            &v.poses,
            &self.connections,
            &self.lit,
            self.effective_throttle(&v.id, time),
            self.centred(&v.poses).1,
        );
        if let Some(sample) = sample {
            p.force = DVec3::ZERO;
            p.torque = DVec3::ZERO;
            let centre = self.centred(&v.poses).1;
            for group in &mut p.groups {
                for engine in &mut group.engines {
                    let scale = *sample
                        .thrust_scales
                        .get(&engine.part_id)
                        .expect("fleet: environment omitted active engine");
                    assert!(
                        scale.is_finite() && (0.0..=1.0).contains(&scale),
                        "fleet: invalid engine scale"
                    );
                    engine.force *= scale;
                    p.force += engine.force;
                    p.torque += (engine.point - centre).cross(engine.force);
                }
            }
        }
        p
    }
    fn propulsion_of(&self, v: &Vessel) -> Propulsion {
        let sample = self.environment_sample(v, self.time);
        self.propulsion_with_environment(v, sample.as_ref())
    }
    pub fn thrust(&self, id: &str) -> Propulsion {
        self.propulsion_of(self.vessel(id))
    }
    /// Read-only vacuum rating for planning. Unlike `thrust`, this asks for full throttle even
    /// when the pilot currently coasts; it does not ignite unstaged engines or mutate controls.
    pub fn full_throttle_vacuum_thrust(&self, id: &str) -> Propulsion {
        let v = self.vessel(id);
        let parts: Vec<_> = v.poses.iter().map(|(id, _)| &self.parts[id]).collect();
        propulsion(
            &parts,
            &v.poses,
            &self.connections,
            &self.lit,
            1.0,
            self.centred(&v.poses).1,
        )
    }
    pub fn fuel(&self, id: &str) -> f64 {
        self.parts[id].fuel_kg
    }
    pub fn control(&self, id: &str) -> VesselControl {
        self.controls[id]
    }
    pub fn set_control(&mut self, id: &str, c: VesselControl) {
        assert!(
            (0.0..=1.0).contains(&c.throttle)
                && c.turn.to_array().iter().all(|x| (-1.0..=1.0).contains(x)),
            "fleet: invalid control"
        );
        assert!(
            c.turn == DVec3::ZERO || self.commanded(self.vessel(id)),
            "fleet: no command part"
        );
        self.vessel(id);
        self.cancel_guidance(id, "manual control");
        self.controls.insert(id.into(), c);
    }
    pub fn set_sas(&mut self, id: &str, on: bool) {
        let v = self.vessel(id);
        if on {
            assert!(self.commanded(v), "fleet: no command part");
        }
        let ground = self.attitude_ground(v);
        if !on {
            self.sas.remove(id);
            return;
        }
        let mut assist = StabilityAssist::new(self.options.steering_torque, SAS_TUNING);
        assist.set_enabled(true);
        self.sas.insert(id.into(), Sas { assist, ground });
    }
    pub fn sas_phase(&self, id: &str) -> SasPhase {
        self.vessel(id);
        self.sas.get(id).map_or(SasPhase::Off, |s| s.assist.phase())
    }
    pub fn launch(
        &mut self,
        craft: &Craft,
        state: FrameState,
        rotation: DQuat,
        angular_velocity: DVec3,
    ) -> String {
        let c = compile(craft).expect("fleet: invalid craft");
        assert!(
            state.position.is_finite() && state.velocity.is_finite(),
            "fleet: invalid initial state"
        );
        assert!(
            rotation.is_finite()
                && (rotation.length() - 1.0).abs() < 1e-9
                && angular_velocity.is_finite(),
            "fleet: invalid attitude"
        );
        for p in &c.parts {
            assert!(
                !p.definition
                    .modules
                    .iter()
                    .any(|m| matches!(m, Module::Engine { .. } | Module::Decoupler { .. }))
                    || p.instance.stage.is_some(),
                "fleet: actionable part needs stage"
            );
        }
        let id = format!("v{}", self.next_vessel);
        self.next_vessel += 1;
        let mut poses = vec![];
        for p in c.parts {
            let pid = format!("{id}/{}", p.instance.id);
            self.parts.insert(
                pid.clone(),
                PropulsionPart {
                    id: pid.clone(),
                    definition: p.definition,
                    fuel_kg: p.instance.fuel_kg,
                    stage: p.instance.stage,
                },
            );
            poses.push((pid, p.pose));
        }
        for link in c.connections {
            self.connections.push(Connection {
                a: format!("{id}/{}", link.a),
                b: format!("{id}/{}", link.b),
                ..link
            });
        }
        let poses = self.centred(&poses).0;
        let run = PropagationRun::new(VesselState {
            time: self.time,
            position: state.position,
            velocity: state.velocity,
            mass_kg: self.mass(&poses),
        });
        let v = Vessel {
            id: id.clone(),
            name: craft.name.clone(),
            root: format!("{id}/{}", c.root_id),
            poses,
            owner: Owner::Orbit {
                run: Box::new(run),
                rotation,
                angular_velocity,
            },
        };
        self.vessels.insert(id.clone(), v);
        self.order.push(id.clone());
        self.controls.insert(id.clone(), VesselControl::default());
        self.event(&id, None, Some(VesselMode::Orbit), None);
        id
    }
    fn ground_index(&self, body: usize) -> usize {
        self.grounds
            .iter()
            .position(|g| g.spec.body_index == body)
            .expect("fleet: body has no ground")
    }
    pub fn launch_landed(&mut self, craft: &Craft, body: usize, d: DVec3) -> String {
        assert!(
            (d.length() - 1.0).abs() < 1e-9,
            "fleet: direction must be unit"
        );
        let c = compile(craft).expect("invalid craft");
        let cy = c.summary(None).center.y;
        let lowest = c
            .parts
            .iter()
            .map(|p| {
                let ay = (p.pose.rotation * DVec3::Y).y;
                let radial = if p.definition.shape == Shape::Box {
                    p.definition.radius
                        * ((p.pose.rotation * DVec3::X).y.abs()
                            + (p.pose.rotation * DVec3::Z).y.abs())
                } else {
                    p.definition.radius * (1.0 - ay * ay).max(0.0).sqrt()
                };
                p.pose.position.y - ay.abs() * p.definition.height / 2.0 - radial
            })
            .fold(f64::INFINITY, f64::min);
        let g = &self.grounds[self.ground_index(body)];
        let r = g.frame.body.radius_meters + g.spec.terrain.height(d) + cy - lowest + 0.05;
        let state = g.frame.to_inertial(
            &self.ephemeris,
            self.time,
            FrameState {
                position: d * r,
                velocity: DVec3::ZERO,
            },
        );
        let axes = self.ground_axes(self.ground_index(body));
        let q = void_landing::upright_at(d);
        self.launch(craft, state, axes * q, axes * g.frame.spin())
    }
    fn event(
        &mut self,
        id: &str,
        from: Option<VesselMode>,
        to: Option<VesselMode>,
        scene: Option<u64>,
    ) {
        self.events.push(FleetEvent {
            time: self.time,
            vessel: id.into(),
            from,
            to,
            scene,
        });
    }
    fn scene_mode(&self, scene: u64) -> VesselMode {
        if self.scenes[&scene].ground.is_some() {
            VesselMode::Ground
        } else {
            VesselMode::Bubble
        }
    }
    fn mode(&self, v: &Vessel) -> VesselMode {
        match v.owner {
            Owner::Orbit { .. } => VesselMode::Orbit,
            Owner::Scene { scene, .. } => self.scene_mode(scene),
        }
    }
    fn ground_axes(&self, g: usize) -> DQuat {
        let b = body_orientation(&self.grounds[g].frame.body.rotation, self.time);
        DQuat::from_mat3(&glam::DMat3::from_cols(b[0], b[1], b[2])).normalize()
    }
    fn axes(&self, s: u64) -> DQuat {
        self.scenes[&s]
            .ground
            .map_or(DQuat::IDENTITY, |g| self.ground_axes(g))
    }
    fn to_inertial(&self, s: u64, state: FrameState) -> FrameState {
        match &self.scenes[&s].world.frame {
            SceneFrame::Bubble(f) => f.to_inertial(self.time, state),
            SceneFrame::Ground(f) => f.to_inertial(&self.ephemeris, self.time, state),
        }
    }
    fn scene_local(&self, s: u64, state: FrameState) -> FrameState {
        match &self.scenes[&s].world.frame {
            SceneFrame::Bubble(f) => f.from_inertial(self.time, state),
            SceneFrame::Ground(f) => f.to_body_fixed(&self.ephemeris, self.time, state),
        }
    }
    fn scene_centre(&self, v: &Vessel) -> FrameState {
        let Owner::Scene { scene, body, push } = v.owner else {
            panic!("expected scene")
        };
        let world = &self.scenes[&scene].world;
        let mut s = world.state(&self.ephemeris, body, push);
        let b = world.body(body);
        s.position += quat64(*b.rotation()) * vec64(b.local_center_of_mass());
        s
    }
    pub fn snapshot(&self, id: &str) -> VesselSnapshot {
        self.snapshot_of(self.vessel(id))
    }
    fn snapshot_of(&self, v: &Vessel) -> VesselSnapshot {
        let (s, q, w, scene) = match &v.owner {
            Owner::Orbit {
                run,
                rotation,
                angular_velocity,
            } => {
                let s = run.state();
                (
                    FrameState {
                        position: s.position,
                        velocity: s.velocity,
                    },
                    *rotation,
                    *angular_velocity,
                    None,
                )
            }
            Owner::Scene { scene, body, .. } => {
                let b = self.scenes[scene].world.body(*body);
                let a = self.axes(*scene);
                (
                    self.to_inertial(*scene, self.scene_centre(v)),
                    (a * quat64(*b.rotation())).normalize(),
                    a * (vec64(b.angvel()) + self.scenes[scene].world.frame.spin()),
                    Some(*scene),
                )
            }
        };
        VesselSnapshot {
            id: v.id.clone(),
            name: v.name.clone(),
            mode: self.mode(v),
            scene,
            position: s.position,
            velocity: s.velocity,
            rotation: q,
            angular_velocity: w,
            mass_kg: self.mass(&v.poses),
            part_ids: v.poses.iter().map(|(id, _)| id.clone()).collect(),
        }
    }
    fn poses_frame(&self, v: &Vessel) -> (DVec3, DQuat) {
        match &v.owner {
            Owner::Orbit { run, rotation, .. } => (run.state().position, *rotation),
            Owner::Scene { scene, body, push } => {
                let world = &self.scenes[scene].world;
                (
                    self.to_inertial(*scene, world.state(&self.ephemeris, *body, *push))
                        .position,
                    self.axes(*scene) * quat64(*world.body(*body).rotation()),
                )
            }
        }
    }
    pub fn part_snapshots(&self, id: &str) -> Vec<PartSnapshot> {
        let v = self.vessel(id);
        let (origin, q) = self.poses_frame(v);
        let p = self.propulsion_of(v);
        v.poses
            .iter()
            .map(|(id, pose)| {
                let part = &self.parts[id];
                PartSnapshot {
                    id: id.clone(),
                    definition: part.definition,
                    position: origin + q * pose.position,
                    rotation: (q * pose.rotation).normalize(),
                    fuel_kg: part.fuel_kg,
                    stage: part.stage,
                    staged: self.staged.contains(id),
                    lit: self.lit.contains(id),
                    firing: p
                        .groups
                        .iter()
                        .any(|g| g.engines.iter().any(|e| &e.part_id == id)),
                }
            })
            .collect()
    }
    pub fn scene_snapshots(&self) -> Vec<SceneSnapshot> {
        self.scenes
            .iter()
            .map(|(&id, s)| SceneSnapshot {
                id,
                kind: self.scene_mode(id),
                members: s.members.clone(),
                origin: self
                    .to_inertial(
                        id,
                        FrameState {
                            position: s.world.origin,
                            velocity: DVec3::ZERO,
                        },
                    )
                    .position,
                rotation: self.axes(id),
                asleep: s.world.asleep(),
                recenters: s.world.recenters,
            })
            .collect()
    }
    pub fn relative(&self, id: &str, to: &str) -> FrameState {
        let a = self.vessel(id);
        let b = self.vessel(to);
        if let (Owner::Scene { scene: sa, .. }, Owner::Scene { scene: sb, .. }) =
            (&a.owner, &b.owner)
            && sa == sb
        {
            let x = self.scene_centre(a);
            let y = self.scene_centre(b);
            let d = x.position - y.position;
            let q = self.axes(*sa);
            return FrameState {
                position: q * d,
                velocity: q
                    * (x.velocity - y.velocity + self.scenes[sa].world.frame.spin().cross(d)),
            };
        }
        let a = self.snapshot(id);
        let b = self.snapshot(to);
        FrameState {
            position: a.position - b.position,
            velocity: a.velocity - b.velocity,
        }
    }
    pub fn vessel_of_part(&self, id: &str) -> String {
        self.order
            .iter()
            .find(|v| self.vessels[*v].poses.iter().any(|(p, _)| p == id))
            .expect("fleet: unknown part")
            .clone()
    }
    pub fn node_frame(&self, id: &str, n: &str) -> (DVec3, DVec3) {
        let vid = self.vessel_of_part(id);
        let v = self.vessel(&vid);
        let (o, q) = self.poses_frame(v);
        let p = &v.poses.iter().find(|(p, _)| p == id).unwrap().1;
        let n = node(self.parts[id].definition, n).expect("unknown node");
        (
            o + q * (p.position + p.rotation * n.position),
            q * (p.rotation * n.direction),
        )
    }
    pub fn free_nodes(&self, id: &str) -> Vec<FreeNode> {
        self.vessel(id)
            .poses
            .iter()
            .flat_map(|(p, _)| {
                self.parts[p]
                    .definition
                    .nodes
                    .iter()
                    .filter(move |n| {
                        !self.connections.iter().any(|c| {
                            (&c.a == p && c.node_a == n.id) || (&c.b == p && c.node_b == n.id)
                        })
                    })
                    .map(move |n| FreeNode {
                        part: p.clone(),
                        node: n.id.clone(),
                        size: n.size,
                    })
            })
            .collect()
    }
    pub fn terrain_tiles(&self) -> Vec<TerrainTile> {
        self.scenes
            .iter()
            .flat_map(|(&id, s)| {
                s.world.terrain_colliders().map(move |t| TerrainTile {
                    scene: id,
                    tile: format!("{:?}", t.collider),
                    position: self
                        .to_inertial(
                            id,
                            FrameState {
                                position: t.origin,
                                velocity: DVec3::ZERO,
                            },
                        )
                        .position,
                    rotation: self.axes(id),
                })
            })
            .collect()
    }
    pub fn terrain_geometry(&self, scene: u64, tile: &str) -> (Vec<[f32; 3]>, Vec<[u32; 3]>) {
        let s = &self.scenes[&scene];
        let t = s
            .world
            .terrain_colliders()
            .find(|t| format!("{:?}", t.collider) == tile)
            .expect("tile not loaded");
        s.world.terrain_collider_mesh(t)
    }

    pub fn vessel_collider_meshes(&self) -> Vec<VesselColliderMesh> {
        self.order
            .iter()
            .flat_map(|id| {
                let vessel = self.vessel(id);
                let Owner::Scene { scene, body, .. } = vessel.owner else {
                    return Vec::new();
                };
                let world = &self.scenes[&scene].world;
                let position = self
                    .to_inertial(
                        scene,
                        FrameState {
                            position: world.position(body),
                            velocity: DVec3::ZERO,
                        },
                    )
                    .position;
                let rotation = self.axes(scene) * quat64(*world.body(body).rotation());
                world
                    .body_collider_meshes(body)
                    .into_iter()
                    .map(|mesh| VesselColliderMesh {
                        vessel: id.clone(),
                        position,
                        rotation,
                        mesh,
                    })
                    .collect()
            })
            .collect()
    }
    fn new_scene(&mut self, ground: Option<usize>, ids: &[String]) -> u64 {
        let mut c = FrameState {
            position: DVec3::ZERO,
            velocity: DVec3::ZERO,
        };
        let mut mass = 0.0;
        for id in ids {
            let s = self.snapshot(id);
            mass += s.mass_kg;
            c.position += s.position * s.mass_kg;
            c.velocity += s.velocity * s.mass_kg;
        }
        c.position /= mass;
        c.velocity /= mass;
        let (frame, terrain, mut options, origin) = if let Some(g) = ground {
            let g = &self.grounds[g];
            (
                SceneFrame::Ground(Box::new(g.frame.clone())),
                Some(g.spec.terrain.clone()),
                g.spec.tiles,
                g.frame
                    .to_body_fixed(&self.ephemeris, self.time, c)
                    .position,
            )
        } else {
            (
                SceneFrame::Bubble(Box::new(FreeFallFrame::new(
                    &self.ephemeris,
                    self.options.tolerances,
                    self.time,
                    c,
                ))),
                None,
                ContactWorldOptions {
                    step_seconds: self.options.step_seconds,
                    tile_level: 0,
                    tile_resolution: 2,
                    tile_reach_meters: 1.0,
                    tile_keep_meters: 2.0,
                    recenter_meters: self.options.recenter_meters,
                    sleeping: false,
                },
                DVec3::ZERO,
            )
        };
        options.step_seconds = self.options.step_seconds;
        options.recenter_meters = self.options.recenter_meters;
        options.sleeping = ground.is_some();
        let world = ContactWorld::new(
            frame,
            terrain,
            options,
            self.time,
            origin,
            &mut self.ephemeris,
        );
        let id = self.next_scene;
        self.next_scene += 1;
        self.scenes.insert(
            id,
            Scene {
                world,
                ground,
                members: vec![],
            },
        );
        id
    }
    fn remove_scene_body(&mut self, v: &Vessel, refill: bool) {
        if let Owner::Scene { scene, body, .. } = v.owner {
            let s = self.scenes.get_mut(&scene).unwrap();
            s.members.retain(|id| id != &v.id);
            s.world.remove_body(body);
            if s.members.is_empty() && !refill {
                self.scenes.remove(&scene);
            }
        }
    }
    fn add_scene_body(
        &mut self,
        v: &mut Vessel,
        scene: u64,
        local: FrameState,
        q: DQuat,
        w: DVec3,
        push: DVec3,
    ) {
        let pieces = v
            .poses
            .iter()
            .map(|(id, p)| {
                let d = self.parts[id].definition;
                Piece {
                    shape: match d.shape {
                        Shape::Box => SimpleShape::Box {
                            half_extents: DVec3::new(d.radius, d.height / 2.0, d.radius),
                        },
                        Shape::Cone => SimpleShape::Cone {
                            radius: d.radius,
                            half_height: d.height / 2.0,
                        },
                        Shape::Cylinder => SimpleShape::Cylinder {
                            radius: d.radius,
                            half_height: d.height / 2.0,
                        },
                    },
                    position: p.position,
                    rotation: Some(p.rotation),
                    mass: Some(PieceMass {
                        kg: self.part_mass(id),
                        principal_inertia_per_kg: part_inertia_per_kg(d),
                    }),
                }
            })
            .collect();
        let spec = ContactBodySpec {
            shape: BodyShape::Compound(pieces),
            mass_kg: self.mass(&v.poses),
            friction: 0.8,
            restitution: 0.0,
            lock_rotations: false,
        };
        let s = self.scenes.get_mut(&scene).unwrap();
        let body = s.world.add_body(&self.ephemeris, &spec, local, q, push);
        s.world.world.bodies[body].set_angvel(vec32(w), true);
        s.members.push(v.id.clone());
        v.owner = Owner::Scene { scene, body, push };
    }
    fn move_to(&mut self, id: &str, scene: u64) {
        self.cancel_guidance(id, "entered contact physics");
        let snap = self.snapshot(id);
        let mut v = self.vessels.remove(id).unwrap();
        let from = self.mode(&v);
        let push = match v.owner {
            Owner::Scene { scene, push, .. } => self.axes(scene) * push,
            Owner::Orbit { .. } => {
                let sample = self.environment_sample(&v, self.time);
                let thrust = self.propulsion_with_environment(&v, sample.as_ref());
                let air = sample.and_then(|s| s.air).map_or(DVec3::ZERO, |source| {
                    source.acceleration(self.time, snap.position, snap.velocity, snap.mass_kg)
                });
                snap.rotation * thrust.force / snap.mass_kg + air
            }
        };
        self.remove_scene_body(&v, false);
        v.poses = self.centred(&v.poses).0;
        let axes = self.axes(scene).conjugate();
        let local = self.scene_local(scene, snap.state());
        let w = axes * snap.angular_velocity - self.scenes[&scene].world.frame.spin();
        self.add_scene_body(&mut v, scene, local, axes * snap.rotation, w, axes * push);
        self.vessels.insert(id.into(), v);
        self.event(id, Some(from), Some(self.scene_mode(scene)), Some(scene));
    }
    fn move_to_orbit(&mut self, id: &str) {
        let s = self.snapshot(id);
        let mut v = self.vessels.remove(id).unwrap();
        self.remove_scene_body(&v, false);
        v.poses = self.centred(&v.poses).0;
        v.owner = Owner::Orbit {
            run: Box::new(PropagationRun::new(VesselState {
                time: self.time,
                position: s.position,
                velocity: s.velocity,
                mass_kg: s.mass_kg,
            })),
            rotation: s.rotation,
            angular_velocity: s.angular_velocity,
        };
        self.vessels.insert(id.into(), v);
        self.event(id, Some(s.mode), Some(VesselMode::Orbit), s.scene);
    }
    fn settle(&mut self, id: &str) {
        let v = self.vessel(id);
        if !matches!(v.owner, Owner::Scene { .. }) || self.centred(&v.poses).1.length() == 0.0 {
            return;
        }
        let local = self.scene_centre(v);
        let Owner::Scene { scene, body, push } = v.owner else {
            unreachable!()
        };
        let b = self.scenes[&scene].world.body(body);
        let q = quat64(*b.rotation());
        let w = vec64(b.angvel());
        let mut v = self.vessels.remove(id).unwrap();
        self.remove_scene_body(&v, true);
        v.poses = self.centred(&v.poses).0;
        self.add_scene_body(&mut v, scene, local, q, w, push);
        self.vessels.insert(id.into(), v);
    }
    fn ground_for(&self, v: &Vessel) -> Option<usize> {
        self.grounds.iter().enumerate().find(|(i,g)| { let inside=matches!(v.owner,Owner::Scene { scene,.. } if self.scenes[&scene].ground==Some(*i)); self.clearance_over(v,*i)<=if inside { g.spec.band_exit_meters } else { g.spec.band_enter_meters+0.1 } }).map(|(i,_)|i)
    }
    fn clearance_over(&self, v: &Vessel, g: usize) -> f64 {
        let ground = &self.grounds[g];
        let p = match v.owner {
            Owner::Scene { scene, .. } if self.scenes[&scene].ground == Some(g) => {
                self.scene_centre(v).position
            }
            _ => {
                ground
                    .frame
                    .to_body_fixed(&self.ephemeris, self.time, self.snapshot(&v.id).state())
                    .position
            }
        };
        let r = p.length();
        let reach = self
            .centred(&v.poses)
            .0
            .iter()
            .map(|(id, p)| {
                let d = self.parts[id].definition;
                p.position.length()
                    + (d.height / 2.0).hypot(if d.shape == Shape::Box {
                        d.radius * 2.0_f64.sqrt()
                    } else {
                        d.radius
                    })
            })
            .fold(0.0, f64::max);
        r - ground.frame.body.radius_meters - ground.spec.terrain.height(p / r) - reach
    }
    pub fn clearance(&self, id: &str, body: usize) -> f64 {
        self.clearance_over(self.vessel(id), self.ground_index(body))
    }
    pub fn body_fixed_state(&self, id: &str, body: usize) -> FrameState {
        let g = self.ground_index(body);
        let v = self.vessel(id);
        if matches!(v.owner,Owner::Scene { scene,.. } if self.scenes[&scene].ground==Some(g)) {
            self.scene_centre(v)
        } else {
            self.grounds[g].frame.to_body_fixed(
                &self.ephemeris,
                self.time,
                self.snapshot(id).state(),
            )
        }
    }
    fn band_safe_seconds(&self) -> f64 {
        let mut safe = f64::INFINITY;
        for id in &self.order {
            let v = self.vessel(id);
            if !matches!(v.owner, Owner::Orbit { .. }) {
                continue;
            }
            for (i, g) in self.grounds.iter().enumerate() {
                let s =
                    g.frame
                        .to_body_fixed(&self.ephemeris, self.time, self.snapshot(id).state());
                let gap = (self.clearance_over(v, i) - g.spec.band_enter_meters).max(0.0);
                let speed = s.velocity.length();
                let a = 1.2 * g.frame.body.gm / s.position.length_squared()
                    + self.propulsion_of(v).force.length() / self.mass(&v.poses);
                safe = safe.min(((-speed + (speed * speed + 2.0 * a * gap).sqrt()) / a).max(1e-3));
            }
        }
        safe
    }
    fn reconcile(&mut self, lookahead: f64) {
        let ids = self.order.clone();
        for (i, a) in ids.iter().enumerate() {
            for b in &ids[i + 1..] {
                self.gate.update(
                    a,
                    self.snapshot(a).state(),
                    b,
                    self.snapshot(b).state(),
                    lookahead,
                );
            }
        }
        let mut clusters: Vec<Vec<String>> = ids.iter().map(|id| vec![id.clone()]).collect();
        for (a, b) in self.gate.active_pairs() {
            let ai = clusters.iter().position(|g| g.contains(&a)).unwrap();
            let bi = clusters.iter().position(|g| g.contains(&b)).unwrap();
            if ai != bi {
                let group = clusters.remove(bi);
                let ai = clusters.iter().position(|g| g.contains(&a)).unwrap();
                clusters[ai].extend(group);
            }
        }
        let mut groups: Vec<SceneGroup> = vec![];
        for cluster in clusters {
            let mut ground_groups: BTreeMap<usize, Vec<String>> = BTreeMap::new();
            let mut air = vec![];
            for id in cluster {
                if let Some(g) = self.ground_for(self.vessel(&id)) {
                    ground_groups.entry(g).or_default().push(id);
                } else {
                    air.push(id);
                }
            }
            for (g, ids) in ground_groups {
                groups.push((Some(g), ids, vec![]));
            }
            if air.len() > 1 {
                groups.push((None, air, vec![]));
            } else {
                for id in air {
                    if matches!(self.vessel(&id).owner, Owner::Scene { .. }) {
                        self.move_to_orbit(&id);
                    }
                }
            }
        }
        for (ground, ids, counts) in &mut groups {
            for id in ids {
                if let Owner::Scene { scene, .. } = self.vessel(id).owner
                    && self.scenes[&scene].ground == *ground
                {
                    if let Some((_, count)) = counts.iter_mut().find(|(s, _)| *s == scene) {
                        *count += 1;
                    } else {
                        counts.push((scene, 1));
                    }
                }
            }
            counts.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
        }
        groups
            .sort_by_key(|(_, _, counts)| std::cmp::Reverse(counts.first().map_or(0, |(_, n)| *n)));
        let mut taken = HashSet::new();
        for (ground, ids, counts) in groups {
            let scene = counts
                .into_iter()
                .find(|(s, _)| !taken.contains(s))
                .map_or_else(|| self.new_scene(ground, &ids), |(s, _)| s);
            taken.insert(scene);
            for id in ids {
                if !matches!(self.vessel(&id).owner,Owner::Scene { scene:s,.. } if s==scene) {
                    self.move_to(&id, scene);
                }
            }
        }
    }
    pub fn stages_left(&self, id: &str) -> Vec<u32> {
        let mut s: Vec<_> = self
            .vessel(id)
            .poses
            .iter()
            .filter(|(id, _)| !self.staged.contains(id))
            .filter_map(|(id, _)| self.parts[id].stage)
            .collect();
        s.sort_unstable();
        s.dedup();
        s
    }
    pub fn stage(&mut self, id: &str) -> Vec<String> {
        self.cancel_guidance(id, "staging");
        let Some(next) = self.stages_left(id).first().copied() else {
            return vec![];
        };
        let parts: Vec<_> = self
            .vessel(id)
            .poses
            .iter()
            .filter(|(p, _)| self.parts[p].stage == Some(next) && !self.staged.contains(p))
            .map(|(p, _)| p.clone())
            .collect();
        let mut split = vec![];
        for p in &parts {
            if self.parts[p]
                .definition
                .modules
                .iter()
                .any(|m| matches!(m, Module::Decoupler { .. }))
            {
                let new = self.decouple(p);
                self.staged.insert(p.clone());
                split.push(new);
            }
        }
        for p in parts {
            if self.parts[&p]
                .definition
                .modules
                .iter()
                .any(|m| matches!(m, Module::Engine { .. }))
            {
                self.staged.insert(p.clone());
                self.lit.insert(p);
            }
        }
        split
    }
    fn components(&self, poses: &Poses) -> Vec<Vec<String>> {
        let mut left: HashSet<_> = poses.iter().map(|(id, _)| id.clone()).collect();
        let mut out = vec![];
        for (id, _) in poses {
            if !left.remove(id) {
                continue;
            }
            let mut g = vec![id.clone()];
            let mut i = 0;
            while i < g.len() {
                for c in &self.connections {
                    let next = if c.a == g[i] {
                        Some(&c.b)
                    } else if c.b == g[i] {
                        Some(&c.a)
                    } else {
                        None
                    };
                    if let Some(n) = next
                        && left.remove(n)
                    {
                        g.push(n.clone());
                    }
                }
                i += 1;
            }
            out.push(g);
        }
        out
    }
    pub fn decouple(&mut self, part: &str) -> String {
        let d = self.parts[part].definition;
        let (node_id, impulse) = d
            .modules
            .iter()
            .find_map(|m| {
                if let Module::Decoupler {
                    node_id,
                    impulse_ns,
                } = m
                {
                    Some((node_id.clone(), *impulse_ns))
                } else {
                    None
                }
            })
            .expect("not a decoupler");
        let edge = self
            .connections
            .iter()
            .position(|c| {
                (c.a == part && c.node_a == node_id) || (c.b == part && c.node_b == node_id)
            })
            .expect("decoupler node not connected");
        let id = self.vessel_of_part(part);
        if matches!(self.vessel(&id).owner, Owner::Orbit { .. }) {
            let scene = self.new_scene(None, std::slice::from_ref(&id));
            self.move_to(&id, scene);
        }
        self.settle(&id);
        let old = self.vessels.remove(&id).unwrap();
        let Owner::Scene { scene, body, push } = old.owner else {
            unreachable!()
        };
        let world = &self.scenes[&scene].world;
        let state = world.state(&self.ephemeris, body, push);
        let q = quat64(*world.body(body).rotation());
        let w = vec64(world.body(body).angvel());
        let control = self.controls[&id];
        self.connections.remove(edge);
        let groups = self.components(&old.poses);
        assert_eq!(groups.len(), 2, "decouple: expected two groups");
        self.remove_scene_body(&old, true);
        let mut made = vec![];
        let mut created = String::new();
        for ids in groups {
            let keeps = ids.contains(&old.root);
            let poses: Poses = old
                .poses
                .iter()
                .filter(|(p, _)| ids.contains(p))
                .cloned()
                .collect();
            let (poses, c) = self.centred(&poses);
            let offset = q * c;
            let local = FrameState {
                position: state.position + offset,
                velocity: state.velocity + w.cross(offset),
            };
            let new_id = if keeps {
                id.clone()
            } else {
                let n = format!("v{}", self.next_vessel);
                self.next_vessel += 1;
                created = n.clone();
                self.order.push(n.clone());
                n
            };
            let root = if keeps {
                old.root.clone()
            } else {
                ids.iter()
                    .find(|p| {
                        self.parts[*p]
                            .definition
                            .modules
                            .iter()
                            .any(|m| matches!(m, Module::Command))
                    })
                    .unwrap_or(&ids[0])
                    .clone()
            };
            let mut v = Vessel {
                id: new_id.clone(),
                name: old.name.clone(),
                root,
                poses,
                owner: old.owner.clone(),
            };
            self.controls.insert(
                new_id.clone(),
                if keeps {
                    control
                } else {
                    VesselControl::default()
                },
            );
            self.add_scene_body(&mut v, scene, local, q, w, push);
            self.vessels.insert(new_id.clone(), v);
            if !keeps {
                self.event(&new_id, None, Some(self.scene_mode(scene)), Some(scene));
            }
            made.push(new_id);
        }
        let own_id = self.vessel_of_part(part);
        let other = made.iter().find(|id| *id != &own_id).unwrap();
        let own = self.vessel(&own_id);
        let pose = own.poses.iter().find(|(p, _)| p == part).unwrap().1;
        let n = node(d, &node_id).unwrap();
        let Owner::Scene { body: a, .. } = own.owner else {
            unreachable!()
        };
        let Owner::Scene { body: b, .. } = self.vessel(other).owner else {
            unreachable!()
        };
        let world = &mut self.scenes.get_mut(&scene).unwrap().world;
        let point =
            vec64(world.body(a).translation()) + q * (pose.position + pose.rotation * n.position);
        let normal = q * (pose.rotation * n.direction);
        world.world.bodies[a].apply_impulse_at_point(vec32(-normal * impulse), vec32(point), true);
        world.world.bodies[b].apply_impulse_at_point(vec32(normal * impulse), vec32(point), true);
        created
    }
    pub fn join(&mut self, part_a: &str, node_a: &str, part_b: &str, node_b: &str) -> String {
        let a = self.vessel_of_part(part_a);
        let b = self.vessel_of_part(part_b);
        assert_ne!(a, b, "join: same vessel");
        let (Owner::Scene { scene: sa, .. }, Owner::Scene { scene: sb, .. }) =
            (&self.vessel(&a).owner, &self.vessel(&b).owner)
        else {
            panic!("join: requires shared scene")
        };
        assert_eq!(sa, sb, "join: requires shared scene");
        let scene = *sa;
        let na = node(self.parts[part_a].definition, node_a).unwrap();
        let nb = node(self.parts[part_b].definition, node_b).unwrap();
        assert_eq!(na.size, nb.size, "join: node sizes differ");
        for (p, n) in [(part_a, node_a), (part_b, node_b)] {
            assert!(
                !self
                    .connections
                    .iter()
                    .any(|c| (c.a == p && c.node_a == n) || (c.b == p && c.node_b == n)),
                "join: occupied node"
            );
        }
        // Core join preserves poses; the lab enforces the 0.25 m debug capture range.
        self.settle(&a);
        self.settle(&b);
        let va = self.vessels.remove(&a).unwrap();
        let vb = self.vessels.remove(&b).unwrap();
        let mut sides = vec![];
        for v in [&va, &vb] {
            let Owner::Scene { body, push, .. } = v.owner else {
                unreachable!()
            };
            let world = &self.scenes[&scene].world;
            sides.push((
                world.state(&self.ephemeris, body, push),
                quat64(*world.body(body).rotation()),
                vec64(world.body(body).angvel()),
                push,
                self.mass(&v.poses),
            ));
        }
        let (aa, qa, wa, pa, ma) = sides[0];
        let (bb, qb, wb, pb, mb) = sides[1];
        let mass = ma + mb;
        let c = (aa.position * ma + bb.position * mb) / mass;
        let velocity = (aa.velocity * ma + bb.velocity * mb) / mass;
        let push = (pa * ma + pb * mb) / mass;
        let spin = self.scenes[&scene].world.frame.spin();
        let mut angular = DVec3::ZERO;
        for (v, (s, q, w, _, m)) in [(&va, (aa, qa, wa, pa, ma)), (&vb, (bb, qb, wb, pb, mb))] {
            let r = DMat3::from_quat(q);
            let d = s.position - c;
            angular += r * self.inertia_of(&v.poses) * r.transpose() * (w + spin)
                + d.cross(s.velocity - velocity + spin.cross(d)) * m;
        }
        let inv = qa.conjugate();
        let to_a = inv * qb;
        let offset = inv * (bb.position - aa.position);
        let mut poses = va.poses.clone();
        poses.extend(vb.poses.iter().map(|(id, p)| {
            (
                id.clone(),
                PartPose {
                    position: offset + to_a * p.position,
                    rotation: to_a * p.rotation,
                },
            )
        }));
        let poses = self.centred(&poses).0;
        let r = DMat3::from_quat(qa);
        let w = (r * self.inertia_of(&poses) * r.transpose()).inverse() * angular - spin;
        self.remove_scene_body(&va, true);
        self.remove_scene_body(&vb, true);
        self.order.retain(|id| id != &b);
        self.gate.remove_vessel(&b);
        self.controls.remove(&b);
        self.sas.remove(&b);
        self.guidance.remove(&b);
        self.event(&b, Some(self.scene_mode(scene)), None, Some(scene));
        self.connections.push(Connection {
            a: part_a.into(),
            node_a: node_a.into(),
            b: part_b.into(),
            node_b: node_b.into(),
        });
        let mut joined = Vessel { poses, ..va };
        self.add_scene_body(
            &mut joined,
            scene,
            FrameState {
                position: c,
                velocity,
            },
            qa,
            w,
            push,
        );
        self.vessels.insert(a.clone(), joined);
        a
    }
    fn attitude_ground(&self, v: &Vessel) -> Option<usize> {
        if let Owner::Scene { scene, .. } = v.owner {
            self.scenes[&scene].ground
        } else {
            None
        }
    }
    fn steering(&mut self, v: &Vessel, q: DQuat, w: DVec3, dt: f64) -> DVec3 {
        let pilot = self.controls[&v.id].turn;
        let ground = self.attitude_ground(v);
        let inertia = rows(self.inertia_of(&self.centred(&v.poses).0));
        let turn = if let Some(sas) = self.sas.get_mut(&v.id) {
            if sas.ground != ground {
                sas.assist.set_enabled(true);
                sas.ground = ground;
            }
            sas.assist.command(q, w, &inertia, pilot, dt)
        } else {
            pilot
        };
        turn * self.options.steering_torque
    }
    fn propagate(
        &mut self,
        run: &mut PropagationRun,
        end: f64,
        control: Option<Control>,
        air: Option<Arc<dyn void_orbit::AirSource>>,
    ) {
        self.propagator.set_air_source(air);
        let outcome =
            self.propagator
                .advance(&mut self.ephemeris, run, end, 100_000, None, control);
        assert_eq!(
            outcome,
            AdvanceOutcome::Reached,
            "fleet: orbital propagation failed"
        );
    }
    fn advance_orbit(&mut self, id: &str, end: f64) {
        let mut v = self.vessels.remove(id).unwrap();
        loop {
            let Owner::Orbit {
                run,
                rotation,
                angular_velocity,
            } = &v.owner
            else {
                panic!("expected orbit")
            };
            let t = run.time;
            if self
                .guidance
                .get(id)
                .is_some_and(|g| g.status == GuidanceStatus::Armed && t >= g.end_time)
            {
                self.guidance.get_mut(id).unwrap().status = GuidanceStatus::Completed;
                self.controls.get_mut(id).unwrap().throttle = 0.0;
            }
            if t + 1e-12 >= end {
                break;
            }
            let q = *rotation;
            let w = *angular_velocity;
            let environment = self.environment_sample(&v, t);
            let guide = self
                .guidance
                .get(id)
                .filter(|g| g.status == GuidanceStatus::Armed)
                .cloned();
            if let Some(g) = &guide {
                if t >= g.end_time {
                    self.guidance.get_mut(id).unwrap().status = GuidanceStatus::Completed;
                    self.controls.get_mut(id).unwrap().throttle = 0.0;
                } else {
                    let rating = self.full_rating_of(&v);
                    if (rating.force - g.force).length() > 1e-8
                        || (rating.flow_kg_per_second - g.flow).abs() > 1e-10
                        || rating.torque.length() > 1e-6
                    {
                        self.cancel_guidance(id, "active propulsion group changed");
                    }
                }
            }
            let guide = self
                .guidance
                .get(id)
                .filter(|g| g.status == GuidanceStatus::Armed)
                .cloned();
            let p = self.propulsion_at(&v, environment.as_ref(), t);
            let burning = p.flow_kg_per_second > 0.0;
            let turning = !(burning && guide.is_some())
                && (w != DVec3::ZERO
                    || p.torque != DVec3::ZERO
                    || self.controls[id].turn != DVec3::ZERO
                    || self.sas.contains_key(id));
            let mut leg = if self.environment.is_some() {
                end.min(t + self.options.flight_chunk_seconds)
            } else {
                end
            };
            if let Some(g) = &guide {
                leg = leg.min(if t < g.start_time {
                    g.start_time
                } else {
                    g.end_time
                });
            }
            if burning && turning {
                leg = leg.min(t + self.options.step_seconds);
            }
            if burning {
                leg = leg.min(t + p.seconds_to_flameout);
            }
            let control = if burning
                && let Some(g) = &guide
                && p.force.length() > 0.0
            {
                let direction = self.propagator.thrust_direction(
                    &self.ephemeris,
                    &g.attitude,
                    t,
                    run.state().position,
                    run.state().velocity,
                );
                let aligned =
                    (DQuat::from_rotation_arc(q * p.force.normalize(), direction) * q).normalize();
                let Owner::Orbit {
                    rotation,
                    angular_velocity,
                    ..
                } = &mut v.owner
                else {
                    unreachable!()
                };
                *rotation = aligned;
                *angular_velocity = DVec3::ZERO;
                Some(Control::Thrust(void_orbit::ThrustControl {
                    thrust_newtons: p.force.length(),
                    exhaust_velocity: p.force.length() / p.flow_kg_per_second,
                    minimum_mass_kg: self.mass(&v.poses)
                        - p.groups.iter().map(|g| g.fuel_kg).sum::<f64>(),
                    attitude: g.attitude,
                }))
            } else if burning {
                Some(Control::Force(ForceControl {
                    force: q * p.force,
                    mass_flow_kg_per_second: p.flow_kg_per_second,
                    minimum_mass_kg: self.mass(&v.poses)
                        - p.groups.iter().map(|g| g.fuel_kg).sum::<f64>(),
                }))
            } else {
                None
            };
            let Owner::Orbit { run, .. } = &mut v.owner else {
                unreachable!()
            };
            self.propagate(run, leg, control, environment.and_then(|s| s.air));
            if burning {
                let duration = if leg == t + p.seconds_to_flameout {
                    p.seconds_to_flameout
                } else {
                    leg - t
                };
                burn(&mut self.parts, &p.groups, duration);
                let mass = self.mass(&v.poses);
                let (poses, c) = self.centred(&v.poses);
                v.poses = poses;
                let Owner::Orbit { run, rotation, .. } = &mut v.owner else {
                    unreachable!()
                };
                assert!(
                    (run.y[6] - mass).abs() < 1e-9 * mass,
                    "fleet: mass mismatch"
                );
                run.y[6] = mass;
                if c != DVec3::ZERO {
                    let d = *rotation * c;
                    let u = if guide.is_some() {
                        DVec3::ZERO
                    } else {
                        w.cross(d)
                    };
                    for (i, value) in d.to_array().iter().enumerate() {
                        run.y[i] += value;
                    }
                    for (i, value) in u.to_array().iter().enumerate() {
                        run.y[i + 3] += value;
                    }
                    **run = run.restarted();
                }
            }
            if burning
                && p.force.length() > 0.0
                && let Some(g) = &guide
            {
                let Owner::Orbit {
                    run,
                    rotation,
                    angular_velocity,
                } = &mut v.owner
                else {
                    unreachable!()
                };
                let state = run.state();
                let direction = self.propagator.thrust_direction(
                    &self.ephemeris,
                    &g.attitude,
                    leg,
                    state.position,
                    state.velocity,
                );
                *rotation = (DQuat::from_rotation_arc(*rotation * p.force.normalize(), direction)
                    * *rotation)
                    .normalize();
                *angular_velocity = DVec3::ZERO;
                if leg == g.end_time {
                    self.guidance.get_mut(id).unwrap().status = GuidanceStatus::Completed;
                    self.controls.get_mut(id).unwrap().throttle = 0.0;
                }
            }
            if turning {
                let mut s = t;
                while s < leg - 1e-12 {
                    let h = self.options.step_seconds.min(leg - s);
                    let Owner::Orbit {
                        rotation,
                        angular_velocity,
                        ..
                    } = &v.owner
                    else {
                        unreachable!()
                    };
                    let torque = p.torque + self.steering(&v, *rotation, *angular_velocity, h);
                    let inertia = rows(self.inertia_of(&v.poses));
                    let Owner::Orbit {
                        rotation,
                        angular_velocity,
                        ..
                    } = &mut v.owner
                    else {
                        unreachable!()
                    };
                    (*rotation, *angular_velocity) = rotation_step(
                        *rotation,
                        *angular_velocity,
                        &inertia,
                        torque,
                        DVec3::ZERO,
                        h,
                    );
                    s += h;
                }
            }
        }
        self.vessels.insert(id.into(), v);
    }
    fn step_scene(&mut self, scene: u64) {
        let dt = self.options.step_seconds;
        let ids = self.scenes[&scene].members.clone();
        let mut plans = vec![];
        for id in ids {
            let v = self.vessel(&id).clone();
            let Owner::Scene { body, push, .. } = v.owner else {
                unreachable!()
            };
            let world = &self.scenes[&scene].world;
            let b = world.body(body);
            let q = quat64(*b.rotation());
            let w = vec64(b.angvel());
            let c = vec64(b.local_center_of_mass());
            let environment = self.environment_sample(&v, self.time);
            let p = self.propulsion_with_environment(&v, environment.as_ref());
            let air = environment.and_then(|s| s.air);
            let snapshot = self.snapshot_of(&v);
            let air_acceleration = air.map_or(DVec3::ZERO, |source| {
                self.axes(scene).conjugate()
                    * source.acceleration(
                        self.time,
                        snapshot.position,
                        snapshot.velocity,
                        snapshot.mass_kg,
                    )
            });
            let (force, tau, burned) = step_thrust(&p, dt, c);
            let resting =
                b.is_sleeping() && self.controls[&id].turn == DVec3::ZERO && p.groups.is_empty();
            let torque = if resting {
                DVec3::ZERO
            } else {
                tau + self.steering(&v, q, w, dt)
            };
            let now = q * force / (self.mass(&v.poses) - burned / 2.0) + air_acceleration;
            self.scenes
                .get_mut(&scene)
                .unwrap()
                .world
                .apply_local_torque(body, torque);
            plans.push((id, body, push, now, p));
        }
        let world = &mut self.scenes.get_mut(&scene).unwrap().world;
        if let SceneFrame::Bubble(f) = &mut world.frame {
            f.advance_origin(&mut self.ephemeris, self.time + dt);
        }
        world.step(
            &mut self.ephemeris,
            Some(&mut |body, _| {
                let p = plans
                    .iter()
                    .find(|(_, b, _, _, _)| *b == body)
                    .expect("scene body has no vessel");
                (p.2 + p.3) / 2.0
            }),
        );
        for (id, body, _, now, p) in plans {
            if let Owner::Scene { push, .. } = &mut self.vessels.get_mut(&id).unwrap().owner {
                *push = now;
            }
            if p.groups.is_empty() {
                continue;
            }
            burn(&mut self.parts, &p.groups, dt);
            let masses: Vec<_> = self
                .vessel(&id)
                .poses
                .iter()
                .map(|(id, _)| self.part_mass(id))
                .collect();
            self.scenes
                .get_mut(&scene)
                .unwrap()
                .world
                .set_piece_masses(body, &masses);
        }
    }
    fn step_all(&mut self) {
        let end = self.time + self.options.step_seconds;
        for scene in self.scenes.keys().copied().collect::<Vec<_>>() {
            self.step_scene(scene);
        }
        for id in self.order.clone() {
            if matches!(self.vessel(&id).owner, Owner::Orbit { .. }) {
                self.advance_orbit(&id, end);
            }
        }
        self.time = end;
        for scene in self.scenes.keys().copied().collect::<Vec<_>>() {
            let s = &self.scenes[&scene];
            let mut mass = 0.0;
            let mut p = DVec3::ZERO;
            for id in &s.members {
                let v = self.vessel(id);
                let Owner::Scene { body, .. } = v.owner else {
                    unreachable!()
                };
                let m = self.mass(&v.poses);
                mass += m;
                p += vec64(s.world.body(body).translation()) * m;
            }
            let c = p / mass;
            if c.length() > self.options.follow_meters {
                let origin = s.world.origin + c;
                self.scenes.get_mut(&scene).unwrap().world.recenter(origin);
            }
        }
    }
    pub fn advance(&mut self, dt: f64) {
        assert!(dt >= 0.0 && dt.is_finite(), "fleet: invalid dt");
        let step = self.options.step_seconds;
        let target = self.time + self.pending + dt;
        loop {
            let lookahead = if self.scenes.is_empty() {
                self.options
                    .flight_chunk_seconds
                    .min((target - self.time).max(0.0))
            } else {
                step
            };
            self.reconcile(lookahead);
            if !self.scenes.is_empty() {
                if self.time + step > target + 1e-12 {
                    self.pending = (target - self.time).max(0.0);
                    return;
                }
                self.step_all();
            } else {
                if self.time + 1e-9 >= target {
                    self.pending = 0.0;
                    return;
                }
                let end = target
                    .min(self.time + self.options.flight_chunk_seconds)
                    .min(self.time + self.band_safe_seconds());
                for id in self.order.clone() {
                    self.advance_orbit(&id, end);
                }
                self.time = end;
            }
        }
    }
    pub fn rails_blocker(&self) -> Option<String> {
        if self
            .guidance
            .values()
            .any(|g| g.status == GuidanceStatus::Armed)
        {
            return Some("scheduled maneuver: use physics time".into());
        }
        for id in &self.order {
            if self.propulsion_of(self.vessel(id)).flow_kg_per_second > 0.0 {
                return Some(format!("engine firing on {id}"));
            }
        }
        if self
            .scenes
            .values()
            .any(|s| s.ground.is_some() && !s.world.asleep())
        {
            Some("moving near the ground".into())
        } else {
            None
        }
    }
    pub fn advance_on_rails(&mut self, dt: f64) -> bool {
        assert!(dt >= 0.0 && dt.is_finite(), "fleet: invalid rails dt");
        assert!(
            self.rails_blocker().is_none(),
            "fleet: rails blocked: {:?}",
            self.rails_blocker()
        );
        let together: HashSet<_> = self.gate.active_pairs().into_iter().collect();
        for id in self.order.clone() {
            if matches!(self.vessel(&id).owner,Owner::Scene { scene,.. } if self.scenes[&scene].ground.is_none())
            {
                self.move_to_orbit(&id);
            }
            if let Owner::Orbit {
                angular_velocity, ..
            } = &mut self.vessels.get_mut(&id).unwrap().owner
            {
                *angular_velocity = DVec3::ZERO;
            }
        }
        let target = self.time + self.pending + dt;
        self.pending = 0.0;
        let mut done = true;
        'coast: while self.time + 1e-9 < target {
            if self.order.iter().any(|id| {
                matches!(self.vessel(id).owner, Owner::Orbit { .. })
                    && self.ground_for(self.vessel(id)).is_some()
            }) {
                done = false;
                break;
            }
            let chunk = if self.environment.is_some() {
                self.options.flight_chunk_seconds
            } else {
                self.options.rails_chunk_seconds
            };
            let end = target
                .min(self.time + chunk)
                .min(self.time + self.band_safe_seconds());
            let ids = self.order.clone();
            for (i, a) in ids.iter().enumerate() {
                for b in &ids[i + 1..] {
                    let pair = self.gate.update(
                        a,
                        self.snapshot(a).state(),
                        b,
                        self.snapshot(b).state(),
                        end - self.time,
                    );
                    let key = if a < b {
                        (a.clone(), b.clone())
                    } else {
                        (b.clone(), a.clone())
                    };
                    if pair.physics && pair.changed && !together.contains(&key) {
                        done = false;
                        break 'coast;
                    }
                }
            }
            for id in ids {
                let sample = self.environment_sample(self.vessel(&id), self.time);
                self.propagator.set_air_source(sample.and_then(|s| s.air));
                if let Owner::Orbit { run, .. } = &mut self.vessels.get_mut(&id).unwrap().owner {
                    let outcome =
                        self.propagator
                            .advance(&mut self.ephemeris, run, end, 100_000, None, None);
                    assert_eq!(outcome, AdvanceOutcome::Reached);
                }
            }
            for s in self.scenes.values_mut() {
                s.world.idle_to(&mut self.ephemeris, end);
            }
            self.time = end;
        }
        for sas in self.sas.values_mut() {
            sas.assist.set_enabled(true);
        }
        done
    }
}

#[path = "checkpoint.rs"]
mod checkpoint;
pub use checkpoint::FleetCheckpoint;
