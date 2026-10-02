//! Standalone TS orbit lab: four plotting frames, Sol/binary, history/prediction/plans,
//! editable burns, target paths and endpoint/event markers. Up/down choose settings;
//! left/right change them; Enter edits numeric values exactly. See docs/orbit-lab.md.
use bevy::{
    input::{
        keyboard::{Key, KeyboardInput},
        mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit},
    },
    prelude::*,
    window::PrimaryWindow,
};
use glam::DVec3;
use void_orbit::*;
use void_orbit_lab::{
    scene::{PlotScene, SceneView, distance, duration},
    *,
};

const SCALE: f64 = 1e-3;
#[derive(Resource)]
struct Lab(OrbitLab);
#[derive(Resource, Default)]
struct View(SceneView);
#[derive(Resource, Default)]
struct Scene(Option<PlotScene>);
#[derive(Resource)]
struct Ui {
    setting: usize,
    edit: Option<String>,
    notice: String,
    distance: f64,
    azimuth: f64,
    elevation: f64,
    frame_body: usize,
    primary: usize,
    secondary: usize,
    rebuild: bool,
    labels: Vec<(Entity, Entity)>,
}
#[derive(Component)]
struct BodyVisual(usize);
#[derive(Component)]
struct Hud;
#[derive(Component)]
struct Panel;
#[derive(Component)]
struct Label {
    slot: usize,
    focus: Option<Focus>,
}
#[derive(Component)]
struct LabelText;
#[derive(Resource)]
struct Bodies {
    mesh: Handle<Mesh>,
    materials: Vec<Handle<StandardMaterial>>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Setting {
    System,
    Frame,
    FrameBody,
    Primary,
    Secondary,
    Focus,
    Trail,
    Vessel,
    Prediction,
    Reference,
    StartPlane,
    Coast,
    Target,
    Burn,
    BurnReference,
    Start,
    Prograde,
    Normal,
    Radial,
}
const SETTINGS: [Setting; 19] = [
    Setting::System,
    Setting::Frame,
    Setting::FrameBody,
    Setting::Primary,
    Setting::Secondary,
    Setting::Focus,
    Setting::Trail,
    Setting::Vessel,
    Setting::Prediction,
    Setting::Reference,
    Setting::StartPlane,
    Setting::Coast,
    Setting::Target,
    Setting::Burn,
    Setting::BurnReference,
    Setting::Start,
    Setting::Prograde,
    Setting::Normal,
    Setting::Radial,
];
fn main() {
    let mut preset = SystemPreset::Sol;
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        assert_eq!(flag, "--system", "use --system sol|binary");
        preset = match args.next().as_deref() {
            Some("sol") => SystemPreset::Sol,
            Some("binary") => SystemPreset::Binary,
            _ => panic!("use --system sol|binary"),
        };
    }
    let model = OrbitLab::new(preset);
    let home = model.home;
    let moon = model.target.unwrap();
    let initial_distance = model.sim.system.bodies[home].radius_meters * SCALE * 4.0;
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "VOID | orbit lab".into(),
                resolution: (1440, 960).into(),
                ..default()
            }),
            ..default()
        }))
        .insert_resource(ClearColor(Color::srgb(0.012, 0.016, 0.04)))
        .insert_resource(GlobalAmbientLight {
            brightness: 700.0,
            ..default()
        })
        .insert_resource(Lab(model))
        .insert_resource(Ui {
            setting: 0,
            edit: None,
            notice: String::new(),
            distance: initial_distance,
            azimuth: -std::f64::consts::FRAC_PI_2,
            elevation: 0.45,
            frame_body: home,
            primary: home,
            secondary: moon,
            rebuild: true,
            labels: Vec::new(),
        })
        .init_resource::<View>()
        .init_resource::<Scene>()
        .add_systems(Startup, setup)
        .add_systems(Update, (input, simulate, plot, rebuild, draw).chain())
        .run();
}
fn hex(s: &str) -> Color {
    Color::from(Srgba::hex(s.trim_start_matches('#')).expect("valid authored colour"))
}
fn setup(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>) {
    let sphere = meshes.add(Sphere::new(1.0).mesh().uv(64, 32));
    commands.insert_resource(Bodies {
        mesh: sphere,
        materials: Vec::new(),
    });
    commands.spawn((
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection {
            fov: 50f32.to_radians(),
            ..default()
        }),
        Transform::default(),
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 12000.0,
            ..default()
        },
        Transform::default(),
    ));
    commands
        .spawn((
            Panel,
            Node {
                position_type: PositionType::Absolute,
                left: px(12),
                top: px(12),
                width: px(390),
                height: percent(95),
                padding: UiRect::all(px(10)),
                overflow: Overflow::scroll_y(),
                ..default()
            },
            ScrollPosition::default(),
            BackgroundColor(Color::BLACK.with_alpha(0.82)),
            ZIndex(10),
        ))
        .with_children(|p| {
            p.spawn((
                Hud,
                Text::new(""),
                TextFont {
                    font_size: FontSize::Px(13.0),
                    ..default()
                },
            ));
        });
}
fn cycle(i: usize, count: usize, step: i32) -> usize {
    (i as i32 + step).rem_euclid(count as i32) as usize
}
fn optional_cycle(value: Option<usize>, count: usize, step: i32) -> Option<usize> {
    let n = cycle(value.map_or(0, |i| i + 1), count + 1, step);
    (n > 0).then(|| n - 1)
}
fn frame_mode(frame: FrameSpec) -> usize {
    match frame {
        FrameSpec::Barycentric => 0,
        FrameSpec::BodyInertial { .. } => 1,
        FrameSpec::BodySurface { .. } => 2,
        FrameSpec::TwoBodyRotating { .. } => 3,
    }
}
fn selected_setting(ui: &Ui) -> Setting {
    SETTINGS[ui.setting]
}
fn numeric(setting: Setting) -> bool {
    matches!(
        setting,
        Setting::Coast | Setting::Start | Setting::Prograde | Setting::Normal | Setting::Radial
    )
}
fn numeric_value(lab: &OrbitLab, setting: Setting) -> Option<f64> {
    if setting == Setting::Coast {
        return Some(lab.sim.plan.coast_seconds());
    }
    let spec = lab.selected_burn.map(|i| lab.sim.plan.maneuver(i))?;
    match setting {
        Setting::Start => Some(spec.start_time),
        Setting::Prograde => Some(spec.prograde),
        Setting::Normal => Some(spec.normal),
        Setting::Radial => Some(spec.radial),
        _ => None,
    }
}
fn set_numeric(lab: &mut OrbitLab, setting: Setting, value: f64) -> Result<(), String> {
    if !value.is_finite() {
        return Err("Enter a finite number".into());
    }
    if setting == Setting::Coast {
        if !(60.0..=999.0 * DAY + 86399.0).contains(&value) {
            return Err("Coast must be 60 s through 999 d 23:59:59".into());
        }
        lab.sim.plan.set_coast_seconds(value);
        return Ok(());
    }
    if setting == Setting::Start && !(0.0..=999.0 * DAY + 86399.0).contains(&value) {
        return Err("Start must be 0 s through 999 d 23:59:59".into());
    }
    lab.edit_burn(|spec| match setting {
        Setting::Start => spec.start_time = value,
        Setting::Prograde => spec.prograde = value,
        Setting::Normal => spec.normal = value,
        Setting::Radial => spec.radial = value,
        _ => panic!("not numeric"),
    })
}
fn change(lab: &mut OrbitLab, ui: &mut Ui, step: i32, factor: f64) -> Result<(), String> {
    let setting = selected_setting(ui);
    let n = lab.sim.system.bodies.len();
    if numeric(setting) {
        let Some(old) = numeric_value(lab, setting) else {
            return Err("Select a burn first".into());
        };
        return set_numeric(lab, setting, old + step as f64 * factor);
    }
    match setting {
        Setting::System => {
            let system = if lab.system == SystemPreset::Sol {
                SystemPreset::Binary
            } else {
                SystemPreset::Sol
            };
            *lab = OrbitLab::new(system);
            ui.frame_body = lab.home;
            ui.primary = lab.home;
            ui.secondary = lab.target.unwrap();
            ui.rebuild = true;
            apply_focus(lab, ui);
        }
        Setting::Frame => {
            let mode = cycle(frame_mode(lab.frame), 4, step);
            lab.set_frame(match mode {
                0 => FrameSpec::Barycentric,
                1 => FrameSpec::BodyInertial {
                    body: ui.frame_body,
                },
                2 => FrameSpec::BodySurface {
                    body: ui.frame_body,
                },
                3 => FrameSpec::TwoBodyRotating {
                    primary: ui.primary,
                    secondary: ui.secondary,
                },
                _ => unreachable!(),
            });
        }
        Setting::FrameBody => {
            ui.frame_body = cycle(ui.frame_body, n, step);
            match lab.frame {
                FrameSpec::BodyInertial { .. } => lab.set_frame(FrameSpec::BodyInertial {
                    body: ui.frame_body,
                }),
                FrameSpec::BodySurface { .. } => lab.set_frame(FrameSpec::BodySurface {
                    body: ui.frame_body,
                }),
                _ => {}
            }
        }
        Setting::Primary | Setting::Secondary => {
            let (old, other) = if setting == Setting::Primary {
                (ui.primary, ui.secondary)
            } else {
                (ui.secondary, ui.primary)
            };
            let mut next = cycle(old, n, step);
            if next == other {
                next = cycle(next, n, step);
            }
            if setting == Setting::Primary {
                ui.primary = next;
            } else {
                ui.secondary = next;
            }
            if matches!(lab.frame, FrameSpec::TwoBodyRotating { .. }) {
                lab.set_frame(FrameSpec::TwoBodyRotating {
                    primary: ui.primary,
                    secondary: ui.secondary,
                });
            }
        }
        Setting::Focus => {
            let old = match lab.focus {
                Focus::Vessel => 0,
                Focus::Body(i) => i + 1,
            };
            let new = cycle(old, n + 1, step);
            lab.set_focus(if new == 0 {
                Focus::Vessel
            } else {
                Focus::Body(new - 1)
            });
            apply_focus(lab, ui);
        }
        Setting::Trail => {
            let i = TRAIL_SPANS
                .iter()
                .position(|&v| v == lab.trail_span)
                .unwrap();
            lab.set_spans(
                TRAIL_SPANS[cycle(i, TRAIL_SPANS.len(), step)],
                lab.vessel_span,
            );
        }
        Setting::Vessel => {
            let i = VESSEL_SPANS
                .iter()
                .position(|&v| v == lab.vessel_span)
                .unwrap();
            lab.set_spans(
                lab.trail_span,
                VESSEL_SPANS[cycle(i, VESSEL_SPANS.len(), step)],
            );
        }
        Setting::Prediction => {
            let i = PREDICTION_SPANS
                .iter()
                .position(|&v| v == lab.sim.prediction_horizon_seconds())
                .unwrap();
            lab.sim.set_prediction_horizon_seconds(
                PREDICTION_SPANS[cycle(i, PREDICTION_SPANS.len(), step)],
            );
        }
        Setting::Reference => {
            lab.sim.reference_choice = optional_cycle(lab.sim.reference_choice, n, step)
        }
        Setting::StartPlane => {
            let choices = lab.start_planes();
            let current = lab.sim.vessel_start().plane;
            let i = choices.iter().position(|p| p == &current).unwrap();
            lab.set_start_plane(choices[cycle(i, choices.len(), step)].clone());
        }
        Setting::Target => lab.target = optional_cycle(lab.target, n, step),
        Setting::Burn => {
            lab.selected_burn = optional_cycle(lab.selected_burn, lab.sim.plan.count(), step)
        }
        Setting::BurnReference => {
            let Some(i) = lab.selected_burn else {
                return Err("Select a burn first".into());
            };
            let spec = lab.sim.plan.maneuver(i);
            let previous =
                (spec.reference_mode == ReferenceMode::Fixed).then_some(spec.reference_body);
            let next = optional_cycle(previous, n, step);
            lab.edit_burn(|s| {
                s.reference_mode = if next.is_some() {
                    ReferenceMode::Fixed
                } else {
                    ReferenceMode::Auto
                };
                if let Some(i) = next {
                    s.reference_body = i;
                }
            })?;
        }
        _ => unreachable!(),
    }
    Ok(())
}
#[allow(clippy::too_many_arguments)]
fn input(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    window: Single<&Window, With<PrimaryWindow>>,
    time: Res<Time>,
    mut events: MessageReader<KeyboardInput>,
    mut lab: ResMut<Lab>,
    mut ui: ResMut<Ui>,
    mut view: ResMut<View>,
    labels: Query<(&Interaction, &Label), Changed<Interaction>>,
    mut panel: Single<&mut ScrollPosition, With<Panel>>,
) {
    if !window.focused {
        events.clear();
        return;
    }
    if ui.edit.is_some() {
        for event in events.read() {
            if !event.state.is_pressed() {
                continue;
            }
            match &event.logical_key {
                Key::Character(text) => {
                    if text
                        .chars()
                        .all(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'e' | 'E'))
                    {
                        ui.edit.as_mut().unwrap().push_str(text);
                    }
                }
                Key::Backspace => {
                    ui.edit.as_mut().unwrap().pop();
                }
                Key::Escape => ui.edit = None,
                Key::Enter => {
                    let text = ui.edit.take().unwrap();
                    let result = text
                        .parse::<f64>()
                        .map_err(|_| "Expected a number".into())
                        .and_then(|v| set_numeric(&mut lab.0, selected_setting(&ui), v));
                    ui.notice = result.err().unwrap_or_default();
                    view.0.invalidate();
                }
                _ => {}
            }
            if ui.edit.is_none() {
                break;
            }
        }
        return;
    }
    events.clear();
    if keys.just_pressed(KeyCode::ArrowUp) {
        ui.setting = cycle(ui.setting, SETTINGS.len(), -1);
    }
    if keys.just_pressed(KeyCode::ArrowDown) {
        ui.setting = cycle(ui.setting, SETTINGS.len(), 1);
    }
    if keys.just_pressed(KeyCode::Enter) && numeric(selected_setting(&ui)) {
        if numeric_value(&lab.0, selected_setting(&ui)).is_some() {
            ui.edit = Some(String::new());
        } else {
            ui.notice = "Select a burn first".into();
        }
        return;
    }
    let alt = keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]);
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    for (key, step) in [(KeyCode::ArrowLeft, -1), (KeyCode::ArrowRight, 1)] {
        if keys.just_pressed(key) {
            ui.notice = change(
                &mut lab.0,
                &mut ui,
                step,
                if alt {
                    100.0
                } else if shift {
                    10.0
                } else {
                    1.0
                },
            )
            .err()
            .unwrap_or_default();
            view.0.invalidate();
        }
    }
    if keys.just_pressed(KeyCode::Space) {
        lab.0.toggle_pause();
    }
    if keys.just_pressed(KeyCode::Comma) {
        lab.0.warp = lab.0.warp.saturating_sub(1);
        lab.0.warp_target = None;
    }
    if keys.just_pressed(KeyCode::Period) {
        lab.0.warp = (lab.0.warp + 1).min(WARPS.len() - 1);
        lab.0.warp_target = None;
    }
    if keys.just_pressed(KeyCode::KeyR) {
        lab.0.reset_vessel();
        view.0.invalidate();
    }
    for (key, mode) in [
        (KeyCode::Digit1, AttitudeMode::Prograde),
        (KeyCode::Digit2, AttitudeMode::Retrograde),
        (KeyCode::Digit3, AttitudeMode::Normal),
        (KeyCode::Digit4, AttitudeMode::Antinormal),
        (KeyCode::Digit5, AttitudeMode::RadialOut),
        (KeyCode::Digit6, AttitudeMode::RadialIn),
        (KeyCode::Digit7, AttitudeMode::Hold),
    ] {
        if keys.just_pressed(key) {
            lab.0.sim.set_attitude(mode);
        }
    }
    let can_throttle = lab.0.sim.impact.is_none() && lab.0.sim.executing_burn().is_none();
    if can_throttle {
        let down = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
        let delta = shift as u8 as f64 - down as u8 as f64;
        lab.0.sim.throttle =
            (lab.0.sim.throttle + delta * 0.5 * f64::from(time.delta_secs())).clamp(0.0, 1.0);
        if keys.just_pressed(KeyCode::KeyZ) {
            lab.0.sim.throttle = 1.0;
        }
    }
    if keys.just_pressed(KeyCode::KeyX) {
        lab.0.sim.throttle = 0.0;
    }
    for (key, action) in [
        (KeyCode::KeyN, 0),
        (KeyCode::Delete, 1),
        (KeyCode::KeyY, 2),
        (KeyCode::KeyU, 3),
        (KeyCode::KeyB, 4),
    ] {
        if keys.just_pressed(key) {
            let result = match action {
                0 => lab.0.add_burn(),
                1 => lab.0.remove_burn(),
                2 => lab.0.snap_burn(ApsisKind::Periapsis),
                3 => lab.0.snap_burn(ApsisKind::Apoapsis),
                4 => lab.0.warp_to_burn(),
                _ => unreachable!(),
            };
            ui.notice = result.err().unwrap_or_default();
            view.0.invalidate();
        }
    }
    if keys.just_pressed(KeyCode::Tab) {
        ui.notice = change_focus(&mut lab.0, if shift { -1 } else { 1 });
        apply_focus(&lab.0, &mut ui);
    }
    for (interaction, label) in &labels {
        if *interaction == Interaction::Pressed
            && let Some(focus) = label.focus
        {
            lab.0.set_focus(focus);
            apply_focus(&lab.0, &mut ui);
        }
    }
    if keys.just_pressed(KeyCode::KeyF) {
        ui.distance = match lab.0.focus {
            Focus::Vessel => 1000.0,
            Focus::Body(i) => lab.0.sim.system.bodies[i].radius_meters * SCALE * 3.0,
        }
        .clamp(0.01, 5e10);
    }
    let over_ui = window.cursor_position().is_some_and(|p| p.x < 415.0);
    if over_ui {
        panel.y = (panel.y - scroll.delta.y * 30.0).max(0.0);
    }
    if !over_ui && (buttons.pressed(MouseButton::Left) || buttons.pressed(MouseButton::Right)) {
        ui.azimuth -= f64::from(motion.delta.x) * 0.005;
        ui.elevation = (ui.elevation + f64::from(motion.delta.y) * 0.005).clamp(-1.55, 1.55);
    }
    if !over_ui {
        let pixels = match scroll.unit {
            MouseScrollUnit::Line => -f64::from(scroll.delta.y) * 100.0,
            MouseScrollUnit::Pixel => -f64::from(scroll.delta.y),
        };
        ui.distance = (ui.distance * (pixels * 0.0012).exp()).clamp(min_distance(&lab.0), 5e10);
    }
}
fn min_distance(lab: &OrbitLab) -> f64 {
    match lab.focus {
        Focus::Vessel => 0.01,
        Focus::Body(i) => lab.sim.system.bodies[i].radius_meters * SCALE * 1.05,
    }
}
fn apply_focus(lab: &OrbitLab, ui: &mut Ui) {
    ui.distance = match lab.focus {
        Focus::Vessel => ui.distance.min(20000.0),
        Focus::Body(i) => lab.sim.system.bodies[i].radius_meters * SCALE * 4.0,
    }
    .clamp(min_distance(lab), 5e10);
}
fn change_focus(lab: &mut OrbitLab, step: i32) -> String {
    let n = lab.sim.system.bodies.len();
    let old = match lab.focus {
        Focus::Vessel => 0,
        Focus::Body(i) => i + 1,
    };
    let i = cycle(old, n + 1, step);
    lab.set_focus(if i == 0 {
        Focus::Vessel
    } else {
        Focus::Body(i - 1)
    });
    String::new()
}
fn simulate(time: Res<Time>, window: Single<&Window, With<PrimaryWindow>>, mut lab: ResMut<Lab>) {
    if window.focused {
        lab.0.tick(f64::from(time.delta_secs()));
    }
}
fn plot(mut lab: ResMut<Lab>, mut view: ResMut<View>, mut scene: ResMut<Scene>) {
    scene.0 = Some(view.0.update(&mut lab.0));
}
fn rebuild(
    mut commands: Commands,
    lab: Res<Lab>,
    mut ui: ResMut<Ui>,
    scene: Res<Scene>,
    mut bodies: ResMut<Bodies>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    old: Query<Entity, With<BodyVisual>>,
) {
    if ui.rebuild {
        for entity in &old {
            commands.entity(entity).despawn();
        }
        for material in bodies.materials.drain(..) {
            materials.remove(material.id());
        }
        for body in &lab.0.sim.system.bodies {
            let material = materials.add(StandardMaterial {
                base_color: hex(&body.color),
                unlit: body.parent_index.is_none() || body.mass_kg > 1e29,
                perceptual_roughness: 0.9,
                ..default()
            });
            bodies.materials.push(material.clone());
            commands.spawn((
                BodyVisual(body.index),
                Mesh3d(bodies.mesh.clone()),
                MeshMaterial3d(material),
                Transform::default(),
            ));
        }
        ui.rebuild = false;
    }
    let scene = scene.0.as_ref().expect("scene calculated before labels");
    while ui.labels.len() < scene.markers.len() {
        let slot = ui.labels.len();
        let text = commands
            .spawn((
                LabelText,
                Text::new(""),
                TextFont {
                    font_size: FontSize::Px(12.0),
                    ..default()
                },
                TextShadow {
                    offset: Vec2::ONE,
                    color: Color::BLACK,
                },
            ))
            .id();
        let marker = commands
            .spawn((
                Label { slot, focus: None },
                Button,
                Node {
                    position_type: PositionType::Absolute,
                    ..default()
                },
                Visibility::Hidden,
            ))
            .id();
        commands.entity(marker).add_child(text);
        ui.labels.push((marker, text));
    }
}
#[allow(clippy::too_many_arguments)]
type CameraFilter = (With<Camera3d>, Without<BodyVisual>);
type LightFilter = (
    With<DirectionalLight>,
    Without<BodyVisual>,
    Without<Camera3d>,
);
type LabelFilter = (With<LabelText>, Without<Hud>);

// Bevy injects each query/resource as a system parameter.
#[allow(clippy::too_many_arguments)]
fn draw(
    lab: Res<Lab>,
    ui: Res<Ui>,
    scene: Res<Scene>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut camera: Single<(&Camera, &mut Transform, &mut Projection), CameraFilter>,
    mut bodies: Query<(&BodyVisual, &mut Transform), Without<Camera3d>>,
    mut lights: Query<&mut Transform, LightFilter>,
    mut labels: Query<(&mut Label, &mut Node, &mut Visibility)>,
    mut texts: Query<(&mut Text, &mut TextColor), LabelFilter>,
    mut hud: Single<&mut Text, (With<Hud>, Without<LabelText>)>,
    mut gizmos: Gizmos,
) {
    let scene = scene.0.as_ref().expect("scene calculated before drawing");
    let ui = &*ui;
    let model = &lab.0;
    let dir = DVec3::new(
        ui.elevation.cos() * ui.azimuth.cos(),
        ui.elevation.cos() * ui.azimuth.sin(),
        ui.elevation.sin(),
    );
    *camera.1 =
        Transform::from_translation((dir * ui.distance).as_vec3()).looking_at(Vec3::ZERO, Vec3::Z);
    if let Projection::Perspective(p) = &mut *camera.2 {
        p.near = (ui.distance * 1e-4) as f32;
        p.far = (ui.distance * 1e9) as f32;
    }
    for (body, mut transform) in &mut bodies {
        let position = &scene.bodies[body.0];
        *transform = Transform::from_translation((position.position * SCALE).as_vec3()).with_scale(
            Vec3::splat((model.sim.system.bodies[body.0].radius_meters * SCALE) as f32),
        );
    }
    // Uniform light is the plain orbit diagnostic scene; reference lines expose each body's spin.
    if let Some(sun) = scene
        .bodies
        .iter()
        .find(|b| model.sim.system.bodies[b.index].parent_index.is_none())
    {
        let ray = -sun.position.normalize();
        if ray.is_finite() {
            for mut light in &mut lights {
                *light = Transform::default().looking_to(ray.as_vec3(), Vec3::Z);
            }
        }
    }
    for body in &scene.bodies {
        let radius = model.sim.system.bodies[body.index].radius_meters * SCALE * 1.003;
        let center = body.position * SCALE;
        if radius / ui.distance * f64::from(window.height()) > 2.0 {
            let equator = (0..=128).map(|i| {
                let a = i as f64 / 128.0 * std::f64::consts::TAU;
                (center + radius * (a.cos() * body.axes[0] + a.sin() * body.axes[1])).as_vec3()
            });
            gizmos.linestrip(equator, Color::WHITE.with_alpha(0.55));
            let meridian = (0..=128).map(|i| {
                let a = -std::f64::consts::FRAC_PI_2 + i as f64 / 128.0 * std::f64::consts::PI;
                (center + radius * (a.cos() * body.axes[0] + a.sin() * body.axes[2])).as_vec3()
            });
            gizmos.linestrip(meridian, Color::WHITE.with_alpha(0.55));
        }
    }
    for path in &scene.paths {
        gizmos.linestrip_gradient(path.points.iter().map(|&(t, p)| {
            let (color, shade) = path.shade(t);
            let rgb = hex(color).to_linear();
            (
                (p * SCALE).as_vec3(),
                Color::linear_rgb(
                    rgb.red * shade as f32,
                    rgb.green * shade as f32,
                    rgb.blue * shade as f32,
                ),
            )
        }));
    }
    if let Some(d) = scene.thrust {
        let from = (scene.vessel * SCALE).as_vec3();
        gizmos.arrow(
            from,
            from + (d * ui.distance * 0.12).as_vec3(),
            if model.sim.effective_throttle() > 0.0 {
                hex("#ff9a3c")
            } else {
                hex("#8a7a6a")
            },
        );
    }
    let mut shown: Vec<(f32, f32, f32)> = Vec::new();
    let projected: Vec<_> = scene
        .markers
        .iter()
        .map(|marker| {
            let at = project(
                marker.position * SCALE,
                dir,
                ui.distance,
                window.width(),
                window.height(),
            )?;
            let width = marker.text.chars().count() as f32 * 7.0 + 14.0;
            let crowded = shown.iter().any(|&(x, y, w)| {
                (at.y - y).abs() < 13.0
                    && if at.x >= x {
                        at.x - x < w
                    } else {
                        x - at.x < width
                    }
            });
            if !crowded {
                shown.push((at.x, at.y, width));
            }
            Some((at, crowded))
        })
        .collect();
    for (mut label, mut node, mut visibility) in &mut labels {
        let Some(marker) = scene.markers.get(label.slot) else {
            *visibility = Visibility::Hidden;
            label.focus = None;
            continue;
        };
        let Some((at, crowded)) = projected[label.slot] else {
            *visibility = Visibility::Hidden;
            label.focus = None;
            continue;
        };
        label.focus = marker.focus;
        node.left = px(at.x);
        node.top = px(at.y - 7.0);
        *visibility = Visibility::Visible;
        let (mut text, mut colour) = texts
            .get_mut(ui.labels[label.slot].1)
            .expect("label text exists");
        text.0 = if crowded {
            if marker.ring { "o" } else { "." }.into()
        } else {
            format!("{} {}", if marker.ring { "o" } else { "." }, marker.text)
        };
        colour.0 = hex(&marker.color);
    }
    hud.0 = hud_text(model, ui, scene);
}
fn project(point: DVec3, dir: DVec3, distance: f64, width: f32, height: f32) -> Option<Vec2> {
    let forward = -dir;
    let right = forward.cross(DVec3::Z).normalize();
    let up = right.cross(forward);
    let relative = point - dir * distance;
    let depth = relative.dot(forward);
    if depth < distance * 1e-4 || depth > distance * 1e9 {
        return None;
    }
    let focal = f64::from(height) / (2.0 * 25f64.to_radians().tan());
    let x = f64::from(width) * 0.5 + relative.dot(right) * focal / depth;
    let y = f64::from(height) * 0.5 - relative.dot(up) * focal / depth;
    if x < 0.0 || x > f64::from(width) || y < 0.0 || y > f64::from(height) {
        return None;
    }
    Some(Vec2::new(x as f32, y as f32))
}
fn setting_text(lab: &OrbitLab, ui: &Ui, setting: Setting) -> (&'static str, String) {
    let name = |i: usize| lab.sim.system.bodies[i].name.clone();
    match setting {
        Setting::System => ("System", format!("{:?}", lab.system)),
        Setting::Frame => (
            "Plot frame",
            match lab.frame {
                FrameSpec::Barycentric => "barycentric",
                FrameSpec::BodyInertial { .. } => "body-inertial",
                FrameSpec::BodySurface { .. } => "body-surface",
                FrameSpec::TwoBodyRotating { .. } => "two-body-rotating",
            }
            .into(),
        ),
        Setting::FrameBody => ("Frame body", name(ui.frame_body)),
        Setting::Primary => ("Pair primary", name(ui.primary)),
        Setting::Secondary => ("Pair secondary", name(ui.secondary)),
        Setting::Focus => (
            "Focus",
            match lab.focus {
                Focus::Vessel => "Vessel".into(),
                Focus::Body(i) => name(i),
            },
        ),
        Setting::Trail => ("Body history", duration(lab.trail_span)),
        Setting::Vessel => ("Vessel history", duration(lab.vessel_span)),
        Setting::Prediction => ("Prediction", duration(lab.sim.prediction_horizon_seconds())),
        Setting::Reference => (
            "Navigation ref",
            lab.sim.reference_choice.map_or("automatic".into(), name),
        ),
        Setting::StartPlane => (
            "Start plane",
            match lab.sim.vessel_start().plane {
                StartPlane::Equatorial { .. } => "equator".into(),
                StartPlane::OrbitOf { body_id } => format!("orbit of {body_id}"),
            },
        ),
        Setting::Coast => (
            "Plan coast (s)",
            format!("{:.1}", lab.sim.plan.coast_seconds()),
        ),
        Setting::Target => ("Plan target", lab.target.map_or("none".into(), name)),
        Setting::Burn => (
            "Selected burn",
            lab.selected_burn
                .map_or("none".into(), |i| format!("#{}", i + 1)),
        ),
        Setting::BurnReference => (
            "Burn reference",
            lab.selected_burn.map_or("none".into(), |i| {
                let spec = lab.sim.plan.maneuver(i);
                format!("{} / {:?}", name(spec.reference_body), spec.reference_mode)
            }),
        ),
        Setting::Start | Setting::Prograde | Setting::Normal | Setting::Radial => {
            let title = match setting {
                Setting::Start => "Start T+ (s)",
                Setting::Prograde => "Prograde (m/s)",
                Setting::Normal => "Normal (m/s)",
                Setting::Radial => "Radial (m/s)",
                _ => unreachable!(),
            };
            (
                title,
                numeric_value(lab, setting).map_or("none".into(), |v| format!("{v:.3}")),
            )
        }
    }
}
fn hud_text(lab: &OrbitLab, ui: &Ui, scene: &PlotScene) -> String {
    let sim = &lab.sim;
    let mut text = format!(
        "VOID / ORBIT LAB\nT+ {} | {} | warp {:.0}x\n{}\n\n",
        duration(sim.time),
        if lab.paused { "PAUSED" } else { "RUNNING" },
        WARPS[lab.warp],
        if !lab.last_report.completed {
            format!("LAGGING: achieved {:.2}x", lab.achieved_warp)
        } else {
            String::new()
        }
    );
    for (i, &setting) in SETTINGS.iter().enumerate() {
        let (name, value) = setting_text(lab, ui, setting);
        text.push_str(&format!(
            "{} {name:<17} {value}\n",
            if ui.setting == i { ">" } else { " " }
        ));
    }
    if let Some(edit) = &ui.edit {
        text.push_str(&format!(
            "\nEnter value: {edit}_\nEnter apply | Esc cancel\n"
        ));
    }
    text.push_str("\nUp/down setting | Left/right change\nEnter numeric value | Shift x10 / Alt x100\nN add | Del remove | Y Pe | U Ap\nB warp to burn (stops 30 s before)\n1..7 attitude | Z full | X cut\nShift/Ctrl throttle | Space pause | R reset\nTab focus | F fit | click body/vessel label\nDrag orbit | wheel zoom | , . warp\n");
    text.push_str(&format!("\nAttitude {} | throttle {:.1}%\nMass {:.3} t | fuel {:.3} t\nDelta-v left {:.1} m/s\nEphemeris {:.2} MiB / step {:.1} s\ncovered {} .. {}\n{} steps last frame\n",sim.attitude_mode().label(),sim.effective_throttle()*100.0,sim.vessel().mass_kg/1000.0,sim.fuel_kg()/1000.0,sim.delta_v_remaining(),sim.ephemeris.retained_bytes() as f64/1048576.0,sim.ephemeris.step_seconds(),duration(sim.ephemeris.start_time()),duration(sim.ephemeris.end_time()),lab.last_report.steps));
    if let Some(impact) = sim.impact {
        text.push_str(&format!(
            "IMPACT {} at {}\n",
            sim.system.bodies[impact.body_index].name,
            duration(impact.time)
        ));
    } else {
        let reference = sim.navigation_reference();
        let body = &sim.system.bodies[reference];
        let n = sim.system.bodies.len();
        let mut positions = vec![DVec3::ZERO; n];
        let mut velocities = positions.clone();
        sim.ephemeris
            .states_at(sim.time, &mut positions, Some(&mut velocities));
        let center = (positions[reference], velocities[reference]);
        let r = sim.vessel().position - center.0;
        let v = sim.vessel().velocity - center.1;
        let osc = osculating_orbit(r, v, body.gm);
        let inclination = (r
            .cross(v)
            .normalize()
            .dot(body.rotation.axis())
            .clamp(-1.0, 1.0))
        .acos()
        .to_degrees();
        text.push_str(&format!("Ref {} | altitude {}\nspeed {:.1} m/s | Pe {} | Ap {}\ne {:.6} | i {:.3} equator / {:.3} ecliptic\n",body.name,distance(r.length()-body.radius_meters),v.length(),distance(osc.periapsis_radius_meters-body.radius_meters),if osc.apoapsis_radius_meters.is_finite(){distance(osc.apoapsis_radius_meters-body.radius_meters)}else{"escape".into()},osc.eccentricity,inclination,osc.inclination_radians.to_degrees()));
    }
    for i in 0..sim.plan.count() {
        let spec = sim.plan.maneuver(i);
        let status = match sim.plan.status(i) {
            Ok(b) => format!(
                "{:.1} m/s / {:.1} s / {:.3} t left",
                b.delta_v,
                b.end_time - b.start_time,
                (b.mass_after_kg - sim.engine.dry_mass_kg) / 1000.0
            ),
            Err(reason) => format!("BLOCKED: {reason}"),
        };
        text.push_str(&format!(
            "#{} {} in {}: {status}\n",
            i + 1,
            if i == 0 && sim.executing_burn().is_some() {
                "BURNING"
            } else {
                ""
            },
            duration(spec.start_time - sim.time)
        ));
    }
    if sim.plan.count() > 0 {
        text.push_str(&format!(
            "Plan computed {} {}\n",
            duration(sim.plan.computed_until()),
            if sim.plan.complete() {
                "complete"
            } else {
                "..."
            }
        ));
    }
    if let Some(gap) = scene.target_gap {
        text.push_str(&format!("Plan end above target: {}\n", distance(gap)));
    }
    if let Some(target) = lab.warp_target {
        text.push_str(&format!("Warp to T+ {}\n", duration(target)));
    }
    text.push_str(&format!("\n{}", ui.notice));
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn systems_have_disjoint_queries() {
        let mut world = World::new();
        let mut schedule = Schedule::default();
        schedule.add_systems((input, simulate, plot, rebuild, draw).chain());
        schedule
            .initialize(&mut world)
            .expect("orbit lab system access");
    }
    #[test]
    fn exact_numeric_edits_and_every_setting_are_reachable() {
        let mut lab = OrbitLab::new(SystemPreset::Sol);
        lab.add_burn().unwrap();
        let mut ui = Ui {
            setting: 0,
            edit: None,
            notice: String::new(),
            distance: 60000.0,
            azimuth: 0.0,
            elevation: 0.45,
            frame_body: lab.home,
            primary: lab.home,
            secondary: lab.target.unwrap(),
            rebuild: false,
            labels: Vec::new(),
        };
        set_numeric(&mut lab, Setting::Start, 1234.567).unwrap();
        set_numeric(&mut lab, Setting::Prograde, -123.456).unwrap();
        set_numeric(&mut lab, Setting::Coast, 3600.5).unwrap();
        assert_eq!(lab.sim.plan.maneuver(0).start_time, 1234.567);
        assert_eq!(lab.sim.plan.maneuver(0).prograde, -123.456);
        assert_eq!(lab.sim.plan.coast_seconds(), 3600.5);
        assert!(set_numeric(&mut lab, Setting::Start, -1.0).is_err());
        assert!(set_numeric(&mut lab, Setting::Normal, f64::NAN).is_err());
        for (i, setting) in SETTINGS.iter().enumerate() {
            ui.setting = i;
            if *setting == Setting::System {
                continue;
            }
            if matches!(
                setting,
                Setting::BurnReference
                    | Setting::Start
                    | Setting::Prograde
                    | Setting::Normal
                    | Setting::Radial
            ) && lab.selected_burn.is_none()
            {
                lab.add_burn().unwrap();
                lab.selected_burn = Some(0);
            }
            change(&mut lab, &mut ui, 1, 1.0).unwrap();
        }
        // System reset must replace every body-index setting before another operation.
        ui.setting = 0;
        change(&mut lab, &mut ui, 1, 1.0).unwrap();
        assert_eq!(lab.system, SystemPreset::Binary);
        assert_eq!(ui.primary, lab.home);
        assert_eq!(ui.secondary, lab.target.unwrap());
    }
    #[test]
    fn projection_uses_the_current_pose_and_has_no_frame_lag() {
        let dir = DVec3::new(0.0, -1.0, 0.0);
        assert_eq!(
            project(DVec3::ZERO, dir, 100.0, 1440.0, 960.0),
            Some(Vec2::new(720.0, 480.0))
        );
        let near = project(DVec3::X, dir, 100.0, 1440.0, 960.0).unwrap();
        let far = project(DVec3::X, dir, 200.0, 1440.0, 960.0).unwrap();
        assert!(((near.x - 720.0) / 2.0 - (far.x - 720.0)).abs() < 1e-4);
        assert!(project(dir * 101.0, dir, 100.0, 1440.0, 960.0).is_none());
    }
}
