//! lab/navball's page: the attitude ball on its own, at 320 px and at 150 px (lab/flight's size).
//! Keys replace the lab's sliders.
//!
//! WASD QE turn the vessel about its own axes (pitch, yaw, roll) as lab/flight steers. `[` `]`
//! latitude, J L velocity heading, I K velocity pitch, `-` `=` speed (log), R back to the start.

use bevy::prelude::*;
use glam::DVec3;
use void_app::navball::{Navball, NavballLabel, draw_navball, spawn_navball};
use void_navball::{
    MARKER_MIN_SPEED, NavballInput, NavballReadout, heading_pitch, horizon_axes, navball_basis,
};

// A planet with its pole on +z and its prime meridian on +x; the vessel stands at longitude 0 and
// the chosen latitude. At ±90° the ball uses grid north, along the prime meridian.
const POLE: DVec3 = DVec3::Z;
const PRIME_MERIDIAN: DVec3 = DVec3::X;
const TURN_DEGREES_PER_SECOND: f64 = 45.0;
const DEG: f64 = std::f64::consts::PI / 180.0;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "void | navball".into(),
                ..default()
            }),
            ..default()
        }))
        .insert_resource(ClearColor(Color::srgb(0.043, 0.051, 0.075)))
        .insert_resource(Lab::default())
        .add_systems(Startup, setup)
        .add_systems(Update, (controls, draw).chain())
        .run();
}

/// The lab's sliders and the vessel's attitude.
#[derive(Resource)]
struct Lab {
    latitude: f64,
    heading: f64,
    pitch: f64,
    roll: f64,
    velocity_heading: f64,
    velocity_pitch: f64,
    /// Speed is 10^(this − 1) m/s, as the lab's slider.
    speed_log: f64,
    nose: DVec3,
    top: DVec3,
}

impl Default for Lab {
    fn default() -> Self {
        let mut lab = Self {
            latitude: 25.0,
            heading: 90.0,
            pitch: 35.0,
            roll: 0.0,
            velocity_heading: 80.0,
            velocity_pitch: 20.0,
            speed_log: 1.0,
            nose: DVec3::X,
            top: DVec3::Z,
        };
        lab.set_attitude_from_sliders();
        lab
    }
}

impl Lab {
    fn up(&self) -> DVec3 {
        DVec3::new(
            (self.latitude * DEG).cos(),
            0.0,
            (self.latitude * DEG).sin(),
        )
    }

    fn horizon(&self, heading: f64, pitch: f64) -> DVec3 {
        let u = self.up();
        let (north, east) = horizon_axes(u, POLE, PRIME_MERIDIAN);
        north * ((pitch * DEG).cos() * (heading * DEG).cos())
            + east * ((pitch * DEG).cos() * (heading * DEG).sin())
            + u * (pitch * DEG).sin()
    }

    fn set_attitude_from_sliders(&mut self) {
        let (h, p, r) = (self.heading, self.pitch, self.roll * DEG);
        self.nose = self.horizon(h, p);
        let top0 = self.horizon(h, p + 90.0);
        let right0 = top0.cross(self.nose);
        self.top = top0 * r.cos() + right0 * r.sin();
    }

    fn set_sliders_from_attitude(&mut self) {
        let basis = navball_basis(&self.input(DVec3::ZERO));
        let (heading, pitch) = heading_pitch(&basis, self.nose);
        let top0 = self.horizon(heading, pitch + 90.0);
        let right0 = top0.cross(self.nose);
        self.heading = heading;
        self.pitch = pitch;
        self.roll = self.top.dot(right0).atan2(self.top.dot(top0)) / DEG;
    }

    fn speed(&self) -> f64 {
        10f64.powf(self.speed_log - 1.0)
    }

    fn input(&self, velocity: DVec3) -> NavballInput {
        NavballInput {
            nose: self.nose,
            top: self.top,
            up: self.up(),
            pole: POLE,
            prime_meridian: PRIME_MERIDIAN,
            velocity,
        }
    }

    /// Turn by small angles about the vessel's own axes, as lab/flight's torques do.
    fn turn(&mut self, pitch: f64, yaw: f64, roll: f64, seconds: f64) {
        let a = TURN_DEGREES_PER_SECOND * DEG * seconds;
        let (pitch, yaw, roll) = (pitch * a, yaw * a, roll * a);
        if pitch == 0.0 && yaw == 0.0 && roll == 0.0 {
            return;
        }
        // The ball's screen right, top × nose: mirrored from KSP, see `void_navball::navball_basis`.
        let right = self.top.cross(self.nose);
        // Pitch up moves the nose toward the top, yaw right toward the right; roll (E) turns the top
        // toward the left.
        let n = (self.nose + self.top * pitch + right * yaw).normalize();
        let t = self.top - self.nose * pitch - right * roll;
        self.top = (t - n * t.dot(n)).normalize();
        self.nose = n;
        self.set_sliders_from_attitude();
    }
}

#[derive(Component)]
struct Readout;

fn setup(mut commands: Commands, mut images: ResMut<Assets<Image>>, window: Single<&Window>) {
    commands.spawn(Camera2d);
    let ratio = window.scale_factor() as f64;
    let font = |size: f32| TextFont {
        font_size: FontSize::Px(size),
        ..default()
    };
    commands.spawn((
        Text::new(""),
        font(14.0),
        Readout,
        Node {
            position_type: PositionType::Absolute,
            top: px(16),
            left: px(16),
            ..default()
        },
    ));
    let row = commands
        .spawn(Node {
            width: percent(100),
            height: percent(100),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            column_gap: px(48),
            ..default()
        })
        .id();
    for (diameter, caption) in [(320.0, "320 px"), (150.0, "150 px, as in lab/flight")] {
        let figure = commands
            .spawn(Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: px(8),
                ..default()
            })
            .id();
        let ball_node = spawn_navball(&mut commands, &mut images, diameter, ratio);
        let caption = commands
            .spawn((
                Text::new(caption),
                font(12.0),
                TextColor(Color::srgb(0.55, 0.58, 0.65)),
            ))
            .id();
        commands.entity(figure).add_children(&[ball_node, caption]);
        commands.entity(row).add_child(figure);
    }
}

fn axis(keys: &ButtonInput<KeyCode>, positive: KeyCode, negative: KeyCode) -> f64 {
    keys.pressed(positive) as i32 as f64 - keys.pressed(negative) as i32 as f64
}

fn controls(keys: Res<ButtonInput<KeyCode>>, time: Res<Time>, mut lab: ResMut<Lab>) {
    if keys.just_pressed(KeyCode::KeyR) {
        *lab = Lab::default();
        return;
    }
    let seconds = (time.delta_secs_f64()).min(0.05);
    let latitude = axis(&keys, KeyCode::BracketRight, KeyCode::BracketLeft);
    if latitude != 0.0 {
        // As the lab's slider: the attitude is set again from heading, pitch and roll.
        lab.latitude = (lab.latitude + latitude * 30.0 * seconds).clamp(-90.0, 90.0);
        lab.set_attitude_from_sliders();
    }
    lab.velocity_heading = (lab.velocity_heading
        + axis(&keys, KeyCode::KeyL, KeyCode::KeyJ) * 60.0 * seconds)
        .rem_euclid(360.0);
    lab.velocity_pitch = (lab.velocity_pitch
        + axis(&keys, KeyCode::KeyI, KeyCode::KeyK) * 30.0 * seconds)
        .clamp(-90.0, 90.0);
    lab.speed_log =
        (lab.speed_log + axis(&keys, KeyCode::Equal, KeyCode::Minus) * seconds).clamp(-1.0, 2.0);
    lab.turn(
        axis(&keys, KeyCode::KeyS, KeyCode::KeyW),
        axis(&keys, KeyCode::KeyD, KeyCode::KeyA),
        axis(&keys, KeyCode::KeyE, KeyCode::KeyQ),
        seconds,
    );
}

fn draw(
    lab: Res<Lab>,
    mut balls: Query<&mut Navball>,
    mut images: ResMut<Assets<Image>>,
    mut labels: Query<(&mut Text, &mut Node, &mut TextColor, &mut Visibility), With<NavballLabel>>,
    mut readout: Single<&mut Text, (With<Readout>, Without<NavballLabel>)>,
) {
    let velocity = lab.horizon(lab.velocity_heading, lab.velocity_pitch) * lab.speed();
    let input = lab.input(velocity);
    let mut reading: Option<NavballReadout> = None;
    for mut ball in &mut balls {
        reading = Some(draw_navball(&mut ball, &input, &mut images, &mut labels));
    }
    let Some(r) = reading else { return };
    readout.0 = format!(
        "NAVBALL LAB\n\n\
         latitude          {:>7.1} deg   [ ]\n\
         nose heading      {:>7.1} deg   A D\n\
         nose pitch        {:>7.1} deg   W S\n\
         roll              {:>7.1} deg   Q E\n\
         velocity heading  {:>7.1} deg   J L\n\
         velocity pitch    {:>7.1} deg   I K\n\
         speed             {:>7.2} m/s   - =\n\n\
         HDG {:05.1} deg   pitch {:.1} deg\n\
         speed {:.2} m/s{}\n\n\
         R: start again",
        lab.latitude,
        lab.heading,
        lab.pitch,
        lab.roll,
        lab.velocity_heading,
        lab.velocity_pitch,
        lab.speed(),
        r.heading,
        r.pitch,
        r.speed,
        if r.speed < MARKER_MIN_SPEED {
            "  (markers hidden)"
        } else {
            ""
        },
    );
}
