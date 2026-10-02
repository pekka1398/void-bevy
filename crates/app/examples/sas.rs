//! lab/sas's page: the stability assist on the demo rocket's real inertias, on lab/landing's flight
//! attitude integrator with no damping. Keys replace the lab's panel.
//!
//! T: SAS on / off | W / S pitch, A / D yaw, Q / E roll | K: kick 0.2 rad/s, Shift+K: 1 rad/s |
//! R: reset | V: full stack / upper stage | `-` `=`: inertia × (0.25–4) | tuning: 1 / 2 rate s,
//! 3 / 4 attitude/rate, 5 / 6 brake, 7 / 8 lock rate, 0 defaults.
//!
//! The cyan arrow is the nose, the grey one the locked attitude's nose. The chart shows the last
//! 15 s: angle from the lock (cyan, °), spin (orange, °/s), largest command (purple, 0–1).

use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;
use glam::{DQuat, DVec3};
use void_app::parts::spawn_shape;
use void_landing::{PartJointRocket, STEERING_TORQUE, demo_rocket, pebble, planet_ephemeris};
use void_rotation::{Mat3, matrix, rotation_step};
use void_sas::{SAS_TUNING, SasTuning, StabilityAssist, attitude_error};

const DT: f64 = 1.0 / 60.0;
const DEG: f64 = 180.0 / std::f64::consts::PI;
const HISTORY_SECONDS: f64 = 15.0;
// Part centres of mass relative to the stack's root, as PartJointRocket stacks them.
const UPPER_OFFSET: f64 = 1.1;
const BOOSTER_OFFSET: f64 = -1.3;
const CHART: Vec2 = Vec2::new(380.0, 130.0);

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "void | sas".into(),
                ..default()
            }),
            ..default()
        }))
        .init_gizmo_group::<ChartGizmos>()
        .insert_resource(ClearColor(Color::srgb_u8(0x03, 0x04, 0x0a)))
        .insert_resource(GlobalAmbientLight {
            color: Color::srgb_u8(0xbf, 0xd2, 0xff),
            brightness: 400.0,
            ..default()
        })
        .add_systems(Startup, setup)
        .add_systems(Update, (controls, simulate, draw).chain())
        .run();
}

#[derive(Default, Reflect, GizmoConfigGroup)]
struct ChartGizmos;

struct Sample {
    t: f64,
    /// Degrees from the lock, None when nothing is locked.
    error: Option<f64>,
    spin: f64,
    command: f64,
}

#[derive(Resource)]
struct Lab {
    /// The demo rocket's controlled inertia: full stack, upper stage (lab/landing's colliders).
    stack: Mat3,
    upper: Mat3,
    /// Centre of mass of the stack above the upper stage's root offset.
    stack_centre: f64,
    upper_stage: bool,
    /// Inertia × 2^this, as the lab's slider (−2 to 2).
    scale_log: f64,
    tuning: SasTuning,
    sas: StabilityAssist,
    rotation: DQuat,
    angular_velocity: DVec3,
    last_command: DVec3,
    elapsed: f64,
    owed: f64,
    history: Vec<Sample>,
    seed: u64,
}

impl Lab {
    fn inertia(&self) -> Mat3 {
        let base = if self.upper_stage {
            self.upper
        } else {
            self.stack
        };
        let k = 2f64.powf(self.scale_log);
        base.map(|v| v * k)
    }

    /// A new controller after a tuning change; SAS stays on or off and locks afresh.
    fn retune(&mut self) {
        let on = self.sas.enabled();
        self.sas = StabilityAssist::new(STEERING_TORQUE, self.tuning);
        self.sas.set_enabled(on);
    }

    fn random(&mut self) -> f64 {
        // xorshift64*: any spread of directions will do for a kick.
        self.seed ^= self.seed >> 12;
        self.seed ^= self.seed << 25;
        self.seed ^= self.seed >> 27;
        (self.seed.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 11) as f64 / (1u64 << 53) as f64
    }

    fn kick(&mut self, size: f64) {
        let d = DVec3::new(
            self.random() - 0.5,
            self.random() - 0.5,
            self.random() - 0.5,
        );
        self.angular_velocity += d / d.length() * size;
    }

    fn reset(&mut self) {
        self.rotation = DQuat::IDENTITY;
        self.angular_velocity = DVec3::ZERO;
        self.history.clear();
        if self.sas.enabled() {
            self.sas.set_enabled(true);
        }
    }

    fn step(&mut self, pilot: DVec3) {
        let inertia = self.inertia();
        self.last_command =
            self.sas
                .command(self.rotation, self.angular_velocity, &inertia, pilot, DT);
        (self.rotation, self.angular_velocity) = rotation_step(
            self.rotation,
            self.angular_velocity,
            &inertia,
            self.last_command * STEERING_TORQUE,
            DVec3::ZERO,
            DT,
        );
        self.elapsed += DT;
        let error = self
            .sas
            .target()
            .map(|t| attitude_error(t, self.rotation).length() * DEG);
        self.history.push(Sample {
            t: self.elapsed,
            error,
            spin: self.angular_velocity.length() * DEG,
            command: self.last_command.abs().max_element(),
        });
        let cut = self.elapsed - HISTORY_SECONDS;
        self.history.retain(|s| s.t >= cut);
    }
}

#[derive(Component)]
struct Craft;

#[derive(Component)]
struct Booster;

#[derive(Component)]
struct UpperPart;

#[derive(Component)]
struct Hud;

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut gizmo_store: ResMut<GizmoConfigStore>,
) {
    // The demo rocket's real inertias, from lab/landing's Rapier colliders.
    let planet = pebble();
    let (mut ephemeris, body_index) = planet_ephemeris(&planet);
    let demo = demo_rocket(&planet.terrain);
    let mut pad = PartJointRocket::landed(
        &mut ephemeris,
        body_index,
        planet.terrain.clone(),
        demo.full.clone(),
        demo.upper.clone(),
        demo.booster.clone(),
        demo.options,
        demo.launch_site,
    );
    let stack = pad.controlled_inertia();
    pad.separate(&ephemeris);
    let upper = pad.controlled_inertia();
    let (upper_mass, booster_mass) = (
        demo.upper.dry_mass_kg + demo.upper.fuel_mass_kg,
        demo.booster.dry_mass_kg + demo.booster.fuel_mass_kg,
    );
    let mut sas = StabilityAssist::new(STEERING_TORQUE, SAS_TUNING);
    sas.set_enabled(false);
    commands.insert_resource(Lab {
        stack,
        upper,
        stack_centre: (upper_mass * UPPER_OFFSET + booster_mass * BOOSTER_OFFSET)
            / (upper_mass + booster_mass),
        upper_stage: false,
        scale_log: 0.0,
        tuning: SAS_TUNING,
        sas,
        rotation: DQuat::IDENTITY,
        angular_velocity: DVec3::ZERO,
        last_command: DVec3::ZERO,
        elapsed: 0.0,
        owed: 0.0,
        history: Vec::new(),
        seed: 0x9e37_79b9_7f4a_7c15,
    });

    commands.spawn((
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection {
            fov: 40f32.to_radians(),
            ..default()
        }),
        Transform::from_xyz(9.0, 4.0, 13.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
    // The chart's own camera on top, so its lines are in screen pixels.
    let (config, _) = gizmo_store.config_mut::<ChartGizmos>();
    config.render_layers = RenderLayers::layer(1);
    config.line.width = 1.5;
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
            illuminance: 6000.0,
            ..default()
        },
        Transform::from_xyz(10.0, 12.0, 6.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    let hull = materials.add(StandardMaterial {
        base_color: Color::srgb(0.92, 0.93, 0.95),
        perceptual_roughness: 0.6,
        ..default()
    });
    let booster_hull = materials.add(StandardMaterial {
        base_color: Color::srgb(0.75, 0.77, 0.8),
        perceptual_roughness: 0.6,
        ..default()
    });
    commands
        .spawn((Craft, Transform::default(), Visibility::default()))
        .with_children(|craft| {
            craft
                .spawn((UpperPart, Transform::default(), Visibility::default()))
                .with_children(|p| spawn_shape(p, &demo.upper_shape, &mut meshes, &hull));
            craft
                .spawn((Booster, Transform::default(), Visibility::default()))
                .with_children(|p| spawn_shape(p, &demo.booster_shape, &mut meshes, &booster_hull));
        });

    commands.spawn((
        Text::new(""),
        TextFont {
            font_size: FontSize::Px(13.0),
            ..default()
        },
        Hud,
        Node {
            position_type: PositionType::Absolute,
            top: px(12),
            left: px(12),
            ..default()
        },
    ));
}

fn axis(keys: &ButtonInput<KeyCode>, positive: KeyCode, negative: KeyCode) -> f64 {
    keys.pressed(positive) as i32 as f64 - keys.pressed(negative) as i32 as f64
}

fn controls(keys: Res<ButtonInput<KeyCode>>, mut lab: ResMut<Lab>) {
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    if keys.just_pressed(KeyCode::KeyT) {
        lab.sas.toggle();
    }
    if keys.just_pressed(KeyCode::KeyK) {
        lab.kick(if shift { 1.0 } else { 0.2 });
    }
    if keys.just_pressed(KeyCode::KeyR) {
        lab.reset();
    }
    if keys.just_pressed(KeyCode::KeyV) {
        lab.upper_stage = !lab.upper_stage;
    }
    let step = |down: KeyCode, up: KeyCode| {
        keys.just_pressed(up) as i32 as f64 - keys.just_pressed(down) as i32 as f64
    };
    lab.scale_log = (lab.scale_log + 0.25 * step(KeyCode::Minus, KeyCode::Equal)).clamp(-2.0, 2.0);
    let mut t = lab.tuning;
    let ratio = t.attitude_seconds / t.rate_seconds;
    t.rate_seconds =
        (t.rate_seconds + 0.01 * step(KeyCode::Digit1, KeyCode::Digit2)).clamp(0.05, 0.6);
    let ratio = (ratio + 0.5 * step(KeyCode::Digit3, KeyCode::Digit4)).clamp(4.0, 12.0);
    t.attitude_seconds = t.rate_seconds * ratio;
    t.brake_fraction =
        (t.brake_fraction + 0.05 * step(KeyCode::Digit5, KeyCode::Digit6)).clamp(0.1, 1.0);
    let lock =
        (t.lock_rate.log10() + 0.1 * step(KeyCode::Digit7, KeyCode::Digit8)).clamp(-4.0, -1.0);
    t.lock_rate = 10f64.powf(lock);
    if keys.just_pressed(KeyCode::Digit0) {
        t = SAS_TUNING;
    }
    if keys.get_just_pressed().any(|k| {
        matches!(
            k,
            KeyCode::Digit0
                | KeyCode::Digit1
                | KeyCode::Digit2
                | KeyCode::Digit3
                | KeyCode::Digit4
                | KeyCode::Digit5
                | KeyCode::Digit6
                | KeyCode::Digit7
                | KeyCode::Digit8
        )
    }) {
        lab.tuning = t;
        lab.retune();
    }
}

fn simulate(time: Res<Time>, keys: Res<ButtonInput<KeyCode>>, mut lab: ResMut<Lab>) {
    let pilot = DVec3::new(
        axis(&keys, KeyCode::KeyS, KeyCode::KeyW),
        axis(&keys, KeyCode::KeyE, KeyCode::KeyQ),
        axis(&keys, KeyCode::KeyD, KeyCode::KeyA),
    );
    lab.owed = (lab.owed + time.delta_secs_f64()).min(0.25);
    while lab.owed >= DT {
        lab.step(pilot);
        lab.owed -= DT;
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn draw(
    lab: Res<Lab>,
    mut craft: Single<&mut Transform, (With<Craft>, Without<Booster>, Without<UpperPart>)>,
    mut upper: Single<&mut Transform, (With<UpperPart>, Without<Booster>)>,
    mut booster: Single<(&mut Transform, &mut Visibility), With<Booster>>,
    mut hud: Single<&mut Text, With<Hud>>,
    mut gizmos: Gizmos,
    mut chart: Gizmos<ChartGizmos>,
    window: Single<&Window>,
) {
    // Layout: the stack turns about its centre of mass, the upper stage about its own.
    let centre = if lab.upper_stage {
        UPPER_OFFSET
    } else {
        lab.stack_centre
    };
    upper.translation.y = (UPPER_OFFSET - centre) as f32;
    booster.0.translation.y = (BOOSTER_OFFSET - centre) as f32;
    *booster.1 = if lab.upper_stage {
        Visibility::Hidden
    } else {
        Visibility::Inherited
    };
    craft.rotation = lab.rotation.as_quat();

    // A fixed wire sphere and axes, so any turning of the rocket shows against them.
    let sphere = Color::srgb_u8(0x1c, 0x24, 0x36);
    for ring in 1..16 {
        let polar = ring as f32 / 16.0 * std::f32::consts::PI;
        gizmos
            .circle(
                Isometry3d::new(
                    Vec3::Y * 40.0 * polar.cos(),
                    Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
                ),
                40.0 * polar.sin(),
                sphere,
            )
            .resolution(48);
    }
    for meridian in 0..12 {
        let angle = meridian as f32 / 24.0 * std::f32::consts::TAU;
        gizmos
            .circle(
                Isometry3d::from_rotation(Quat::from_rotation_y(angle)),
                40.0,
                sphere,
            )
            .resolution(48);
    }
    gizmos.axes(Transform::IDENTITY, 1.5);
    let arrow = |gizmos: &mut Gizmos, q: DQuat, color: Color| {
        gizmos
            .arrow(Vec3::ZERO, q.as_quat() * Vec3::Y * 5.0, color)
            .with_tip_length(0.5);
    };
    arrow(&mut gizmos, lab.rotation, Color::srgb_u8(0x6f, 0xd3, 0xff));
    if let Some(target) = lab.sas.target() {
        arrow(&mut gizmos, target, Color::srgb_u8(0x8a, 0x8f, 0x9c));
    }

    // Readout. The chart's top, in degrees and degrees per second, as the lab prints it.
    let top = lab
        .history
        .iter()
        .map(|s| s.error.unwrap_or(0.0).max(s.spin))
        .fold(1.0_f64, f64::max);
    let inertia = lab.inertia();
    let r = matrix(lab.rotation);
    let w = lab.angular_velocity;
    let local = [
        r[0] * w.x + r[3] * w.y + r[6] * w.z,
        r[1] * w.x + r[4] * w.y + r[7] * w.z,
        r[2] * w.x + r[5] * w.y + r[8] * w.z,
    ];
    let row = |v: [f64; 3], digits: usize| {
        v.iter()
            .map(|x| format!("{x:>9.digits$}"))
            .collect::<String>()
    };
    let diagonal = [inertia[0], inertia[4], inertia[8]];
    let error = lab
        .sas
        .target()
        .map(|t| format!("{:.3} deg", attitude_error(t, lab.rotation).length() * DEG))
        .unwrap_or_else(|| "-".into());
    let t = lab.tuning;
    let c = lab.last_command;
    hud.0 = format!(
        "SAS LAB\n\n\
         SAS {}   {}\n\
         vehicle   {}   (V)\n\
         inertia x {:.2}   (- =)\n\n\
         inertia  {} kg m2  (pitch roll yaw)\n\
         max accel{} rad/s2\n\
         spin     {} rad/s   |{:.4}|\n\
         command  {}\n\
         from lock {error}\n\n\
         tuning   rate {:.2} s (1 2)   attitude/rate {:.1} (3 4)\n\
         \x20        brake {:.2} (5 6)   lock rate {:.0e} (7 8)   0 defaults\n\
         {{ rateSeconds: {}, attitudeSeconds: {}, brakeFraction: {}, lockRate: {} }}\n\n\
         T SAS | W/S pitch | A/D yaw | Q/E roll | K kick 0.2 rad/s, Shift+K 1 rad/s | R reset\n\
         Keys use the game's steering torque ({STEERING_TORQUE} N m). No damping: with SAS off a spin never stops.\n\
         Cyan arrow: nose. Grey arrow: locked attitude's nose.\n\
         Chart, last {HISTORY_SECONDS} s: angle from lock (cyan, deg), spin (orange, deg/s), |command| (purple); top {:.2}",
        if lab.sas.enabled() { "ON " } else { "off" },
        lab.sas.phase().label(),
        if lab.upper_stage {
            "upper stage"
        } else {
            "full stack"
        },
        2f64.powf(lab.scale_log),
        row(diagonal, 0),
        row(diagonal.map(|d| STEERING_TORQUE / d), 3),
        row(local, 3),
        w.length(),
        row([c.x, c.y, c.z], 3),
        t.rate_seconds,
        t.attitude_seconds / t.rate_seconds,
        t.brake_fraction,
        t.lock_rate,
        t.rate_seconds,
        (t.attitude_seconds * 1000.0).round() / 1000.0,
        t.brake_fraction,
        format!("{:.1e}", t.lock_rate).parse::<f64>().unwrap(),
        top,
    );

    // Chart, bottom left, in screen pixels (the 2D camera's origin is the window's centre).
    let size = Vec2::new(window.width(), window.height());
    let origin = Vec2::new(-size.x / 2.0 + 12.0, -size.y / 2.0 + 12.0);
    let frame = Color::srgb_u8(0x2a, 0x30, 0x40);
    chart.rect_2d(
        Isometry2d::from_translation(origin + CHART / 2.0),
        CHART,
        frame,
    );
    if lab.history.len() < 2 {
        return;
    }
    let t0 = lab.elapsed - HISTORY_SECONDS;
    let mut series = |pick: &dyn Fn(&Sample) -> Option<f64>, color: Color, scale: f64| {
        let mut run: Vec<Vec2> = Vec::new();
        for s in &lab.history {
            match pick(s) {
                Some(v) => run.push(
                    origin
                        + Vec2::new(
                            ((s.t - t0) / HISTORY_SECONDS) as f32 * CHART.x,
                            4.0 + (v / scale) as f32 * (CHART.y - 8.0),
                        ),
                ),
                None => {
                    chart.linestrip_2d(run.drain(..), color);
                }
            }
        }
        chart.linestrip_2d(run, color);
    };
    series(&|s| Some(s.command), Color::srgb_u8(0x8a, 0x6c, 0xff), 1.0);
    series(&|s| Some(s.spin), Color::srgb_u8(0xff, 0xb3, 0x47), top);
    series(&|s| s.error, Color::srgb_u8(0x6f, 0xd3, 0xff), top);
}
