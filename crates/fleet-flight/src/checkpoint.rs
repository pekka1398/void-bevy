//! Direct world save. Logical state and native owner caches are restored without any pilot replay.
use crate::{
    FleetFlight,
    session::{InitialWorld, MODEL_VERSION, world_mark},
};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::{BufWriter, Write},
    path::Path,
};
use void_vessels::{Fleet, FleetCheckpoint};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlightCheckpoint {
    version: u32,
    model_version: u32,
    catalog: serde_json::Value,
    pub initial: InitialWorld,
    fleet: FleetCheckpoint,
    selected: String,
    presentation: crate::presentation::Presentation,
    maneuver_warp: crate::warp::ManeuverWarp,
    plans: std::collections::BTreeMap<String, crate::plans::SavedVesselPlan>,
    ephemeris_end: f64,
    coupled_world: Option<void_multiscale::CoupledCheckpoint>,
    mark: serde_json::Value,
}
impl FlightCheckpoint {
    pub fn capture(sim: &FleetFlight, initial: InitialWorld) -> Self {
        assert_eq!(
            sim.fleet.options.air_dynamics, initial.air_dynamics,
            "world checkpoint: air dynamics differs from initial world"
        );
        assert_eq!(
            sim.planet.body_id, initial.launch_body,
            "world checkpoint: launch body changed"
        );
        assert_eq!(
            sim.launch_site, initial.launch_site,
            "world checkpoint: launch site changed"
        );
        assert_eq!(
            serde_json::to_value(&sim.world).unwrap(),
            serde_json::to_value(&initial.world).unwrap(),
            "world checkpoint: terrain changed"
        );
        Self {
            version: 1,
            model_version: MODEL_VERSION,
            catalog: serde_json::to_value(void_assembly::catalog()).unwrap(),
            initial,
            fleet: sim.fleet.checkpoint(),
            selected: sim.selected.clone(),
            presentation: sim.presentation.clone(),
            maneuver_warp: sim.maneuver_warp.clone(),
            plans: sim.plan_checkpoints(),
            ephemeris_end: sim.fleet.ephemeris.end_time(),
            coupled_world: sim.coupled_world.as_ref().map(|w| w.borrow().checkpoint()),
            mark: world_mark(sim),
        }
    }
    fn validate_header(&self) {
        assert_eq!(self.version, 1, "world checkpoint: unsupported version");
        assert_eq!(
            self.model_version, MODEL_VERSION,
            "world checkpoint: incompatible model"
        );
        assert_eq!(
            self.catalog,
            serde_json::to_value(void_assembly::catalog()).unwrap(),
            "world checkpoint: catalog changed"
        );
        assert!(
            self.ephemeris_end.is_finite(),
            "world checkpoint: invalid ephemeris bound"
        );
    }
    pub fn restore(&self) -> FleetFlight {
        self.validate_header();
        assert_eq!(
            self.initial.world.stellar.is_some(),
            self.coupled_world.is_some(),
            "world checkpoint: missing or unexpected coupled state"
        );
        let planet = self.initial.planet();
        let home = self.initial.world.body_index(&self.initial.launch_body);
        let mut built = self
            .initial
            .world
            .build_with_coupled_checkpoint(self.coupled_world.clone());
        if self.coupled_world.is_some() {
            assert_eq!(
                built.ephemeris.end_time(),
                self.ephemeris_end,
                "world checkpoint: coupled ephemeris bound differs"
            );
        } else {
            built.ephemeris.extend_to(self.ephemeris_end);
        }
        let (ephemeris, environment) = (built.ephemeris, built.environment);
        let fleet = Fleet::from_checkpoint(ephemeris, environment, self.fleet.clone());
        assert_eq!(
            fleet.options.air_dynamics, self.initial.air_dynamics,
            "world checkpoint: air dynamics differs from initial world"
        );
        let mut sim = FleetFlight {
            fleet,
            world: self.initial.world.clone(),
            coupled_world: built.coupled_world,
            terrains: built.terrains,
            planet,
            home,
            selected: self.selected.clone(),
            presentation: self.presentation.clone(),
            maneuver_warp: self.maneuver_warp.clone(),
            launch_site: self.initial.launch_site,
            plans: std::collections::BTreeMap::new(),
        };
        sim.restore_plans(self.plans.clone());
        sim.validate_warp();
        assert_eq!(
            world_mark(&sim),
            self.mark,
            "world checkpoint: restored state differs"
        );
        sim
    }
    /// Validate an external candidate without mutating a running session. Existing
    /// invariant validators panic; this boundary converts only candidate restoration
    /// failures into an explicit rejection. Internal simulation panics remain fatal.
    pub fn read_external(path: impl AsRef<Path>) -> Result<(Self, FleetFlight), String> {
        let bytes = fs::read(path).map_err(|e| format!("Read checkpoint: {e}"))?;
        let checkpoint: Self =
            serde_json::from_slice(&bytes).map_err(|e| format!("Checkpoint format: {e}"))?;
        let restored =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| checkpoint.restore()))
                .map_err(|e| {
                    let reason = e
                        .downcast_ref::<String>()
                        .map(String::as_str)
                        .or_else(|| e.downcast_ref::<&str>().copied())
                        .unwrap_or("invalid checkpoint invariant");
                    format!("Checkpoint rejected: {reason}")
                })?;
        Ok((checkpoint, restored))
    }
    /// Atomic checked filesystem write; capture and internal invariants stay strict.
    pub fn write_external(&self, path: impl AsRef<Path>) -> Result<(), String> {
        self.validate_header();
        let path = path.as_ref();
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let name = path
            .file_name()
            .ok_or("Checkpoint path requires a filename")?;
        fs::create_dir_all(parent).map_err(|e| format!("Create save directory: {e}"))?;
        let tmp = parent.join(format!(
            ".{}.{}.ui.tmp",
            name.to_string_lossy(),
            std::process::id()
        ));
        let mut owns_tmp = false;
        let result = (|| -> Result<(), String> {
            let file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&tmp)
                .map_err(|e| format!("Create save: {e}"))?;
            owns_tmp = true;
            let mut file = BufWriter::with_capacity(64 * 1024, file);
            serde_json::to_writer(&mut file, self).map_err(|e| format!("Encode save: {e}"))?;
            file.write_all(b"\n")
                .and_then(|_| file.flush())
                .map_err(|e| format!("Write save: {e}"))?;
            file.get_ref()
                .sync_all()
                .map_err(|e| format!("Sync save: {e}"))?;
            drop(file);
            fs::rename(&tmp, path).map_err(|e| format!("Replace save: {e}"))?;
            fs::File::open(parent)
                .and_then(|f| f.sync_all())
                .map_err(|e| format!("Sync save directory: {e}"))?;
            Ok(())
        })();
        if result.is_err() && owns_tmp {
            let _ = fs::remove_file(tmp);
        }
        result
    }
    pub fn read(path: impl AsRef<Path>) -> Self {
        let saved: Self =
            serde_json::from_slice(&fs::read(path).expect("world checkpoint: read file"))
                .expect("world checkpoint: invalid file");
        saved.validate_header();
        saved
    }
    pub fn write(&self, path: impl AsRef<Path>) {
        self.validate_header();
        let path = path.as_ref();
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        fs::create_dir_all(parent).expect("world checkpoint: create directory");
        let tmp = parent.join(format!(
            ".{}.{}.tmp",
            path.file_name()
                .expect("checkpoint filename")
                .to_string_lossy(),
            std::process::id()
        ));
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
            .expect("world checkpoint: create temp file");
        // JSON's token-sized writes must not become millions of filesystem writes on the UI thread.
        let mut file = BufWriter::with_capacity(64 * 1024, file);
        serde_json::to_writer(&mut file, self).expect("world checkpoint: encode file");
        file.write_all(b"\n")
            .expect("world checkpoint: finish file");
        file.flush().expect("world checkpoint: flush file");
        file.get_ref()
            .sync_all()
            .expect("world checkpoint: sync file");
        drop(file);
        fs::rename(tmp, path).expect("world checkpoint: replace file");
        fs::File::open(parent)
            .expect("world checkpoint: open directory")
            .sync_all()
            .expect("world checkpoint: sync directory");
    }
}
