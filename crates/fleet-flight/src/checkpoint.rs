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
    mark: serde_json::Value,
}
impl FlightCheckpoint {
    pub fn capture(sim: &FleetFlight, initial: InitialWorld) -> Self {
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
        let planet = self.initial.planet();
        let home = self.initial.world.body_index(&self.initial.launch_body);
        let mut built = self.initial.world.build();
        built.ephemeris.extend_to(self.ephemeris_end);
        let (ephemeris, environment) = (built.ephemeris, built.environment);
        let fleet = Fleet::from_checkpoint(ephemeris, environment, self.fleet.clone());
        let mut sim = FleetFlight {
            fleet,
            world: self.initial.world.clone(),
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
