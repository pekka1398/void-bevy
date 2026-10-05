//! Direct Fleet checkpoints: logical IDs/graph plus versioned owner caches. No input replay.
use super::*;
use crate::free_fall::FreeFallCheckpoint;
use void_assembly::Part;
use void_landing::ContactWorldCheckpoint;
use void_terrain::TerrainConfig;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedGround {
    body_index: usize,
    terrain: TerrainConfig,
    tiles: ContactWorldOptions,
    band_enter_meters: f64,
    band_exit_meters: f64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
enum SavedFrame {
    Ground { index: usize },
    Bubble { origin: Box<FreeFallCheckpoint> },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedScene {
    id: u64,
    frame: SavedFrame,
    world: ContactWorldCheckpoint,
    members: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedPart {
    id: String,
    definition_id: String,
    #[serde(deserialize_with = "void_assembly::unique_map")]
    resources: void_assembly::Resources,
    #[serde(deserialize_with = "void_assembly::unique_map")]
    modules: BTreeMap<String, void_assembly::ModuleState>,
    stage: Option<u32>,
    #[serde(deserialize_with = "void_assembly::unique_map")]
    module_stages: BTreeMap<String, Option<u32>>,
    pose: PartPose,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FleetCheckpoint {
    version: u32,
    options: FleetOptions,
    time: f64,
    pending: f64,
    next_vessel: u64,
    next_scene: u64,
    grounds: Vec<SavedGround>,
    parts: Vec<SavedPart>,
    connections: Vec<Connection>,
    vessels: Vec<Vessel>,
    order: Vec<String>,
    controls: BTreeMap<String, VesselControl>,
    #[serde(deserialize_with = "void_assembly::unique_map")]
    rcs_controls: BTreeMap<String, RcsControl>,
    sas: BTreeMap<String, Sas>,
    guidance: BTreeMap<String, GuidedBurn>,
    scenes: Vec<SavedScene>,
    active_pairs: Vec<(String, String)>,
}
impl Fleet {
    pub fn checkpoint(&self) -> FleetCheckpoint {
        // By ID.
        let parts = self
            .parts
            .parts()
            .map(|part| SavedPart {
                id: part.id.clone(),
                definition_id: part.definition.id.clone(),
                resources: part.resources.clone(),
                modules: part.modules.clone(),
                stage: part.stage,
                module_stages: part.module_stages.clone(),
                pose: part.pose,
            })
            .collect();
        FleetCheckpoint {
            version: 5,
            options: self.options,
            time: self.time,
            pending: self.pending,
            next_vessel: self.next_vessel,
            next_scene: self.next_scene,
            grounds: self
                .grounds
                .iter()
                .map(|g| SavedGround {
                    body_index: g.spec.body_index,
                    terrain: self.terrain(g.spec.body_index).config().clone(),
                    tiles: g.spec.tiles,
                    band_enter_meters: g.spec.band_enter_meters,
                    band_exit_meters: g.spec.band_exit_meters,
                })
                .collect(),
            parts,
            connections: self.parts.connections().to_vec(),
            vessels: self.vessels.values().cloned().collect(),
            order: self.order.clone(),
            controls: self
                .controls
                .iter()
                .map(|(id, c)| (id.clone(), *c))
                .collect(),
            rcs_controls: self.rcs_controls.clone(),
            sas: self
                .sas
                .iter()
                .map(|(id, sas)| (id.clone(), sas.clone()))
                .collect(),
            scenes: self
                .scenes
                .iter()
                .map(|(id, scene)| SavedScene {
                    id: *id,
                    frame: match &scene.world.frame {
                        SceneFrame::Ground(_) => SavedFrame::Ground {
                            index: scene.ground.expect("ground scene index"),
                        },
                        SceneFrame::Bubble(origin) => SavedFrame::Bubble {
                            origin: Box::new(origin.checkpoint()),
                        },
                    },
                    world: scene.world.checkpoint(),
                    members: scene.members.clone(),
                })
                .collect(),
            guidance: self.guidance.clone(),
            active_pairs: self.gate.active_pairs(),
        }
    }
    /// `environment` is the world's, as the caller built it; the saved terrain
    /// must be the environment's.
    pub fn from_checkpoint(
        ephemeris: impl EphemerisSource + 'static,
        environment: Arc<Environment>,
        saved: FleetCheckpoint,
    ) -> Self {
        assert_eq!(saved.version, 5, "fleet checkpoint: unsupported version");
        assert!(
            saved.time.is_finite()
                && saved.pending.is_finite()
                && saved.pending >= 0.0
                && saved.pending < saved.options.step_seconds + 1e-9,
            "fleet checkpoint: invalid pending clock"
        );
        let grounds = saved
            .grounds
            .into_iter()
            .map(|g| {
                assert!(
                    environment
                        .body(g.body_index)
                        .and_then(|b| b.terrain.as_ref())
                        .is_some_and(|t| *t.config() == g.terrain),
                    "fleet checkpoint: terrain differs from the environment's"
                );
                GroundSpec {
                    body_index: g.body_index,
                    tiles: g.tiles,
                    band_enter_meters: g.band_enter_meters,
                    band_exit_meters: g.band_exit_meters,
                }
            })
            .collect();
        let mut fleet = Fleet::new(ephemeris, environment, saved.time, grounds, saved.options);
        for part in saved.parts {
            let definition = void_assembly::definition(&part.definition_id)
                .expect("fleet checkpoint: unknown part");
            fleet.parts.insert(Part {
                id: part.id,
                definition,
                resources: part.resources.clone(),
                modules: part.modules.clone(),
                stage: part.stage,
                module_stages: part.module_stages.clone(),
                pose: part.pose,
            });
        }
        fleet.parts.restore_connections(saved.connections);
        fleet.order = saved.order;
        fleet.controls = saved.controls.into_iter().collect();
        fleet.rcs_controls = saved.rcs_controls;
        fleet.sas = saved.sas.into_iter().collect();
        fleet.guidance = saved.guidance;
        fleet.pending = saved.pending;
        fleet.next_vessel = saved.next_vessel;
        fleet.next_scene = saved.next_scene;
        for scene in saved.scenes {
            let (frame, ground, terrain) = match scene.frame {
                SavedFrame::Ground { index } => {
                    let g = fleet
                        .grounds
                        .get(index)
                        .expect("fleet checkpoint: unknown ground");
                    (
                        SceneFrame::Ground(Box::new(g.frame.clone())),
                        Some(index),
                        Some(fleet.terrain(g.spec.body_index).clone()),
                    )
                }
                SavedFrame::Bubble { origin } => (
                    SceneFrame::Bubble(Box::new(FreeFallFrame::from_checkpoint(
                        &fleet.ephemeris,
                        fleet.options.tolerances,
                        *origin,
                    ))),
                    None,
                    None,
                ),
            };
            let world = ContactWorld::from_checkpoint(frame, terrain, scene.world);
            assert_eq!(
                world.time, fleet.time,
                "fleet checkpoint: scene clock differs"
            );
            fleet.insert_scene(scene.id, world, ground, scene.members);
        }
        // After the scenes, so each vessel's frame can hang under its scene.
        for vessel in saved.vessels {
            fleet.put(vessel);
        }
        fleet.gate.restore_pairs(saved.active_pairs);
        fleet.validate_checkpoint();
        fleet
    }
    fn validate_checkpoint(&self) {
        let mut order = HashSet::new();
        assert_eq!(
            self.order.len(),
            self.vessels.len(),
            "fleet checkpoint: vessel order differs"
        );
        for id in &self.order {
            assert!(
                order.insert(id) && self.vessels.contains_key(id),
                "fleet checkpoint: invalid vessel order"
            );
        }
        let mut used = HashSet::new();
        for (id, vessel) in &self.vessels {
            assert!(
                vessel.members.contains(&vessel.root),
                "fleet checkpoint: missing vessel root"
            );
            for part in &vessel.members {
                assert!(
                    self.parts.contains(part) && used.insert(part),
                    "fleet checkpoint: invalid part ownership"
                );
            }
            assert_eq!(
                self.parts.components(&vessel.members).len(),
                1,
                "fleet checkpoint: vessel {id} is not one connected component"
            );
            let rcs = self
                .rcs_controls
                .get(id)
                .expect("fleet checkpoint: missing RCS control");
            assert!(
                rcs.force.is_finite() && rcs.torque.is_finite(),
                "fleet checkpoint: invalid RCS control"
            );
            let control = self
                .controls
                .get(id)
                .expect("fleet checkpoint: missing control");
            assert!(
                (0.0..=1.0).contains(&control.throttle)
                    && control
                        .turn
                        .to_array()
                        .iter()
                        .all(|v| (-1.0..=1.0).contains(v)),
                "fleet checkpoint: invalid control"
            );
            match &vessel.owner {
                Owner::Orbit {
                    run,
                    rotation,
                    angular_velocity,
                } => assert!(
                    run.time == self.time
                        && run.y.iter().chain(&run.dy).all(|v| v.is_finite())
                        && run.y[6] > 0.0
                        && run.step_hint.is_finite()
                        && run.step_hint > 0.0
                        && rotation.is_finite()
                        && angular_velocity.is_finite(),
                    "fleet checkpoint: invalid orbital owner"
                ),
                Owner::Scene { scene, body, push } => {
                    let s = self
                        .scenes
                        .get(scene)
                        .expect("fleet checkpoint: unknown scene");
                    assert!(
                        s.members.iter().filter(|m| *m == id).count() == 1
                            && s.world.world.bodies.get(*body).is_some()
                            && push.is_finite(),
                        "fleet checkpoint: invalid contact owner"
                    );
                }
            }
        }
        assert_eq!(
            used.len(),
            self.parts.parts().count(),
            "fleet checkpoint: orphan parts"
        );
        assert_eq!(
            self.rcs_controls.len(),
            self.vessels.len(),
            "orphan RCS controls"
        );
        assert_eq!(
            self.controls.len(),
            self.vessels.len(),
            "fleet checkpoint: orphan controls"
        );
        for (id, g) in &self.guidance {
            let vessel = self.vessel(id);
            assert!(
                g.start_time.is_finite() && g.end_time.is_finite() && g.end_time > g.start_time,
                "fleet checkpoint: invalid guidance times"
            );
            assert!(
                g.force.is_finite() && g.force.length() > 0.0 && g.flow.is_finite() && g.flow > 0.0,
                "fleet checkpoint: invalid guidance engine"
            );
            Control::Thrust(void_orbit::ThrustControl {
                thrust_newtons: g.force.length(),
                exhaust_velocity: g.force.length() / g.flow,
                minimum_mass_kg: 1.0,
                attitude: g.attitude,
            })
            .assert_valid(self.ephemeris.bodies().len());
            if g.status == GuidanceStatus::Armed {
                assert!(
                    matches!(vessel.owner, Owner::Orbit { .. }) && self.time < g.end_time,
                    "fleet checkpoint: guidance on a nonorbital or completed vessel"
                );
            }
        }
        for id in self.sas.keys() {
            assert!(
                self.vessels.contains_key(id),
                "fleet checkpoint: orphan SAS"
            );
        }
        // Restoring the connections checked their parts and nodes.
        for connection in self.parts.connections() {
            assert!(
                self.vessels.values().any(|v| {
                    v.members.contains(&connection.a) && v.members.contains(&connection.b)
                }),
                "fleet checkpoint: connection crosses owners"
            );
        }
        for (a, b) in self.gate.active_pairs() {
            assert!(
                self.vessels.contains_key(&a) && self.vessels.contains_key(&b),
                "fleet checkpoint: encounter pair has unknown vessel"
            );
        }
        for (id, scene) in &self.scenes {
            assert!(!scene.members.is_empty(), "fleet checkpoint: empty scene");
            for member in &scene.members {
                assert!(
                    matches!(self.vessels.get(member).map(|v|&v.owner),Some(Owner::Scene {scene,..}) if scene==id),
                    "fleet checkpoint: scene has foreign member"
                );
            }
            let handles: HashSet<_> = scene
                .members
                .iter()
                .map(|member| {
                    let Owner::Scene { body, .. } = self.vessels[member].owner else {
                        unreachable!()
                    };
                    body
                })
                .collect();
            assert_eq!(
                handles.len(),
                scene.members.len(),
                "fleet checkpoint: shared rigid-body owner"
            );
            assert_eq!(
                handles,
                scene.world.body_handles().collect(),
                "fleet checkpoint: unowned contact body"
            );
        }
    }
}
