//! Portable, deterministic Fleet command journals, optionally starting from a direct world checkpoint.
use crate::FleetFlight;
use glam::DVec3;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
};
use void_assembly::{Craft, catalog};
use void_landing::LandingPlanet;
use void_vessels::VesselControl;

pub mod durable;

pub const FORMAT_VERSION: u32 = 1;
/// Changes to simulation rules must bump this, even if the JSON schema remains readable.
pub const MODEL_VERSION: u32 = 25;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InitialWorld {
    pub air_dynamics: void_vessels::AirDynamics,
    pub world: crate::world::WorldDescription,
    pub launch_body: String,
    pub craft: Craft,
    pub launch_site: DVec3,
}
impl InitialWorld {
    pub fn new(planet: &LandingPlanet, craft: &Craft, site: DVec3, air: bool) -> Self {
        Self {
            air_dynamics: void_vessels::AirDynamics::ForceOnly,
            world: crate::world::WorldDescription::single(planet, air),
            launch_body: planet.body_id.clone(),
            craft: craft.clone(),
            launch_site: site,
        }
    }
    pub fn planet(&self) -> LandingPlanet {
        self.world.landing_planet(&self.launch_body)
    }
    pub fn with_air_dynamics(mut self, mode: void_vessels::AirDynamics) -> Self {
        self.air_dynamics = mode;
        self
    }
    pub fn build(&self) -> FleetFlight {
        let mut sim = FleetFlight::from_world(
            self.world.clone(),
            &self.launch_body,
            &self.craft,
            self.launch_site,
        );
        sim.fleet.options.air_dynamics = self.air_dynamics;
        sim
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum Action {
    Vehicle {
        control: void_assembly::VehicleControl,
    },
    RcsNozzle {
        part: String,
        module: String,
        enabled: bool,
    },
    Rcs {
        control: void_vessels::RcsControl,
    },
    Dock {
        part_a: String,
        module_a: String,
        part_b: String,
        module_b: String,
    },
    Undock {
        part: String,
        module: String,
    },
    ArmDock {
        part: String,
        module: String,
        armed: bool,
    },
    SetModuleStage {
        part: String,
        module: String,
        stage: Option<u32>,
    },
    Parachute {
        part: String,
        module: String,
        deploy: bool,
    },
    View {
        command: crate::presentation::ViewCommand,
    },
    EndFrame {
        paused: bool,
        rate: usize,
    },
    ResetWorld {
        initial: Box<InitialWorld>,
    },
    LoadWorld {
        checkpoint: Box<crate::checkpoint::FlightCheckpoint>,
    },
    Select {
        vessel: String,
    },
    Control {
        throttle: f64,
        turn: DVec3,
    },
    Sas {
        enabled: bool,
    },
    AddManeuver {
        spec: void_orbit::ManeuverSpec,
    },
    EditManeuver {
        index: usize,
        spec: void_orbit::ManeuverSpec,
    },
    RemoveManeuver {
        index: usize,
    },
    SelectManeuver {
        index: usize,
    },
    PlaceManeuverAtApsis {
        index: usize,
        apsis: void_orbit::ApsisKind,
    },
    BeginManeuverWarp,
    CancelManeuverWarp,
    ExecuteManeuver,
    AbortManeuver,
    Stage,
    LaunchState {
        craft: Craft,
        position: DVec3,
        velocity: DVec3,
        rotation: glam::DQuat,
        angular_velocity: DVec3,
    },
    /// Explicit split-state fixture/initial-state command, not a propulsion or teleport action.
    LaunchSplitState {
        craft: Craft,
        system: void_frames::SystemId,
        position: void_frames::SplitPosition,
        velocity: DVec3,
        rotation: glam::DQuat,
        angular_velocity: DVec3,
    },
    LaunchFlightAt {
        body: String,
        craft: Craft,
        position: DVec3,
        velocity: DVec3,
    },
    LaunchGroundAt {
        body: String,
        craft: Craft,
        site: DVec3,
    },
    LaunchOrbitAt {
        body: String,
        craft: Craft,
        offset: DVec3,
    },
    LaunchGround {
        craft: Craft,
        site: DVec3,
    },
    LaunchOrbit {
        craft: Craft,
        offset: DVec3,
    },
    Advance {
        seconds: f64,
        rails: bool,
    },
    Join {
        part_a: String,
        node_a: String,
        part_b: String,
        node_b: String,
    },
}
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(tag = "kind", content = "value", deny_unknown_fields)]
pub enum Outcome {
    Applied,
    Spawned(String),
    Staged(Vec<String>),
    Advanced(bool),
    Refused(String),
}
impl Action {
    fn replacement_initial(&self) -> Option<&InitialWorld> {
        match self {
            Self::ResetWorld { initial } => Some(initial),
            Self::LoadWorld { checkpoint } => Some(&checkpoint.initial),
            _ => None,
        }
    }
    fn apply(&self, sim: &mut FleetFlight) -> Outcome {
        let outcome = match self {
            Self::Vehicle { control } => match sim
                .fleet
                .set_vehicle_control(&sim.selected.clone(), *control)
            {
                Ok(()) => Outcome::Applied,
                Err(reason) => Outcome::Refused(reason),
            },
            Self::SetModuleStage {
                part,
                module,
                stage,
            } => {
                sim.fleet.set_module_stage(part, module, *stage);
                Outcome::Applied
            }
            Self::Parachute {
                part,
                module,
                deploy,
            } => {
                sim.fleet.parachute_command(
                    part,
                    module,
                    if *deploy {
                        void_modules::parachute::Command::Deploy
                    } else {
                        void_modules::parachute::Command::Cut
                    },
                );
                Outcome::Applied
            }
            Self::View { command } => {
                sim.view_command(command);
                Outcome::Applied
            }
            Self::EndFrame { paused, rate } => {
                assert!(*rate < 9, "view: invalid frame rate");
                sim.presentation.paused = *paused;
                sim.presentation.rate = *rate;
                Outcome::Applied
            }
            Self::ResetWorld { initial } => {
                let main_camera = sim.presentation.main_camera;
                *sim = initial.build();
                sim.view_command(&crate::presentation::ViewCommand::Configure { main_camera });
                Outcome::Applied
            }
            Self::LoadWorld { checkpoint } => {
                *sim = checkpoint.restore();
                Outcome::Applied
            }
            Self::RcsNozzle {
                part,
                module,
                enabled,
            } => {
                sim.fleet.set_rcs_nozzle_enabled(part, module, *enabled);
                Outcome::Applied
            }
            Self::Rcs { control } => {
                assert!(
                    control.force.is_finite() && control.torque.is_finite(),
                    "invalid RCS command"
                );
                if (control.enabled
                    || control.force != DVec3::ZERO
                    || control.torque != DVec3::ZERO)
                    && !sim.fleet.has_command(&sim.selected)
                {
                    return Outcome::Refused("RCS requires a functioning command part".into());
                }
                sim.fleet.set_rcs_control(&sim.selected, *control);
                Outcome::Applied
            }
            Self::ArmDock {
                part,
                module,
                armed,
            } => {
                sim.fleet.arm_docking_port(part, module, *armed);
                Outcome::Applied
            }
            Self::Dock {
                part_a,
                module_a,
                part_b,
                module_b,
            } => match sim.fleet.dock(part_a, module_a, part_b, module_b) {
                Ok(id) => {
                    sim.select(&id);
                    sim.update_plans();
                    Outcome::Spawned(id)
                }
                Err(e) => Outcome::Refused(e),
            },
            Self::Undock { part, module } => match sim.fleet.undock(part, module) {
                Ok(id) => {
                    sim.update_plans();
                    Outcome::Spawned(id)
                }
                Err(e) => Outcome::Refused(e),
            },
            Self::Select { vessel } => {
                sim.select(vessel);
                sim.view_command(&crate::presentation::ViewCommand::Focus { body: None });
                Outcome::Applied
            }
            Self::Control { throttle, turn } => {
                assert!(
                    throttle.is_finite() && (0.0..=1.0).contains(throttle) && turn.is_finite(),
                    "invalid vessel control"
                );
                if sim.fleet.command_failed(&sim.selected)
                    && (*throttle > 0.0 || *turn != DVec3::ZERO)
                {
                    return Outcome::Refused("command part thermally failed".into());
                }
                sim.control(VesselControl {
                    throttle: *throttle,
                    turn: *turn,
                });
                Outcome::Applied
            }
            Self::Sas { enabled } => {
                if *enabled && !sim.fleet.has_command(&sim.selected) {
                    return Outcome::Refused("SAS requires a functioning command part".into());
                }
                match sim.fleet.request_sas(&sim.selected, *enabled) {
                    Ok(()) => Outcome::Applied,
                    Err(reason) => Outcome::Refused(reason),
                }
            }
            Self::AddManeuver { spec } => match sim.add_maneuver(&sim.selected.clone(), *spec) {
                Ok(()) => Outcome::Applied,
                Err(e) => Outcome::Refused(e),
            },
            Self::EditManeuver { index, spec } => {
                match sim.edit_maneuver(&sim.selected.clone(), *index, *spec) {
                    Ok(()) => Outcome::Applied,
                    Err(e) => Outcome::Refused(e),
                }
            }
            Self::RemoveManeuver { index } => {
                match sim.remove_maneuver(&sim.selected.clone(), *index) {
                    Ok(()) => Outcome::Applied,
                    Err(e) => Outcome::Refused(e),
                }
            }
            Self::SelectManeuver { index } => {
                sim.select_maneuver(&sim.selected.clone(), *index);
                Outcome::Applied
            }
            Self::PlaceManeuverAtApsis { index, apsis } => {
                match sim.place_maneuver_at_apsis(&sim.selected.clone(), *index, *apsis) {
                    Ok(()) => Outcome::Applied,
                    Err(e) => Outcome::Refused(e),
                }
            }
            Self::BeginManeuverWarp => match sim.begin_maneuver_warp(&sim.selected.clone()) {
                Ok(()) => Outcome::Applied,
                Err(e) => Outcome::Refused(e),
            },
            Self::CancelManeuverWarp => {
                sim.cancel_maneuver_warp("pilot cancelled warp");
                Outcome::Applied
            }
            Self::ExecuteManeuver => match sim.execute_maneuver(&sim.selected.clone()) {
                Ok(()) => Outcome::Applied,
                Err(e) => Outcome::Refused(e),
            },
            Self::AbortManeuver => {
                sim.abort_maneuver(&sim.selected.clone());
                Outcome::Applied
            }
            Self::Stage => {
                if sim.fleet.command_failed(&sim.selected) {
                    return Outcome::Refused("command part thermally failed".into());
                }
                Outcome::Staged(sim.stage())
            }
            Self::LaunchState {
                craft,
                position,
                velocity,
                rotation,
                angular_velocity,
            } => Outcome::Spawned(sim.fleet.launch(
                craft,
                void_landing::FrameState {
                    position: *position,
                    velocity: *velocity,
                },
                *rotation,
                *angular_velocity,
            )),
            Self::LaunchSplitState {
                craft,
                system,
                position,
                velocity,
                rotation,
                angular_velocity,
            } => Outcome::Spawned(sim.fleet.launch_at_split(
                craft,
                *system,
                *position,
                *velocity,
                *rotation,
                *angular_velocity,
            )),
            Self::LaunchFlightAt {
                body,
                craft,
                position,
                velocity,
            } => Outcome::Spawned(sim.launch_flight_at(
                body,
                craft,
                void_landing::FrameState {
                    position: *position,
                    velocity: *velocity,
                },
            )),
            Self::LaunchGroundAt { body, craft, site } => {
                Outcome::Spawned(sim.launch_ground_at(body, craft, *site))
            }
            Self::LaunchOrbitAt {
                body,
                craft,
                offset,
            } => {
                assert!(offset.is_finite(), "session: non-finite orbital offset");
                Outcome::Spawned(sim.launch_orbital_at(sim.world.body_index(body), craft, *offset))
            }
            Self::LaunchGround { craft, site } => {
                let id = sim.fleet.launch_landed(craft, sim.home, *site);
                sim.fleet.advance(0.0);
                Outcome::Spawned(id)
            }
            Self::LaunchOrbit { craft, offset } => {
                assert!(offset.is_finite(), "session: non-finite orbital offset");
                Outcome::Spawned(sim.launch_orbital(craft, *offset))
            }
            Self::Advance { seconds, rails } => {
                assert!(
                    seconds.is_finite() && *seconds >= 0.0,
                    "session: invalid duration"
                );
                match sim.advance(*seconds, *rails) {
                    Ok(done) => Outcome::Advanced(done),
                    Err(reason) => Outcome::Refused(reason),
                }
            }
            Self::Join {
                part_a,
                node_a,
                part_b,
                node_b,
            } => {
                let id = sim.fleet.join(part_a, node_a, part_b, node_b);
                sim.select(&id);
                sim.update_plans();
                Outcome::Spawned(id)
            }
        };
        sim.update_presentation();
        outcome
    }
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub action: Action,
    pub outcome: Outcome,
}

/// Ordered full-state observations, not a hash of just the focused vessel. Sorted IDs preserve
/// deterministic output while recording graph edges, fuel, owners, controls and SAS targets.
pub fn world_mark(sim: &FleetFlight) -> serde_json::Value {
    sim.presentation.validate(sim);
    use serde_json::json;
    assert!(
        sim.fleet.time().is_finite() && sim.fleet.pending_seconds().is_finite(),
        "session: non-finite world clock"
    );
    let mut connections = sim.fleet.connection_snapshots();
    connections.sort_by(|a, b| {
        (&a.a, &a.node_a, &a.b, &a.node_b).cmp(&(&b.a, &b.node_a, &b.b, &b.node_b))
    });
    let ships: Vec<_> = sim.fleet.vessel_ids().iter().map(|id| {
        let s = sim.fleet.snapshot(id);
        let precise = sim.fleet.precise_snapshot(id);
        let c = sim.fleet.control(id);
        assert!(s.position.is_finite() && s.velocity.is_finite() && s.rotation.is_finite()
            && s.angular_velocity.is_finite() && s.mass_kg.is_finite() && s.mass_kg > 0.0
            && c.throttle.is_finite() && c.turn.is_finite(),
            "session: non-finite vessel state {id}");
        if let Some(target) = sim.fleet.sas_target(id) {
            assert!(target.is_finite(),"session: non-finite SAS target {id}");
        }
        let parts: Vec<_> = sim.fleet.part_snapshots(id).iter().map(|p| {
            assert!(p.position.is_finite() && p.rotation.is_finite()
                && p.fuel_kg.is_finite() && p.fuel_kg >= 0.0,
                "session: invalid part state {}",p.id);
            json!({
            "id": p.id, "definition": p.definition.id, "position": p.position,
            "rotation": p.rotation, "fuel": p.fuel_kg, "stage": p.stage,
            "staged": p.staged, "lit": p.lit, "firing": p.firing,
            "resources":p.resources,"modules":p.modules,"moduleStages":p.module_stages,
        })}).collect();
        json!({ "id": s.id, "name": s.name, "mode": format!("{:?}",s.mode),
            "system": precise.system, "galaxyPosition": precise.position,
            "systemPosition": precise.local.position, "systemVelocity": precise.local.velocity,
            "anchor": precise.anchor, "residualPosition": precise.residual.position,
            "residualVelocity": precise.residual.velocity,
            "scene": s.scene, "position": s.position, "velocity": s.velocity,
            "rotation": s.rotation, "angularVelocity": s.angular_velocity, "mass": s.mass_kg,
            "parts": parts, "control": { "throttle":c.throttle, "turn":c.turn },
            "rcsControl": sim.fleet.rcs_control(id),
            "guidance": sim.fleet.guidance(id),
            "sasPhase": format!("{:?}",sim.fleet.sas_phase(id)), "sasTarget":sim.fleet.sas_target(id) })
    }).collect();
    let scenes: Vec<_> = sim.fleet.scene_snapshots().iter().map(|s| {
        assert!(s.origin.is_finite() && s.rotation.is_finite(),
            "session: non-finite scene {}",s.id);
        json!({
        "id":s.id, "kind":format!("{:?}",s.kind), "members":s.members,
        "origin":s.origin, "rotation":s.rotation, "asleep":s.asleep, "recenters":s.recenters,
    })}).collect();
    let bodies: Vec<_> = (0..sim.fleet.ephemeris.bodies().len())
        .map(|i| {
            let (position, velocity) = void_frames::BodyStates::body_state(
                &sim.fleet.ephemeris,
                void_frames::BodyId(i),
                sim.fleet.time(),
            );
            assert!(
                position.is_finite() && velocity.is_finite(),
                "session: non-finite ephemeris body {i}"
            );
            json!({"id":sim.fleet.ephemeris.bodies()[i].id,"position":position,"velocity":velocity})
        })
        .collect();
    json!({ "world":sim.world, "airDynamics":sim.fleet.options.air_dynamics, "launchBody":sim.planet.body_id, "time":sim.fleet.time(), "pending":sim.fleet.pending_seconds(),
        "selected":sim.selected, "presentation":sim.presentation, "maneuverWarp":sim.maneuver_warp, "ships":ships, "scenes":scenes,
        "connections":connections, "bodies":bodies, "plans":sim.plan_checkpoints() })
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Mark {
    pub after_actions: usize,
    pub state: serde_json::Value,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Recording {
    pub format_version: u32,
    pub model_version: u32,
    pub catalog: serde_json::Value,
    pub initial: InitialWorld,
    #[serde(default)]
    pub base: Option<crate::checkpoint::FlightCheckpoint>,
    pub entries: Vec<Entry>,
    pub marks: Vec<Mark>,
}
impl Recording {
    pub fn validate(&self) {
        assert_eq!(
            self.format_version, FORMAT_VERSION,
            "session: unsupported format"
        );
        assert_eq!(
            self.model_version, MODEL_VERSION,
            "session: incompatible simulation model"
        );
        assert_eq!(
            self.catalog,
            serde_json::to_value(catalog()).unwrap(),
            "session: catalog changed"
        );
        assert!(!self.marks.is_empty(), "session: missing state marks");
        if let Some(base) = &self.base {
            assert_eq!(
                serde_json::to_value(&base.initial).unwrap(),
                serde_json::to_value(&self.initial).unwrap(),
                "session: checkpoint and recording describe different worlds"
            );
        }
        let mut previous = None;
        for mark in &self.marks {
            assert!(
                mark.after_actions <= self.entries.len(),
                "session: mark beyond recording"
            );
            if let Some(p) = previous {
                assert!(mark.after_actions > p, "session: unordered marks");
            }
            previous = Some(mark.after_actions);
        }
        assert_eq!(
            self.marks.last().unwrap().after_actions,
            self.entries.len(),
            "session: missing final mark"
        );
    }
    pub fn read(path: impl AsRef<Path>) -> Self {
        let bytes = fs::read(path).expect("session: read file");
        if durable::is_stream(&bytes) {
            return durable::read_complete(&bytes);
        }
        let record: Self = serde_json::from_slice(&bytes).expect("session: invalid file");
        record.validate();
        record
    }
    /// Write beside the destination, sync, then atomically replace it. A failed write leaves the
    /// previous save intact. Never treat a partial file as a usable save.
    pub fn write(&self, path: impl AsRef<Path>) {
        self.validate();
        let path = path.as_ref();
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        fs::create_dir_all(parent).expect("session: create directory");
        let name = path
            .file_name()
            .expect("session: destination needs a filename");
        let tmp = parent.join(format!(
            ".{}.{}.tmp",
            name.to_string_lossy(),
            std::process::id()
        ));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
            .expect("session: create temporary file");
        serde_json::to_writer(&mut file, self).expect("session: serialize save");
        file.write_all(b"\n").expect("session: finish save");
        file.sync_all().expect("session: sync save");
        drop(file);
        fs::rename(&tmp, path).expect("session: replace save");
        fs::File::open(parent)
            .expect("session: open save directory")
            .sync_all()
            .expect("session: sync save directory");
    }
}

pub struct FlightSession {
    sim: FleetFlight,
    current_initial: InitialWorld,
    recording: Option<Recording>,
    stream: Option<durable::Writer>,
}
impl FlightSession {
    /// Read-only observation prevents callers from bypassing the command journal.
    pub fn sim(&self) -> &FleetFlight {
        &self.sim
    }
    pub fn predict(&mut self, horizon: f64) -> void_landing::CoastPrediction {
        self.sim.predict(horizon)
    }
    pub fn new(initial: InitialWorld) -> Self {
        let sim = initial.build();
        let current_initial = initial.clone();
        Self {
            sim,
            current_initial,
            recording: None,
            stream: None,
        }
    }
    /// Explicit opt-in for headless regression scripts. Live games use --record/begin_stream.
    pub fn with_recording(mut self) -> Self {
        self.enable_recording();
        self
    }
    fn enable_recording(&mut self) {
        assert!(
            self.recording.is_none(),
            "session: recording already enabled"
        );
        let base =
            crate::checkpoint::FlightCheckpoint::capture(&self.sim, self.current_initial.clone());
        self.recording = Some(Recording {
            format_version: FORMAT_VERSION,
            model_version: MODEL_VERSION,
            catalog: serde_json::to_value(catalog()).unwrap(),
            initial: self.current_initial.clone(),
            base: Some(base),
            entries: vec![],
            marks: vec![Mark {
                after_actions: 0,
                state: world_mark(&self.sim),
            }],
        });
    }
    /// Number of commands and full state marks retained in memory. Disabled means exactly zero.
    pub fn retained_counts(&self) -> (usize, usize) {
        self.recording
            .as_ref()
            .map_or((0, 0), |r| (r.entries.len(), r.marks.len()))
    }
    pub fn execute(&mut self, action: Action) -> Outcome {
        let index = self.retained_counts().0;
        if let Some(stream) = &mut self.stream {
            stream.intent(index, &action);
        }
        let outcome = action.apply(&mut self.sim);
        if let Some(initial) = action.replacement_initial() {
            self.current_initial = initial.clone();
        }
        if let Some(stream) = &mut self.stream {
            stream.commit(index, &outcome);
        }
        if let Some(recording) = &mut self.recording {
            recording.entries.push(Entry {
                action,
                outcome: outcome.clone(),
            });
        }
        outcome
    }
    pub fn recording_initial(&self) -> &InitialWorld {
        &self.current_initial
    }
    pub fn mark(&mut self) {
        let Some(recording) = &mut self.recording else {
            return;
        };
        let n = recording.entries.len();
        let mark = Mark {
            after_actions: n,
            state: world_mark(&self.sim),
        };
        if let Some(stream) = &mut self.stream {
            stream.mark(mark.clone());
        }
        if recording.marks.last().is_some_and(|m| m.after_actions == n) {
            *recording.marks.last_mut().unwrap() = mark;
        } else {
            recording.marks.push(mark);
        }
    }
    pub fn begin_stream(&mut self, path: impl AsRef<Path>) {
        assert!(self.stream.is_none(), "journal: recording already active");
        if self.recording.is_none() {
            self.enable_recording();
        }
        let recording = self.recording();
        self.stream = Some(durable::Writer::create(path.as_ref(), recording));
    }
    pub fn finish_stream(&mut self) {
        assert!(self.stream.is_some(), "journal: recording not active");
        self.mark();
        let count = self.retained_counts().0;
        self.stream.take().unwrap().finish(count);
        self.recording = None;
    }
    pub fn streaming(&self) -> bool {
        self.stream.is_some()
    }
    pub fn save(&mut self, path: impl AsRef<Path>) {
        self.mark();
        self.recording
            .as_ref()
            .expect("session: recording is disabled")
            .write(path);
    }
    pub fn recording(&mut self) -> Recording {
        self.mark();
        self.recording
            .as_ref()
            .expect("session: recording is disabled")
            .clone()
    }
    pub fn from_recording(recording: Recording) -> Self {
        recording.validate();
        let mut sim = recording
            .base
            .as_ref()
            .map_or_else(|| recording.initial.build(), |base| base.restore());
        let mut current_initial = recording.initial.clone();
        let mut marks = recording.marks.iter().peekable();
        for index in 0..=recording.entries.len() {
            if index > 0 {
                let entry = &recording.entries[index - 1];
                assert_eq!(
                    entry.action.apply(&mut sim),
                    entry.outcome,
                    "session: action {index} changed its outcome"
                );
                if let Some(initial) = entry.action.replacement_initial() {
                    current_initial = initial.clone();
                }
            }
            if marks.peek().is_some_and(|m| m.after_actions == index) {
                assert_eq!(
                    world_mark(&sim),
                    marks.next().unwrap().state,
                    "session: world diverged after action {index}"
                );
            }
        }
        assert!(marks.next().is_none(), "session: unreached mark");
        Self {
            sim,
            current_initial,
            recording: None,
            stream: None,
        }
    }
    pub fn load(path: impl AsRef<Path>) -> Self {
        Self::from_recording(Recording::read(path))
    }
    pub fn save_checkpoint(&self, path: impl AsRef<Path>) {
        crate::checkpoint::FlightCheckpoint::capture(&self.sim, self.current_initial.clone())
            .write(path);
    }
    pub fn load_checkpoint(path: impl AsRef<Path>) -> Self {
        Self::from_checkpoint(crate::checkpoint::FlightCheckpoint::read(path))
    }
    pub fn from_checkpoint(base: crate::checkpoint::FlightCheckpoint) -> Self {
        let sim = base.restore();
        let current_initial = base.initial.clone();
        Self {
            sim,
            current_initial,
            recording: None,
            stream: None,
        }
    }
}

/// Incremental playback for a window. UI recordings use explicit EndFrame commands, including
/// paused frames with no physics. Core-only journals without frame markers step at Advance.
/// Neither path depends on the playback machine's wall-clock delta.
pub struct Playback {
    recording: Recording,
    cursor: usize,
    mark_cursor: usize,
    explicit_frames: bool,
}
impl Playback {
    pub fn new(recording: Recording) -> (Self, FlightSession) {
        recording.validate();
        let session = if let Some(base) = &recording.base {
            FlightSession::from_checkpoint(base.clone())
        } else {
            FlightSession::new(recording.initial.clone())
        };
        let explicit_frames = recording
            .entries
            .iter()
            .any(|e| matches!(e.action, Action::EndFrame { .. }));
        let mut playback = Self {
            explicit_frames,
            recording,
            cursor: 0,
            mark_cursor: 0,
        };
        playback.check_marks(&session);
        (playback, session)
    }
    fn check_marks(&mut self, session: &FlightSession) {
        while self
            .recording
            .marks
            .get(self.mark_cursor)
            .is_some_and(|mark| mark.after_actions == self.cursor)
        {
            assert_eq!(
                world_mark(session.sim()),
                self.recording.marks[self.mark_cursor].state,
                "session: world diverged after action {}",
                self.cursor
            );
            self.mark_cursor += 1;
        }
    }
    /// Returns false after the final frame. The restored session can then continue normally.
    pub fn next_frame(&mut self, session: &mut FlightSession) -> bool {
        while let Some(entry) = self.recording.entries.get(self.cursor) {
            let frame_end = if self.explicit_frames {
                matches!(entry.action, Action::EndFrame { .. })
            } else {
                matches!(entry.action, Action::Advance { .. })
            };
            assert_eq!(
                session.execute(entry.action.clone()),
                entry.outcome,
                "session: action {} changed its outcome",
                self.cursor + 1
            );
            self.cursor += 1;
            self.check_marks(session);
            if frame_end {
                return true;
            }
        }
        assert_eq!(
            self.mark_cursor,
            self.recording.marks.len(),
            "session: unreached mark"
        );
        false
    }
}
