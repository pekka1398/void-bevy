//! lab/aerodynamics's page on `void-aero`: the wind tunnel, the A-01 aircraft on Rapier and the
//! C-01 capsule's reentry. A settings list replaces the lab's form; the panels are plain text.
//! Standalone, as `void-assembly-lab`: `void-aero` reads the assembly craft, and `void-app` does
//! not depend on assembly.
//!
//! 1 / 2 / 3: tunnel, aircraft, reentry | Up / Down: pick a setting, Left / Right: change it (hold
//! to repeat, faster after a second) | R: reload | Space: pause | F: fit view | drag: orbit,
//! wheel: zoom | aircraft: W / S pitch, A / D roll, Q / E rudder, Shift / Ctrl throttle, X cut,
//! B brakes | tunnel: click the left plot to pick the angle of attack.

mod plot;

use bevy::camera::visibility::RenderLayers;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit};
use bevy::prelude::*;
use glam::{DQuat, DVec3};
use plot::{Label, Plot, PlotLabel, PlotLines, Series};
use void_aero::*;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "VOID | aerodynamics lab".into(),
                resolution: (1440, 900).into(),
                ..default()
            }),
            ..default()
        }))
        .init_gizmo_group::<PlotLines>()
        .init_gizmo_group::<OnTop>()
        .insert_resource(ClearColor(hex(0x101921)))
        .insert_resource(GlobalAmbientLight {
            color: hex(0xdcecff),
            brightness: 500.0,
            ..default()
        })
        .insert_resource(Lab::new())
        .insert_resource(OrbitView::default())
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (settings, mouse, simulate, rebuild, draw_scene, panels).chain(),
        )
        .run();
}

fn hex(rgb: u32) -> Color {
    Color::srgb_u8((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

/// Drawn over the model, as the lab's centre-of-mass marker.
#[derive(Default, Reflect, GizmoConfigGroup)]
struct OnTop;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Tunnel,
    Flight,
    Entry,
}

impl Mode {
    fn bit(self) -> u8 {
        match self {
            Self::Tunnel => 1,
            Self::Flight => 2,
            Self::Entry => 4,
        }
    }
}

const TUNNEL: u8 = 1;
const FLIGHT: u8 = 2;
const ENTRY: u8 = 4;
const ALL: u8 = 7;

#[derive(Clone, Copy)]
enum Kind {
    Number {
        min: f64,
        max: f64,
        step: f64,
        decimals: usize,
    },
    Toggle,
    Choice(&'static [&'static str]),
}

struct Field {
    key: &'static str,
    label: &'static str,
    kind: Kind,
    value: f64,
    /// Scenes that show it.
    modes: u8,
    /// Changing it reloads the scene, as the lab's air, vehicle, empty-tank and start fields.
    reload: bool,
}

fn number(
    key: &'static str,
    label: &'static str,
    modes: u8,
    (min, max, step, decimals): (f64, f64, f64, usize),
    value: f64,
) -> Field {
    Field {
        key,
        label,
        kind: Kind::Number {
            min,
            max,
            step,
            decimals,
        },
        value,
        modes,
        reload: false,
    }
}

fn toggle(key: &'static str, label: &'static str, modes: u8, on: bool, reload: bool) -> Field {
    Field {
        key,
        label,
        kind: Kind::Toggle,
        value: if on { 1.0 } else { 0.0 },
        modes,
        reload,
    }
}

fn choice(
    key: &'static str,
    label: &'static str,
    modes: u8,
    options: &'static [&'static str],
    reload: bool,
) -> Field {
    Field {
        key,
        label,
        kind: Kind::Choice(options),
        value: 0.0,
        modes,
        reload,
    }
}

const VEHICLES: &[&str] = &["A-01 aircraft", "Assembly rocket", "C-01 capsule"];
const STARTS: &[&str] = &["in flight", "runway"];
const FLIGHT_RATES: &[&str] = &["1x", "4x"];
const ENTRY_RATES: &[&str] = &["1x", "4x", "16x", "64x"];
const SURFACE: (f64, f64, f64, usize) = (-1.0, 1.0, 0.01, 2);

/// The lab's form, in its order.
fn fields() -> Vec<Field> {
    vec![
        choice("vehicle", "Vehicle", TUNNEL, VEHICLES, true),
        number(
            "altitude",
            "Altitude m",
            TUNNEL,
            (0.0, 150_000.0, 100.0, 0),
            700.0,
        ),
        number("speed", "Airspeed m/s", TUNNEL, (0.0, 9000.0, 5.0, 0), 70.0),
        number(
            "alpha",
            "Angle of attack deg",
            TUNNEL,
            (-45.0, 45.0, 0.25, 2),
            3.0,
        ),
        number("beta", "Sideslip deg", TUNNEL, (-30.0, 30.0, 0.25, 2), 0.0),
        toggle("empty", "Empty tanks", TUNNEL, false, true),
        choice("start", "Start", FLIGHT, STARTS, true),
        number("wind", "Crosswind m/s", FLIGHT, (-40.0, 40.0, 1.0, 0), 0.0),
        number("throttle", "Throttle", FLIGHT, (0.0, 1.0, 0.01, 2), 0.28),
        number("trim", "Elevator trim", FLIGHT, (-0.5, 0.5, 0.005, 3), 0.0),
        toggle("brakes", "Wheel brakes (B)", FLIGHT, false, false),
        number(
            "entry_alt",
            "Start altitude km",
            ENTRY,
            (80.0, 160.0, 1.0, 0),
            120.0,
        ),
        number(
            "entry_speed",
            "Airspeed km/s",
            ENTRY,
            (1.0, 9.0, 0.1, 1),
            7.6,
        ),
        number(
            "entry_gamma",
            "Flight path deg",
            ENTRY,
            (-40.0, -0.1, 0.1, 1),
            -2.0,
        ),
        number(
            "entry_alpha",
            "Angle of attack deg",
            ENTRY,
            (-180.0, 180.0, 1.0, 0),
            0.0,
        ),
        toggle("shield", "Heat shield and ablator", ENTRY, true, false),
        number("ablator", "Ablator kg", ENTRY, (0.0, 400.0, 5.0, 0), 130.0),
        number("elevator", "Elevator", TUNNEL | FLIGHT, SURFACE, 0.0),
        number("aileron", "Aileron", TUNNEL | FLIGHT, SURFACE, 0.0),
        number("rudder", "Rudder", TUNNEL | FLIGHT, SURFACE, 0.0),
        choice("flight_rate", "Time rate", FLIGHT, FLIGHT_RATES, false),
        choice("entry_rate", "Time rate", ENTRY, ENTRY_RATES, false),
        toggle("air", "Physical atmosphere", ALL, true, true),
        toggle("arrows", "Force arrows", ALL, true, false),
    ]
}

impl Field {
    fn text(&self) -> String {
        match self.kind {
            Kind::Number { decimals, .. } => format!("{:.*}", decimals, self.value),
            Kind::Toggle => (if self.value != 0.0 { "on" } else { "off" }).into(),
            Kind::Choice(options) => options[self.value as usize].into(),
        }
    }

    /// One step in `direction` (±1), `fast` ten at a time.
    fn nudge(&mut self, direction: f64, fast: bool) {
        match self.kind {
            Kind::Number { min, max, step, .. } => {
                let step = if fast { step * 10.0 } else { step };
                // Snap to the step grid so repeated steps stay round.
                let v = ((self.value + direction * step) / step).round() * step;
                self.value = v.clamp(min, max);
            }
            Kind::Toggle => self.value = 1.0 - self.value,
            Kind::Choice(options) => {
                let n = options.len() as f64;
                self.value = (self.value + direction).rem_euclid(n);
            }
        }
    }
}

struct Sample {
    time: f64,
    altitude: f64,
    speed: f64,
    q: f64,
    heat: f64,
}

#[derive(Resource)]
struct Lab {
    mode: Mode,
    fields: Vec<Field>,
    /// Selected field per scene, an index into `fields`.
    selected: [usize; 3],
    atmosphere: Atmosphere,
    vehicle: Vehicle,
    tunnel_data: VehicleResources,
    tunnel_loads: VehicleLoads,
    flight: Option<AircraftFlight>,
    entry: Option<EntryFlight>,
    paused: bool,
    accumulator: f64,
    history: Vec<Sample>,
    last_history: f64,
    rebuild: bool,
    fit: bool,
    /// Held arrow key: which, for how long, and when it repeats next.
    repeat: Option<(KeyCode, f64, f64)>,
    since_readout: f64,
    plots: [Plot; 2],
    left_text: String,
    right_text: String,
    head_text: String,
    terminal: Option<String>,
    part_materials: Vec<Handle<StandardMaterial>>,
}

impl Lab {
    fn new() -> Self {
        let vehicle = aircraft();
        let tunnel_data = resources(&vehicle);
        let empty_plot = || Plot {
            title: String::new(),
            series: vec![],
            x_label: "",
            left_label: "",
            right_label: "",
        };
        let mut lab = Self {
            mode: Mode::Tunnel,
            fields: fields(),
            selected: [0; 3],
            atmosphere: Atmosphere::earth(),
            tunnel_loads: evaluate_vehicle(
                &vehicle,
                &tunnel_data,
                &AeroState {
                    center: DVec3::ZERO,
                    velocity: DVec3::ZERO,
                    rotation: DQuat::IDENTITY,
                    angular_velocity: DVec3::ZERO,
                },
                &Atmosphere::Vacuum.sample(0.0),
                DVec3::ZERO,
                &NEUTRAL,
                250.0,
            ),
            vehicle,
            tunnel_data,
            flight: None,
            entry: None,
            paused: false,
            accumulator: 0.0,
            history: vec![],
            last_history: f64::NEG_INFINITY,
            rebuild: true,
            fit: true,
            repeat: None,
            since_readout: f64::INFINITY,
            plots: [empty_plot(), empty_plot()],
            left_text: String::new(),
            right_text: String::new(),
            head_text: String::new(),
            terminal: None,
            part_materials: vec![],
        };
        for mode in [Mode::Tunnel, Mode::Flight, Mode::Entry] {
            lab.selected[mode as usize] = lab.visible(mode)[0];
        }
        lab.reload();
        lab
    }

    fn field(&self, key: &str) -> &Field {
        self.fields
            .iter()
            .find(|f| f.key == key)
            .unwrap_or_else(|| panic!("no field {key}"))
    }

    fn get(&self, key: &str) -> f64 {
        self.field(key).value
    }

    fn on(&self, key: &str) -> bool {
        self.get(key) != 0.0
    }

    fn set(&mut self, key: &str, value: f64) {
        self.fields
            .iter_mut()
            .find(|f| f.key == key)
            .unwrap_or_else(|| panic!("no field {key}"))
            .value = value;
    }

    fn visible(&self, mode: Mode) -> Vec<usize> {
        (0..self.fields.len())
            .filter(|&i| self.fields[i].modes & mode.bit() != 0)
            .collect()
    }

    fn rate(&self) -> f64 {
        let (key, rates): (&str, &[f64]) = match self.mode {
            Mode::Flight => ("flight_rate", &[1.0, 4.0]),
            _ => ("entry_rate", &[1.0, 4.0, 16.0, 64.0]),
        };
        rates[self.get(key) as usize]
    }

    /// The lab's `reload`: a fresh scene from the current settings.
    fn reload(&mut self) {
        self.atmosphere = if self.on("air") {
            Atmosphere::earth()
        } else {
            Atmosphere::Vacuum
        };
        self.flight = None;
        self.entry = None;
        match self.mode {
            Mode::Flight => {
                self.vehicle = aircraft();
                let start = if self.get("start") == 0.0 {
                    FlightStart::Cruise
                } else {
                    FlightStart::Runway
                };
                self.flight = Some(AircraftFlight::new(
                    self.vehicle.clone(),
                    start,
                    self.atmosphere.clone(),
                ));
            }
            Mode::Entry => {
                self.vehicle = capsule(self.on("shield"), self.get("ablator"));
                self.entry = Some(EntryFlight::new(
                    self.vehicle.clone(),
                    EntryOptions {
                        altitude_meters: self.get("entry_alt") * 1000.0,
                        speed: self.get("entry_speed") * 1000.0,
                        flight_path_degrees: self.get("entry_gamma"),
                        angle_of_attack_degrees: self.get("entry_alpha"),
                        bank_degrees: 0.0,
                    },
                    self.atmosphere.clone(),
                ));
            }
            Mode::Tunnel => {
                self.vehicle = match self.get("vehicle") as usize {
                    0 => aircraft(),
                    1 => demo_rocket(),
                    _ => capsule(true, DEFAULT_ABLATOR_KG),
                };
                self.tunnel_data = resources(&self.vehicle);
                if self.on("empty") {
                    self.tunnel_data.fuel.iter_mut().for_each(|f| *f = 0.0);
                }
            }
        }
        self.paused = false;
        self.accumulator = 0.0;
        self.history.clear();
        self.last_history = f64::NEG_INFINITY;
        self.rebuild = true;
        self.fit = true;
        self.since_readout = f64::INFINITY;
    }

    fn data(&self) -> &VehicleResources {
        match (&self.flight, &self.entry) {
            (Some(f), _) => &f.resources,
            (_, Some(e)) => &e.resources,
            _ => &self.tunnel_data,
        }
    }

    fn loads(&self) -> &VehicleLoads {
        match (&self.flight, &self.entry) {
            (Some(f), _) => &f.loads,
            (_, Some(e)) => &e.loads,
            _ => &self.tunnel_loads,
        }
    }

    fn time(&self) -> f64 {
        match (&self.flight, &self.entry) {
            (Some(f), _) => f.time,
            (_, Some(e)) => e.time,
            _ => 0.0,
        }
    }

    fn altitude(&self) -> f64 {
        match (&self.flight, &self.entry) {
            (Some(f), _) => f.altitude(),
            (_, Some(e)) => e.altitude(),
            _ => self.get("altitude"),
        }
    }

    /// The lab's `controlCommand`: the sliders plus held keys (elevator keys and trim only in
    /// flight).
    fn controls(&self, keys: &ButtonInput<KeyCode>) -> Controls {
        let key = |k: KeyCode| if keys.pressed(k) { 1.0 } else { 0.0 };
        let flight = self.mode == Mode::Flight;
        let elevator = self.get("elevator")
            + if flight {
                self.get("trim") + key(KeyCode::KeyS) - key(KeyCode::KeyW)
            } else {
                0.0
            };
        Controls {
            elevator: elevator.clamp(-1.0, 1.0),
            aileron: (self.get("aileron") + key(KeyCode::KeyD) - key(KeyCode::KeyA))
                .clamp(-1.0, 1.0),
            rudder: (self.get("rudder") + key(KeyCode::KeyE) - key(KeyCode::KeyQ)).clamp(-1.0, 1.0),
        }
    }

    /// The tunnel's still body in the flow at `alpha` degrees; the rocket points its nose (+y)
    /// into the flow (+z).
    fn tunnel_state(&self, alpha: f64) -> AeroState {
        let (a, b, speed) = (alpha * DEG, self.get("beta") * DEG, self.get("speed"));
        AeroState {
            center: DVec3::ZERO,
            velocity: DVec3::new(
                speed * b.sin() * a.cos(),
                -speed * a.sin(),
                speed * b.cos() * a.cos(),
            ),
            rotation: if self.vehicle.id == "rocket" {
                align(DVec3::Y, DVec3::Z)
            } else {
                DQuat::IDENTITY
            },
            angular_velocity: DVec3::ZERO,
        }
    }
}

#[derive(Resource)]
struct OrbitView {
    yaw: f32,
    pitch: f32,
    distance: f32,
}

impl Default for OrbitView {
    fn default() -> Self {
        Self::looking_from(Vec3::new(12.0, 8.0, 16.0))
    }
}

impl OrbitView {
    fn looking_from(p: Vec3) -> Self {
        let distance = p.length();
        Self {
            yaw: p.x.atan2(p.z),
            pitch: (p.y / distance).asin(),
            distance,
        }
    }

    fn eye(&self) -> Vec3 {
        self.distance
            * Vec3::new(
                self.pitch.cos() * self.yaw.sin(),
                self.pitch.sin(),
                self.pitch.cos() * self.yaw.cos(),
            )
    }
}

#[derive(Component)]
struct ModelRoot;

#[derive(Component)]
struct Floor;

#[derive(Component)]
struct Earth;

#[derive(Component)]
struct AirShell;

#[derive(Component)]
struct LeftPanel;

#[derive(Component)]
struct RightPanel;

#[derive(Component)]
struct Head;

#[derive(Component)]
struct Terminal;

const PANEL: Color = Color::srgba(0.03, 0.05, 0.07, 0.82);
const LEFT_WIDTH: f32 = 330.0;
const RIGHT_WIDTH: f32 = 350.0;
const PLOT_HEIGHT: f32 = 220.0;

/// The two plot areas between the side panels, top-left origin.
fn plot_rects(window: Vec2) -> [Rect; 2] {
    let left = LEFT_WIDTH + 24.0;
    let right = window.x - RIGHT_WIDTH - 24.0;
    let top = window.y - PLOT_HEIGHT - 12.0;
    let middle = (left + right) / 2.0;
    [
        Rect::new(left, top, middle - 8.0, window.y - 12.0),
        Rect::new(middle + 8.0, top, right, window.y - 12.0),
    ]
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut gizmo_store: ResMut<GizmoConfigStore>,
) {
    commands.spawn((
        Camera3d::default(),
        Tonemapping::AcesFitted,
        Projection::Perspective(PerspectiveProjection {
            fov: 48f32.to_radians(),
            near: 0.03,
            far: 4e7,
            ..default()
        }),
        Transform::default(),
    ));
    // The plots' camera on top, so their lines are in screen pixels.
    let (config, _) = gizmo_store.config_mut::<PlotLines>();
    config.render_layers = RenderLayers::layer(1);
    config.line.width = 1.6;
    let (on_top, _) = gizmo_store.config_mut::<OnTop>();
    on_top.depth_bias = -1.0;
    on_top.line.width = 3.0;
    commands.spawn((
        Camera2d,
        Camera {
            order: 1,
            clear_color: ClearColorConfig::None,
            ..default()
        },
        RenderLayers::layer(1),
    ));
    commands.spawn((
        DirectionalLight {
            color: hex(0xffe4c5),
            illuminance: 9000.0,
            ..default()
        },
        Transform::from_xyz(20.0, 35.0, 20.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    commands.spawn((ModelRoot, Transform::default(), Visibility::default()));
    commands.spawn((
        Floor,
        Mesh3d(meshes.add(Plane3d::default().mesh().size(2e6, 2e6))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: hex(0x263b35),
            perceptual_roughness: 1.0,
            ..default()
        })),
        Transform::default(),
    ));
    commands.spawn((
        Earth,
        Mesh3d(meshes.add(Sphere::new(6_371_000.0).mesh().uv(96, 64))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: hex(0x274964),
            perceptual_roughness: 1.0,
            ..default()
        })),
        Transform::default(),
    ));
    commands.spawn((
        AirShell,
        Mesh3d(meshes.add(Sphere::new(6_371_000.0 + 80_000.0).mesh().uv(64, 32))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: hex(0x4b8cce).with_alpha(0.08),
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            cull_mode: None,
            ..default()
        })),
        Transform::default(),
    ));
    let font = |size: f32| TextFont {
        font_size: FontSize::Px(size),
        ..default()
    };
    let panel = |left: Option<f32>, right: Option<f32>, width: f32| Node {
        position_type: PositionType::Absolute,
        top: px(12),
        left: left.map_or(Val::Auto, px),
        right: right.map_or(Val::Auto, px),
        width: px(width),
        padding: UiRect::all(px(10)),
        ..default()
    };
    commands.spawn((
        LeftPanel,
        Text::new(""),
        font(12.0),
        panel(Some(12.0), None, LEFT_WIDTH),
        BackgroundColor(PANEL),
    ));
    commands.spawn((
        RightPanel,
        Text::new(""),
        font(12.0),
        panel(None, Some(12.0), RIGHT_WIDTH),
        BackgroundColor(PANEL),
    ));
    commands.spawn((
        Head,
        Text::new(""),
        font(13.0),
        TextColor(hex(0xd6e2ea)),
        Node {
            position_type: PositionType::Absolute,
            top: px(14),
            left: px(LEFT_WIDTH + 36.0),
            ..default()
        },
    ));
    commands.spawn((
        Terminal,
        Text::new(""),
        font(18.0),
        TextColor(hex(0xff6b5e)),
        Node {
            position_type: PositionType::Absolute,
            top: px(40),
            left: px(LEFT_WIDTH + 36.0),
            ..default()
        },
        Visibility::Hidden,
    ));
    for _ in 0..64 {
        commands.spawn((
            PlotLabel,
            Text::new(""),
            font(10.0),
            TextColor(Color::WHITE),
            Node {
                position_type: PositionType::Absolute,
                ..default()
            },
            Visibility::Hidden,
        ));
    }
}

fn settings(time: Res<Time>, keys: Res<ButtonInput<KeyCode>>, mut lab: ResMut<Lab>) {
    for (key, mode) in [
        (KeyCode::Digit1, Mode::Tunnel),
        (KeyCode::Digit2, Mode::Flight),
        (KeyCode::Digit3, Mode::Entry),
    ] {
        if keys.just_pressed(key) && lab.mode != mode {
            lab.mode = mode;
            lab.reload();
        }
    }
    if keys.just_pressed(KeyCode::KeyR) {
        lab.reload();
    }
    if keys.just_pressed(KeyCode::Space) && lab.mode != Mode::Tunnel {
        lab.paused = !lab.paused;
    }
    if keys.just_pressed(KeyCode::KeyF) {
        lab.fit = true;
    }
    if lab.mode == Mode::Flight {
        if keys.just_pressed(KeyCode::KeyX) {
            lab.set("throttle", 0.0);
        }
        if keys.just_pressed(KeyCode::KeyB) {
            let on = lab.on("brakes");
            lab.set("brakes", if on { 0.0 } else { 1.0 });
        }
    }
    let visible = lab.visible(lab.mode);
    let slot = lab.mode as usize;
    let at = visible
        .iter()
        .position(|&i| i == lab.selected[slot])
        .unwrap_or(0);
    if keys.just_pressed(KeyCode::ArrowDown) {
        lab.selected[slot] = visible[(at + 1) % visible.len()];
    }
    if keys.just_pressed(KeyCode::ArrowUp) {
        lab.selected[slot] = visible[(at + visible.len() - 1) % visible.len()];
    }
    // Left / Right: one step on press, then repeats while held, ten steps at a time after a second.
    let dt = time.delta_secs_f64();
    let mut steps = 0.0;
    let mut fast = false;
    for (key, direction) in [(KeyCode::ArrowLeft, -1.0), (KeyCode::ArrowRight, 1.0)] {
        if keys.just_pressed(key) {
            lab.repeat = Some((key, 0.0, 0.35));
            steps = direction;
        } else if keys.pressed(key)
            && let Some((held_key, held, next)) = lab.repeat
            && held_key == key
        {
            let held = held + dt;
            let mut next = next;
            if held >= next {
                steps = direction;
                fast = held > 1.0;
                next = held + 0.05;
            }
            lab.repeat = Some((key, held, next));
        }
    }
    if !keys.any_pressed([KeyCode::ArrowLeft, KeyCode::ArrowRight]) {
        lab.repeat = None;
    }
    if steps != 0.0 {
        let index = lab.selected[slot];
        lab.fields[index].nudge(steps, fast);
        if lab.fields[index].reload {
            lab.reload();
        }
    }
}

fn mouse(
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    window: Single<&Window>,
    mut view: ResMut<OrbitView>,
    mut lab: ResMut<Lab>,
) {
    let size = Vec2::new(window.width(), window.height());
    let cursor = window.cursor_position();
    // The lab's click on the sweep plot picks the angle of attack.
    if lab.mode == Mode::Tunnel
        && buttons.just_pressed(MouseButton::Left)
        && let Some(c) = cursor
    {
        let rect = plot_rects(size)[0];
        if rect.contains(c) {
            let (pl, pr, ..) = plot::padding(false);
            let alpha =
                -35.0 + 70.0 * f64::from(c.x - rect.min.x - pl) / f64::from(rect.width() - pl - pr);
            lab.set("alpha", (alpha.clamp(-35.0, 35.0) * 4.0).round() / 4.0);
            return;
        }
    }
    if buttons.pressed(MouseButton::Left) {
        view.yaw -= motion.delta.x * 0.006;
        view.pitch = (view.pitch + motion.delta.y * 0.006).clamp(-1.5, 1.5);
    }
    let notches = match scroll.unit {
        MouseScrollUnit::Line => scroll.delta.y,
        MouseScrollUnit::Pixel => scroll.delta.y / 40.0,
    };
    if notches != 0.0 {
        view.distance = (view.distance * 0.9_f32.powf(notches)).clamp(3.0, 1e7);
    }
    if lab.fit {
        *view = OrbitView::looking_from(if lab.mode == Mode::Entry {
            Vec3::new(8.0, 6.0, 12.0)
        } else {
            Vec3::new(12.0, 8.0, 16.0)
        });
        lab.fit = false;
    }
}

fn simulate(time: Res<Time>, keys: Res<ButtonInput<KeyCode>>, mut lab: ResMut<Lab>) {
    let wall = time.delta_secs_f64().clamp(0.0, 0.05);
    let controls = lab.controls(&keys);
    match lab.mode {
        Mode::Tunnel => {
            let state = lab.tunnel_state(lab.get("alpha"));
            let air = lab.atmosphere.sample(lab.get("altitude"));
            lab.tunnel_loads = evaluate_vehicle(
                &lab.vehicle,
                &lab.tunnel_data,
                &state,
                &air,
                DVec3::ZERO,
                &controls,
                250.0,
            );
        }
        Mode::Flight => {
            let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
            let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
            let delta = f64::from(u8::from(shift)) - f64::from(u8::from(ctrl));
            if delta != 0.0 {
                let throttle = (lab.get("throttle") + delta * wall * 0.5).clamp(0.0, 1.0);
                lab.set("throttle", throttle);
            }
            if !lab.paused {
                let lab = &mut *lab;
                lab.accumulator += wall * lab.rate();
                let command = FlightCommand {
                    controls,
                    throttle: lab.get("throttle"),
                    brakes: lab.on("brakes"),
                };
                let wind = DVec3::new(lab.get("wind"), 0.0, 0.0);
                let flight = lab.flight.as_mut().expect("flight scene");
                flight.wind = wind;
                while lab.accumulator >= FLIGHT_STEP && flight.failure.is_none() {
                    flight.step(&command);
                    lab.accumulator -= FLIGHT_STEP;
                }
                if flight.failure.is_some() {
                    lab.accumulator = 0.0;
                }
            }
        }
        Mode::Entry => {
            if !lab.paused {
                let seconds = wall * lab.rate();
                lab.entry
                    .as_mut()
                    .expect("entry scene")
                    .advance(seconds, 6000);
            }
        }
    }
    if lab.mode != Mode::Tunnel {
        let t = lab.time();
        if t - lab.last_history >= 0.25 || lab.history.is_empty() {
            let loads = lab.loads();
            let sample = Sample {
                time: t,
                altitude: lab.altitude(),
                speed: loads.aero.speed,
                q: loads.aero.q_pa,
                heat: loads.max_flux_wm2(),
            };
            lab.last_history = t;
            lab.history.push(sample);
            if lab.history.len() > 6000 {
                lab.history.remove(0);
            }
        }
    }
    lab.terminal = lab
        .flight
        .as_ref()
        .and_then(|f| f.failure.clone())
        .or_else(|| lab.entry.as_ref().and_then(|e| e.terminal.clone()));
}

/// The vehicle's parts and wheels as meshes under the model root, after a reload.
fn rebuild(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    root: Single<Entity, With<ModelRoot>>,
    mut lab: ResMut<Lab>,
) {
    if !lab.rebuild {
        return;
    }
    lab.rebuild = false;
    commands.entity(*root).despawn_related::<Children>();
    let mut handles = vec![];
    let wheel_material = materials.add(StandardMaterial {
        base_color: hex(0x182127),
        perceptual_roughness: 0.9,
        ..default()
    });
    commands.entity(*root).with_children(|model| {
        for p in &lab.vehicle.parts {
            let mesh = match p.shape {
                PartShape::Box { size } => {
                    meshes.add(Cuboid::new(size.x as f32, size.y as f32, size.z as f32))
                }
                PartShape::Cone { radius, length } => meshes.add(Cone {
                    radius: radius as f32,
                    height: length as f32,
                }),
                PartShape::Cylinder { radius, length } => {
                    meshes.add(Cylinder::new(radius as f32, length as f32))
                }
            };
            let material = materials.add(StandardMaterial {
                base_color: hex(p.color),
                perceptual_roughness: if p.id == "shield" { 0.95 } else { 0.48 },
                metallic: 0.2,
                ..default()
            });
            handles.push(material.clone());
            model.spawn((
                Mesh3d(mesh),
                MeshMaterial3d(material),
                Transform::from_translation(p.position.as_vec3())
                    .with_rotation(p.rotation.as_quat()),
            ));
        }
        for wheel in &lab.vehicle.wheels {
            model.spawn((
                Mesh3d(meshes.add(Sphere::new(wheel.radius as f32).mesh().uv(16, 10))),
                MeshMaterial3d(wheel_material.clone()),
                Transform::from_translation(wheel.position.as_vec3()),
            ));
        }
    });
    lab.part_materials = handles;
}

/// Scene axes: the entry's planet frame has +z north, drawn as +y up.
fn to_scene(mode: Mode, v: DVec3) -> Vec3 {
    if mode == Mode::Entry {
        Vec3::new(v.x as f32, v.z as f32, -v.y as f32)
    } else {
        v.as_vec3()
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn draw_scene(
    lab: Res<Lab>,
    view: Res<OrbitView>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut camera: Single<&mut Transform, (With<Camera3d>, Without<ModelRoot>)>,
    mut model: Single<&mut Transform, (With<ModelRoot>, Without<Camera3d>)>,
    mut floor: Single<
        (&mut Transform, &mut Visibility),
        (With<Floor>, Without<ModelRoot>, Without<Camera3d>),
    >,
    mut earth: Single<
        (&mut Transform, &mut Visibility),
        (
            With<Earth>,
            Without<Floor>,
            Without<ModelRoot>,
            Without<Camera3d>,
        ),
    >,
    mut shell: Single<
        (&mut Transform, &mut Visibility),
        (
            With<AirShell>,
            Without<Earth>,
            Without<Floor>,
            Without<ModelRoot>,
            Without<Camera3d>,
        ),
    >,
    mut gizmos: Gizmos,
    mut on_top: Gizmos<OnTop>,
) {
    let mode = lab.mode;
    **camera = Transform::from_translation(view.eye()).looking_at(Vec3::ZERO, Vec3::Y);
    let props = mass_properties(&lab.vehicle, lab.data());
    let (q, origin, position) = match (&lab.flight, &lab.entry) {
        (Some(f), _) => {
            let b = f.rigid_body();
            let translation = DVec3::new(
                f64::from(b.translation().x),
                f64::from(b.translation().y),
                f64::from(b.translation().z),
            );
            (f.rotation(), translation - f.position(), f.position())
        }
        (_, Some(e)) => (
            e.rotation(),
            -rotate(e.rotation(), props.center),
            e.position(),
        ),
        _ => {
            let q = lab.tunnel_state(lab.get("alpha")).rotation;
            (q, -rotate(q, props.center), DVec3::ZERO)
        }
    };
    let render_q = if mode == Mode::Entry {
        let entry_to_scene = DQuat::from_xyzw(
            -std::f64::consts::FRAC_1_SQRT_2,
            0.0,
            0.0,
            std::f64::consts::FRAC_1_SQRT_2,
        );
        quat_multiply(entry_to_scene, q)
    } else {
        q
    };
    model.rotation = render_q.as_quat().normalize();
    model.translation = to_scene(mode, origin);

    let show = |on: bool| {
        if on {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        }
    };
    *floor.1 = show(mode == Mode::Flight);
    *earth.1 = show(mode == Mode::Entry);
    *shell.1 = show(mode == Mode::Entry && lab.on("air"));
    if mode == Mode::Flight {
        floor.0.translation = Vec3::new(0.0, (-position.y - 0.01) as f32, 0.0);
        // The lab's two grids: 20 m cells to ±1 km and 1 m cells to ±40 m, slid under the craft.
        let grid =
            |gizmos: &mut Gizmos, cell: f64, half: i32, lift: f64, centre: u32, line: u32| {
                let offset = DVec3::new(-position.x % cell, -position.y + lift, -position.z % cell);
                let extent = cell * f64::from(half);
                for k in -half..=half {
                    let c = if k == 0 { hex(centre) } else { hex(line) };
                    let d = cell * f64::from(k);
                    gizmos.line(
                        (offset + DVec3::new(d, 0.0, -extent)).as_vec3(),
                        (offset + DVec3::new(d, 0.0, extent)).as_vec3(),
                        c,
                    );
                    gizmos.line(
                        (offset + DVec3::new(-extent, 0.0, d)).as_vec3(),
                        (offset + DVec3::new(extent, 0.0, d)).as_vec3(),
                        c,
                    );
                }
            };
        grid(&mut gizmos, 20.0, 50, 0.0, 0x607a70, 0x3b5248);
        grid(&mut gizmos, 1.0, 40, 0.005, 0x587369, 0x30473d);
    }
    if mode == Mode::Entry {
        let at = to_scene(mode, -position);
        earth.0.translation = at;
        shell.0.translation = at;
    }

    // Centre of mass.
    on_top.sphere(Isometry3d::IDENTITY, 0.14, hex(0xffcf76));
    let loads = lab.loads();
    if lab.on("arrows") {
        let gain = 4.0 / length(loads.aero.force).max(1000.0);
        let mut arrow = |v: DVec3, point: DVec3, color: u32| {
            let n = length(v);
            if n < 1e-6 {
                return;
            }
            let size = (n * gain).min(5.0) as f32;
            let start = to_scene(mode, point - position);
            let direction = to_scene(mode, v / n);
            gizmos
                .arrow(start, start + direction * size, hex(color))
                .with_tip_length((size * 0.3).min(0.2));
        };
        for e in &loads.aero.elements {
            arrow(e.lift, e.point, 0x75d6b5);
            arrow(e.drag, e.point, 0xf3a264);
        }
    }
    if mode == Mode::Tunnel {
        let v = lab.tunnel_state(lab.get("alpha")).velocity;
        let n = length(v);
        if n > 0.0 {
            let d = (-v / n).as_vec3();
            for x in (-4..=4).step_by(2) {
                for y in (-2..=2).step_by(2) {
                    let start = Vec3::new(x as f32, y as f32, 9.0);
                    gizmos
                        .arrow(start, start + d * 3.0, hex(0x497581))
                        .with_tip_length(0.25);
                }
            }
        }
    }
    // Hot skins glow: red from 550 K, towards orange from 1400 K. Alpha 0 keeps it out of the
    // camera's exposure, as Three's emissive.
    for (thermal, handle) in lab.data().thermal.iter().zip(&lab.part_materials) {
        if let Some(mut m) = materials.get_mut(handle) {
            m.emissive = LinearRgba::new(
                ((thermal.skin_k - 550.0) / 1200.0).clamp(0.0, 1.0) as f32,
                ((thermal.skin_k - 1400.0) / 1200.0).clamp(0.0, 0.35) as f32,
                0.0,
                0.0,
            );
        }
    }
}

fn metric(out: &mut String, name: &str, value: String) {
    out.push_str(&format!("{name:<22}{value}\n"));
}

/// Text panels and plots: refreshed every 120 ms as the lab's; the plot lines every frame.
#[allow(clippy::type_complexity)]
fn panels(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mut lab: ResMut<Lab>,
    window: Single<&Window>,
    mut texts: ParamSet<(
        Single<&mut Text, With<LeftPanel>>,
        Single<&mut Text, With<RightPanel>>,
        Single<&mut Text, With<Head>>,
        Single<(&mut Text, &mut Visibility), With<Terminal>>,
    )>,
    mut pool: Query<
        (
            &mut Text,
            &mut TextFont,
            &mut TextColor,
            &mut Node,
            &mut Visibility,
        ),
        (
            With<PlotLabel>,
            Without<LeftPanel>,
            Without<RightPanel>,
            Without<Head>,
            Without<Terminal>,
        ),
    >,
    mut gizmos: Gizmos<PlotLines>,
) {
    lab.since_readout += time.delta_secs_f64();
    if lab.since_readout >= 0.12 {
        lab.since_readout = 0.0;
        let controls = lab.controls(&keys);
        refresh(&mut lab, &controls);
    }
    texts.p0().0.clone_from(&lab.left_text);
    texts.p1().0.clone_from(&lab.right_text);
    texts.p2().0.clone_from(&lab.head_text);
    {
        let mut terminal = texts.p3();
        match &lab.terminal {
            Some(t) => {
                terminal.0.0.clone_from(t);
                *terminal.1 = Visibility::Inherited;
            }
            None => *terminal.1 = Visibility::Hidden,
        }
    }
    let size = Vec2::new(window.width(), window.height());
    let mut labels: Vec<Label> = vec![];
    for (plot, rect) in lab.plots.iter().zip(plot_rects(size)) {
        plot::draw(plot, rect, size, &mut gizmos, &mut labels);
    }
    plot::place_labels(&labels, &mut pool);
}

/// The lab's `readout` and `charts`.
fn refresh(lab: &mut Lab, controls: &Controls) {
    let mode = lab.mode;
    // Left: scenes, settings, keys and the lab's explanation.
    let mut left = String::from("VOID AERODYNAMICS LAB\n");
    for (k, (m, name)) in [
        (Mode::Tunnel, "wind tunnel"),
        (Mode::Flight, "aircraft"),
        (Mode::Entry, "reentry"),
    ]
    .iter()
    .enumerate()
    {
        let mark = if *m == mode { ">" } else { " " };
        left.push_str(&format!("{mark}{} {name}  ", k + 1));
    }
    left.push_str("\n\nSETTINGS  Up/Down pick, Left/Right change\n");
    let selected = lab.selected[mode as usize];
    for i in lab.visible(mode) {
        let f = &lab.fields[i];
        let mark = if i == selected { ">" } else { " " };
        let note = if f.reload { " *" } else { "" };
        left.push_str(&format!("{mark} {:<24}{}{note}\n", f.label, f.text()));
    }
    left.push_str("  * reloads the scene\n\nKEYS\n");
    left.push_str("R reload   F fit view\nDrag to orbit, wheel to zoom\n");
    match mode {
        Mode::Tunnel => left.push_str(
            "A/D aileron, Q/E rudder (held)\nClick the left plot to pick the angle of attack.\n\n\
             Positive pitching moment is nose-up; a moment falling as the angle rises means \
             local static stability.",
        ),
        Mode::Flight => left.push_str(
            "Space pause\nW/S pitch  A/D roll  Q/E rudder\nShift/Ctrl throttle  X cut  B brakes\n\n\
             Start at 700 m and 70 m/s, or on the runway. Fixed test aircraft on rolling gear. \
             Inputs move the control surfaces; there is no SAS. Neutral controls are not an \
             autopilot: trim the elevator and throttle. From the runway: full throttle, then \
             about +0.15 elevator from 40 m/s.",
        ),
        Mode::Entry => left.push_str(
            "Space pause   start settings apply on R\n\nSpinning spherical Terra, 6,371 km. \
             Aerodynamic moments turn the capsule. Compare protection, entry angle and \
             ablator mass. Overheating or overload ends the trial; no parachute before \
             touchdown.",
        ),
    }
    lab.left_text = left;

    // Right: flight data, forces and heat per part.
    let altitude = lab.altitude();
    let air = lab.atmosphere.sample(altitude);
    let data = lab.data();
    let loads = lab.loads();
    let props = mass_properties(&lab.vehicle, data);
    let q = loads.aero.q_pa;
    let max_q = match (&lab.flight, &lab.entry) {
        (Some(f), _) => f.max_q_pa,
        (_, Some(e)) => e.max_q_pa,
        _ => q,
    };
    let mut right = String::from("FLIGHT DATA\n");
    metric(
        &mut right,
        "Airspeed",
        format!("{:.1} m/s", loads.aero.speed),
    );
    metric(
        &mut right,
        "Altitude",
        format!("{:.3} km", altitude / 1000.0),
    );
    metric(
        &mut right,
        "Dyn. pressure / max",
        format!("{:.2} / {:.2} kPa", q / 1000.0, max_q / 1000.0),
    );
    metric(
        &mut right,
        "Mach",
        if air.density > 0.0 {
            format!("{:.2}", loads.aero.mach)
        } else {
            "vacuum".into()
        },
    );
    metric(&mut right, "Density", format!("{:.2e} kg/m3", air.density));
    metric(
        &mut right,
        "Pressure",
        format!("{:.2} kPa", air.pressure_pa / 1000.0),
    );
    metric(
        &mut right,
        "Air temperature",
        format!("{:.1} K", air.temperature_k),
    );
    metric(&mut right, "Mass", format!("{:.1} kg", props.mass));
    if let Some(f) = &lab.flight {
        metric(
            &mut right,
            "Thrust / fuel",
            format!("{:.0} N / {:.1} kg", f.thrust_n, f.fuel_kg()),
        );
    }
    if let Some(e) = &lab.entry {
        metric(
            &mut right,
            "Aero decel / peak",
            format!(
                "{:.2} / {:.2} g",
                length(loads.aero.force) / props.mass / 9.80665,
                e.max_g
            ),
        );
    }
    if mode == Mode::Tunnel {
        let v = lab.tunnel_state(lab.get("alpha")).velocity;
        let drag = if length(v) > 0.0 {
            -loads.aero.force.dot(normalize(v))
        } else {
            0.0
        };
        let lift_y: f64 = loads.aero.elements.iter().map(|e| e.lift.y).sum();
        metric(
            &mut right,
            "Vertical lift / drag",
            format!("{:.2} / {:.2} kN", lift_y / 1000.0, drag / 1000.0),
        );
        metric(
            &mut right,
            "Nose-up moment",
            format!("{:.2} kN m", -loads.aero.torque.x / 1000.0),
        );
    }
    right.push_str("\nFORCES / PART\n");
    for e in &loads.aero.elements {
        right.push_str(&format!(
            "{:<11}a {:>6.1} deg  {:>8.2} kN\n{:<11}stall {:>3.0}%  CL {:.2}  CD {:.3}\n",
            e.id,
            e.alpha_radians / DEG,
            length(e.force) / 1000.0,
            "",
            100.0 * e.stall,
            e.cl,
            e.cd
        ));
    }
    right.push_str("\nTHERMAL / PART\n");
    for (i, p) in lab.vehicle.parts.iter().enumerate() {
        let t = &data.thermal[i];
        let heat = &loads.heat[i];
        let filled = ((t.skin_k / p.thermal.max_skin_k) * 20.0).clamp(0.0, 20.0) as usize;
        let state = match p.thermal.ablator {
            Some(a) => format!("ablator {:.1} / {:.0} kg", t.ablator_kg, a.mass_kg),
            None if heat.env.exposed => "in the airflow".into(),
            None => "shielded from direct flow".into(),
        };
        right.push_str(&format!(
            "{:<11}{:.3} MW/m2{}\n{:<11}skin {:.0} K / core {:.0} K\n{:<11}[{}{}]\n{:<11}{state}\n",
            p.id,
            heat.load.flux_wm2 / 1e6,
            if t.failed { "  FAILED" } else { "" },
            "",
            t.skin_k,
            t.core_k,
            "",
            "#".repeat(filled),
            ".".repeat(20 - filled),
            "",
        ));
    }
    let head = format!(
        "{}    {}",
        lab.vehicle.name,
        if mode == Mode::Tunnel {
            "STATIC".to_string()
        } else {
            format!(
                "{}T+ {:.1} s   {}x",
                if lab.paused { "PAUSED  " } else { "" },
                lab.time(),
                lab.rate()
            )
        }
    );

    // Plots.
    let plots = if mode == Mode::Tunnel {
        let air = lab.atmosphere.sample(lab.get("altitude"));
        let elements = aero_elements(&lab.vehicle, props.center);
        let (mut lifts, mut drags, mut moments) = (vec![], vec![], vec![]);
        for alpha in -35..=35 {
            let alpha = f64::from(alpha);
            let state = lab.tunnel_state(alpha);
            let force = aerodynamic_forces(&elements, &state, &air, DVec3::ZERO, controls);
            lifts.push((
                alpha,
                force.elements.iter().map(|e| e.lift.y).sum::<f64>() / 1000.0,
            ));
            let speed = length(state.velocity);
            drags.push((
                alpha,
                if speed > 0.0 {
                    -force.force.dot(normalize(state.velocity)) / 1000.0
                } else {
                    0.0
                },
            ));
            moments.push((alpha, -force.torque.x / 1000.0));
        }
        [
            Plot {
                title: "Alpha sweep (click to pick alpha)".into(),
                series: vec![
                    Series {
                        label: "vertical lift",
                        color: hex(0x75d6b5),
                        points: lifts,
                        right: false,
                    },
                    Series {
                        label: "drag",
                        color: hex(0xf3a264),
                        points: drags,
                        right: false,
                    },
                ],
                x_label: "alpha deg",
                left_label: "kN",
                right_label: "",
            },
            Plot {
                title: "Pitching moment (positive nose-up)".into(),
                series: vec![Series {
                    label: "pitching moment",
                    color: hex(0xa5c9ef),
                    points: moments,
                    right: false,
                }],
                x_label: "alpha deg",
                left_label: "kN m",
                right_label: "",
            },
        ]
    } else {
        let entry = mode == Mode::Entry;
        let altitude = Plot {
            title: "Altitude".into(),
            series: vec![Series {
                label: "altitude",
                color: hex(0x75d6b5),
                points: lab
                    .history
                    .iter()
                    .map(|h| (h.time, h.altitude / if entry { 1000.0 } else { 1.0 }))
                    .collect(),
                right: false,
            }],
            x_label: "time s",
            left_label: if entry { "km" } else { "m" },
            right_label: "",
        };
        let second = if entry {
            Plot {
                title: "Heat flux and dynamic pressure".into(),
                series: vec![
                    Series {
                        label: "heat",
                        color: hex(0xf3a264),
                        points: lab.history.iter().map(|h| (h.time, h.heat / 1e6)).collect(),
                        right: false,
                    },
                    Series {
                        label: "dyn. pressure",
                        color: hex(0xa5c9ef),
                        points: lab.history.iter().map(|h| (h.time, h.q / 1000.0)).collect(),
                        right: true,
                    },
                ],
                x_label: "time s",
                left_label: "MW/m2",
                right_label: "kPa",
            }
        } else {
            Plot {
                title: "Airspeed".into(),
                series: vec![Series {
                    label: "airspeed",
                    color: hex(0xa5c9ef),
                    points: lab.history.iter().map(|h| (h.time, h.speed)).collect(),
                    right: false,
                }],
                x_label: "time s",
                left_label: "m/s",
                right_label: "",
            }
        };
        [altitude, second]
    };
    lab.right_text = right;
    lab.head_text = head;
    lab.plots = plots;
}
