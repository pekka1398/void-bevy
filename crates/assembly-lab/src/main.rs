//! Independent assembly editor. No dependency on void-app.
use bevy::camera::Hdr;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit};
use bevy::input_focus::InputFocus;
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;
use bevy::text::{EditableText, TextCursorStyle};
use glam::DVec3;
use void_assembly::*;
use void_assembly_lab::parts::{PartMesh, RenderAssets, SceneLines};

fn main() {
    let mut args = std::env::args().skip(1);
    let mut craft = demo_craft();
    let mut path = "void-craft.json".to_string();
    if let Some(flag) = args.next() {
        assert_eq!(flag, "--craft", "use --craft PATH");
        path = args.next().expect("--craft requires a path");
        assert!(args.next().is_none(), "unexpected arguments");
        craft = import_craft(&std::fs::read_to_string(&path).expect("read --craft file"))
            .expect("invalid --craft file");
    }
    App::new()
        .add_plugins((
            DefaultPlugins.set(WindowPlugin {
                primary_window: Some(Window {
                    title: "VOID | assembly lab".into(),
                    resolution: (1440, 900).into(),
                    ..default()
                }),
                ..default()
            }),
            MeshPickingPlugin,
        ))
        .init_gizmo_group::<SceneLines>()
        .insert_resource(ClearColor(Color::srgb(0.045, 0.065, 0.085)))
        .insert_resource(GlobalAmbientLight {
            brightness: 150.0,
            ..default()
        })
        .insert_resource(Lab::new(craft, path))
        .insert_resource(Orbit::default())
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                buttons,
                inputs,
                controls,
                actions,
                rebuild,
                draw,
                scene_lines,
            )
                .chain(),
        )
        .run();
}
#[derive(Resource)]
struct Lab {
    craft: Craft,
    compiled: CompiledCraft,
    selected: String,
    pending: Option<String>,
    child_node: String,
    notice: String,
    path: String,
    dirty: bool,
    fit: bool,
    show_com: bool,
    queue: Vec<Action>,
    drag_pixels: f32,
}
impl Lab {
    fn new(craft: Craft, path: String) -> Self {
        let compiled = compile(&craft).expect("initial craft");
        let selected = compiled.root_id.clone();
        Self {
            craft,
            compiled,
            selected,
            pending: None,
            child_node: "top".into(),
            notice: "Choose a part, then click a green stack node.".into(),
            path,
            dirty: true,
            fit: true,
            show_com: true,
            queue: vec![],
            drag_pixels: 0.0,
        }
    }
    fn set_craft(&mut self, c: Craft) -> ModelResult<()> {
        let compiled = compile(&c)?;
        self.craft = c;
        self.compiled = compiled;
        if !self.craft.parts.iter().any(|p| p.id == self.selected) {
            self.selected = self.compiled.root_id.clone();
        }
        self.dirty = true;
        Ok(())
    }
    fn offset(&self) -> f64 {
        -self
            .compiled
            .parts
            .iter()
            .map(|p| p.pose.position.y - p.definition.height / 2.0)
            .fold(f64::INFINITY, f64::min)
            + 0.04
    }
    fn part_pose(&self, id: &str) -> PartPose {
        let mut p = self.compiled.part(id).pose;
        p.position.y += self.offset();
        p
    }
    fn act(&mut self, a: Action) -> ModelResult<()> {
        match a {
            Action::New => {
                self.selected = "p1".into();
                self.pending = None;
                self.set_craft(fresh_craft())?;
                self.fit = true;
            }
            Action::Demo => {
                self.selected = "p1".into();
                self.pending = None;
                self.set_craft(demo_craft())?;
                self.fit = true;
            }
            Action::Palette(id) => {
                self.pending = Some(id.clone());
                self.child_node = definition(&id)?.nodes[0].id.clone();
                self.dirty = true;
            }
            Action::ChildNode => {
                if let Some(id) = &self.pending {
                    let d = definition(id)?;
                    let i = d
                        .nodes
                        .iter()
                        .position(|n| n.id == self.child_node)
                        .expect("selected authored node");
                    self.child_node = d.nodes[(i + 1) % d.nodes.len()].id.clone();
                    self.dirty = true;
                }
            }
            Action::Cancel => {
                self.pending = None;
                self.dirty = true;
            }
            Action::Select(id) => {
                self.selected = id;
            }
            Action::Attach(parent, n) => {
                if let Some(d) = self.pending.clone() {
                    let c = add_part(&self.craft, &d, &parent, &n, &self.child_node)?;
                    self.selected = c.parts.last().expect("added part").id.clone();
                    self.set_craft(c)?;
                    self.pending = None;
                }
            }
            Action::Delete => {
                let c = remove_subtree(&self.craft, &self.selected)?;
                self.set_craft(c)?;
            }
            Action::Fuel(delta) => {
                let mut c = self.craft.clone();
                let p = c
                    .parts
                    .iter_mut()
                    .find(|p| p.id == self.selected)
                    .expect("selected part");
                let cap = tank_capacity(definition(&p.definition_id)?);
                let r = void_assembly::ResourceId::LiquidPropellant;
                let quantity = p
                    .resources
                    .get_mut(&r)
                    .ok_or_else(|| "selected part has no liquid propellant tank".to_string())?;
                *quantity = (*quantity + delta * cap).clamp(0.0, cap);
                self.set_craft(c)?;
            }
            Action::StageEdit(delta) => {
                let mut c = self.craft.clone();
                let p = c
                    .parts
                    .iter_mut()
                    .find(|p| p.id == self.selected)
                    .expect("selected part");
                if !actionable(definition(&p.definition_id)?) {
                    return Err("Select an engine or decoupler to assign a stage.".into());
                }
                let n = p.stage.map_or(-1, |s| s as i32) + delta;
                p.stage = if n < 0 { None } else { Some(n.min(99) as u32) };
                self.set_craft(c)?;
            }
            Action::Name(name) => {
                let mut c = self.craft.clone();
                c.name = name;
                self.set_craft(c)?;
            }
            Action::Save => {
                let text = export_craft(&self.craft)?;
                let mut f = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&self.path)
                    .map_err(|e| {
                        format!(
                            "Save {}: {e}. Choose a new path if the file exists.",
                            self.path
                        )
                    })?;
                use std::io::Write;
                f.write_all(text.as_bytes())
                    .map_err(|e| format!("Save: {e}"))?;
                self.notice = format!("Saved {}", self.path);
            }
            Action::Load => {
                let text = std::fs::read_to_string(&self.path)
                    .map_err(|e| format!("Load {}: {e}", self.path))?;
                let c = import_craft(&text)?;
                self.set_craft(c)?;
                self.pending = None;
                self.fit = true;
                self.notice = format!("Loaded {}", self.path);
            }
            Action::Fit => self.fit = true,
            Action::Com => self.show_com = !self.show_com,
        }
        Ok(())
    }
}
#[derive(Clone, Component)]
enum Action {
    New,
    Demo,
    Palette(String),
    ChildNode,
    Cancel,
    Select(String),
    Attach(String, String),
    Delete,
    Fuel(f64),
    StageEdit(i32),
    Name(String),
    Save,
    Load,
    Fit,
    Com,
}
#[derive(Resource)]
struct Orbit {
    distance: f64,
    yaw: f64,
    pitch: f64,
    pan: DVec3,
}
impl Default for Orbit {
    fn default() -> Self {
        Self {
            distance: 22.0,
            yaw: 0.6,
            pitch: 0.3,
            pan: DVec3::ZERO,
        }
    }
}
#[derive(Component)]
struct Visual;
#[derive(Component)]
struct Com;
#[derive(Component)]
struct Hud;
#[derive(Component)]
struct Inspector;
#[derive(Component)]
struct PendingLabel;
#[derive(Component)]
struct StageList;
#[derive(Component)]
struct CraftName;
#[derive(Component)]
struct FilePath;
#[derive(Component)]
struct MainCamera;
fn font(size: f32) -> TextFont {
    TextFont {
        font_size: FontSize::Px(size),
        ..default()
    }
}
fn label(parent: &mut ChildSpawnerCommands, text: &str, size: f32) {
    parent.spawn((
        Text::new(text),
        font(size),
        TextColor(Color::srgb(0.73, 0.8, 0.86)),
    ));
}
fn button(parent: &mut ChildSpawnerCommands, text: &str, action: Action) {
    parent
        .spawn((
            Button,
            action,
            Node {
                min_height: px(30),
                width: percent(100),
                padding: px(7).all(),
                margin: px(2).vertical(),
                ..default()
            },
            BackgroundColor(Color::srgb(0.12, 0.19, 0.24)),
        ))
        .with_children(|p| {
            p.spawn((Text::new(text), font(13.0)));
        });
}
fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut gizmo_config: ResMut<GizmoConfigStore>,
    lab: Res<Lab>,
) {
    gizmo_config
        .config_mut::<DefaultGizmoConfigGroup>()
        .0
        .depth_bias = -1.0;
    commands.insert_resource(RenderAssets::new(&mut meshes, &mut materials));
    commands.spawn((
        Camera3d::default(),
        MainCamera,
        Hdr,
        Tonemapping::AcesFitted,
        Projection::Perspective(PerspectiveProjection {
            fov: 40f32.to_radians(),
            near: 0.05,
            far: 100000.0,
            ..default()
        }),
        DistanceFog {
            color: Color::srgb_u8(18, 27, 36),
            falloff: FogFalloff::ExponentialSquared { density: 0.006 },
            ..default()
        },
        Transform::from_xyz(12.0, 8.0, 18.0).looking_at(Vec3::new(0.0, 4.0, 0.0), Vec3::Y),
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 3000.0,
            color: Color::srgb_u8(255, 234, 209),
            ..default()
        },
        Transform::from_xyz(6.0, 12.0, 8.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 2000.0,
            color: Color::srgb_u8(147, 217, 239),
            ..default()
        },
        Transform::from_xyz(-6.0, 4.0, -5.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(2000.0, 2000.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb_u8(25, 39, 49),
            perceptual_roughness: 1.0,
            ..default()
        })),
        Transform::from_xyz(0.0, -0.012, 0.0),
        Pickable::IGNORE,
    ));
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: px(12),
                left: px(260),
                right: px(296),
                ..default()
            },
            Pickable::IGNORE,
        ))
        .with_children(|p| {
            p.spawn((Text::new(""), font(15.0), Hud));
        });
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                top: px(0),
                bottom: px(0),
                width: px(246),
                padding: px(14).all(),
                flex_direction: FlexDirection::Column,
                ..default()
            },
            BackgroundColor(Color::srgb(0.055, 0.085, 0.115)),
        ))
        .with_children(|p| {
            label(p, "VOID / ASSEMBLY", 20.0);
            label(p, "CRAFT NAME", 11.0);
            p.spawn((
                EditableText::new(lab.craft.name.clone()),
                TextCursorStyle::default(),
                font(13.0),
                CraftName,
                Node {
                    min_height: px(30),
                    padding: px(6).all(),
                    ..default()
                },
                BackgroundColor(Color::srgb(0.12, 0.17, 0.21)),
            ));
            button(p, "New command pod", Action::New);
            button(p, "Two-stage demo", Action::Demo);
            label(p, "PART LIBRARY", 12.0);
            for d in catalog() {
                // Labels are authored part data, including the flight rocket catalog.
                button(p, &d.name, Action::Palette(d.id.clone()));
            }
            p.spawn((Text::new(""), font(12.0), PendingLabel));
            button(p, "Switch new part's node", Action::ChildNode);
            button(p, "Cancel attachment", Action::Cancel);
            label(p, "JSON FILE PATH", 11.0);
            p.spawn((
                EditableText::new(lab.path.clone()),
                TextCursorStyle::default(),
                font(12.0),
                FilePath,
                Node {
                    min_height: px(30),
                    padding: px(6).all(),
                    ..default()
                },
                BackgroundColor(Color::srgb(0.12, 0.17, 0.21)),
            ));
            button(p, "Load JSON", Action::Load);
            button(p, "Export JSON (new file)", Action::Save);
        });
    commands.spawn((Node { position_type:PositionType::Absolute,right:px(0),top:px(0),bottom:px(0),width:px(282),padding:px(14).all(),flex_direction:FlexDirection::Column,..default() },BackgroundColor(Color::srgb(0.055,0.085,0.115))))
        .with_children(|p| {
            label(p,"SELECTED PART",14.0); p.spawn((Text::new(""),font(13.0),Inspector));
            button(p,"Fuel -10% capacity",Action::Fuel(-0.1)); button(p,"Fuel +10% capacity",Action::Fuel(0.1));
            button(p,"Stage -1 (below 0 = unset)",Action::StageEdit(-1)); button(p,"Stage +1",Action::StageEdit(1)); button(p,"Remove selected subtree",Action::Delete);
            label(p,"STAGING / LOW TO HIGH",12.0); p.spawn((Text::new(""),font(12.0),StageList));
            button(p,"F / frame craft",Action::Fit); button(p,"C / center of mass",Action::Com);
            label(p,"Click a hull to select.\nDrag left: orbit\nDrag right: pan\nWheel: zoom",12.0);
        });
}
fn buttons(
    mut lab: ResMut<Lab>,
    mut focus: ResMut<InputFocus>,
    mut query: Query<(&Interaction, &Action, &mut BackgroundColor), Changed<Interaction>>,
) {
    for (interaction, action, mut color) in &mut query {
        color.0 = match interaction {
            Interaction::Hovered => Color::srgb(0.2, 0.31, 0.37),
            Interaction::Pressed => {
                focus.clear();
                lab.queue.push(action.clone());
                Color::srgb(0.27, 0.44, 0.46)
            }
            Interaction::None => Color::srgb(0.12, 0.19, 0.24),
        };
    }
}
#[allow(clippy::type_complexity)] // Bevy query data and its change filter.
fn inputs(
    mut lab: ResMut<Lab>,
    input: Query<(&EditableText, Option<&CraftName>, Option<&FilePath>), Changed<EditableText>>,
) {
    for (edit, name, path) in &input {
        let value: String = edit.value().into_iter().collect();
        if name.is_some() && value != lab.craft.name {
            lab.queue.push(Action::Name(value));
        } else if path.is_some() {
            lab.path = value;
        }
    }
}
fn actions(mut lab: ResMut<Lab>, mut names: Query<&mut EditableText, With<CraftName>>) {
    let mut sync_name = false;
    for a in std::mem::take(&mut lab.queue) {
        let reset = matches!(a, Action::New | Action::Demo | Action::Load);
        match lab.act(a) {
            Err(e) => lab.notice = e,
            Ok(()) => sync_name |= reset,
        }
    }
    if sync_name {
        for mut name in &mut names {
            name.editor.set_text(&lab.craft.name);
        }
    }
}
#[allow(clippy::too_many_arguments)] // Independent system parameters injected by Bevy.
fn controls(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    window: Single<&Window>,
    focus: Res<InputFocus>,
    edits: Query<(), With<EditableText>>,
    mut lab: ResMut<Lab>,
    mut orbit: ResMut<Orbit>,
) {
    let typing = focus.get().is_some_and(|e| edits.contains(e));
    if !typing {
        for (key, action) in [
            (KeyCode::KeyF, Action::Fit),
            (KeyCode::KeyC, Action::Com),
            (KeyCode::Escape, Action::Cancel),
            (KeyCode::Delete, Action::Delete),
        ] {
            if keys.just_pressed(key) {
                lab.queue.push(action);
            }
        }
    }
    if mouse.just_pressed(MouseButton::Left) {
        lab.drag_pixels = 0.0;
    }
    if mouse.pressed(MouseButton::Left) {
        lab.drag_pixels += motion.delta.length();
    }
    if !window
        .cursor_position()
        .is_some_and(|p| p.x > 246.0 && p.x < window.width() - 282.0)
    {
        return;
    }
    if mouse.pressed(MouseButton::Left) {
        orbit.yaw -= motion.delta.x as f64 * 0.006;
        orbit.pitch = (orbit.pitch + motion.delta.y as f64 * 0.006).clamp(-1.45, 1.45);
    }
    if mouse.pressed(MouseButton::Right) {
        let q = DVec3::new(orbit.yaw.cos(), 0.0, -orbit.yaw.sin());
        let scale = orbit.distance * 0.0015;
        orbit.pan +=
            q * (-motion.delta.x as f64 * scale) + DVec3::Y * (motion.delta.y as f64 * scale);
    }
    let wheel = match scroll.unit {
        MouseScrollUnit::Line => scroll.delta.y as f64,
        MouseScrollUnit::Pixel => scroll.delta.y as f64 / 40.0,
    };
    orbit.distance = (orbit.distance * (-wheel * 0.12).exp()).clamp(1.5, 2000.0);
}
fn pick(
    event: On<Pointer<Click>>,
    mut lab: ResMut<Lab>,
    mut focus: ResMut<InputFocus>,
    parts: Query<&PartMesh>,
    nodes: Query<&Action>,
) {
    if event.button != PointerButton::Primary || lab.drag_pixels > 5.0 {
        return;
    }
    focus.clear();
    if let Ok(action) = nodes.get(event.entity) {
        lab.queue.push(action.clone());
    } else if let Ok(p) = parts.get(event.entity) {
        lab.queue.push(Action::Select(p.id.clone()));
    }
}
fn rebuild(
    mut commands: Commands,
    mut lab: ResMut<Lab>,
    assets: Res<RenderAssets>,
    old: Query<Entity, With<Visual>>,
    mut orbit: ResMut<Orbit>,
) {
    if !lab.dirty {
        return;
    }
    lab.dirty = false;
    for e in &old {
        commands.entity(e).despawn();
    }
    for p in &lab.compiled.parts {
        let pose = lab.part_pose(&p.instance.id);
        let root = Transform::from_translation(pose.position.as_vec3())
            .with_rotation(pose.rotation.as_quat());
        // Engine flames are drawn only in flight, which the editor has none of.
        for piece in assets.parts[&p.definition.id].iter().filter(|p| !p.flame) {
            commands
                .spawn((
                    Visual,
                    Mesh3d(piece.mesh.clone()),
                    MeshMaterial3d(piece.material.clone()),
                    root.mul_transform(piece.local),
                    PartMesh {
                        id: p.instance.id.clone(),
                        local: piece.local,
                    },
                ))
                .observe(pick);
        }
    }
    if lab.pending.is_some() {
        for free in lab.compiled.free_nodes() {
            commands
                .spawn((
                    Visual,
                    Action::Attach(free.part_id, free.node.id.clone()),
                    Mesh3d(assets.ball.clone()),
                    MeshMaterial3d(assets.green.clone()),
                    Transform::from_translation(
                        (free.pose.position + DVec3::Y * lab.offset()).as_vec3(),
                    ),
                ))
                .observe(pick);
        }
    }
    commands.spawn((
        Visual,
        Com,
        Mesh3d(assets.ball.clone()),
        MeshMaterial3d(assets.center.clone()),
        Transform::default(),
        Pickable::IGNORE,
    ));
    if lab.fit {
        orbit.pan = DVec3::ZERO;
    }
}
#[allow(clippy::type_complexity, clippy::too_many_arguments)] // Disjoint Bevy render queries.
fn draw(
    mut lab: ResMut<Lab>,
    mut orbit: ResMut<Orbit>,
    mut gizmos: Gizmos,
    mut parts: Query<(&PartMesh, &mut Transform), (Without<MainCamera>, Without<Com>)>,
    mut com: Query<
        (&mut Transform, &mut Visibility),
        (
            With<Com>,
            Without<PartMesh>,
            Without<MainCamera>,
        ),
    >,
    mut camera: Single<
        &mut Transform,
        (
            With<MainCamera>,
            Without<PartMesh>,
            Without<Com>,
        ),
    >,
    mut texts: Query<(
        &mut Text,
        Option<&Hud>,
        Option<&Inspector>,
        Option<&PendingLabel>,
        Option<&StageList>,
    )>,
) {
    let center = lab.compiled.summary(None).center + DVec3::Y * lab.offset();
    if lab.fit {
        let min = lab
            .compiled
            .parts
            .iter()
            .map(|p| p.pose.position.y - p.definition.height / 2.0)
            .fold(f64::INFINITY, f64::min);
        let max = lab
            .compiled
            .parts
            .iter()
            .map(|p| p.pose.position.y + p.definition.height / 2.0)
            .fold(f64::NEG_INFINITY, f64::max);
        orbit.distance = ((max - min) * 2.2).max(5.0);
        orbit.pan = DVec3::ZERO;
        lab.fit = false;
    }
    let aim = center + orbit.pan;
    let direction = DVec3::new(
        orbit.pitch.cos() * orbit.yaw.sin(),
        orbit.pitch.sin(),
        orbit.pitch.cos() * orbit.yaw.cos(),
    );
    **camera = Transform::from_translation((aim + direction * orbit.distance).as_vec3())
        .looking_at(aim.as_vec3(), Vec3::Y);
    for (p, mut t) in &mut parts {
        let pose = lab.part_pose(&p.id);
        *t = Transform::from_translation(pose.position.as_vec3())
            .with_rotation(pose.rotation.as_quat())
            .mul_transform(p.local);
    }
    for (mut t, mut v) in &mut com {
        t.translation = center.as_vec3();
        *v = if lab.show_com {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
    if lab.show_com {
        let center = center.as_vec3();
        gizmos.sphere(center, 0.13, Color::srgb(1.0, 0.78, 0.25));
        gizmos.axes(Transform::from_translation(center), 0.65);
    }
    let p = lab.compiled.part(&lab.selected);
    let s = lab.compiled.summary(None);
    for (mut text, hud, inspector, pending, stages) in &mut texts {
        if hud.is_some() {
            text.0 = format!(
                "EDITOR | {} parts | {:.0} kg total / {:.1} kg fuel\n{}",
                lab.craft.parts.len(),
                s.mass_kg,
                s.fuel_kg,
                lab.notice
            );
        }
        if inspector.is_some() {
            let fuel = p.instance.resource_mass();
            text.0 = format!(
                "{} / {}\nDry mass: {:.0} kg\nFuel: {:.1} / {:.0} kg\nStage: {}\n{}",
                p.instance.id,
                p.instance.definition_id,
                p.definition.dry_mass_kg,
                fuel,
                tank_capacity(p.definition),
                p.instance.stage.map_or("unset".into(), |n| n.to_string()),
                if p.definition.category == Category::Engine {
                    format!(
                        "Fuel sources: {}",
                        lab.compiled
                            .fuel_sources(&p.instance.id, &std::collections::HashSet::new())
                            .join(", ")
                    )
                } else {
                    "".into()
                }
            );
        }
        if pending.is_some() {
            text.0 = lab
                .pending
                .as_ref()
                .map_or("No part selected for attachment".into(), |id| {
                    format!("Attach: {id}\nNew part node: {}", lab.child_node)
                });
        }
        if stages.is_some() {
            let mut nums: Vec<_> = lab.craft.parts.iter().filter_map(|p| p.stage).collect();
            nums.sort_unstable();
            nums.dedup();
            text.0 = nums
                .iter()
                .map(|n| {
                    format!(
                        "{n}: {}",
                        lab.craft
                            .parts
                            .iter()
                            .filter(|p| p.stage == Some(*n))
                            .map(|p| p.id.clone())
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
        }
    }
}

fn scene_lines(lab: Res<Lab>, assets: Res<RenderAssets>, mut lines: Gizmos<SceneLines>) {
    // A grid 200 m across in 100 divisions, with brighter central axes.
    for i in 0..=100 {
        let p = i as f32 * 2.0 - 100.0;
        let color = if i == 50 {
            Color::srgb_u8(68, 96, 112)
        } else {
            Color::srgb_u8(39, 59, 72)
        };
        lines.line(
            Vec3::new(p, 0.002, -100.0),
            Vec3::new(p, 0.002, 100.0),
            color,
        );
        lines.line(
            Vec3::new(-100.0, 0.002, p),
            Vec3::new(100.0, 0.002, p),
            color,
        );
    }
    lines
        .circle(
            Isometry3d::new(
                Vec3::new(0.0, 0.015, 0.0),
                Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2),
            ),
            2.52,
            Color::srgb_u8(215, 170, 88),
        )
        .resolution(64);
    // Selection is an outline over the hull's own material.
    let p = lab.compiled.part(&lab.selected);
    let pose = lab.part_pose(&lab.selected);
    let root =
        Transform::from_translation(pose.position.as_vec3()).with_rotation(pose.rotation.as_quat());
    for edge in assets.outlines[&p.definition.id].as_chunks::<2>().0 {
        lines.line(
            root.transform_point(edge[0]),
            root.transform_point(edge[1]),
            Color::srgba(145.0 / 255.0, 217.0 / 255.0, 195.0 / 255.0, 0.8),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bevy_system_queries_initialize_without_conflicting_access() {
        let mut world = World::new();
        let mut schedule = Schedule::default();
        schedule.add_systems(
            (
                buttons,
                inputs,
                controls,
                actions,
                rebuild,
                draw,
                scene_lines,
            )
                .chain(),
        );
        schedule
            .initialize(&mut world)
            .expect("assembly Bevy systems must initialize");
    }
    #[test]
    fn invalid_edits_and_load_preserve_editor() {
        let mut lab = Lab::new(
            demo_craft(),
            "/no-such-assembly-directory/invalid.json".into(),
        );
        let before = lab.craft.clone();
        assert!(lab.act(Action::Delete).is_err());
        assert!(lab.act(Action::Name("  ".into())).is_err());
        assert!(lab.act(Action::Load).is_err());
        assert_eq!(lab.craft, before);
        lab.selected = "p3".into();
        lab.act(Action::StageEdit(-2)).unwrap();
    }
    #[test]
    fn editor_exports_and_loads_the_actual_craft() {
        let path =
            std::env::temp_dir().join(format!("void-assembly-test-{}.json", std::process::id()));
        let mut lab = Lab::new(demo_craft(), path.to_str().unwrap().into());
        lab.act(Action::Save).unwrap();
        let original = std::fs::read_to_string(&path).unwrap();
        lab.act(Action::New).unwrap();
        assert!(lab.act(Action::Save).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
        lab.act(Action::Load).unwrap();
        assert_eq!(lab.craft, demo_craft());
        std::fs::write(&path, "{bad json}").unwrap();
        let before = lab.craft.clone();
        assert!(lab.act(Action::Load).is_err());
        assert_eq!(lab.craft, before);
        std::fs::remove_file(path).unwrap();
    }
}
