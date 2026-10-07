use crate::{FreeFallFrame, Propulsion, burn, propulsion, step_thrust};
use glam::{DMat3, DQuat, DVec3};
use rapier3d::prelude::RigidBodyHandle;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;
use void_assembly::{
    Connection, Craft, Module, PartDefinition, PartGraph, PartPose, Shape, compile, node,
    part_bound_radius, part_box_size, part_inertia_per_kg,
};
use void_environment::Environment;
use void_frames::{
    BodyId, FrameId, FrameSource, FrameTree, Motion, Snapshot, SplitPosition, State, SystemId,
};
use void_landing::{
    BodyShape, ContactBodySpec, ContactFrame, ContactWorld, ContactWorldOptions,
    EncounterPhysicsGate, EncounterRanges, FrameState, Piece, PieceMass, PlanetFrame, SimpleShape,
};
pub use void_modules::rcs::RcsControl;
use void_modules::{Conditions, has_atmosphere, vessel_air_at};
use void_orbit::{
    AdvanceOutcome, AirSource, CelestialBody, Control, EphemerisSource, ForceControl,
    PropagationRun, SystemFrames, Tolerances, VesselPropagator, VesselState,
};
use void_rotation::{Mat3, rotation_step};
use void_sas::{SAS_TUNING, SasPhase, StabilityAssist};
use void_terrain::Terrain;

mod guidance;
mod thermal;
mod vehicles;
mod wrenches;
pub use guidance::{GuidanceStatus, GuidedBurn};
use wrenches::{GuidedAirSource, RigidFlightSource, SceneStepSource};

type SceneGroup = (Option<usize>, Vec<String>, Vec<(u64, usize)>);
/// Explicit physics configurations; full air dynamics is accepted in its lab before opting
/// the main game in. ForceOnly retains the original no-spin air sampling and force pathway.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AirDynamics {
    ForceOnly,
    ForceAndTorque,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct FleetOptions {
    pub air_dynamics: AirDynamics,
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
            air_dynamics: AirDynamics::ForceOnly,
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
/// Contact for one body; its terrain is the world `Environment`'s.
pub struct GroundSpec {
    pub body_index: usize,
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
    /// Inertial (origin frame) placement.
    pub position: DVec3,
    pub rotation: DQuat,
    /// The vessel's parts frame, and the part's pose in it: renderers go from here.
    pub frame: FrameId,
    pub local_position: DVec3,
    pub local_rotation: DQuat,
    pub fuel_kg: f64,
    pub resources: void_assembly::Resources,
    pub modules: BTreeMap<String, void_assembly::ModuleState>,
    pub module_stages: BTreeMap<String, Option<u32>>,
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
    /// The vessel's parts frame; the mesh is in its coordinates.
    pub frame: FrameId,
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
    /// The scene's contact frame; the tile sits at `local_position` in its axes.
    pub frame: FrameId,
    pub local_position: DVec3,
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
    /// The contact world's coordinates: the ground body's surface frame, or the bubble's
    /// free-falling origin under the origin system.
    contact: FrameId,
    /// The contact world's floating origin, a child of `contact`.
    floating: FrameId,
}
/// What a dynamic frame of the fleet's tree follows.
#[derive(Clone, Debug)]
enum Dynamic {
    /// A bubble's free-falling origin.
    Bubble(u64),
    /// A scene's floating origin.
    Floating(u64),
    /// A vessel's parts frame (its parts' poses are in it).
    Vessel(String),
    /// A part, at its pose in its vessel's parts frame.
    Part(String),
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
/// One connected group of the part graph and its physics owner. The members' order is the order
/// their masses, inertia and thrust are summed in.
struct Vessel {
    id: String,
    name: String,
    root: String,
    members: Vec<String>,
    owner: Owner,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Sas {
    assist: StabilityAssist,
    ground: Option<usize>,
}
/// Fleet owns the part graph. Every connected group of it is a vessel with exactly one physics
/// owner.
pub struct Fleet {
    pub ephemeris: Box<dyn EphemerisSource>,
    pub options: FleetOptions,
    pub events: Vec<FleetEvent>,
    propagator: VesselPropagator,
    /// The world's gravity, air, terrain and sea.
    environment: Arc<Environment>,
    grounds: Vec<Ground>,
    /// Every part's state and pose, and the connections between parts.
    parts: PartGraph,
    vessels: BTreeMap<String, Vessel>,
    order: Vec<String>,
    controls: HashMap<String, VesselControl>,
    rcs_controls: BTreeMap<String, RcsControl>,
    guidance: BTreeMap<String, GuidedBurn>,
    sas: HashMap<String, Sas>,
    scenes: BTreeMap<u64, Scene>,
    /// Star systems and bodies, with scenes and vessels as dynamic frames below them.
    frames: SystemFrames,
    dynamic: HashMap<u64, Dynamic>,
    vessel_frames: HashMap<String, FrameId>,
    /// Every part's frame, under its vessel's parts frame.
    part_frames: HashMap<String, FrameId>,
    next_key: u64,
    gate: EncounterPhysicsGate,
    time: f64,
    pending: f64,
    next_vessel: u64,
    next_scene: u64,
}
/// The fleet's tree reads systems and bodies from the ephemeris and scenes and vessels from the
/// fleet's own state, which exists at the fleet's time only.
impl FrameSource for Fleet {
    fn system_state(&self, system: SystemId, t: f64) -> (SplitPosition, DVec3) {
        self.ephemeris.system_state(system, t)
    }
    fn body_in_system(&self, body: BodyId, t: f64) -> (DVec3, DVec3) {
        self.ephemeris.body_in_system(body, t)
    }
    fn dynamic_motion(&self, key: u64, t: f64) -> Motion {
        assert_eq!(t, self.time, "fleet frames exist at the fleet's time only");
        match self
            .dynamic
            .get(&key)
            .expect("fleet: unknown dynamic frame")
        {
            Dynamic::Bubble(scene) => {
                let SceneFrame::Bubble(f) = &self.scenes[scene].world.frame else {
                    panic!("fleet: scene {scene} is not a bubble")
                };
                let o = f.origin(t);
                Motion::new(o.position, o.velocity, DQuat::IDENTITY, DVec3::ZERO)
            }
            Dynamic::Floating(scene) => {
                Motion::fixed(self.scenes[scene].world.origin, DQuat::IDENTITY)
            }
            Dynamic::Vessel(id) => self.vessel_motion(self.vessel(id)),
            Dynamic::Part(id) => {
                let pose = self.parts.part(id).pose;
                Motion::fixed(pose.position, pose.rotation)
            }
        }
    }
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
fn state_of(s: FrameState) -> State {
    State {
        position: s.position,
        velocity: s.velocity,
    }
}
fn frame_state(s: State) -> FrameState {
    FrameState {
        position: s.position,
        velocity: s.velocity,
    }
}
impl Fleet {
    pub fn new(
        mut ephemeris: impl EphemerisSource + 'static,
        environment: Arc<Environment>,
        time: f64,
        grounds: Vec<GroundSpec>,
        options: FleetOptions,
    ) -> Self {
        environment.assert_compatible(&ephemeris);
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
                // The environment already holds terrain to its body's sphere.
                assert!(
                    environment
                        .body(spec.body_index)
                        .is_some_and(|b| b.terrain.is_some()),
                    "fleet: ground on a body without terrain"
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
        let frames = SystemFrames::new(&ephemeris);
        Self {
            ephemeris: Box::new(ephemeris),
            options,
            events: vec![],
            propagator,
            environment,
            grounds,
            parts: PartGraph::new(),
            vessels: BTreeMap::new(),
            order: vec![],
            controls: HashMap::new(),
            rcs_controls: BTreeMap::new(),
            guidance: BTreeMap::new(),
            sas: HashMap::new(),
            scenes: BTreeMap::new(),
            frames,
            dynamic: HashMap::new(),
            vessel_frames: HashMap::new(),
            part_frames: HashMap::new(),
            next_key: 0,
            gate: EncounterPhysicsGate::new(options.encounter),
            time,
            pending: 0.0,
            next_vessel: 1,
            next_scene: 1,
        }
    }
    pub fn environment(&self) -> &Arc<Environment> {
        &self.environment
    }
    /// The star systems' and bodies' frames under `frames()`, for environment queries.
    pub fn system_frames(&self) -> &SystemFrames {
        &self.frames
    }
    fn terrain(&self, body: usize) -> &Arc<Terrain> {
        self.environment
            .body(body)
            .and_then(|b| b.terrain.as_ref())
            .expect("fleet: body has no terrain")
    }
    fn conditions(&self, v: &Vessel, time: f64) -> Conditions {
        if !has_atmosphere(&self.environment) {
            return Conditions::VACUUM;
        }
        let snapshot = self.snapshot_of(v);
        Conditions::at(
            &self.environment,
            &*self.ephemeris,
            time,
            State {
                position: snapshot.position,
                velocity: snapshot.velocity,
            },
        )
    }
    fn air_source(&self, v: &Vessel) -> Option<Arc<dyn AirSource>> {
        if !has_atmosphere(&self.environment) {
            return None;
        }
        let snapshot = self.snapshot_of(v);
        vessel_air_at(
            &self.environment,
            &self.parts,
            &v.members,
            self.centre(&v.members),
            snapshot.rotation,
            match &v.owner {
                Owner::Orbit { run, .. } => run.time,
                _ => self.time,
            },
        )
        .map(|air| air.with_controls(self.controls[&v.id].turn))
        .map(|air| Arc::new(air) as Arc<dyn AirSource>)
    }
    pub fn air_data(&self, id: &str) -> void_modules::AirData {
        let v = self.vessel(id);
        let snap = self.snapshot_of(v);
        vessel_air_at(
            &self.environment,
            &self.parts,
            &v.members,
            self.centre(&v.members),
            snap.rotation,
            self.time,
        )
        .map(|air| air.with_controls(self.controls[&v.id].turn))
        .map_or(void_modules::AirData::default(), |air| {
            air.air_data(
                &*self.ephemeris,
                self.time,
                State {
                    position: snap.position,
                    velocity: snap.velocity,
                },
                snap.rotation,
                snap.angular_velocity,
            )
        })
    }
    /// Current passive aerodynamic load in the origin frame, about the live COM. No state is
    /// prepared/committed and no force is applied; useful to HUDs and acceptance diagnostics.
    pub fn aerodynamic_wrench(&self, id: &str) -> void_modules::Wrench {
        let v = self.vessel(id);
        let snap = self.snapshot_of(v);
        vessel_air_at(
            &self.environment,
            &self.parts,
            &v.members,
            self.centre(&v.members),
            snap.rotation,
            self.time,
        )
        .map(|air| air.with_controls(self.controls[&v.id].turn))
        .map_or(
            void_modules::Wrench::zero(self.origin_frame(), snap.position),
            |air| {
                if self.options.air_dynamics == AirDynamics::ForceOnly {
                    return void_modules::Wrench {
                        force: air.acceleration(
                            &*self.ephemeris,
                            self.time,
                            snap.position,
                            snap.velocity,
                            snap.mass_kg,
                        ) * snap.mass_kg,
                        ..void_modules::Wrench::zero(self.origin_frame(), snap.position)
                    };
                }
                air.wrench(
                    &*self.ephemeris,
                    self.time,
                    State {
                        position: snap.position,
                        velocity: snap.velocity,
                    },
                    snap.rotation,
                    snap.angular_velocity,
                )
            },
        )
    }
    /// Commands are addressed to immutable part/module identities, never vector positions.
    pub fn set_module_stage(&mut self, part: &str, module: &str, stage: Option<u32>) {
        let id = self.vessel_of_part(part);
        self.cancel_guidance(&id, "module staging changed");
        self.parts.set_module_stage(part, module, stage);
    }
    pub fn parachute_command(
        &mut self,
        part: &str,
        module: &str,
        command: void_modules::parachute::Command,
    ) {
        let id = self.vessel_of_part(part);
        self.cancel_guidance(&id, "parachute command");
        let void_assembly::ModuleState::Parachute { state } = self
            .parts
            .part(part)
            .modules
            .get(module)
            .expect("unknown parachute module")
        else {
            panic!("module is not parachute")
        };
        let state = void_modules::parachute::command(*state, command);
        self.parts.set_module_state(
            part,
            module,
            void_assembly::ModuleState::Parachute { state },
        );
    }
    fn active_parachutes(&self) -> bool {
        self.parts.parts().any(|p|p.modules.values().any(|m|matches!(m,void_assembly::ModuleState::Parachute{state} if void_modules::parachute::active(*state))))
    }
    fn prepare_parachutes(&mut self) {
        let mut changes = vec![];
        for id in &self.order {
            let v = self.vessel(id);
            let snap = self.snapshot_of(v);
            let centre = self.centre(&v.members);
            for pid in &v.members {
                let part = self.parts.part(pid);
                for m in &part.definition.modules {
                    if let Module::Parachute {
                        id: mid,
                        parameters,
                    } = m
                    {
                        let void_assembly::ModuleState::Parachute { state } = part.modules[mid]
                        else {
                            panic!("parachute state mismatch")
                        };
                        let position = snap.position
                            + snap.rotation
                                * (part.pose.position + part.pose.rotation * parameters.point
                                    - centre);
                        let velocity =
                            snap.velocity + snap.angular_velocity.cross(position - snap.position);
                        let frames = self.environment.frames();
                        let at = frames.tree.at(self.time, &*self.ephemeris);
                        let conditions = (0..self.environment.bodies().len()).find_map(|body| {
                            let sample = self.environment.surroundings(
                                &at,
                                frames,
                                frames.origin,
                                State { position, velocity },
                                body,
                            );
                            sample.air.map(|air| void_modules::parachute::Conditions {
                                pressure_pa: air.air.pressure_pa,
                                dynamic_pressure_pa: 0.5
                                    * air.air.density
                                    * air.airspeed.length_squared(),
                                altitude_meters: air.altitude,
                            })
                        });
                        changes.push((
                            pid.clone(),
                            mid.clone(),
                            void_modules::parachute::prepare(state, parameters, conditions),
                        ));
                    }
                }
            }
        }
        for (part, module, state) in changes {
            self.parts.set_module_state(
                &part,
                &module,
                void_assembly::ModuleState::Parachute { state },
            );
        }
    }
    fn commit_parachutes(&mut self, seconds: f64) {
        let changes: Vec<_> = self
            .parts
            .parts()
            .flat_map(|p| {
                p.definition.modules.iter().filter_map(|m| {
                    if let Module::Parachute { id, parameters } = m {
                        let void_assembly::ModuleState::Parachute { state } = p.modules[id] else {
                            panic!("parachute state mismatch")
                        };
                        Some((
                            p.id.clone(),
                            id.clone(),
                            void_modules::parachute::commit(state, parameters, seconds),
                        ))
                    } else {
                        None
                    }
                })
            })
            .collect();
        for (part, module, state) in changes {
            self.parts.set_module_state(
                &part,
                &module,
                void_assembly::ModuleState::Parachute { state },
            );
        }
    }
    pub fn time(&self) -> f64 {
        self.time
    }
    /// Substep time already requested but not yet integrated by the fixed-step owners.
    pub fn pending_seconds(&self) -> f64 {
        self.pending
    }
    pub fn connection_snapshots(&self) -> Vec<Connection> {
        self.parts.connections().to_vec()
    }
    /// Every part's state and pose (in its vessel's parts frame), and the connections.
    pub fn parts(&self) -> &PartGraph {
        &self.parts
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
    fn mass(&self, members: &[String]) -> f64 {
        self.parts.mass(members)
    }
    fn part_mass(&self, id: &str) -> f64 {
        self.parts.part(id).mass_kg()
    }
    /// The members' centre of mass in their vessel's parts frame.
    fn centre(&self, members: &[String]) -> DVec3 {
        members.iter().fold(DVec3::ZERO, |sum, id| {
            sum + self.parts.part(id).pose.position * self.part_mass(id)
        }) / self.mass(members)
    }
    /// Moves the parts frame's origin to the members' centre of mass; returns where it was.
    fn recentre(&mut self, members: &[String]) -> DVec3 {
        let c = self.centre(members);
        for id in members {
            let mut pose = self.parts.part(id).pose;
            pose.position -= c;
            self.parts.set_pose(id, pose);
        }
        c
    }
    /// The members' inertia about `about` in their parts frame's axes.
    fn inertia_of(&self, members: &[String], about: DVec3) -> DMat3 {
        members.iter().fold(DMat3::ZERO, |sum, id| {
            let part = self.parts.part(id);
            let d = part_inertia_per_kg(part.definition);
            let r = DMat3::from_quat(part.pose.rotation);
            let v = part.pose.position - about;
            let outer = DMat3::from_cols(v * v.x, v * v.y, v * v.z);
            sum + (r * DMat3::from_diagonal(d) * r.transpose()
                + DMat3::IDENTITY * v.length_squared()
                - outer)
                * self.part_mass(id)
        })
    }
    pub fn inertia(&self, id: &str) -> Mat3 {
        let v = self.vessel(id);
        rows(self.inertia_of(&v.members, self.centre(&v.members)))
    }
    pub fn has_command(&self, id: &str) -> bool {
        self.commanded(self.vessel(id))
    }
    pub fn command_failed(&self, id: &str) -> bool {
        !self.has_command(id)
            && self
                .vessel(id)
                .members
                .iter()
                .any(|p| self.parts.part(p).is_command() && self.parts.part(p).thermally_failed())
    }
    fn commanded(&self, v: &Vessel) -> bool {
        v.members
            .iter()
            .any(|id| self.parts.part(id).is_command() && !self.parts.part(id).thermally_failed())
    }
    fn propulsion_at(&self, v: &Vessel, conditions: &Conditions, time: f64) -> Propulsion {
        propulsion(
            &self.parts,
            &v.members,
            self.effective_throttle(&v.id, time),
            self.centre(&v.members),
            conditions,
        )
        .combined(crate::rcs_propulsion(
            &self.parts,
            &v.members,
            self.centre(&v.members),
            self.rcs_controls[&v.id],
        ))
    }
    fn propulsion_of(&self, v: &Vessel) -> Propulsion {
        self.propulsion_at(v, &self.conditions(v, self.time), self.time)
    }
    pub fn thrust(&self, id: &str) -> Propulsion {
        self.propulsion_of(self.vessel(id))
    }
    /// Read-only vacuum rating for planning. Unlike `thrust`, this asks for full throttle even
    /// when the pilot currently coasts; it does not ignite unstaged engines or mutate controls.
    pub fn full_throttle_vacuum_thrust(&self, id: &str) -> Propulsion {
        let v = self.vessel(id);
        propulsion(
            &self.parts,
            &v.members,
            1.0,
            self.centre(&v.members),
            &Conditions::VACUUM,
        )
    }
    pub fn fuel(&self, id: &str) -> f64 {
        self.parts.part(id).fuel_kg()
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
    pub fn set_rcs_nozzle_enabled(&mut self, part: &str, module: &str, enabled: bool) {
        self.parts
            .set_module_state(part, module, void_assembly::ModuleState::Rcs { enabled });
    }
    pub fn rcs_control(&self, id: &str) -> RcsControl {
        self.rcs_controls[id]
    }
    pub fn set_rcs_control(&mut self, id: &str, control: RcsControl) {
        assert!(
            control.force.is_finite() && control.torque.is_finite(),
            "invalid RCS control"
        );
        assert!(
            self.commanded(self.vessel(id))
                || (!control.enabled
                    && control.force == DVec3::ZERO
                    && control.torque == DVec3::ZERO),
            "RCS requires command part"
        );
        if control.enabled && (control.force != DVec3::ZERO || control.torque != DVec3::ZERO) {
            self.cancel_guidance(id, "manual RCS control");
        }
        self.rcs_controls.insert(id.into(), control);
    }
    /// Allocation/HUD observation is recomputed from the authoritative graph and controls.
    pub fn rcs_allocation(&self, id: &str) -> void_modules::rcs::Allocation {
        let v = self.vessel(id);
        void_modules::rcs::allocate(
            &self.parts,
            &v.members,
            self.centre(&v.members),
            self.rcs_controls[id],
        )
    }
    pub fn has_reaction_wheel(&self, id: &str) -> bool {
        self.vessel(id).members.iter().any(|part| {
            self.parts
                .part(part)
                .definition
                .modules
                .iter()
                .any(|module| {
                    matches!(
                        module,
                        Module::Command {
                            reaction_wheel: true,
                            ..
                        }
                    )
                })
        })
    }
    /// Capability-aware user request; unsupported hardware is an explicit rejection.
    pub fn request_sas(&mut self, id: &str, on: bool) -> Result<(), String> {
        if on && !self.has_reaction_wheel(id) {
            return Err(
                "SAS reaction wheel unavailable: vessel has no reaction-wheel command module"
                    .into(),
            );
        }
        self.set_sas(id, on);
        Ok(())
    }
    pub fn set_sas(&mut self, id: &str, on: bool) {
        let v = self.vessel(id);
        if on {
            assert!(
                self.commanded(v) && self.has_reaction_wheel(id),
                "fleet: SAS requires reaction-wheel command module"
            );
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
                p.definition
                    .modules
                    .iter()
                    .filter(|m| matches!(m, Module::Engine { .. } | Module::Decoupler { .. }))
                    .all(|m| p
                        .instance
                        .module_stages
                        .get(m.id())
                        .copied()
                        .unwrap_or(p.instance.stage)
                        .is_some()),
                "fleet: engine/decoupler module needs stage"
            );
        }
        let id = format!("v{}", self.next_vessel);
        self.next_vessel += 1;
        let members = self.parts.add(&c, &id);
        self.recentre(&members);
        let run = PropagationRun::new(VesselState {
            time: self.time,
            position: state.position,
            velocity: state.velocity,
            mass_kg: self.mass(&members),
        });
        let v = Vessel {
            id: id.clone(),
            name: craft.name.clone(),
            root: format!("{id}/{}", c.root_id),
            members,
            owner: Owner::Orbit {
                run: Box::new(run),
                rotation,
                angular_velocity,
            },
        };
        self.put(v);
        self.order.push(id.clone());
        self.controls.insert(id.clone(), VesselControl::default());
        self.rcs_controls.insert(id.clone(), RcsControl::default());
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
                let hull = if p.definition.box_size_meters.is_none() {
                    // Keep unchanged Craft2 launch arithmetic, including its subtraction order.
                    let radial = if p.definition.shape == Shape::Box {
                        p.definition.radius
                            * ((p.pose.rotation * DVec3::X).y.abs()
                                + (p.pose.rotation * DVec3::Z).y.abs())
                    } else {
                        p.definition.radius * (1.0 - ay * ay).max(0.0).sqrt()
                    };
                    p.pose.position.y - ay.abs() * p.definition.height / 2.0 - radial
                } else {
                    let extent = if p.definition.shape == Shape::Box {
                        let h = part_box_size(p.definition) / 2.0;
                        h.x * (p.pose.rotation * DVec3::X).y.abs()
                            + h.y * ay.abs()
                            + h.z * (p.pose.rotation * DVec3::Z).y.abs()
                    } else {
                        ay.abs() * p.definition.height / 2.0
                            + p.definition.radius * (1.0 - ay * ay).max(0.0).sqrt()
                    };
                    p.pose.position.y - extent
                };
                p.definition.modules.iter().fold(hull, |lowest, m| match m {
                    Module::Wheel { parameters: d, .. } => {
                        let hub = p.pose.position + p.pose.rotation * d.suspension_origin;
                        let end = hub
                            + p.pose.rotation
                                * d.suspension_direction
                                * (d.rest_length_meters + d.travel_meters);
                        lowest.min(end.y - d.radius_meters)
                    }
                    _ => lowest,
                })
            })
            .fold(f64::INFINITY, f64::min);
        let g = &self.grounds[self.ground_index(body)];
        let r = g.frame.body.radius_meters + self.terrain(body).height(d) + cy - lowest + 0.05;
        let ground = self
            .frames()
            .transform(self.frames.surface[body], self.frames.origin);
        let state = frame_state(ground.apply_state(State {
            position: d * r,
            velocity: DVec3::ZERO,
        }));
        let q = void_landing::upright_at(d);
        self.launch(
            craft,
            state,
            ground.rotation() * q,
            ground.to_motion().angular_velocity,
        )
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
    /// The frame tree at the fleet's time. "Inertial" fleet states are in `origin_frame`.
    pub fn frames(&self) -> Snapshot<'_, Self> {
        self.frames.tree.at(self.time, self)
    }
    pub fn frame_tree(&self) -> &FrameTree {
        &self.frames.tree
    }
    /// The system the ephemeris' physics view, and every fleet state, is relative to.
    pub fn origin_frame(&self) -> FrameId {
        self.frames.origin
    }
    /// A body's (inertial, surface) frames.
    pub fn body_frames(&self, body: usize) -> (FrameId, FrameId) {
        (self.frames.inertial[body], self.frames.surface[body])
    }
    /// A vessel's centre of mass in its parts frame.
    /// Stable authored anchor in the vessel's parts frame. The root stays with the controlled
    /// upper stage during separation, unlike the fuel-dependent centre of mass.
    pub fn root_position_local(&self, id: &str) -> DVec3 {
        self.parts.part(&self.vessel(id).root).pose.position
    }

    pub fn centre_of_mass_local(&self, id: &str) -> DVec3 {
        match &self.vessel(id).owner {
            // The propagator carries the centre of mass as the frame's origin.
            Owner::Orbit { .. } => DVec3::ZERO,
            Owner::Scene { scene, body, .. } => {
                vec64(self.scenes[scene].world.body(*body).local_center_of_mass())
            }
        }
    }
    /// A part's frame: under its vessel's parts frame, at the part's pose.
    pub fn part_frame(&self, id: &str) -> FrameId {
        *self
            .part_frames
            .get(id)
            .unwrap_or_else(|| panic!("fleet: no frame for part {id}"))
    }
    /// A vessel's parts frame: part poses are in its coordinates.
    pub fn vessel_frame(&self, id: &str) -> FrameId {
        *self
            .vessel_frames
            .get(id)
            .unwrap_or_else(|| panic!("fleet: no frame for vessel {id}"))
    }
    /// A scene's contact frame, and its floating origin.
    pub fn scene_frames(&self, scene: u64) -> (FrameId, FrameId) {
        let s = self.scenes.get(&scene).expect("fleet: unknown scene");
        (s.contact, s.floating)
    }
    fn surface(&self, g: usize) -> FrameId {
        self.frames.surface[self.grounds[g].spec.body_index]
    }
    /// Inertial state to a ground's body-fixed coordinates.
    fn body_fixed(&self, g: usize, state: FrameState) -> FrameState {
        let s = self
            .frames()
            .transform(self.frames.origin, self.surface(g))
            .apply_state(state_of(state));
        frame_state(s)
    }
    fn axes(&self, s: u64) -> DQuat {
        self.frames()
            .transform(self.scenes[&s].contact, self.frames.origin)
            .rotation()
    }
    fn to_inertial(&self, s: u64, state: FrameState) -> FrameState {
        frame_state(
            self.frames()
                .transform(self.scenes[&s].contact, self.frames.origin)
                .apply_state(state_of(state)),
        )
    }
    fn scene_local(&self, s: u64, state: FrameState) -> FrameState {
        frame_state(
            self.frames()
                .transform(self.frames.origin, self.scenes[&s].contact)
                .apply_state(state_of(state)),
        )
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
    /// Motion of a vessel's parts frame relative to its parent: the origin system in orbit, the
    /// scene's contact frame in a scene. Rapier's velocity is the centre of mass's.
    fn vessel_motion(&self, v: &Vessel) -> Motion {
        match &v.owner {
            Owner::Orbit {
                run,
                rotation,
                angular_velocity,
            } => {
                let s = run.state();
                Motion::new(s.position, s.velocity, *rotation, *angular_velocity)
            }
            Owner::Scene { scene, body, push } => {
                let world = &self.scenes[scene].world;
                let s = world.state(&self.ephemeris, *body, *push);
                let b = world.body(*body);
                let q = quat64(*b.rotation());
                let w = vec64(b.angvel());
                let c = q * vec64(b.local_center_of_mass());
                Motion::new(s.position, s.velocity - w.cross(c), q, w)
            }
        }
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
                let up = self.vessel_motion(v);
                let out = self
                    .frames()
                    .transform(self.scenes[scene].contact, self.frames.origin);
                let centre = State {
                    position: vec64(self.scenes[scene].world.body(*body).local_center_of_mass()),
                    velocity: DVec3::ZERO,
                };
                let motion = up.then(&out.to_motion());
                (
                    frame_state(out.apply_state(up.apply_state(centre))),
                    motion.rotation,
                    motion.angular_velocity,
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
            mass_kg: self.mass(&v.members),
            part_ids: v.members.clone(),
        }
    }
    pub fn part_snapshots(&self, id: &str) -> Vec<PartSnapshot> {
        let v = self.vessel(id);
        let frame = self.vessel_frame(id);
        let frames = self.frames();
        let p = self.propulsion_of(v);
        v.members
            .iter()
            .map(|id| {
                let part = self.parts.part(id);
                let pose = &part.pose;
                let placed = frames.transform(self.part_frame(id), self.frames.origin);
                PartSnapshot {
                    id: id.clone(),
                    definition: part.definition,
                    position: placed.apply_point(DVec3::ZERO),
                    rotation: placed.rotation(),
                    frame,
                    local_position: pose.position,
                    local_rotation: pose.rotation,
                    fuel_kg: part.fuel_kg(),
                    resources: part.resources.clone(),
                    modules: part.modules.clone(),
                    module_stages: part.module_stages.clone(),
                    stage: part.stage,
                    staged: part.staged(),
                    lit: part.lit(),
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
            .find(|v| self.vessels[*v].members.iter().any(|p| p == id))
            .expect("fleet: unknown part")
            .clone()
    }
    /// A part's node and its outward direction, in inertial (origin frame) coordinates.
    pub fn node_frame(&self, id: &str, n: &str) -> (DVec3, DVec3) {
        self.node_in(id, n, self.frames.origin)
    }
    /// The distance between two parts' nodes, in the second part's frame.
    pub fn node_gap(&self, a: &str, node_a: &str, b: &str, node_b: &str) -> f64 {
        let frame = self.part_frame(b);
        (self.node_in(a, node_a, frame).0 - self.node_in(b, node_b, frame).0).length()
    }
    /// A part's node and its outward direction in `frame`'s coordinates, through the two frames'
    /// common ancestor: two nodes measured in one of their parts' frames keep the digits that
    /// inertial coordinates lose.
    pub fn node_in(&self, id: &str, n: &str, frame: FrameId) -> (DVec3, DVec3) {
        let part = self.parts.part(id);
        let n = node(part.definition, n).expect("unknown node");
        let t = self.frames().transform(self.part_frame(id), frame);
        (t.apply_point(n.position), t.apply_direction(n.direction))
    }
    pub fn free_nodes(&self, id: &str) -> Vec<FreeNode> {
        self.parts
            .free_nodes(&self.vessel(id).members)
            .into_iter()
            .map(|(part, n)| FreeNode {
                part,
                node: n.id.clone(),
                size: n.size,
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
                    frame: s.contact,
                    local_position: t.origin,
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
                        frame: self.vessel_frame(id),
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
        let (frame, terrain, mut options, origin) = if let Some(g_index) = ground {
            let g = &self.grounds[g_index];
            (
                SceneFrame::Ground(Box::new(g.frame.clone())),
                Some(self.terrain(g.spec.body_index).clone()),
                g.spec.tiles,
                self.body_fixed(g_index, c).position,
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
        self.insert_scene(id, world, ground, vec![]);
        id
    }
    fn add_dynamic(&mut self, parent: FrameId, what: Dynamic) -> FrameId {
        let key = self.next_key;
        self.next_key += 1;
        self.dynamic.insert(key, what);
        self.frames.tree.add_dynamic(parent, key)
    }
    fn insert_scene(
        &mut self,
        id: u64,
        world: ContactWorld<SceneFrame>,
        ground: Option<usize>,
        members: Vec<String>,
    ) {
        assert!(!self.scenes.contains_key(&id), "fleet: duplicate scene");
        let contact = match ground {
            Some(g) => self.surface(g),
            None => self.add_dynamic(self.frames.origin, Dynamic::Bubble(id)),
        };
        let floating = self.add_dynamic(contact, Dynamic::Floating(id));
        self.scenes.insert(
            id,
            Scene {
                world,
                ground,
                members,
                contact,
                floating,
            },
        );
    }
    fn remove_scene(&mut self, id: u64) {
        let scene = self.scenes.remove(&id).expect("fleet: unknown scene");
        self.frames.tree.remove(scene.floating);
        if scene.ground.is_none() {
            self.frames.tree.remove(scene.contact);
        }
        self.dynamic
            .retain(|_, d| !matches!(d, Dynamic::Bubble(s) | Dynamic::Floating(s) if *s == id));
    }
    /// Stores a vessel, hangs its parts frame under its owner's frame and its parts' frames
    /// under its parts frame.
    fn put(&mut self, v: Vessel) {
        let parent = match v.owner {
            Owner::Orbit { .. } => self.frames.origin,
            Owner::Scene { scene, .. } => self.scenes[&scene].contact,
        };
        let frame = match self.vessel_frames.get(&v.id) {
            Some(&frame) => {
                if self.frames.tree.parent(frame) != Some(parent) {
                    self.frames.tree.reparent(frame, parent);
                }
                frame
            }
            None => {
                let frame = self.add_dynamic(parent, Dynamic::Vessel(v.id.clone()));
                self.vessel_frames.insert(v.id.clone(), frame);
                frame
            }
        };
        for id in &v.members {
            match self.part_frames.get(id) {
                Some(&part) => {
                    if self.frames.tree.parent(part) != Some(frame) {
                        self.frames.tree.reparent(part, frame);
                    }
                }
                None => {
                    let part = self.add_dynamic(frame, Dynamic::Part(id.clone()));
                    self.part_frames.insert(id.clone(), part);
                }
            }
        }
        assert!(
            self.vessels.insert(v.id.clone(), v).is_none(),
            "fleet: vessel stored twice"
        );
    }
    /// A vessel that no longer exists.
    fn forget_vessel_frame(&mut self, id: &str) {
        let frame = self
            .vessel_frames
            .remove(id)
            .expect("fleet: vessel has no frame");
        self.frames.tree.remove(frame);
        self.dynamic
            .retain(|_, d| !matches!(d, Dynamic::Vessel(v) if v == id));
    }
    fn remove_scene_body(&mut self, v: &Vessel, refill: bool) {
        if let Owner::Scene { scene, body, .. } = v.owner {
            // Out of the scene's frame until `put` hangs it under its next owner.
            let frame = self.vessel_frame(&v.id);
            self.frames.tree.reparent(frame, self.frames.origin);
            let s = self.scenes.get_mut(&scene).unwrap();
            s.members.retain(|id| id != &v.id);
            s.world.remove_body(body);
            if s.members.is_empty() && !refill {
                self.remove_scene(scene);
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
            .members
            .iter()
            .map(|id| {
                let part = self.parts.part(id);
                let (d, p) = (part.definition, &part.pose);
                Piece {
                    shape: match d.shape {
                        Shape::Box => SimpleShape::Box {
                            half_extents: part_box_size(d) / 2.0,
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
            mass_kg: self.mass(&v.members),
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
                let thrust = self.propulsion_of(&v);
                let air = vessel_air_at(
                    &self.environment,
                    &self.parts,
                    &v.members,
                    self.centre(&v.members),
                    snap.rotation,
                    self.time,
                )
                .map(|air| air.with_controls(self.controls[&v.id].turn))
                .map_or(DVec3::ZERO, |source| {
                    if self.options.air_dynamics == AirDynamics::ForceOnly {
                        return source.acceleration(
                            &*self.ephemeris,
                            self.time,
                            snap.position,
                            snap.velocity,
                            snap.mass_kg,
                        );
                    }
                    source
                        .wrench(
                            &*self.ephemeris,
                            self.time,
                            State {
                                position: snap.position,
                                velocity: snap.velocity,
                            },
                            snap.rotation,
                            snap.angular_velocity,
                        )
                        .force
                        / snap.mass_kg
                });
                snap.rotation * thrust.force / snap.mass_kg + air
            }
        };
        self.remove_scene_body(&v, false);
        self.recentre(&v.members);
        let axes = self.axes(scene).conjugate();
        let local = self.scene_local(scene, snap.state());
        let w = axes * snap.angular_velocity - self.scenes[&scene].world.frame.spin();
        self.add_scene_body(&mut v, scene, local, axes * snap.rotation, w, axes * push);
        self.put(v);
        self.event(id, Some(from), Some(self.scene_mode(scene)), Some(scene));
    }
    fn move_to_orbit(&mut self, id: &str) {
        let s = self.snapshot(id);
        let mut v = self.vessels.remove(id).unwrap();
        self.remove_scene_body(&v, false);
        self.recentre(&v.members);
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
        self.put(v);
        self.event(id, Some(s.mode), Some(VesselMode::Orbit), s.scene);
    }
    fn settle(&mut self, id: &str) {
        let v = self.vessel(id);
        if !matches!(v.owner, Owner::Scene { .. }) || self.centre(&v.members).length() == 0.0 {
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
        self.recentre(&v.members);
        self.add_scene_body(&mut v, scene, local, q, w, push);
        self.put(v);
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
            _ => self.body_fixed(g, self.snapshot(&v.id).state()).position,
        };
        let c = self.centre(&v.members);
        let reach = v
            .members
            .iter()
            .map(|id| {
                let part = self.parts.part(id);
                let d = part.definition;
                (part.pose.position - c).length() + part_bound_radius(d)
            })
            .fold(0.0, f64::max);
        let body = ground.spec.body_index;
        self.environment
            .ground(
                &self.frames(),
                &self.frames,
                self.frames.surface[body],
                p,
                body,
            )
            .expect("fleet: ground body has no terrain")
            .clearance
            - reach
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
            self.body_fixed(g, self.snapshot(id).state())
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
                let s = self.body_fixed(i, self.snapshot(id).state());
                let gap = (self.clearance_over(v, i) - g.spec.band_enter_meters).max(0.0);
                let speed = s.velocity.length();
                let a = 1.2 * g.frame.body.gm / s.position.length_squared()
                    + self.propulsion_of(v).force.length() / self.mass(&v.members);
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
        let mut stages: Vec<_> = self
            .vessel(id)
            .members
            .iter()
            .flat_map(|pid| {
                let p = self.parts.part(pid);
                p.module_stages.iter().filter_map(|(mid, stage)| {
                    (!p.module_activated(mid)).then_some(*stage).flatten()
                })
            })
            .collect();
        stages.sort_unstable();
        stages.dedup();
        stages
    }
    pub fn stage(&mut self, id: &str) -> Vec<String> {
        self.cancel_guidance(id, "staging");
        let Some(next) = self.stages_left(id).first().copied() else {
            return vec![];
        };
        let actions: Vec<_> = self
            .vessel(id)
            .members
            .iter()
            .flat_map(|pid| {
                let p = self.parts.part(pid);
                p.definition
                    .modules
                    .iter()
                    .filter(move |m| {
                        p.module_stages.get(m.id()) == Some(&Some(next))
                            && !p.module_activated(m.id())
                    })
                    .map(move |m| {
                        (
                            pid.clone(),
                            m.id().to_string(),
                            matches!(m, Module::Decoupler { .. }),
                        )
                    })
            })
            .collect();
        let mut split = vec![];
        for (part, module, cut) in &actions {
            if *cut {
                let new = self.decouple_module(part, module);
                self.parts.stage_module(part, module);
                split.push(new);
            }
        }
        for (part, module, cut) in actions {
            if !cut {
                self.parts.stage_module(&part, &module);
            }
        }
        split
    }
    pub fn decouple_module(&mut self, part: &str, module: &str) -> String {
        let (node_id, impulse) = self.parts.part(part).decoupler_module(module);
        self.decouple_at(part, node_id, impulse)
    }
    pub fn decouple(&mut self, part: &str) -> String {
        let (node_id, impulse) = self.parts.part(part).decoupler().expect("not a decoupler");
        self.decouple_at(part, node_id, impulse)
    }
    fn decouple_at(&mut self, part: &str, node_id: &str, impulse: f64) -> String {
        let d = self.parts.part(part).definition;
        assert!(
            self.parts.connection_at(part, node_id).is_some(),
            "decoupler node not connected"
        );
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
        self.parts.disconnect(part, node_id);
        let groups = self.parts.components(&old.members);
        assert_eq!(groups.len(), 2, "decouple: expected two groups");
        self.remove_scene_body(&old, true);
        let mut made = vec![];
        let mut created = String::new();
        for ids in groups {
            let keeps = ids.contains(&old.root);
            let members: Vec<String> = old
                .members
                .iter()
                .filter(|p| ids.contains(p))
                .cloned()
                .collect();
            let c = self.recentre(&members);
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
                    .find(|p| self.parts.part(p).is_command())
                    .unwrap_or(&ids[0])
                    .clone()
            };
            let mut v = Vessel {
                id: new_id.clone(),
                name: old.name.clone(),
                root,
                members,
                owner: old.owner.clone(),
            };
            self.rcs_controls.insert(
                new_id.clone(),
                if keeps {
                    self.rcs_controls[&id]
                } else {
                    RcsControl::default()
                },
            );
            self.controls.insert(
                new_id.clone(),
                if keeps {
                    control
                } else {
                    VesselControl::default()
                },
            );
            self.add_scene_body(&mut v, scene, local, q, w, push);
            self.put(v);
            if !keeps {
                self.event(&new_id, None, Some(self.scene_mode(scene)), Some(scene));
            }
            made.push(new_id);
        }
        let own_id = self.vessel_of_part(part);
        let other = made.iter().find(|id| *id != &own_id).unwrap();
        let own = self.vessel(&own_id);
        let pose = self.parts.part(part).pose;
        let n = node(d, node_id).unwrap();
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
    fn docking_port(
        &self,
        part: &str,
        module: &str,
    ) -> (&'static str, void_assembly::DockingDefinition) {
        match self
            .parts
            .part(part)
            .definition
            .modules
            .iter()
            .find(|m| m.id() == module)
            .expect("unknown docking module")
        {
            Module::DockingPort {
                node_id,
                parameters,
                ..
            } => (node_id.as_str(), *parameters),
            _ => panic!("not a docking port"),
        }
    }
    pub fn arm_docking_port(&mut self, part: &str, module: &str, armed: bool) {
        self.docking_port(part, module);
        self.parts.set_module_state(
            part,
            module,
            void_assembly::ModuleState::DockingPort { armed },
        );
    }
    /// Physical capture. Ordinary eligibility failures are explicit refusal reasons.
    pub fn dock(
        &mut self,
        part_a: &str,
        module_a: &str,
        part_b: &str,
        module_b: &str,
    ) -> Result<String, String> {
        let a = self.vessel_of_part(part_a);
        let b = self.vessel_of_part(part_b);
        if a == b {
            return Err("self docking".into());
        }
        let (na, pa) = self.docking_port(part_a, module_a);
        let (nb, pb) = self.docking_port(part_b, module_b);
        for (part, module, node_id) in [(part_a, module_a, na), (part_b, module_b, nb)] {
            if self.parts.part(part).thermally_failed() {
                return Err("port has thermally failed".into());
            }
            if self.parts.part(part).modules[module]
                != (void_assembly::ModuleState::DockingPort { armed: true })
            {
                return Err("port is disarmed".into());
            }
            if self.parts.connection_at(part, node_id).is_some() {
                return Err("port is occupied".into());
            }
        }
        let an = node(self.parts.part(part_a).definition, na).unwrap();
        let bn = node(self.parts.part(part_b).definition, nb).unwrap();
        if an.size != bn.size {
            return Err("port sizes differ".into());
        }
        let sa = self.snapshot(&a);
        let sb = self.snapshot(&b);
        let ap = self.parts.part(part_a).pose;
        let bp = self.parts.part(part_b).pose;
        let ar =
            sa.rotation * (ap.position + ap.rotation * an.position - self.centre_of_mass_local(&a));
        let br =
            sb.rotation * (bp.position + bp.rotation * bn.position - self.centre_of_mass_local(&b));
        let relative_centres = self.relative(&b, &a);
        let delta = relative_centres.position + br - ar;
        if delta.length() > pa.capture_distance_m.min(pb.capture_distance_m) {
            return Err("outside capture distance".into());
        }
        let normal_a = sa.rotation * ap.rotation * an.direction;
        let normal_b = sb.rotation * bp.rotation * bn.direction;
        if normal_a.dot(-normal_b) < pa.max_angle_radians.min(pb.max_angle_radians).cos() {
            return Err("port directions misaligned".into());
        }
        let relative = relative_centres.velocity + sb.angular_velocity.cross(br)
            - sa.angular_velocity.cross(ar);
        if relative.length() > pa.max_speed_mps.min(pb.max_speed_mps) {
            return Err("relative port speed too high".into());
        }
        if (sb.angular_velocity - sa.angular_velocity).length()
            > pa.max_spin_radians_per_second
                .min(pb.max_spin_radians_per_second)
        {
            return Err("relative spin too high".into());
        }
        match (sa.scene, sb.scene) {
            (Some(x), Some(y)) if x != y => {
                return Err("ports have different physics scenes".into());
            }
            (Some(scene), None) => self.move_to(&b, scene),
            (None, Some(scene)) => self.move_to(&a, scene),
            (None, None) => {
                let scene = self.new_scene(None, &[a.clone(), b.clone()]);
                self.move_to(&a, scene);
                self.move_to(&b, scene);
            }
            _ => {}
        }
        Ok(self.join(part_a, na, part_b, nb))
    }
    pub fn undock(&mut self, part: &str, module: &str) -> Result<String, String> {
        let (node_id, p) = self.docking_port(part, module);
        let Some(connection) = self.parts.connection_at(part, node_id).cloned() else {
            return Err("port is not connected".into());
        };
        let (other, other_node) = if connection.a == part {
            (&connection.b, &connection.node_b)
        } else {
            (&connection.a, &connection.node_a)
        };
        let other_module = self
            .parts
            .part(other)
            .definition
            .modules
            .iter()
            .find_map(|m| match m {
                Module::DockingPort { id, node_id, .. } if node_id == other_node => {
                    Some(id.clone())
                }
                _ => None,
            })
            .ok_or("connection is not a docking pair")?;
        self.arm_docking_port(part, module, false);
        self.arm_docking_port(other, &other_module, false);
        Ok(self.decouple_at(part, node_id, p.separation_impulse_ns))
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
        let connection = Connection {
            a: part_a.into(),
            node_a: node_a.into(),
            b: part_b.into(),
            node_b: node_b.into(),
        };
        self.parts.check_connection(&connection);
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
                self.mass(&v.members),
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
            angular += r * self.inertia_of(&v.members, DVec3::ZERO) * r.transpose() * (w + spin)
                + d.cross(s.velocity - velocity + spin.cross(d)) * m;
        }
        let inv = qa.conjugate();
        let to_a = inv * qb;
        let offset = inv * (bb.position - aa.position);
        for id in &vb.members {
            let pose = self.parts.part(id).pose;
            self.parts.set_pose(
                id,
                PartPose {
                    position: offset + to_a * pose.position,
                    rotation: to_a * pose.rotation,
                },
            );
        }
        let members: Vec<String> = va.members.iter().chain(&vb.members).cloned().collect();
        self.recentre(&members);
        let r = DMat3::from_quat(qa);
        let w =
            (r * self.inertia_of(&members, DVec3::ZERO) * r.transpose()).inverse() * angular - spin;
        self.remove_scene_body(&va, true);
        self.remove_scene_body(&vb, true);
        self.order.retain(|id| id != &b);
        let frame = self.vessel_frame(&a);
        for id in &vb.members {
            self.frames.tree.reparent(self.part_frames[id], frame);
        }
        self.forget_vessel_frame(&b);
        self.gate.remove_vessel(&b);
        self.controls.remove(&b);
        self.rcs_controls.remove(&b);
        self.sas.remove(&b);
        self.guidance.remove(&b);
        self.event(&b, Some(self.scene_mode(scene)), None, Some(scene));
        self.parts.connect(connection);
        let mut joined = Vessel { members, ..va };
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
        self.put(joined);
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
        if !v.members.iter().any(|id| {
            self.parts.part(id).definition.modules.iter().any(|module| {
                matches!(
                    module,
                    Module::Command {
                        reaction_wheel: true,
                        ..
                    }
                )
            })
        }) {
            return DVec3::ZERO;
        }
        let pilot = self.controls[&v.id].turn;
        let ground = self.attitude_ground(v);
        let inertia = rows(self.inertia_of(&v.members, self.centre(&v.members)));
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
        if air.is_some() || self.propagator.has_air_source() {
            run.invalidate_force_derivative();
        }
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
    /// Advance one accepted coupled leg through the same orbit propagator. A predictor supplies
    /// the midpoint load; rotation during every adaptive translation trial is derived from it.
    fn coupled_orbit_leg(&mut self, v: &mut Vessel, p: &Propulsion, end: f64) {
        let Owner::Orbit {
            run,
            rotation: q,
            angular_velocity: w,
        } = &v.owner
        else {
            unreachable!()
        };
        let (t, q, w, state) = (run.time, *q, *w, run.state());
        let dt = end - t;
        // A final fuel interval can be smaller than this clock's ulp. It is an accepted fuel
        // exhaustion event at the same representable time, not a zero-duration rotation step.
        assert!(
            dt > 0.0 || (p.flow_kg_per_second > 0.0 && end == t + p.seconds_to_flameout),
            "coupled leg has no accepted time or fuel event"
        );
        let burn_seconds = if end == t + p.seconds_to_flameout {
            p.seconds_to_flameout
        } else {
            dt
        };
        let inertia = rows(self.inertia_of(&v.members, self.centre(&v.members)));
        let air = vessel_air_at(
            &self.environment,
            &self.parts,
            &v.members,
            self.centre(&v.members),
            q,
            t,
        )
        .map(|air| air.with_controls(self.controls[&v.id].turn))
        .map(Arc::new);
        let initial = air.as_ref().map(|air| {
            air.wrench(
                &*self.ephemeris,
                t,
                State {
                    position: state.position,
                    velocity: state.velocity,
                },
                q,
                w,
            )
        });
        let steering = if dt == 0.0 {
            DVec3::ZERO
        } else {
            self.steering(v, q, w, dt)
        };
        let tau0 = p.torque + steering + initial.map_or(DVec3::ZERO, |a| q.conjugate() * a.torque);
        let (qm, wm) = if dt == 0.0 {
            (q, w)
        } else {
            rotation_step(q, w, &inertia, tau0, DVec3::ZERO, dt / 2.0)
        };
        let gravity = self
            .propagator
            .gravity_at(&*self.ephemeris, t, state.position);
        let acceleration =
            gravity + (q * p.force + initial.map_or(DVec3::ZERO, |a| a.force)) / state.mass_kg;
        self.ephemeris.extend_to(t + dt / 2.0);
        let middle = State {
            position: state.position + state.velocity * (dt / 2.0) + acceleration * (dt * dt / 8.0),
            velocity: state.velocity + acceleration * (dt / 2.0),
        };
        let mid_air = air
            .as_ref()
            .map(|air| air.wrench(&*self.ephemeris, t + dt / 2.0, middle, qm, wm));
        let tau = p.torque + steering + mid_air.map_or(DVec3::ZERO, |a| qm.conjugate() * a.torque);
        let source = Arc::new(RigidFlightSource {
            air,
            start: t,
            rotation: q,
            angular_velocity: w,
            inertia,
            torque_local: tau,
            force_local: p.force,
        });
        let control = (p.flow_kg_per_second > 0.0).then_some(Control::Force(ForceControl {
            // The source supplies thrust in its evolving attitude; Control owns only mass flow.
            force: DVec3::ZERO,
            mass_flow_kg_per_second: p.flow_kg_per_second,
            minimum_mass_kg: self.mass(&v.members)
                - p.groups.iter().map(|g| g.fuel_kg).sum::<f64>(),
        }));
        let Owner::Orbit { run, .. } = &mut v.owner else {
            unreachable!()
        };
        self.propagate(run, end, control, Some(source.clone()));
        let (next_q, next_w) = source.attitude(end);
        if p.flow_kg_per_second > 0.0 {
            burn(&mut self.parts, &p.groups, burn_seconds);
        }
        let mass = self.mass(&v.members);
        let centre_shift = self.recentre(&v.members);
        let Owner::Orbit {
            run,
            rotation,
            angular_velocity,
        } = &mut v.owner
        else {
            unreachable!()
        };
        assert!(
            (run.y[6] - mass).abs() < 1e-9 * mass,
            "fleet: mass mismatch"
        );
        run.y[6] = mass;
        *rotation = next_q;
        *angular_velocity = next_w;
        if centre_shift != DVec3::ZERO {
            let d = next_q * centre_shift;
            let velocity_shift = next_w.cross(d);
            for (i, x) in d.to_array().iter().enumerate() {
                run.y[i] += x;
            }
            for (i, x) in velocity_shift.to_array().iter().enumerate() {
                run.y[i + 3] += x;
            }
            **run = run.restarted();
        }
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
            let conditions = self.conditions(&v, t);
            let air = self.air_source(&v);
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
            let p = self.propulsion_at(&v, &conditions, t);
            let burning = p.flow_kg_per_second > 0.0;
            // Even a torque-free vessel outside the ceiling can enter air during a trial.
            // Full dynamics therefore couples every leg in an atmospheric world; checking
            // only the accepted boundary load would omit the first entry leg's torque.
            let aerodynamic = self.options.air_dynamics == AirDynamics::ForceAndTorque
                && has_atmosphere(&self.environment);
            let turning = !(burning && guide.is_some())
                && (w != DVec3::ZERO
                    || p.torque != DVec3::ZERO
                    || self.controls[id].turn != DVec3::ZERO
                    || self.sas.contains_key(id));
            let mut leg = if has_atmosphere(&self.environment) {
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
            let ideal_pointing = burning && guide.is_some() && p.force.length() > 0.0;
            let coupled = self.options.air_dynamics == AirDynamics::ForceAndTorque
                && !ideal_pointing
                && (aerodynamic || turning);
            if coupled
                || (burning && turning)
                || (self.options.air_dynamics == AirDynamics::ForceAndTorque
                    && aerodynamic
                    && guide.is_some())
            {
                leg = leg.min(t + self.options.step_seconds);
            }
            if burning {
                leg = leg.min(t + p.seconds_to_flameout);
            }
            // Legs summed from `t` (steps while burning and turning) round differently from the
            // fleet's clock. A leg that would stop within the loop's tolerance of `end` ends on it,
            // so the run keeps the fleet's time and a checkpoint taken now restores.
            if leg + 1e-12 >= end {
                leg = end;
            }
            if coupled {
                self.coupled_orbit_leg(&mut v, &p, leg);
                continue;
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
                    minimum_mass_kg: self.mass(&v.members)
                        - p.groups.iter().map(|g| g.fuel_kg).sum::<f64>(),
                    attitude: g.attitude,
                }))
            } else if burning {
                Some(Control::Force(ForceControl {
                    force: q * p.force,
                    mass_flow_kg_per_second: p.flow_kg_per_second,
                    minimum_mass_kg: self.mass(&v.members)
                        - p.groups.iter().map(|g| g.fuel_kg).sum::<f64>(),
                }))
            } else {
                None
            };
            let guided_air = if self.options.air_dynamics == AirDynamics::ForceAndTorque
                && burning
                && p.force.length() > 0.0
                && air.is_some()
                && let Some(g) = &guide
            {
                let Owner::Orbit { rotation, .. } = &v.owner else {
                    unreachable!()
                };
                let geometry = vessel_air_at(
                    &self.environment,
                    &self.parts,
                    &v.members,
                    self.centre(&v.members),
                    *rotation,
                    t,
                )
                .map(|air| air.with_controls(self.controls[&v.id].turn))
                .unwrap();
                Some(Arc::new(GuidedAirSource {
                    air: Arc::new(geometry),
                    rotation: *rotation,
                    thrust_axis: p.force.normalize(),
                    law: g.attitude,
                    evaluator: std::sync::Mutex::new(VesselPropagator::new(
                        &*self.ephemeris,
                        self.options.tolerances,
                    )),
                }))
            } else {
                None
            };
            let air = guided_air
                .as_ref()
                .map(|a| a.clone() as Arc<dyn AirSource>)
                .or(air);
            let Owner::Orbit { run, .. } = &mut v.owner else {
                unreachable!()
            };
            self.propagate(run, leg, control, air);
            if burning {
                let duration = if leg == t + p.seconds_to_flameout {
                    p.seconds_to_flameout
                } else {
                    leg - t
                };
                burn(&mut self.parts, &p.groups, duration);
                let mass = self.mass(&v.members);
                let c = self.recentre(&v.members);
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
                *rotation = if let Some(source) = &guided_air {
                    source.attitude(
                        &*self.ephemeris,
                        leg,
                        State {
                            position: state.position,
                            velocity: state.velocity,
                        },
                    )
                } else {
                    (DQuat::from_rotation_arc(*rotation * p.force.normalize(), direction)
                        * *rotation)
                        .normalize()
                };
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
                    let inertia = rows(self.inertia_of(&v.members, DVec3::ZERO));
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
        self.put(v);
    }
    fn step_scene(&mut self, scene: u64) {
        let dt = self.options.step_seconds;
        let full_air = self.options.air_dynamics == AirDynamics::ForceAndTorque;
        self.ephemeris.extend_to(self.time + dt);
        if let SceneFrame::Bubble(f) = &mut self.scenes.get_mut(&scene).unwrap().world.frame {
            f.advance_origin(&mut self.ephemeris, self.time + dt);
        }
        let wheel_loads = self.wheel_loads(scene);
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
            let p = self.propulsion_of(&v);
            let snapshot = self.snapshot_of(&v);
            let air_wrench = vessel_air_at(
                &self.environment,
                &self.parts,
                &v.members,
                self.centre(&v.members),
                snapshot.rotation,
                self.time,
            )
            .map(|air| air.with_controls(self.controls[&v.id].turn))
            .map(|source| {
                let local = if full_air {
                    self.scene_centre(&v)
                } else {
                    world.state(&*self.ephemeris, body, push)
                };
                if !full_air {
                    return void_modules::Wrench {
                        force: source.acceleration_in(
                            &self.frames(),
                            self.scenes[&scene].contact,
                            state_of(local),
                            snapshot.mass_kg,
                        ) * snapshot.mass_kg,
                        ..void_modules::Wrench::zero(self.scenes[&scene].contact, local.position)
                    };
                }
                source.wrench_in(
                    &self.frames(),
                    self.scenes[&scene].contact,
                    State {
                        position: local.position,
                        velocity: local.velocity,
                    },
                    q,
                    w,
                )
            });
            let local = self.scene_centre(&v);
            let spin = world.frame.spin();
            let frame_acceleration = world.frame.acceleration(
                &*self.ephemeris,
                self.time,
                local.position,
                local.velocity,
            );
            let air_acceleration = air_wrench.map_or(DVec3::ZERO, |w| w.force / snapshot.mass_kg);
            let air_torque = air_wrench.map_or(DVec3::ZERO, |w| q.conjugate() * w.torque);
            let (force, tau, burned) = step_thrust(&p, dt, c);
            let resting =
                b.is_sleeping() && self.controls[&id].turn == DVec3::ZERO && p.groups.is_empty();
            let active =
                q * force / (self.mass(&v.members) - if full_air { 0.0 } else { burned / 2.0 });
            let now = active + air_acceleration;
            let torque = if resting {
                DVec3::ZERO
            } else {
                let steering = self.steering(&v, q, w, dt);
                if !full_air {
                    tau + steering
                } else {
                    let initial = tau + air_torque + steering;
                    let inertia = rows(self.inertia_of(&v.members, self.centre(&v.members)));
                    let (qm, wm) = rotation_step(q, w, &inertia, initial, spin, dt / 2.0);
                    let acceleration = frame_acceleration + now;
                    let middle = State {
                        position: local.position
                            + local.velocity * (dt / 2.0)
                            + acceleration * (dt * dt / 8.0),
                        velocity: local.velocity + acceleration * (dt / 2.0),
                    };
                    let source = SceneStepSource {
                        fleet: self,
                        scene,
                        start: self.time,
                        end: self.time + dt,
                    };
                    let at = self.frames.tree.at(self.time + dt / 2.0, &source);
                    let mid_torque = vessel_air_at(
                        &self.environment,
                        &self.parts,
                        &v.members,
                        self.centre(&v.members),
                        q,
                        self.time,
                    )
                    .map(|air| air.with_controls(self.controls[&v.id].turn))
                    .map_or(DVec3::ZERO, |air| {
                        qm.conjugate()
                            * air
                                .wrench_in(&at, self.scenes[&scene].contact, middle, qm, wm)
                                .torque
                    });
                    tau + steering + mid_torque
                }
            };
            self.scenes
                .get_mut(&scene)
                .unwrap()
                .world
                .apply_local_torque(body, torque);
            plans.push((id, body, push, now, p, active));
        }
        let world = &mut self.scenes.get_mut(&scene).unwrap().world;
        if let SceneFrame::Bubble(f) = &mut world.frame {
            f.advance_origin(&mut self.ephemeris, self.time + dt);
        }
        // Tires solve a finite dt impulse coupled to rotor inertia. Apply that same impulse to
        // chassis/support, rather than averaging it with the previous atmospheric/thrust load.
        for (body, (force, torque)) in &wheel_loads.bodies {
            world.apply_wrench_impulse(*body, *force * dt, *torque * dt);
        }
        world.step_with_passive(
            &mut self.ephemeris,
            Some(&mut |body, _| {
                let p = plans
                    .iter()
                    .find(|(_, b, _, _, _, _)| *b == body)
                    .expect("scene body has no vessel");
                p.5
            }),
            Some(&mut |body, _| {
                let p = plans
                    .iter()
                    .find(|p| p.1 == body)
                    .expect("scene body has no vessel");
                // Preserve the previous total kick for half-step state reconstruction. Only the
                // current active thrust wakes a body; the remaining trapezoidal kick is passive.
                (p.2 + p.3) / 2.0 - p.5
            }),
        );
        for (pid, mid, state) in wheel_loads.updates {
            self.parts.set_module_state(&pid, &mid, state);
        }
        for (id, body, _, now, p, _) in plans {
            if let Owner::Scene { push, .. } = &mut self.vessels.get_mut(&id).unwrap().owner {
                *push = now;
            }
            if !p.groups.is_empty() {
                burn(&mut self.parts, &p.groups, dt);
            }
            if !p.groups.is_empty() {
                let masses: Vec<_> = self
                    .vessel(&id)
                    .members
                    .iter()
                    .map(|id| self.part_mass(id))
                    .collect();
                self.scenes
                    .get_mut(&scene)
                    .unwrap()
                    .world
                    .set_piece_masses(body, &masses);
            }
            if !full_air {
                continue;
            }
            // The staggered velocity reconstruction needs the load at the new boundary. Keeping
            // the start load here delays every attitude-dependent thrust/drag by half a step.
            let v = self.vessel(&id);
            let local = self.scene_centre(v);
            let b = self.scenes[&scene].world.body(body);
            let q = quat64(*b.rotation());
            let w = vec64(b.angvel());
            let source = SceneStepSource {
                fleet: self,
                scene,
                start: self.time,
                end: self.time + dt,
            };
            let at = self.frames.tree.at(self.time + dt, &source);
            let contact = self.scenes[&scene].contact;
            let conditions = Conditions {
                air: (0..self.environment.bodies().len()).find_map(|body| {
                    self.environment
                        .surroundings(
                            &at,
                            self.environment.frames(),
                            contact,
                            state_of(local),
                            body,
                        )
                        .air
                }),
            };
            let rating = self.propulsion_at(v, &conditions, self.time + dt);
            let next_air = vessel_air_at(
                &self.environment,
                &self.parts,
                &v.members,
                self.centre(&v.members),
                q,
                self.time,
            )
            .map(|air| air.with_controls(self.controls[&v.id].turn))
            .map_or(DVec3::ZERO, |air| {
                air.wrench_in(&at, contact, state_of(local), q, w).force
            });
            let next = (q * rating.force + next_air) / self.mass(&v.members);
            if let Owner::Scene { push, .. } = &mut self.vessels.get_mut(&id).unwrap().owner {
                *push = next;
            }
        }
    }
    fn step_all(&mut self) {
        self.prepare_parachutes();
        let end = self.time + self.options.step_seconds;
        for scene in self.scenes.keys().copied().collect::<Vec<_>>() {
            self.step_scene(scene);
        }
        for id in self.order.clone() {
            if matches!(self.vessel(&id).owner, Owner::Orbit { .. }) {
                self.advance_orbit(&id, end);
            }
        }
        self.commit_parachutes(self.options.step_seconds);
        self.time = end;
        self.commit_thermal(self.options.step_seconds);
        for scene in self.scenes.keys().copied().collect::<Vec<_>>() {
            let s = &self.scenes[&scene];
            let mut mass = 0.0;
            let mut p = DVec3::ZERO;
            for id in &s.members {
                let v = self.vessel(id);
                let Owner::Scene { body, .. } = v.owner else {
                    unreachable!()
                };
                let m = self.mass(&v.members);
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
            if !self.scenes.is_empty()
                || self.active_parachutes()
                || self.thermal_rails_blocker().is_some()
            {
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
                let elapsed = end - self.time;
                for id in self.order.clone() {
                    self.advance_orbit(&id, end);
                }
                self.time = end;
                self.commit_thermal(elapsed);
            }
        }
    }
    pub fn rails_blocker(&self) -> Option<String> {
        if let Some(reason) = self.thermal_rails_blocker() {
            return Some(reason);
        }
        if self
            .guidance
            .values()
            .any(|g| g.status == GuidanceStatus::Armed)
        {
            return Some("scheduled maneuver: use physics time".into());
        }
        for id in &self.order {
            let v = self.vessel(id);
            let active=v.members.iter().any(|pid|self.parts.part(pid).modules.values().any(|m|matches!(m,void_assembly::ModuleState::Parachute{state} if void_modules::parachute::active(*state))));
            if active && has_atmosphere(&self.environment) {
                return Some(format!(
                    "active parachute requires physics in atmospheric world on {id}"
                ));
            }
            let resting = matches!(v.owner, Owner::Scene { scene, body, .. }
                if self.scenes[&scene].ground.is_some() && self.scenes[&scene].world.body(body).is_sleeping());
            if !resting && self.options.air_dynamics == AirDynamics::ForceAndTorque {
                let load = self.aerodynamic_wrench(id);
                if load.force != DVec3::ZERO || load.torque != DVec3::ZERO {
                    return Some(format!("aerodynamic load requires physics on {id}"));
                }
            }
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
            if self.rails_blocker().is_some() {
                done = false;
                break;
            }
            if self.order.iter().any(|id| {
                matches!(self.vessel(id).owner, Owner::Orbit { .. })
                    && self.ground_for(self.vessel(id)).is_some()
            }) {
                done = false;
                break;
            }
            let chunk = if has_atmosphere(&self.environment) {
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
                let air = self.air_source(self.vessel(&id));
                self.propagator.set_air_source(air);
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
            let elapsed = end - self.time;
            self.commit_parachutes(elapsed);
            self.time = end;
            self.commit_thermal(elapsed);
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
