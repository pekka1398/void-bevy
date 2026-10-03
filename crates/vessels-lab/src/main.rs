//! Independent multi-vessel acceptance lab; the Fleet is the only simulation owner.
use bevy::asset::RenderAssetUsages;
use bevy::camera::Hdr;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;
use glam::DVec3;
use std::collections::{HashMap, HashSet};
use void_assembly_lab::parts::RenderAssets;
use void_frames::{BodyId, BodyStates};
use void_landing::FrameState;
use void_orbit::{AdvanceOutcome, VesselPropagator};
use void_vessels::*;
struct Lab {
    scene: LabScene,
    selected: String,
    paused: bool,
    warp: usize,
    all: bool,
    wire: bool,
    origins: bool,
    yaw: f32,
    pitch: f32,
    distance: f32,
    dirty: bool,
    notice: String,
    reference_error: f64,
    queue: Vec<Action>,
    drag: f32,
    /// Vessels spawned so far; each one goes further out, as the TS page does.
    spawned: u32,
    visual_parts: HashMap<String, Vec<Entity>>,
    terrain: HashMap<(u64, String), (Entity, Handle<Mesh>)>,
}
#[derive(Clone, Copy)]
enum Action {
    Scenario(Scenario),
    Reset,
    Pause,
    Step(f64),
    Next,
    Stage,
    Separate,
    Sas,
    Spawn,
    Join,
    Fit,
    All,
    Warp,
    Wire,
    Origins,
}
impl Lab {
    fn new(scenario: Scenario) -> Self {
        Self {
            scene: create_lab_scene(scenario),
            selected: "v1".into(),
            paused: true,
            warp: 0,
            all: false,
            wire: false,
            origins: true,
            yaw: 0.7,
            pitch: 0.3,
            distance: 35.0,
            dirty: true,
            notice: String::new(),
            reference_error: 0.0,
            queue: vec![],
            drag: 0.0,
            spawned: 0,
            visual_parts: HashMap::new(),
            terrain: HashMap::new(),
        }
    }
    fn reset(&mut self, scenario: Scenario) {
        let parts = std::mem::take(&mut self.visual_parts);
        let terrain = std::mem::take(&mut self.terrain);
        *self = Self::new(scenario);
        self.visual_parts = parts;
        self.terrain = terrain;
    }
    fn references(&mut self) {
        let time = self.scene.fleet.time();
        let mut prop = VesselPropagator::new(
            &self.scene.fleet.ephemeris,
            self.scene.fleet.options.tolerances,
        );
        for (id, run) in &mut self.scene.references {
            let result = prop.advance(
                &mut self.scene.fleet.ephemeris,
                run,
                time,
                100_000,
                None,
                None,
            );
            assert_eq!(result, AdvanceOutcome::Reached);
            self.reference_error = self
                .reference_error
                .max((self.scene.fleet.snapshot(id).position - run.state().position).length());
        }
    }
    fn advance(&mut self, seconds: f64) {
        if self.warp < 3 {
            self.scene.fleet.advance(seconds);
        } else if let Some(reason) = self.scene.fleet.rails_blocker() {
            self.warp = 0;
            self.notice = reason;
        } else if !self.scene.fleet.advance_on_rails(seconds) {
            self.warp = 0;
            self.notice = "Encounter / ground band: returned to physics time".into();
            self.scene.fleet.advance(0.0);
        }
        self.references();
    }
    /// After a structural change, settle ownership at once, as the TS page does.
    fn structural(&mut self) {
        self.scene.fleet.advance(0.0);
        self.scene.references.clear();
        self.dirty = true;
    }
    fn fit(&mut self) {
        let fleet = &self.scene.fleet;
        self.distance = if self.all {
            fleet
                .vessel_ids()
                .iter()
                .map(|id| fleet.relative(id, &self.selected).position.length() as f32)
                .fold(20.0, f32::max)
                * 2.5
        } else {
            35.0
        };
    }
    fn select(&mut self, id: String) {
        let mut previous = self.scene.fleet.control(&self.selected);
        previous.turn = DVec3::ZERO;
        self.scene.fleet.set_control(&self.selected, previous);
        let mut next = self.scene.fleet.control(&id);
        next.turn = DVec3::ZERO;
        self.scene.fleet.set_control(&id, next);
        self.selected = id;
        self.fit();
    }
    fn act(&mut self, a: Action) {
        match a {
            Action::Scenario(s) => self.reset(s),
            Action::Reset => self.reset(self.scene.scenario),
            Action::Pause => self.paused = !self.paused,
            Action::Step(dt) => {
                self.scene.fleet.advance(dt);
                self.references();
            }
            Action::Next => {
                let ids = self.scene.fleet.vessel_ids();
                let i = ids.iter().position(|id| id == &self.selected).unwrap();
                self.select(ids[(i + 1) % ids.len()].clone());
            }
            Action::Stage => {
                self.scene.fleet.stage(&self.selected);
                self.structural();
            }
            Action::Separate => {
                // A decoupler whose own node is still connected.
                let f = &self.scene.fleet;
                let free = f.free_nodes(&self.selected);
                let part = f
                    .part_snapshots(&self.selected)
                    .into_iter()
                    .find(|p| {
                        p.definition.modules.iter().any(|m| {
                            matches!(m, void_assembly::Module::Decoupler { node_id, .. }
                                if !free.iter().any(|n| n.part == p.id && &n.node == node_id))
                        })
                    })
                    .map(|p| p.id);
                if let Some(p) = part {
                    self.scene.fleet.decouple(&p);
                    self.structural();
                } else {
                    self.notice = "No connected decoupler on selected vessel".into();
                }
            }
            Action::Sas => {
                let on = self.scene.fleet.sas_phase(&self.selected) == SasPhase::Off;
                if self
                    .scene
                    .fleet
                    .part_snapshots(&self.selected)
                    .iter()
                    .any(|p| {
                        p.definition
                            .modules
                            .iter()
                            .any(|m| matches!(m, void_assembly::Module::Command))
                    })
                {
                    self.scene.fleet.set_sas(&self.selected, on);
                } else {
                    self.notice = "Debris has no command part".into();
                }
            }
            Action::Spawn => {
                let s = self.scene.fleet.snapshot(&self.selected);
                if s.mode == VesselMode::Ground {
                    let local = self
                        .scene
                        .fleet
                        .body_fixed_state(&self.selected, self.scene.body_index);
                    self.spawned += 1;
                    let site = nearby_site(
                        local.position.normalize(),
                        15.0 * self.spawned as f64,
                        self.scene.planet.terrain.radius_meters,
                    );
                    self.scene.fleet.launch_landed(
                        &pod_tank("Spawned vessel"),
                        self.scene.body_index,
                        site,
                    );
                } else {
                    self.spawned += 1;
                    self.scene.fleet.launch(
                        &pod_tank("Spawned vessel"),
                        FrameState {
                            position: s.position
                                + s.rotation * DVec3::X * (25.0 * self.spawned as f64),
                            velocity: s.velocity,
                        },
                        s.rotation,
                        s.angular_velocity,
                    );
                }
                self.scene.fleet.advance(0.0);
                self.dirty = true;
            }
            Action::Join => {
                let f = &self.scene.fleet;
                let sa = f.snapshot(&self.selected);
                let a = f.free_nodes(&self.selected);
                let mut candidate = None;
                let mut gap = 0.25;
                for id in f.vessel_ids() {
                    if id == self.selected
                        || sa.scene.is_none()
                        || f.snapshot(&id).scene != sa.scene
                    {
                        continue;
                    }
                    for na in &a {
                        for nb in f.free_nodes(&id) {
                            if na.size != nb.size {
                                continue;
                            }
                            let d = (f.node_frame(&na.part, &na.node).0
                                - f.node_frame(&nb.part, &nb.node).0)
                                .length();
                            if d <= gap {
                                gap = d;
                                candidate = Some((na.clone(), nb));
                            }
                        }
                    }
                }
                if let Some((a, b)) = candidate {
                    self.scene.fleet.join(&a.part, &a.node, &b.part, &b.node);
                    self.structural();
                } else {
                    self.notice =
                        "Join requires free equal-size nodes within 0.25 m in one scene".into();
                }
            }
            Action::Fit => self.fit(),
            Action::All => {
                self.all = !self.all;
                self.fit();
            }
            Action::Warp => self.warp = (self.warp + 1) % WARPS.len(),
            Action::Wire => self.wire = !self.wire,
            Action::Origins => self.origins = !self.origins,
        }
    }
}
const WARPS: [f64; 6] = [1.0, 2.0, 4.0, 20.0, 100.0, 1000.0];
#[derive(Component)]
struct Hud;
#[derive(Component)]
struct FleetCamera;
#[derive(Component)]
struct VisualPart {
    id: String,
    local: Transform,
    flame: bool,
}
#[derive(Component)]
struct ButtonAction(Action);
#[derive(Component)]
struct TileVisual;
#[derive(Component)]
struct PlanetVisual;
fn main() {
    let scenario = std::env::args().nth(1).map_or(Scenario::Launch, |s| {
        let n = s.parse::<usize>().expect("scenario number 1..6");
        assert!((1..=6).contains(&n));
        Scenario::ALL[n - 1]
    });
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "VOID | vessels lab".into(),
                resolution: (1440, 900).into(),
                ..default()
            }),
            ..default()
        }))
        .insert_non_send(Lab::new(scenario))
        .insert_resource(ClearColor(Color::srgb_u8(7, 18, 27)))
        .insert_resource(GlobalAmbientLight {
            brightness: 300.0,
            ..default()
        })
        .add_systems(Startup, setup)
        .add_systems(Update, (buttons, controls, simulate, visuals).chain())
        .run();
}
fn button(parent: &mut ChildSpawnerCommands, text: &str, a: Action) {
    parent
        .spawn((
            Button,
            ButtonAction(a),
            Node {
                padding: px(7).all(),
                margin: px(2).all(),
                ..default()
            },
            BackgroundColor(Color::srgb_u8(28, 48, 62)),
        ))
        .with_child((
            Text::new(text),
            TextFont {
                font_size: FontSize::Px(14.0),
                ..default()
            },
        ));
}
fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(RenderAssets::new(&mut meshes, &mut materials));
    commands.spawn((
        Camera3d::default(),
        Hdr,
        Tonemapping::AcesFitted,
        FleetCamera,
        Projection::Perspective(PerspectiveProjection {
            near: 0.05,
            far: 2e9,
            ..default()
        }),
    ));
    commands.spawn((
        Mesh3d(meshes.add(Sphere::new(1.0).mesh().uv(48, 32))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb_u8(35, 59, 61),
            perceptual_roughness: 1.0,
            ..default()
        })),
        Transform::default(),
        PlanetVisual,
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 3500.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(4.0, 6.0, 8.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            left: px(12),
            top: px(12),
            width: px(340),
            flex_direction: FlexDirection::Column,
            ..default()
        })
        .with_children(|p| {
            p.spawn((
                Text::new("VOID / MULTIPLE VESSELS"),
                TextFont {
                    font_size: FontSize::Px(22.0),
                    ..default()
                },
            ));
            for (i, s) in Scenario::ALL.into_iter().enumerate() {
                button(p, &format!("{} / {}", i + 1, s.name()), Action::Scenario(s));
            }
            p.spawn(Node {
                flex_wrap: FlexWrap::Wrap,
                ..default()
            })
            .with_children(|p| {
                for (text, a) in [
                    ("Pause / P", Action::Pause),
                    ("Reset / R", Action::Reset),
                    ("Step", Action::Step(1.0 / 60.0)),
                    ("+10 s physics", Action::Step(10.0)),
                    ("+60 s physics", Action::Step(60.0)),
                    ("Next / Tab", Action::Next),
                    ("Stage / Space", Action::Stage),
                    ("Decouple", Action::Separate),
                    ("SAS / T", Action::Sas),
                    ("Spawn nearby", Action::Spawn),
                    ("Debug join / J", Action::Join),
                    ("Frame / F", Action::Fit),
                    ("All / G", Action::All),
                    ("Warp", Action::Warp),
                    ("Colliders / B", Action::Wire),
                    ("Origins / O", Action::Origins),
                ] {
                    button(p, text, a);
                }
            });
            p.spawn((
                Text::new(""),
                TextFont {
                    font_size: FontSize::Px(14.0),
                    ..default()
                },
                Hud,
            ));
        });
}
fn buttons(
    mut lab: NonSendMut<Lab>,
    buttons: Query<(&Interaction, &ButtonAction), Changed<Interaction>>,
) {
    for (i, a) in &buttons {
        if *i == Interaction::Pressed {
            lab.queue.push(a.0);
        }
    }
}
fn pick(event: On<Pointer<Click>>, parts: Query<&VisualPart>, mut lab: NonSendMut<Lab>) {
    if event.button != PointerButton::Primary || lab.drag > 5.0 {
        return;
    }
    if let Ok(p) = parts.get(event.entity) {
        let id = lab.scene.fleet.vessel_of_part(&p.id);
        lab.select(id);
    }
}
fn controls(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    time: Res<Time>,
    window: Single<&Window>,
    mut lab: NonSendMut<Lab>,
) {
    for (key, a) in [
        (KeyCode::KeyP, Action::Pause),
        (KeyCode::KeyR, Action::Reset),
        (KeyCode::Tab, Action::Next),
        (KeyCode::Space, Action::Stage),
        (KeyCode::KeyT, Action::Sas),
        (KeyCode::KeyJ, Action::Join),
        (KeyCode::KeyF, Action::Fit),
        (KeyCode::KeyG, Action::All),
        (KeyCode::KeyB, Action::Wire),
        (KeyCode::KeyO, Action::Origins),
    ] {
        if keys.just_pressed(key) {
            lab.queue.push(a);
        }
    }
    for (key, s) in [
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
        KeyCode::Digit6,
    ]
    .into_iter()
    .zip(Scenario::ALL)
    {
        if keys.just_pressed(key) {
            lab.queue.push(Action::Scenario(s));
        }
    }
    if mouse.just_pressed(MouseButton::Left) {
        lab.drag = 0.0;
    }
    if window.cursor_position().is_some_and(|p| p.x > 365.0) {
        if mouse.pressed(MouseButton::Left) {
            lab.drag += motion.delta.length();
            lab.yaw -= motion.delta.x * 0.006;
            lab.pitch = (lab.pitch + motion.delta.y * 0.006).clamp(-1.5, 1.5);
        }
        lab.distance = (lab.distance * (-scroll.delta.y * 0.12).exp()).clamp(2.0, 2e8);
    }
    let actions = std::mem::take(&mut lab.queue);
    for a in actions {
        lab.act(a);
    }
    let id = lab.selected.clone();
    let f = &mut lab.scene.fleet;
    let mut c = f.control(&id);
    c.throttle = (c.throttle
        + if keys.pressed(KeyCode::ShiftLeft) {
            time.delta_secs_f64() * 0.5
        } else if keys.pressed(KeyCode::ControlLeft) {
            -time.delta_secs_f64() * 0.5
        } else {
            0.0
        })
    .clamp(0.0, 1.0);
    if keys.just_pressed(KeyCode::KeyX) {
        c.throttle = 0.0;
    }
    c.turn = if window.focused {
        pilot_turn(&keys)
    } else {
        DVec3::ZERO
    };
    if f.part_snapshots(&id).iter().all(|p| {
        !p.definition
            .modules
            .iter()
            .any(|m| matches!(m, void_assembly::Module::Command))
    }) {
        c.turn = DVec3::ZERO;
    }
    let changed = c.throttle != f.control(&id).throttle || c.turn != DVec3::ZERO;
    if changed {
        lab.scene.references.clear();
    }
    lab.scene.fleet.set_control(&id, c);
}
fn simulate(time: Res<Time>, window: Single<&Window>, mut lab: NonSendMut<Lab>) {
    if !lab.paused && window.focused {
        let seconds = time.delta_secs_f64().min(0.1) * WARPS[lab.warp];
        lab.advance(seconds);
    }
}
fn owner_color(mode: VesselMode) -> Color {
    match mode {
        VesselMode::Orbit => Color::srgb_u8(150, 181, 255),
        VesselMode::Bubble => Color::srgb_u8(102, 223, 195),
        VesselMode::Ground => Color::srgb_u8(246, 183, 108),
    }
}
// Systems need the camera and mesh transforms disjoint; the HUD is separate text.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn visuals(
    mut commands: Commands,
    mut lab: NonSendMut<Lab>,
    assets: Res<RenderAssets>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut parts: Query<(&VisualPart, &mut Transform, &mut Visibility), Without<FleetCamera>>,
    mut camera: Single<&mut Transform, With<FleetCamera>>,
    mut hud: Single<&mut Text, With<Hud>>,
    mut gizmos: Gizmos,
    mut planet: Single<
        &mut Transform,
        (
            With<PlanetVisual>,
            Without<VisualPart>,
            Without<FleetCamera>,
        ),
    >,
) {
    if lab.dirty {
        for entities in lab.visual_parts.values() {
            for &e in entities {
                commands.entity(e).despawn();
            }
        }
        lab.visual_parts.clear();
        for (e, mesh) in lab.terrain.values() {
            commands.entity(*e).despawn();
            meshes.remove(mesh.id());
        }
        lab.terrain.clear();
        lab.dirty = false;
    }
    let f = &lab.scene.fleet;
    let selected = f.snapshot(&lab.selected);
    let focus = selected.position;
    let ids = f.vessel_ids();
    let snapshots: Vec<_> = ids.iter().flat_map(|id| f.part_snapshots(id)).collect();
    let poses: HashMap<_, _> = snapshots.iter().map(|p| (p.id.clone(), p)).collect();
    let live: HashSet<_> = poses.keys().cloned().collect();
    let mut tiles = vec![];
    for tile in f.terrain_tiles() {
        let key = (tile.scene, tile.tile.clone());
        let data = if lab.terrain.contains_key(&key) {
            None
        } else {
            Some(f.terrain_geometry(tile.scene, &tile.tile))
        };
        tiles.push((key, tile, data));
    }
    let (planet_centre, _) = f
        .ephemeris
        .body_state(BodyId(lab.scene.body_index), f.time());
    **planet = Transform::from_translation((planet_centre - focus).as_vec3())
        .with_scale(Vec3::splat(lab.scene.planet.terrain.radius_meters as f32));
    let up = if selected.mode == VesselMode::Ground {
        (focus - planet_centre).normalize().as_vec3()
    } else {
        (selected.rotation * DVec3::Y).as_vec3()
    };
    // The vessel's x axis, or its z axis if x lies along the vertical (a rocket on its side).
    let east = [DVec3::X, DVec3::Z]
        .map(|a| (selected.rotation * a).as_vec3())
        .into_iter()
        .find(|e| e.cross(up).length() > 0.1)
        .expect("one of two perpendicular axes leaves the vertical");
    let forward = east.cross(up).normalize();
    let side = up.cross(forward).normalize();
    let direction = side * (lab.yaw.cos() * lab.pitch.cos())
        + forward * (lab.yaw.sin() * lab.pitch.cos())
        + up * lab.pitch.sin();
    **camera = Transform::from_translation(direction * lab.distance).looking_at(Vec3::ZERO, up);
    for s in f.scene_snapshots() {
        if lab.origins {
            let p = (s.origin - focus).as_vec3();
            for (axis, color) in [
                (DVec3::X, Color::srgb_u8(255, 80, 80)),
                (DVec3::Y, Color::srgb_u8(80, 255, 80)),
                (DVec3::Z, Color::srgb_u8(80, 130, 255)),
            ] {
                gizmos.line(p, p + (s.rotation * axis * 5.0).as_vec3(), color);
            }
        }
    }
    for id in &ids {
        let s = f.snapshot(id);
        let p = f.relative(id, &lab.selected).position.as_vec3();
        gizmos.cross(
            Isometry3d::from_translation(p),
            if id == &lab.selected { 1.0 } else { 0.5 },
            owner_color(s.mode),
        );
    }
    for n in f.free_nodes(&lab.selected) {
        let (p, d) = f.node_frame(&n.part, &n.node);
        let p = (p - focus).as_vec3();
        gizmos.line(p, p + (d * 0.3).as_vec3(), Color::srgb_u8(102, 223, 195));
    }
    let control = f.control(&lab.selected);
    let mut text = format!(
        "{} / {}\nT+{:.3} s | {}{}\n{} vessels / {} bubbles / {} ground\n{} | {:?} | scene {:?}\n{:.1} kg | {:.2} m/s | fuel {:.1} kg\nThrottle {:.0}% | SAS {:?}\nStages {:?}\n",
        lab.scene.scenario.name(),
        if lab.all { "all" } else { "selected" },
        f.time(),
        if lab.paused { "PAUSED / " } else { "" },
        WARPS[lab.warp],
        ids.len(),
        f.bubble_count(),
        f.ground_count(),
        selected.id,
        selected.mode,
        selected.scene,
        selected.mass_kg,
        selected.velocity.length(),
        snapshots
            .iter()
            .filter(|p| selected.part_ids.contains(&p.id))
            .map(|p| p.fuel_kg)
            .sum::<f64>(),
        control.throttle * 100.0,
        f.sas_phase(&lab.selected),
        f.stages_left(&lab.selected)
    );
    for id in &ids {
        let r = f.relative(id, &lab.selected);
        text.push_str(&format!(
            "{id}: {:.2} m / {:.3} m/s / {:?}\n",
            r.position.length(),
            r.velocity.length(),
            f.snapshot(id).mode
        ));
    }
    if !lab.scene.references.is_empty() {
        text.push_str(&format!(
            "Reference max error: {:.6} m\n",
            lab.reference_error
        ));
    }
    if selected.mode == VesselMode::Ground {
        text.push_str(&format!(
            "AGL: {:.2} m\n",
            f.clearance(&lab.selected, lab.scene.body_index)
        ));
    }
    text.push_str("Shift/Ctrl throttle, X cut\nW/S A/D Q/E turn\nDrag orbit / wheel zoom\n");
    text.push_str(&lab.notice);
    for e in f.events.iter().rev().take(5).rev() {
        let mode = |m: Option<VesselMode>| m.map_or("none".into(), |m| format!("{m:?}"));
        text.push_str(&format!(
            "\n{:.2} s {} {} -> {}",
            e.time,
            e.vessel,
            mode(e.from),
            mode(e.to)
        ));
    }
    **hud = Text::new(text);
    for (p, mut t, mut visible) in &mut parts {
        if let Some(s) = poses.get(&p.id) {
            let mut local = p.local;
            if p.flame {
                local.scale.y *= 0.5
                    + lab
                        .scene
                        .fleet
                        .control(&lab.scene.fleet.vessel_of_part(&p.id))
                        .throttle as f32;
            }
            *t = Transform {
                translation: (s.position - focus).as_vec3(),
                rotation: s.rotation.as_quat(),
                ..default()
            }
            .mul_transform(local);
            *visible = if p.flame && !s.firing {
                Visibility::Hidden
            } else {
                Visibility::Visible
            };
        }
    }
    let obsolete: Vec<_> = lab
        .visual_parts
        .keys()
        .filter(|id| !live.contains(*id))
        .cloned()
        .collect();
    for id in obsolete {
        for e in lab.visual_parts.remove(&id).unwrap() {
            commands.entity(e).despawn();
        }
    }
    for s in snapshots {
        if !lab.visual_parts.contains_key(&s.id) {
            let mut entities = vec![];
            for p in &assets.parts[&s.definition.id] {
                let transform = Transform {
                    translation: (s.position - focus).as_vec3(),
                    rotation: s.rotation.as_quat(),
                    ..default()
                }
                .mul_transform(p.local);
                let e = commands
                    .spawn((
                        Mesh3d(p.mesh.clone()),
                        MeshMaterial3d(p.material.clone()),
                        transform,
                        if p.flame && !s.firing {
                            Visibility::Hidden
                        } else {
                            Visibility::Visible
                        },
                        VisualPart {
                            id: s.id.clone(),
                            local: p.local,
                            flame: p.flame,
                        },
                    ))
                    .observe(pick)
                    .id();
                entities.push(e);
            }
            lab.visual_parts.insert(s.id.clone(), entities);
        }
        if lab.wire {
            let d = s.definition;
            let q = s.rotation;
            let centre = s.position - focus;
            let top = d.height / 2.0;
            let color = Color::srgb_u8(255, 192, 95);
            if d.shape == void_assembly::Shape::Box {
                let h = DVec3::new(d.radius, top, d.radius);
                for axis in 0..3 {
                    for a in [-1.0, 1.0] {
                        for b in [-1.0, 1.0] {
                            let mut p = h;
                            p[(axis + 1) % 3] *= a;
                            p[(axis + 2) % 3] *= b;
                            let mut end = p;
                            end[axis] = -end[axis];
                            gizmos.line(
                                (centre + q * p).as_vec3(),
                                (centre + q * end).as_vec3(),
                                color,
                            );
                        }
                    }
                }
                continue;
            }
            for i in 0..32 {
                let a = i as f64 * std::f64::consts::TAU / 32.0;
                let b = (i + 1) as f64 * std::f64::consts::TAU / 32.0;
                let at = |angle: f64, y: f64| {
                    (centre + q * DVec3::new(d.radius * angle.cos(), y, d.radius * angle.sin()))
                        .as_vec3()
                };
                gizmos.line(at(a, -top), at(b, -top), color);
                if d.shape == void_assembly::Shape::Cylinder {
                    gizmos.line(at(a, top), at(b, top), color);
                    if i % 8 == 0 {
                        gizmos.line(at(a, -top), at(a, top), color);
                    }
                } else if i % 8 == 0 {
                    gizmos.line(at(a, -top), (centre + q * DVec3::Y * top).as_vec3(), color);
                }
            }
        }
    }
    let active: HashSet<_> = tiles.iter().map(|(key, _, _)| key.clone()).collect();
    let obsolete: Vec<_> = lab
        .terrain
        .keys()
        .filter(|k| !active.contains(*k))
        .cloned()
        .collect();
    for key in obsolete {
        let (e, m) = lab.terrain.remove(&key).unwrap();
        commands.entity(e).despawn();
        meshes.remove(m.id());
    }
    for (key, tile, data) in tiles {
        let transform = Transform {
            translation: (tile.position - focus).as_vec3(),
            rotation: tile.rotation.as_quat(),
            ..default()
        };
        if let Some((e, _)) = lab.terrain.get(&key) {
            commands.entity(*e).insert(transform);
        } else {
            let (vertices, indices) = data.expect("new tile geometry");
            let mut mesh = Mesh::new(
                PrimitiveTopology::TriangleList,
                RenderAssetUsages::default(),
            );
            mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, vertices);
            mesh.insert_indices(Indices::U32(indices.into_iter().flatten().collect()));
            mesh.compute_normals();
            let mesh = meshes.add(mesh);
            let material = materials.add(StandardMaterial {
                base_color: Color::srgb_u8(35, 59, 61),
                perceptual_roughness: 1.0,
                ..default()
            });
            let e = commands
                .spawn((
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(material),
                    transform,
                    TileVisual,
                ))
                .id();
            lab.terrain.insert(key, (e, mesh));
        }
    }
}
// Match lab/vessels's pilot(): positive pitch S, roll E, yaw D.
fn pilot_turn(keys: &ButtonInput<KeyCode>) -> DVec3 {
    let axis = |positive, negative| {
        f64::from(keys.pressed(positive) as u8) - f64::from(keys.pressed(negative) as u8)
    };
    DVec3::new(
        axis(KeyCode::KeyS, KeyCode::KeyW),
        axis(KeyCode::KeyE, KeyCode::KeyQ),
        axis(KeyCode::KeyD, KeyCode::KeyA),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pilot_keys_match_ts_local_axes() {
        for (key, expected) in [
            (KeyCode::KeyW, -DVec3::X),
            (KeyCode::KeyS, DVec3::X),
            (KeyCode::KeyQ, -DVec3::Y),
            (KeyCode::KeyE, DVec3::Y),
            (KeyCode::KeyA, -DVec3::Z),
            (KeyCode::KeyD, DVec3::Z),
        ] {
            let mut keys = ButtonInput::default();
            keys.press(key);
            assert_eq!(pilot_turn(&keys), expected);
        }
        let mut keys = ButtonInput::default();
        for key in [
            KeyCode::KeyW,
            KeyCode::KeyS,
            KeyCode::KeyQ,
            KeyCode::KeyE,
            KeyCode::KeyA,
            KeyCode::KeyD,
        ] {
            keys.press(key);
        }
        assert_eq!(pilot_turn(&keys), DVec3::ZERO);
    }
    #[test]
    fn bevy_systems_initialize_without_query_conflicts() {
        let mut world = World::new();
        let mut schedule = Schedule::default();
        schedule.add_systems((buttons, controls, simulate, visuals).chain());
        schedule
            .initialize(&mut world)
            .expect("vessels lab system access");
    }
    #[test]
    fn controls_spawn_select_stage_and_reset_real_fleet() {
        let mut lab = Lab::new(Scenario::Separate);
        lab.act(Action::Spawn);
        assert_eq!(lab.scene.fleet.vessel_ids().len(), 2);
        assert_eq!(lab.scene.fleet.bubble_count(), 1);
        lab.act(Action::Stage);
        assert_eq!(lab.scene.fleet.stages_left("v1"), [1]);
        lab.act(Action::Stage);
        assert_eq!(lab.scene.fleet.vessel_ids().len(), 3);
        lab.act(Action::Next);
        assert_eq!(lab.selected, "v2");
        lab.act(Action::All);
        assert!(lab.distance > 35.0);
        lab.act(Action::Reset);
        assert_eq!(lab.scene.fleet.vessel_ids(), ["v1"]);
        assert!(lab.paused);
    }
    #[test]
    fn debug_join_requires_range_and_preserves_fleet_selection() {
        let mut lab = Lab::new(Scenario::Join);
        lab.act(Action::Join);
        assert_eq!(lab.scene.fleet.vessel_ids().len(), 2);
        assert!(!lab.notice.is_empty());
        while (lab.scene.fleet.node_frame("v1/p2", "bottom").0
            - lab.scene.fleet.node_frame("v2/p2", "bottom").0)
            .length()
            > 0.08
            && lab.scene.fleet.time() < 60.0
        {
            lab.scene.fleet.advance(1.0 / 60.0);
        }
        lab.act(Action::Join);
        assert_eq!(lab.scene.fleet.vessel_ids(), ["v1"]);
        assert_eq!(lab.selected, "v1");
        assert_eq!(lab.scene.fleet.part_snapshots("v1").len(), 4);
    }
}
