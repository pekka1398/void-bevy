//! lab/multiscale's page, interstellar scene: three star systems a few light-years apart, placed
//! 30,000 light-years out, and a probe coasting from Aster to Beryl. One continuous world from
//! metres to light-years: everything is drawn through the frame tree into a camera hung on the
//! focus's frame (systems meet at the galaxy with exact split subtraction), then scaled to a
//! render unit of a thousandth of the camera distance, and only then goes to f32.
//!
//! P: run / pause | R: reset | N: one day | Y: +1 year | T: +10 years | 1 cluster, 2 system,
//! 3 planet, 4 beside the probe | Up / Down pick a setting, Left / Right change it | drag: orbit,
//! wheel: zoom, click a label to focus it.

use std::collections::VecDeque;

use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit};
use bevy::prelude::*;
use glam::DVec3;
use void_app::map::color as css;
use void_frames::{FrameId, Motion, SplitPosition};
use void_multiscale::*;
use void_orbit::SystemFrames;
use void_view::{OrbitCamera, ellipse_points};

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "VOID | multiscale".into(),
                resolution: (1440, 900).into(),
                ..default()
            }),
            ..default()
        }))
        .insert_resource(ClearColor(hex(0x050b12)))
        .insert_resource(GlobalAmbientLight {
            color: Color::WHITE,
            brightness: 400.0,
            ..default()
        })
        .insert_resource(Lab::new())
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (keys, mouse, simulate, rebuild, draw, panel).chain(),
        )
        .run();
}

fn hex(rgb: u32) -> Color {
    Color::srgb_u8((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

const PROBE_COLOR: u32 = 0x65e7c4;
const PLACEMENTS: &[&str] = &["30,000 ly out", "near the origin"];
const SPEEDS: &[f64] = &[0.0001, 0.001, 0.005, 0.01, 0.02, 0.03, 0.04, 0.05];
const RATES: &[(f64, &str)] = &[
    (1.0, "1x"),
    (4.0, "4x"),
    (YEAR, "1 year/s"),
    (10.0 * YEAR, "10 years/s"),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Focus {
    Probe,
    System(usize),
    Body(usize),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Setting {
    Placement,
    Speed,
    Rate,
    Focus,
    Frame,
}

const SETTINGS: [Setting; 5] = [
    Setting::Placement,
    Setting::Speed,
    Setting::Rate,
    Setting::Focus,
    Setting::Frame,
];

#[derive(Resource)]
struct Lab {
    world: CoupledWorld,
    frames: SystemFrames,
    probe: Traveller,
    placement: usize,
    speed: usize,
    rate: usize,
    focus: Focus,
    selected: usize,
    running: bool,
    pending: f64,
    orbit: OrbitCamera,
    trail: VecDeque<SplitPosition>,
    last_trail_time: f64,
    notice: String,
    rebuild: bool,
    held: Option<(KeyCode, f64)>,
}

impl Lab {
    fn new() -> Self {
        let world = wide_world(default_galaxy());
        let probe = transfer(&world, 0.02);
        let frames = world.frames(&world.ids[0]);
        let mut lab = Self {
            world,
            frames,
            probe,
            placement: 0,
            speed: 4,
            rate: 2,
            focus: Focus::System(0),
            selected: 0,
            running: false,
            pending: 0.0,
            orbit: OrbitCamera::new(DVec3::new(-0.35, -0.7, 0.62).normalize(), 8.0 * LIGHT_YEAR),
            trail: VecDeque::new(),
            last_trail_time: f64::NEG_INFINITY,
            notice: String::new(),
            rebuild: true,
            held: None,
        };
        lab.reset();
        lab
    }

    /// The lab's `reset`: a fresh world and probe from the settings, paused.
    fn reset(&mut self) {
        let galaxy = if self.placement == 0 {
            default_galaxy()
        } else {
            SplitPosition::ORIGIN
        };
        self.world = wide_world(galaxy);
        self.frames = self.world.frames(&self.world.ids[0]);
        self.probe = transfer(&self.world, SPEEDS[self.speed]);
        self.running = false;
        self.pending = 0.0;
        self.focus = Focus::System(0);
        self.orbit.distance = 8.0 * LIGHT_YEAR;
        self.trail.clear();
        self.last_trail_time = f64::NEG_INFINITY;
        self.rebuild = true;
        self.notice =
            "Paused. Try T (+10 years): about halfway the probe moves from Aster's frame to Beryl's."
                .into();
    }

    fn now(&self) -> f64 {
        self.probe.time
    }

    fn focus_choices(&self) -> Vec<Focus> {
        let mut out = vec![Focus::Probe];
        out.extend((0..self.world.ids.len()).map(Focus::System));
        out.extend((0..self.world.bodies.len()).map(Focus::Body));
        out
    }

    fn focus_name(&self, focus: Focus) -> String {
        match focus {
            Focus::Probe => "probe".into(),
            Focus::System(i) => format!("{} system", self.world.ids[i]),
            Focus::Body(i) => self.world.bodies[i].name.clone(),
        }
    }

    /// Tree frame for body/system focus. Probe focus uses a separate split galaxy anchor;
    /// it can be light-years from its system, so its position must not become a local f64.
    fn focus_frame(&self) -> (FrameId, DVec3) {
        match self.focus {
            Focus::Probe => (self.probe.state.frame, DVec3::ZERO),
            Focus::System(i) => (self.frames.systems[i], DVec3::ZERO),
            Focus::Body(i) => (self.frames.inertial[i], DVec3::ZERO),
        }
    }

    fn current_system(&self) -> usize {
        match self.focus {
            Focus::System(i) => i,
            Focus::Body(i) => self.world.membership[i].system,
            Focus::Probe => self.world.frame_system(self.probe.state.frame),
        }
    }

    fn request_advance(&mut self, seconds: f64) {
        self.running = false;
        self.pending = self.pending.max(self.now()) + seconds;
    }

    fn toggle_run(&mut self) {
        if self.running || self.pending > self.now() {
            self.running = false;
            self.pending = self.now();
        } else {
            self.running = true;
            self.pending = self.now();
        }
    }

    /// Left / Right on a setting.
    fn change(&mut self, setting: Setting, step: i32) {
        let cycle = |i: usize, n: usize| (i as i32 + step).rem_euclid(n as i32) as usize;
        match setting {
            Setting::Placement => {
                self.placement = cycle(self.placement, PLACEMENTS.len());
                self.reset();
            }
            Setting::Speed => {
                self.speed = (self.speed as i32 + step).clamp(0, SPEEDS.len() as i32 - 1) as usize;
                self.reset();
            }
            Setting::Rate => self.rate = cycle(self.rate, RATES.len()),
            Setting::Focus => {
                let choices = self.focus_choices();
                let at = choices.iter().position(|f| *f == self.focus).unwrap_or(0);
                self.focus = choices[cycle(at, choices.len())];
            }
            Setting::Frame => {
                let at = self.world.frame_system(self.probe.state.frame);
                let frame = self
                    .world
                    .system_frame(&self.world.ids[cycle(at, self.world.ids.len())]);
                self.probe.set_frame(&self.world, frame);
            }
        }
    }

    fn setting_text(&self, setting: Setting) -> (&'static str, String) {
        match setting {
            Setting::Placement => ("World placement *", PLACEMENTS[self.placement].into()),
            Setting::Speed => ("Probe speed, c *", format!("{}", SPEEDS[self.speed])),
            Setting::Rate => ("Time rate", RATES[self.rate].1.into()),
            Setting::Focus => ("Focus", self.focus_name(self.focus)),
            Setting::Frame => (
                "Probe frame",
                self.world.ids[self.world.frame_system(self.probe.state.frame)].clone(),
            ),
        }
    }
}

fn distance_text(m: f64) -> String {
    if m >= 0.01 * LIGHT_YEAR {
        format!("{:.4} ly", m / LIGHT_YEAR)
    } else if m >= AU * 0.01 {
        format!("{:.4} AU", m / AU)
    } else if m >= 10_000.0 {
        format!("{:.2} km", m / 1000.0)
    } else {
        format!("{m:.3} m")
    }
}

#[derive(Component)]
struct BodyMesh(usize);

#[derive(Component)]
struct ProbeMesh;

/// A clickable label: what it names, its dot (shown when the body is too small to draw) and text.
#[derive(Component)]
struct Marker {
    focus: Focus,
    dot: Entity,
}

#[derive(Component)]
struct Panel;

#[derive(Component)]
struct Readout;

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
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
            illuminance: 8000.0,
            ..default()
        },
        Transform::from_xyz(3.0, -4.0, 5.0).looking_at(Vec3::ZERO, Vec3::Z),
    ));
    commands.spawn((
        ProbeMesh,
        Mesh3d(meshes.add(Sphere::new(1.0).mesh().uv(12, 8))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: hex(PROBE_COLOR),
            unlit: true,
            ..default()
        })),
        Transform::default(),
    ));
    let panel = |left: bool| Node {
        position_type: PositionType::Absolute,
        top: px(12),
        left: if left { px(12) } else { Val::Auto },
        right: if left { Val::Auto } else { px(12) },
        width: px(if left { 380 } else { 420 }),
        padding: UiRect::all(px(10)),
        ..default()
    };
    let font = TextFont {
        font_size: FontSize::Px(12.0),
        ..default()
    };
    commands.spawn((
        Panel,
        Text::new(""),
        font.clone(),
        panel(true),
        BackgroundColor(Color::srgba(0.02, 0.04, 0.07, 0.85)),
    ));
    commands.spawn((
        Readout,
        Text::new(""),
        font,
        panel(false),
        BackgroundColor(Color::srgba(0.02, 0.04, 0.07, 0.85)),
    ));
}

/// The bodies' spheres and every label, after a reset.
#[allow(clippy::type_complexity)]
fn rebuild(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    old: Query<Entity, Or<(With<BodyMesh>, With<Marker>)>>,
    mut lab: ResMut<Lab>,
) {
    if !lab.rebuild {
        return;
    }
    lab.rebuild = false;
    for e in &old {
        commands.entity(e).despawn();
    }
    let sphere = meshes.add(Sphere::new(1.0).mesh().uv(32, 24));
    let mut labels = vec![(Focus::Probe, "Probe".to_string(), hex(PROBE_COLOR))];
    for (i, body) in lab.world.bodies.iter().enumerate() {
        let color = css(&body.color);
        let star = body.parent_index.is_none();
        commands.spawn((
            BodyMesh(i),
            Mesh3d(sphere.clone()),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: color,
                unlit: star,
                perceptual_roughness: 0.8,
                ..default()
            })),
            Transform::default(),
        ));
        labels.push((Focus::Body(i), body.name.clone(), color));
    }
    for (focus, name, color) in labels {
        let dot = commands
            .spawn((
                Node {
                    width: px(5),
                    height: px(5),
                    border_radius: BorderRadius::MAX,
                    ..default()
                },
                BackgroundColor(color),
            ))
            .id();
        let text = commands
            .spawn((
                Text::new(name),
                TextFont {
                    font_size: FontSize::Px(11.0),
                    ..default()
                },
                TextShadow {
                    offset: Vec2::ONE,
                    color: Color::BLACK.with_alpha(0.8),
                },
            ))
            .id();
        commands
            .spawn((
                Marker { focus, dot },
                Button,
                Node {
                    position_type: PositionType::Absolute,
                    align_items: AlignItems::Center,
                    column_gap: px(4),
                    ..default()
                },
                Visibility::Hidden,
            ))
            .add_children(&[dot, text]);
    }
}

fn keys(time: Res<Time>, input: Res<ButtonInput<KeyCode>>, mut lab: ResMut<Lab>) {
    if input.just_pressed(KeyCode::KeyP) {
        lab.toggle_run();
    }
    if input.just_pressed(KeyCode::KeyR) {
        lab.reset();
    }
    if input.just_pressed(KeyCode::KeyN) {
        lab.request_advance(86400.0);
    }
    if input.just_pressed(KeyCode::KeyY) {
        lab.request_advance(YEAR);
    }
    if input.just_pressed(KeyCode::KeyT) {
        lab.request_advance(10.0 * YEAR);
    }
    // The lab's zoom buttons.
    if input.just_pressed(KeyCode::Digit1) {
        lab.focus = Focus::System(0);
        lab.orbit.distance = 8.0 * LIGHT_YEAR;
    }
    if input.just_pressed(KeyCode::Digit2) {
        lab.focus = Focus::System(lab.current_system());
        lab.orbit.distance = 3.0 * AU;
    }
    if input.just_pressed(KeyCode::Digit3) {
        let system = lab.world.ids[lab.current_system()].clone();
        let index = lab
            .world
            .bodies
            .iter()
            .position(|b| b.id == format!("{system}/planet"))
            .expect("every system has a planet");
        lab.focus = Focus::Body(index);
        lab.orbit.distance = lab.world.bodies[index].radius_meters * 3.0;
    }
    if input.just_pressed(KeyCode::Digit4) {
        lab.focus = Focus::Probe;
        lab.orbit.distance = 18.0;
    }
    if input.just_pressed(KeyCode::ArrowDown) {
        lab.selected = (lab.selected + 1) % SETTINGS.len();
    }
    if input.just_pressed(KeyCode::ArrowUp) {
        lab.selected = (lab.selected + SETTINGS.len() - 1) % SETTINGS.len();
    }
    // Left / Right: once on press, then repeating while held.
    let dt = time.delta_secs_f64();
    for (key, step) in [(KeyCode::ArrowLeft, -1), (KeyCode::ArrowRight, 1)] {
        let fire = if input.just_pressed(key) {
            lab.held = Some((key, -0.35));
            true
        } else if input.pressed(key)
            && let Some((held, wait)) = lab.held
            && held == key
        {
            let wait = wait + dt;
            let fire = wait >= 0.0;
            lab.held = Some((key, if fire { wait - 0.12 } else { wait }));
            fire
        } else {
            false
        };
        if fire {
            let setting = SETTINGS[lab.selected];
            lab.change(setting, step);
        }
    }
}

fn mouse(
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    markers: Query<(&Interaction, &Marker)>,
    mut lab: ResMut<Lab>,
) {
    let mut over_label = false;
    for (interaction, marker) in &markers {
        if *interaction != Interaction::None {
            over_label = true;
        }
        if *interaction == Interaction::Pressed && buttons.just_pressed(MouseButton::Left) {
            lab.focus = marker.focus;
        }
    }
    if buttons.pressed(MouseButton::Left) && !over_label && motion.delta != Vec2::ZERO {
        lab.orbit.drag(
            f64::from(motion.delta.x),
            f64::from(motion.delta.y),
            DVec3::Z,
        );
    }
    let pixels = match scroll.unit {
        MouseScrollUnit::Line => -f64::from(scroll.delta.y) * 100.0,
        MouseScrollUnit::Pixel => -f64::from(scroll.delta.y),
    };
    if pixels != 0.0 {
        lab.orbit.zoom(
            (pixels.clamp(-500.0, 500.0) * 0.003).exp(),
            2.0,
            30.0 * LIGHT_YEAR,
        );
    }
}

fn simulate(time: Res<Time>, mut lab: ResMut<Lab>) {
    let wall = time.delta_secs_f64().clamp(0.0, 0.05);
    if lab.running {
        let now = lab.now();
        lab.pending = lab.pending.max(now) + wall * RATES[lab.rate].0;
    }
    if lab.pending > lab.now() && lab.probe.terminal.is_none() {
        let lab = &mut *lab;
        lab.probe.advance_to(&mut lab.world, lab.pending, 24);
    }
    if let Some(terminal) = lab.probe.terminal.clone() {
        lab.running = false;
        lab.pending = lab.probe.time;
        lab.notice = terminal;
    }
    let t = lab.now();
    if t != lab.last_trail_time {
        let p = lab.probe.position(&lab.world);
        lab.trail.push_back(p);
        if lab.trail.len() > 1000 {
            lab.trail.pop_front();
        }
        lab.last_trail_time = t;
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn draw(
    lab: Res<Lab>,
    mut camera: Single<(&Camera, &mut Transform, &mut Projection)>,
    mut bodies: Query<(&BodyMesh, &mut Transform, &mut Visibility), Without<Camera>>,
    mut probe: Single<&mut Transform, (With<ProbeMesh>, Without<Camera>, Without<BodyMesh>)>,
    mut markers: Query<
        (&Marker, &mut Node, &mut Visibility),
        (Without<BodyMesh>, Without<ProbeMesh>),
    >,
    mut dots: Query<
        &mut Visibility,
        (
            Without<Marker>,
            Without<BodyMesh>,
            Without<ProbeMesh>,
            Without<Camera>,
        ),
    >,
    mut gizmos: Gizmos,
) {
    let world = &lab.world;
    let t = lab.now();
    let states = world.at(t);
    let frames = lab.frames.tree.at(t, world);
    // The camera frame: at the focus, in the galaxy's axes (shared by every system).
    let probe_anchor = matches!(lab.focus, Focus::Probe).then(|| lab.probe.position(world));
    let (focus, local) = lab.focus_frame();
    let turn = frames.transform(focus, lab.frames.systems[0]).rotation();
    let camera_frame = Motion::fixed(local, turn.inverse());
    // The render unit follows the zoom; the tree subtracts before any f32.
    let u = (lab.orbit.distance / 1000.0).max(1.0);
    let render = |from: FrameId, p: DVec3| {
        let relative = match &probe_anchor {
            Some(anchor) => frames.relative_to_galaxy_anchor(from, p, anchor),
            None => frames
                .transform(from, focus)
                .into_child(&camera_frame)
                .apply_point(p),
        };
        (relative / u).as_vec3()
    };
    let to_render = |p: &SplitPosition| {
        let relative = match &probe_anchor {
            Some(anchor) => p.relative(anchor),
            None => camera_frame.unapply_point(frames.from_galaxy(p, focus)),
        };
        (relative / u).as_vec3()
    };
    let (cam, transform, projection) = &mut *camera;
    let eye = (lab.orbit.direction * (lab.orbit.distance / u)).as_vec3();
    **transform = Transform::from_translation(eye).looking_at(Vec3::ZERO, Vec3::Z);
    if let Projection::Perspective(p) = &mut **projection {
        let distance = (lab.orbit.distance / u) as f32;
        p.near = (distance * 1e-5).max(1e-4);
        p.far = ((40.0 * LIGHT_YEAR / u) as f32).max(1e6);
    }
    let global = GlobalTransform::from(**transform);
    let height = cam.logical_viewport_size().map_or(900.0, |s| s.y) as f64;
    let screen = |p: Vec3| cam.world_to_viewport(&global, p).ok();

    // Where each label goes, and whether the body is drawn as a sphere.
    let mut placed: Vec<(Focus, Option<Vec2>, bool)> = vec![];
    for (BodyMesh(i), mut tf, mut visibility) in &mut bodies {
        let body = &world.bodies[*i];
        let at = render(lab.frames.inertial[*i], DVec3::ZERO);
        tf.translation = at;
        tf.scale = Vec3::splat((body.radius_meters / u) as f32);
        let projected = body.radius_meters / (f64::from(at.distance(eye)) * u).max(1.0) * height;
        let shown = projected > 0.4;
        *visibility = if shown {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        let mut position = screen(at);
        // At light-year zoom a planet shares its star's pixel: keep the star's label.
        if let (Some(parent), Some(p)) = (body.parent_index, position) {
            let centre = render(lab.frames.inertial[parent], DVec3::ZERO);
            if screen(centre).is_some_and(|c| c.distance(p) < 24.0) {
                position = None;
            }
        }
        placed.push((Focus::Body(*i), position, shown));
        // The planet's osculating orbit about its star, out to 500 AU of zoom.
        if let Some(parent) = body.parent_index
            && lab.orbit.distance < 500.0 * AU
        {
            let m = world.membership[*i];
            let pm = world.membership[parent];
            let g = &states[m.system];
            let r = g.body_position(m.local) - g.body_position(pm.local);
            let v = g.body_velocity(m.local) - g.body_velocity(pm.local);
            let centre = g.body_position(pm.local);
            let points = ellipse_points(r, v, body.gm + world.bodies[parent].gm, 128);
            let color = css(&body.color).with_alpha(0.25);
            let system = lab.frames.systems[m.system];
            gizmos.linestrip(
                points
                    .iter()
                    .chain(std::iter::once(&points[0]))
                    .map(|p| render(system, centre + *p)),
                color,
            );
        }
    }
    let probe_at = to_render(&lab.probe.position(world));
    probe.translation = probe_at;
    probe.scale = Vec3::splat(((lab.orbit.distance * 0.005).max(1.0) / u) as f32);
    placed.push((Focus::Probe, screen(probe_at), true));
    gizmos.linestrip(
        lab.trail.iter().map(to_render),
        hex(PROBE_COLOR).with_alpha(0.6),
    );

    let size = cam.logical_viewport_size().unwrap_or(Vec2::ONE);
    for (marker, mut node, mut visibility) in &mut markers {
        let Some((_, Some(p), shown)) = placed.iter().find(|(f, ..)| *f == marker.focus) else {
            *visibility = Visibility::Hidden;
            continue;
        };
        if p.x < 0.0 || p.y < 0.0 || p.x > size.x || p.y > size.y {
            *visibility = Visibility::Hidden;
            continue;
        }
        *visibility = Visibility::Inherited;
        node.left = px(p.x - 2.5);
        node.top = px(p.y - 7.0);
        if let Ok(mut dot) = dots.get_mut(marker.dot) {
            *dot = if *shown && marker.focus != Focus::Probe {
                Visibility::Hidden
            } else {
                Visibility::Inherited
            };
        }
    }
}

#[allow(clippy::type_complexity)]
fn panel(
    lab: Res<Lab>,
    mut texts: ParamSet<(
        Single<&mut Text, With<Panel>>,
        Single<&mut Text, With<Readout>>,
    )>,
) {
    let t = lab.now();
    let mut left = String::from(
        "VOID MULTISCALE\nFrom metres to light-years. Three star systems, one continuous world; \
         zoom and frame changes do not change the physics.\n\n01 INTERSTELLAR COAST\n\
         (02, two ships colliding 30,000 ly out, waits for the vessels port.)\n\n\
         SETTINGS  Up/Down pick, Left/Right change\n",
    );
    for (k, setting) in SETTINGS.iter().enumerate() {
        let (label, value) = lab.setting_text(*setting);
        let mark = if k == lab.selected { ">" } else { " " };
        left.push_str(&format!("{mark} {label:<20}{value}\n"));
    }
    left.push_str(
        "  * resets the scene\n\nKEYS\nP run / pause   R reset\nN +1 day   Y +1 year   T +10 years\n\
         1 star cluster   2 system   3 planet   4 beside the probe\nDrag to orbit, wheel to zoom, \
         click a label to focus.\n\nThe probe travels by the nearest system's frame and changes \
         frame on its own; you can also change it by hand and check the jump.\n\n",
    );
    left.push_str(&lab.notice);
    texts.p0().0 = left;

    let lag = lab.pending - t > (RATES[lab.rate].0 * 0.1).max(1.0);
    let state = if lag {
        "ADVANCING"
    } else if lab.running {
        "RUNNING"
    } else {
        "PAUSED"
    };
    let p = lab.probe.position(&lab.world);
    let world = &lab.world;
    let beryl = world.at(t)[world.system_index("Beryl")].origin;
    let mut right = format!(
        "{state}   NEWTONIAN, Z UP\n\nSIMULATION TIME  {:.5} yr\n{}\n\nREFERENCE FRAME  {}\n\
         to Beryl's barycentre  {}\ncamera distance  {}\n\nLOCAL PHYSICS  {} probe steps\n\
         {} celestial steps, {} samples kept\n\nSPLIT POSITION (integer cells + local metres)\n\
         CELL    {} / {} / {}\nOFFSET  {:.6} / {:.6} / {:.6} m\n\nFRAME HAND-OFFS\n",
        t / YEAR,
        if lag {
            format!("target {:.3} yr", lab.pending / YEAR)
        } else {
            "world clock, no relativity".into()
        },
        world.ids[world.frame_system(lab.probe.state.frame)],
        distance_text(p.relative(&beryl).length()),
        distance_text(lab.orbit.distance),
        lab.probe.steps,
        world.steps,
        world.sample_count(),
        p.cell[0],
        p.cell[1],
        p.cell[2],
        p.offset.x,
        p.offset.y,
        p.offset.z,
    );
    let events = &lab.probe.events;
    if events.is_empty() {
        right.push_str("none yet; change the frame by hand to check one.\n");
    }
    for e in events.iter().rev().take(6).rev() {
        right.push_str(&format!(
            "{:.3} yr  {} -> {}\n  dp {:.2e} m  dv {:.2e} m/s\n",
            e.time / YEAR,
            world.ids[world.frame_system(e.from)],
            world.ids[world.frame_system(e.to)],
            e.position_jump,
            e.velocity_jump
        ));
    }
    texts.p1().0 = right;
}
