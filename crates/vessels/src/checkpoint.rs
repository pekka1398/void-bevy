//! Direct Fleet checkpoints: logical IDs/graph plus versioned owner caches. No input replay.
use super::*;
use crate::free_fall::FreeFallCheckpoint;
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
    fuel_kg: f64,
    stage: Option<u32>,
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
    lit: Vec<String>,
    staged: Vec<String>,
    sas: BTreeMap<String, Sas>,
    scenes: Vec<SavedScene>,
    active_pairs: Vec<(String, String)>,
}
impl Fleet {
    pub fn checkpoint(&self) -> FleetCheckpoint {
        let mut parts: Vec<_> = self
            .parts
            .values()
            .map(|part| SavedPart {
                id: part.id.clone(),
                definition_id: part.definition.id.clone(),
                fuel_kg: part.fuel_kg,
                stage: part.stage,
            })
            .collect();
        parts.sort_by(|a, b| a.id.cmp(&b.id));
        let mut lit: Vec<_> = self.lit.iter().cloned().collect();
        lit.sort();
        let mut staged: Vec<_> = self.staged.iter().cloned().collect();
        staged.sort();
        FleetCheckpoint {
            version: 1,
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
                    terrain: g.spec.terrain.config().clone(),
                    tiles: g.spec.tiles,
                    band_enter_meters: g.spec.band_enter_meters,
                    band_exit_meters: g.spec.band_exit_meters,
                })
                .collect(),
            parts,
            connections: self.connections.clone(),
            vessels: self.vessels.values().cloned().collect(),
            order: self.order.clone(),
            controls: self
                .controls
                .iter()
                .map(|(id, c)| (id.clone(), *c))
                .collect(),
            lit,
            staged,
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
            active_pairs: self.gate.active_pairs(),
        }
    }
    pub fn from_checkpoint(
        ephemeris: impl EphemerisSource + 'static,
        saved: FleetCheckpoint,
        environment: Option<Arc<dyn FleetEnvironment>>,
    ) -> Self {
        assert_eq!(saved.version, 1, "fleet checkpoint: unsupported version");
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
            .map(|g| GroundSpec {
                body_index: g.body_index,
                terrain: Arc::new(Terrain::from_config(&g.terrain)),
                tiles: g.tiles,
                band_enter_meters: g.band_enter_meters,
                band_exit_meters: g.band_exit_meters,
            })
            .collect();
        let mut fleet = Fleet::new(ephemeris, saved.time, grounds, saved.options);
        // Install environment before restoring integrator caches; set_environment intentionally
        // invalidates the caches of existing vessels when a live environment changes.
        fleet.environment = environment;
        for part in saved.parts {
            let definition = void_assembly::definition(&part.definition_id)
                .expect("fleet checkpoint: unknown part");
            assert!(
                part.fuel_kg.is_finite()
                    && part.fuel_kg >= 0.0
                    && part.fuel_kg <= void_assembly::tank_capacity(definition),
                "fleet checkpoint: invalid fuel"
            );
            let id = part.id.clone();
            assert!(
                fleet
                    .parts
                    .insert(
                        id,
                        PropulsionPart {
                            id: part.id,
                            definition,
                            fuel_kg: part.fuel_kg,
                            stage: part.stage
                        }
                    )
                    .is_none(),
                "fleet checkpoint: duplicate part"
            );
        }
        for vessel in saved.vessels {
            let id = vessel.id.clone();
            assert!(
                fleet.vessels.insert(id, vessel).is_none(),
                "fleet checkpoint: duplicate vessel"
            );
        }
        fleet.connections = saved.connections;
        fleet.order = saved.order;
        fleet.controls = saved.controls.into_iter().collect();
        fleet.lit = saved.lit.into_iter().collect();
        fleet.staged = saved.staged.into_iter().collect();
        fleet.sas = saved.sas.into_iter().collect();
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
                        Some(g.spec.terrain.clone()),
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
            assert!(
                fleet
                    .scenes
                    .insert(
                        scene.id,
                        Scene {
                            world,
                            ground,
                            members: scene.members
                        }
                    )
                    .is_none(),
                "fleet checkpoint: duplicate scene"
            );
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
                !vessel.poses.is_empty() && vessel.poses.iter().any(|(id, _)| id == &vessel.root),
                "fleet checkpoint: missing vessel root"
            );
            for (part, pose) in &vessel.poses {
                assert!(
                    self.parts.contains_key(part)
                        && used.insert(part)
                        && pose.position.is_finite()
                        && pose.rotation.is_finite()
                        && (pose.rotation.length() - 1.0).abs() < 1e-6,
                    "fleet checkpoint: invalid part ownership/pose"
                );
            }
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
            self.parts.len(),
            "fleet checkpoint: orphan parts"
        );
        assert_eq!(
            self.controls.len(),
            self.vessels.len(),
            "fleet checkpoint: orphan controls"
        );
        for part in self.lit.iter().chain(&self.staged) {
            assert!(
                self.parts.contains_key(part),
                "fleet checkpoint: unknown staged/lit part"
            );
        }
        for id in self.sas.keys() {
            assert!(
                self.vessels.contains_key(id),
                "fleet checkpoint: orphan SAS"
            );
        }
        for connection in &self.connections {
            let a = self
                .parts
                .get(&connection.a)
                .expect("fleet checkpoint: missing connection part");
            let b = self
                .parts
                .get(&connection.b)
                .expect("fleet checkpoint: missing connection part");
            node(a.definition, &connection.node_a)
                .expect("fleet checkpoint: missing connection node");
            node(b.definition, &connection.node_b)
                .expect("fleet checkpoint: missing connection node");
            assert!(
                self.vessels.values().any(|v| {
                    v.poses.iter().any(|(id, _)| id == &connection.a)
                        && v.poses.iter().any(|(id, _)| id == &connection.b)
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
