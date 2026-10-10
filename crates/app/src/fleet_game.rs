//! The main game: one world, one entry. Situations are set up in game with DEV "place ship".
mod docking;
mod hud;
mod place;
#[cfg(test)]
mod tests;
mod ui;
use crate::{overlay::unique_edges, tiles::Tile};
use bevy::{
    ecs::system::SystemParam,
    input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll},
    pbr::wireframe::WireframePlugin,
    prelude::*,
    render::settings::{WgpuFeatures, WgpuSettings},
};
use docking::{Docking, docking_controls, docking_description, port_pose, refresh_ports};
use glam::{DQuat, DVec3};
use hud::*;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use void_assembly::{Craft, Module, import_craft};
use void_assembly_lab::parts::RenderAssets;
use void_fleet_flight::presentation::{Toggle, ViewCommand};
use void_fleet_flight::session::{Action, FlightSession, Outcome, Playback, Recording};
use void_lod::{LodCamera, LodView};
use void_vessels::nearby_site;

const RATES: [f64; 9] = crate::flight::TIME_RATES;
/// F6 writes and F7 reads this file, relative to the working directory.
const QUICKSAVE: &str = "saves/quicksave.json";
const SCREENSHOTS: &str = "screenshots";

/// The flight simulation and its clock. NonSend: the session's ephemeris is not `Send`.
pub(crate) struct Flight {
    session: FlightSession,
    /// The craft N and O launch.
    craft: Craft,
    paused: bool,
    rate: usize,
    /// Simulated frames, for periodic journal marks.
    frames: usize,
    playback: Option<Playback>,
    /// The active `--record` journal.
    recording: Option<PathBuf>,
    /// Craft launched beside the launch site with N.
    ground_spawns: u32,
}
impl Flight {
    fn new(session: FlightSession, craft: Craft) -> Self {
        Self {
            session,
            craft,
            paused: true,
            rate: 0,
            frames: 0,
            playback: None,
            recording: None,
            ground_spawns: 0,
        }
    }
}
impl Drop for Flight {
    fn drop(&mut self) {
        if !std::thread::panicking()
            && let Some(path) = self.recording.take()
        {
            self.session.finish_stream();
            eprintln!("Fleet recording saved: {}", path.display());
        }
    }
}
/// The last message for the pilot: refusals, confirmations.
#[derive(Resource, Default)]
pub(crate) struct Notice(String);
/// The automatic coast forecast of the selected vessel.
#[derive(Resource)]
pub(crate) struct Forecast {
    coast: Option<void_landing::CoastPrediction>,
    at: f64,
    generation: u64,
}
impl Default for Forecast {
    fn default() -> Self {
        Self {
            coast: None,
            at: f64::NEG_INFINITY,
            generation: 0,
        }
    }
}
/// The camera this frame, sampled from the journalled presentation.
#[derive(Resource, Default)]
pub(crate) struct CameraView {
    view: Option<void_view::ViewState>,
    eye: DVec3,
    focus: DVec3,
    pointer_over_label: bool,
}
/// Map trajectories and body paths, cached between frames.
#[derive(Resource, Default)]
pub(crate) struct MapPlots {
    bodies: void_view::plot::BodyPlots,
    coast: void_view::plot::PlotPath,
    plan: void_view::plot::PlotPath,
    plan_vessel: String,
}
/// Render entities of parts and collider outlines; `rebuild` drops them after a world change.
#[derive(Resource, Default)]
pub(crate) struct PartVisuals {
    parts: HashMap<String, Vec<Entity>>,
    collision: HashMap<(u64, String), Vec<Vec3>>,
    rebuild: bool,
}
/// `--profile <file>`: CPU spans written on exit or F9.
#[derive(Resource, Default)]
pub(crate) struct Profiling(Option<(void_diagnostics::Profiler, PathBuf)>);
impl Drop for Profiling {
    fn drop(&mut self) {
        if !std::thread::panicking()
            && let Some((profile, path)) = self.0.take()
        {
            profile.write(path);
        }
    }
}
/// The world's terrain, air and far-body scenery.
#[derive(Resource)]
pub(crate) struct Ground(Box<crate::world_scenery::WorldScenery>);

/// What pilot actions change: the flight and the state derived from it.
#[derive(SystemParam)]
pub(crate) struct Pilot<'w> {
    flight: NonSendMut<'w, Flight>,
    notice: ResMut<'w, Notice>,
    docking: ResMut<'w, Docking>,
    forecast: ResMut<'w, Forecast>,
    visuals: ResMut<'w, PartVisuals>,
}

fn neutral_pilot(session: &mut FlightSession) {
    let id = &session.sim().selected;
    let c = session.sim().fleet.control(id);
    let rcs = session.sim().fleet.rcs_control(id);
    if c.turn != DVec3::ZERO {
        session.execute(Action::Control {
            throttle: c.throttle,
            turn: DVec3::ZERO,
        });
    }
    if rcs.force != DVec3::ZERO || rcs.torque != DVec3::ZERO {
        session.execute(Action::Rcs {
            control: void_vessels::RcsControl {
                enabled: rcs.enabled,
                ..Default::default()
            },
        });
    }
}
fn select_pilot(pilot: &mut Pilot, id: &str) {
    let session = &mut pilot.flight.session;
    session.sim().fleet.snapshot(id);
    neutral_pilot(session);
    // Dock already selects its surviving owner. Reselecting it resets the user's camera.
    if session.sim().selected != id {
        session.execute(Action::Select { vessel: id.into() });
    }
    neutral_pilot(session);
    pilot.docking.own = docking::ports(session, true).first().cloned();
    pilot.docking.target = None;
    refresh_ports(&mut pilot.docking, &pilot.flight.session);
}
fn scenery_preset(pilot: &mut Pilot, body: usize, view: &str) {
    let sim = pilot.flight.session.sim();
    let fleet = &sim.fleet;
    let radius = fleet.ephemeris.bodies()[body].radius_meters;
    let emissive = sim
        .world
        .bodies
        .get(&fleet.ephemeris.bodies()[body].id)
        .is_some_and(|d| {
            matches!(
                d.visual.surface,
                void_scenery::solar::SurfaceRecipe::EmissiveStar { .. }
            )
        });
    let root = fleet
        .ephemeris
        .bodies()
        .iter()
        .find(|b| {
            b.parent_index.is_none()
                && fleet.ephemeris.system_of(b.index) == fleet.ephemeris.system_of(body)
        })
        .expect("world system root")
        .index;
    let local = if body == root {
        DVec3::new(1.0, 0.2, 0.3).normalize()
    } else {
        let sun = fleet
            .frames()
            .transform(fleet.body_frames(root).0, fleet.body_frames(body).1)
            .apply_point(DVec3::ZERO)
            .normalize();
        let east = if sun.z.abs() < 0.99 {
            DVec3::Z.cross(sun).normalize()
        } else {
            DVec3::X.cross(sun).normalize()
        };
        (sun + east * 0.7 + DVec3::Z * 0.15).normalize()
    };
    let direction = fleet
        .frames()
        .transform(fleet.body_frames(body).1, fleet.origin_frame())
        .apply_direction(local);
    let ratio = match view {
        "near" => 1.025,
        "orbit" => {
            if fleet.ephemeris.bodies()[body].id.rsplit('/').next() == Some("halo") {
                6.0
            } else {
                3.5
            }
        }
        "far" => 12.0,
        _ => panic!("view must be near/orbit/far"),
    };
    let name = fleet.ephemeris.bodies()[body].name.clone();
    pilot.flight.session.execute(Action::View {
        command: ViewCommand::BodyPreset {
            body,
            direction,
            distance: radius * ratio,
        },
    });
    pilot.flight.session.execute(Action::View {
        command: ViewCommand::Exposure {
            value: if emissive { 0.1 } else { 6.309_573 },
        },
    });
    pilot.notice.0 = format!("{name} {view} view; Home returns to ship");
}
/// F1: cycle the observed body's near / orbit / far view.
fn next_body_view(pilot: &mut Pilot) {
    let sim = pilot.flight.session.sim();
    let body = sim.observation_body();
    let radius = sim.fleet.ephemeris.bodies()[body].radius_meters;
    let ratio = sim.presentation.distance / radius;
    let view = if ratio < 1.1 {
        "orbit"
    } else if ratio < 10.0 {
        "far"
    } else {
        "near"
    };
    scenery_preset(pilot, body, view);
}

/// A part's placement in the render world, from its own parts frame.
fn part_transform(
    to_camera: &mut impl FnMut(void_frames::FrameId) -> void_frames::Transform,
    p: &void_vessels::PartSnapshot,
) -> Transform {
    let into = to_camera(p.frame);
    Transform::from_translation(into.apply_point(p.local_position).as_vec3())
        .with_rotation((into.rotation() * p.local_rotation).as_quat())
}
#[derive(Component)]
struct MainCamera;
#[derive(Component)]
struct SceneSun;
#[derive(Component)]
struct Hud;
#[derive(Component)]
struct NavballHeading;
#[derive(Component)]
struct Visual {
    id: String,
    local: Transform,
    flame: bool,
    wheel: Option<String>,
}

const HELP: &str = "VOID\n\
Usage: void-app [--craft <craft.json>] [--load <save.json>] [--record <journal>] [--profile <file>]\n\
       void-app --replay <journal> [--profile <file>]\n\
       void-app --verify <journal> [--profile <file>]\n\
       void-app --recover-recording <stream> --output <journal>\n\
\n\
--craft     fly this craft (JSON) instead of the default rocket; crafts/ has an aircraft,\n\
            a rover and a reentry capsule\n\
--load      start from a world save (F6 writes saves/quicksave.json)\n\
--record    journal every command so the session can be replayed or verified\n\
--replay    play a journal back in the window, checking every state mark\n\
--verify    replay a journal without a window and report the end state\n\
--recover-recording  turn an interrupted --record stream into a journal\n\
--profile   write CPU timing spans (JSON) on exit or F9\n\
\n\
In game: ` DEV panel (place ship, view toggles) | ? keys";

#[derive(Default)]
struct Arguments {
    craft: Option<String>,
    load: Option<String>,
    record: Option<String>,
    replay: Option<String>,
    verify: Option<String>,
    recover: Option<String>,
    output: Option<String>,
    profile: Option<String>,
    help: bool,
}
impl Arguments {
    /// Unknown or repeated arguments and meaningless combinations are errors, never ignored.
    fn parse(args: impl IntoIterator<Item = String>) -> Result<Self, String> {
        let mut parsed = Self::default();
        let mut args = args.into_iter();
        while let Some(name) = args.next() {
            if name == "--help" {
                parsed.help = true;
                continue;
            }
            let slot = match name.as_str() {
                "--craft" => &mut parsed.craft,
                "--load" => &mut parsed.load,
                "--record" => &mut parsed.record,
                "--replay" => &mut parsed.replay,
                "--verify" => &mut parsed.verify,
                "--recover-recording" => &mut parsed.recover,
                "--output" => &mut parsed.output,
                "--profile" => &mut parsed.profile,
                _ => return Err(format!("unknown argument {name:?}; see --help")),
            };
            let value = args.next().ok_or(format!("{name} needs a value"))?;
            if slot.replace(value).is_some() {
                return Err(format!("{name} given twice"));
            }
        }
        let a = &parsed;
        if a.output.is_some() != a.recover.is_some() {
            return Err("--recover-recording and --output go together".into());
        }
        if a.recover.is_some()
            && (a.craft.is_some()
                || a.load.is_some()
                || a.record.is_some()
                || a.replay.is_some()
                || a.verify.is_some()
                || a.profile.is_some())
        {
            return Err("--recover-recording takes only --output".into());
        }
        if a.verify.is_some()
            && (a.craft.is_some() || a.load.is_some() || a.record.is_some() || a.replay.is_some())
        {
            return Err("--verify takes only --profile".into());
        }
        if a.replay.is_some() && (a.craft.is_some() || a.load.is_some() || a.record.is_some()) {
            return Err(
                "--replay brings its own world; it cannot take --craft, --load or --record".into(),
            );
        }
        if a.load.is_some() && a.craft.is_some() {
            return Err("--load brings its own craft; it cannot take --craft".into());
        }
        Ok(parsed)
    }
}

pub fn run() {
    let args = Arguments::parse(std::env::args().skip(1)).unwrap_or_else(|e| panic!("{e}"));
    if args.help {
        println!("{HELP}");
        return;
    }
    if let (Some(path), Some(output)) = (&args.recover, &args.output) {
        let recovery = void_fleet_flight::session::durable::Recovery::read(path);
        recovery.write(output);
        println!(
            "Recovered Fleet recording: {} committed actions, pending command {}, {} EOF bytes discarded; report {}.recovery.json",
            recovery.recording.entries.len(),
            recovery.pending.is_some(),
            recovery.discarded_tail_bytes,
            output
        );
        return;
    }
    if let Some(path) = &args.verify {
        let mut profile = void_diagnostics::Profiler::new();
        let started = std::time::Instant::now();
        let session = FlightSession::load(path);
        profile.span("headless_verify", started, std::time::Instant::now());
        if let Some(output) = &args.profile {
            profile.write(output);
        }
        println!(
            "Verified Fleet session: T+{:.6} s, {} vessels, selected {}",
            session.sim().fleet.time(),
            session.sim().fleet.vessel_ids().len(),
            session.sim().selected
        );
        return;
    }
    let mut flight = if let Some(path) = &args.replay {
        let (playback, session) = Playback::new(Recording::read(path));
        let craft = session.recording_initial().craft.clone();
        let mut flight = Flight::new(session, craft);
        flight.playback = Some(playback);
        flight.paused = false;
        flight
    } else if let Some(path) = &args.load {
        let session = FlightSession::load_checkpoint(path);
        let craft = session.recording_initial().craft.clone();
        Flight::new(session, craft)
    } else {
        let craft = args
            .craft
            .as_ref()
            .map_or_else(void_assembly::rcs_flight_rocket, |path| {
                import_craft(
                    &std::fs::read_to_string(path)
                        .unwrap_or_else(|e| panic!("read craft {path}: {e}")),
                )
                .unwrap_or_else(|e| panic!("invalid craft {path}: {e:?}"))
            });
        let mut flight = Flight::new(
            FlightSession::new(void_fleet_flight::world::main_game(&craft)),
            craft,
        );
        flight.paused = false;
        flight.session.execute(Action::View {
            command: ViewCommand::Configure { main_camera: true },
        });
        flight.session.execute(Action::EndFrame {
            paused: false,
            rate: 0,
        });
        flight
    };
    if let Some(path) = &args.record {
        flight.session.begin_stream(path);
        flight.recording = Some(path.into());
    }
    let profiling = Profiling(
        args.profile
            .map(|path| (void_diagnostics::Profiler::new(), path.into())),
    );
    let mut app = App::new();
    app.add_plugins((
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "VOID".into(),
                    ..default()
                }),
                ..default()
            })
            .set(bevy::render::RenderPlugin {
                render_creation: WgpuSettings {
                    features: WgpuFeatures::POLYGON_MODE_LINE,
                    ..default()
                }
                .into(),
                ..default()
            }),
        WireframePlugin::default(),
        crate::scenery::SceneryPlugin,
    ))
    .insert_resource(ClearColor(Color::BLACK))
    .insert_resource(GlobalAmbientLight {
        brightness: 40.0,
        color: Color::srgb_u8(0xcb, 0xe7, 0xff),
        ..default()
    })
    .insert_resource(profiling);
    insert_game(&mut app, flight);
    app.add_systems(
        Update,
        (
            begin_profile_frame,
            ui::interactions,
            ui::scroll_panels,
            controls,
            simulate,
            refresh_scenery,
            draw,
            draw_map,
            instruments,
            ui::refresh,
            ui::stages,
            ui::indicators,
            ui::apply_font,
            update_scenery,
            capture_frame,
        )
            .chain(),
    );
    app.run();
}
/// The game's state and startup, shared by the window and headless tests.
fn insert_game(app: &mut App, flight: Flight) {
    app.insert_non_send(flight)
        .init_resource::<Notice>()
        .init_resource::<Docking>()
        .init_resource::<Forecast>()
        .init_resource::<CameraView>()
        .init_resource::<MapPlots>()
        .init_resource::<PartVisuals>()
        .init_resource::<Profiling>()
        .add_systems(Startup, (setup, setup_scenery).chain());
}
// Capture the GPU window image independently of the desktop/VNC presentation path.
fn capture_frame(mut commands: Commands, keys: Res<ButtonInput<KeyCode>>) {
    if keys.just_pressed(KeyCode::PrintScreen) {
        use bevy::render::view::screenshot::{Screenshot, save_to_disk};
        std::fs::create_dir_all(SCREENSHOTS).expect("create screenshot directory");
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("screenshot clock predates Unix epoch")
            .as_nanos();
        let path = format!("{SCREENSHOTS}/frame-{}-{stamp}.png", std::process::id());
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(path));
    }
}
#[allow(clippy::too_many_arguments)]
fn setup(
    mut commands: Commands,
    flight: NonSend<Flight>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut fonts: ResMut<Assets<Font>>,
    window: Single<&Window>,
) {
    let assets = RenderAssets::new(&mut meshes, &mut materials);
    commands.insert_resource(assets);
    commands.spawn((
        Camera3d::default(),
        Transform::default(),
        bevy::ui::IsDefaultUiCamera,
        MainCamera,
    ));
    commands.spawn((
        SceneSun,
        DirectionalLight {
            illuminance: 1000.0,
            ..default()
        },
        Transform::default().looking_to(Vec3::new(-1.0, -0.4, -0.7), Vec3::Z),
    ));
    crate::map::spawn_map_labels(&mut commands, flight.session.sim().fleet.ephemeris.bodies());
    commands.insert_resource(place::PlaceDraft::new(flight.session.sim()));
    ui::spawn(
        &mut commands,
        &mut images,
        &mut fonts,
        f64::from(window.scale_factor()),
    );
}

#[allow(clippy::type_complexity)]
fn instruments(
    flight: NonSend<Flight>,
    mut balls: Query<&mut crate::navball::Navball>,
    mut images: ResMut<Assets<Image>>,
    mut labels: Query<
        (&mut Text, &mut Node, &mut TextColor, &mut Visibility),
        With<crate::navball::NavballLabel>,
    >,
    mut heading: Query<&mut Text, (With<NavballHeading>, Without<crate::navball::NavballLabel>)>,
) {
    if balls.is_empty() {
        return;
    }
    let sim = flight.session.sim();
    let fleet = &sim.fleet;
    let ship = fleet.snapshot(&sim.selected);
    let reference = sim.navigation_body(&sim.selected);
    let local = fleet
        .frames()
        .transform(
            fleet.vessel_frame(&sim.selected),
            fleet.body_frames(reference).1,
        )
        .apply_state(void_frames::State {
            position: fleet.centre_of_mass_local(&sim.selected),
            velocity: DVec3::ZERO,
        });
    let inertial = fleet
        .frames()
        .transform(
            fleet.vessel_frame(&sim.selected),
            fleet.body_frames(reference).0,
        )
        .apply_state(void_frames::State {
            position: fleet.centre_of_mass_local(&sim.selected),
            velocity: DVec3::ZERO,
        });
    let inertial_axes = fleet
        .frames()
        .transform(fleet.body_frames(reference).0, fleet.origin_frame())
        .rotation();
    let q = surface_axes(fleet, reference);
    let (nose, top) =
        void_fleet_flight::placement::nose_and_top(fleet.control_profile(&sim.selected));
    let input = void_navball::NavballInput {
        nose: (ship.rotation * nose).normalize(),
        top: (ship.rotation * top).normalize(),
        up: q * local.position.normalize(),
        pole: fleet.ephemeris.bodies()[reference].rotation.axis(),
        prime_meridian: q * DVec3::X,
        velocity: if sim.presentation.speed_surface {
            q * local.velocity
        } else {
            inertial_axes * inertial.velocity
        },
    };
    for mut ball in &mut balls {
        let reading = crate::navball::draw_navball(&mut ball, &input, &mut images, &mut labels);
        for mut text in &mut heading {
            text.0 = format!(
                "HDG {:03}° · {:+.0}°",
                reading.heading.round() as i64 % 360,
                reading.pitch
            );
        }
    }
}
/// A body's surface axes in origin-frame coordinates; the render world uses the observed body's.
fn surface_axes(fleet: &void_vessels::Fleet, body: usize) -> glam::DQuat {
    fleet
        .frames()
        .transform(fleet.body_frames(body).1, fleet.origin_frame())
        .rotation()
}
fn crew_transfer(pilot: &mut Pilot) {
    let fleet = &pilot.flight.session.sim().fleet;
    let selected = pilot.flight.session.sim().selected.clone();
    let action = if fleet.eva_crew(&selected).is_some() {
        let frames = fleet.frames();
        let mut candidates = Vec::new();
        for carrier in fleet.vessel_ids().into_iter().filter(|id| id != &selected) {
            for seat in fleet
                .crew_seats(&carrier)
                .into_iter()
                .filter(|s| s.occupant.is_none())
            {
                let hatch = frames
                    .transform(fleet.part_frame(&seat.part), fleet.vessel_frame(&selected))
                    .apply_point(seat.parameters.hatch_position);
                candidates.push((hatch.length_squared(), seat.part, seat.module));
            }
        }
        candidates.sort_by(|a, b| a.0.total_cmp(&b.0));
        let Some((_, part, module)) = candidates.into_iter().next() else {
            pilot.notice.0 = "Board refused: no empty seat".into();
            return;
        };
        Action::EvaBoard { part, module }
    } else {
        let Some(seat) = fleet
            .crew_seats(&selected)
            .into_iter()
            .find(|s| s.occupant.is_some())
        else {
            pilot.notice.0 = "Exit refused: no crew in selected vehicle".into();
            return;
        };
        Action::EvaExit {
            part: seat.part,
            module: seat.module,
        }
    };
    match pilot.flight.session.execute(action) {
        Outcome::Refused(reason) => pilot.notice.0 = reason,
        Outcome::Spawned(_) => {
            pilot.notice.0.clear();
            pilot.forecast.coast = None;
            pilot.docking.own = None;
            pilot.docking.target = None;
        }
        other => panic!("unexpected crew transfer outcome {other:?}"),
    }
}

fn axis(keys: &ButtonInput<KeyCode>, plus: KeyCode, minus: KeyCode) -> f64 {
    keys.pressed(plus) as i32 as f64 - keys.pressed(minus) as i32 as f64
}
fn save_game(pilot: &mut Pilot) {
    pilot.flight.session.save_checkpoint(QUICKSAVE);
    pilot.notice.0 = format!("Saved {}", absolute(QUICKSAVE));
}
fn load_game(pilot: &mut Pilot) {
    if !std::path::Path::new(QUICKSAVE).exists() {
        pilot.notice.0 = format!("Load refused: no save at {}", absolute(QUICKSAVE));
        return;
    }
    let checkpoint = void_fleet_flight::checkpoint::FlightCheckpoint::read(QUICKSAVE);
    let flight = &mut *pilot.flight;
    flight.session.execute(Action::LoadWorld {
        checkpoint: Box::new(checkpoint),
    });
    flight.craft = flight.session.recording_initial().craft.clone();
    flight.paused = true;
    flight.rate = 0;
    pilot.visuals.rebuild = true;
    pilot.forecast.coast = None;
    pilot.docking.own = None;
    pilot.docking.target = None;
    neutral_pilot(&mut pilot.flight.session);
    pilot.notice.0 = format!("Loaded {}", absolute(QUICKSAVE));
}
fn absolute(path: &str) -> String {
    std::env::current_dir()
        .expect("working directory")
        .join(path)
        .display()
        .to_string()
}
#[allow(clippy::too_many_arguments)]
fn controls(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    window: Single<&Window>,
    markers: Query<(&Interaction, &crate::map::MapMarker)>,
    ui_state: Res<ui::UiState>,
    mut camera: ResMut<CameraView>,
    mut profiling: ResMut<Profiling>,
    mut pilot: Pilot,
) {
    let pilot = &mut pilot;
    if ui_state.editing {
        if pilot.flight.playback.is_none() {
            neutral_pilot(&mut pilot.flight.session);
        }
        return;
    }
    let empty_buttons = ButtonInput::default();
    let empty_motion = AccumulatedMouseMotion::default();
    let empty_scroll = AccumulatedMouseScroll::default();
    let over_ui = ui_state.pointer;
    let buttons = if over_ui { &empty_buttons } else { &*buttons };
    let motion = if over_ui { &empty_motion } else { &*motion };
    let scroll = if over_ui { &empty_scroll } else { &*scroll };
    let (over_label, clicked) =
        crate::map::label_click(&markers, buttons, camera.view.map_or(0.0, |s| s.map_weight));
    if pilot.flight.playback.is_none()
        && let Some(kind) = clicked
    {
        let bodies = pilot.flight.session.sim().fleet.ephemeris.bodies();
        let body = match kind {
            void_view::LabelKind::Body(i) => Some(i),
            void_view::LabelKind::Star => Some(
                bodies
                    .iter()
                    .find(|b| b.parent_index.is_none())
                    .expect("root body")
                    .index,
            ),
            void_view::LabelKind::Vessel | void_view::LabelKind::Apsis => None,
        };
        pilot.flight.session.execute(Action::View {
            command: ViewCommand::Focus { body },
        });
    }
    camera.pointer_over_label = over_label || over_ui;
    if !window.focused {
        if pilot.flight.playback.is_none() {
            neutral_pilot(&mut pilot.flight.session);
        }
        return;
    }
    if pilot.flight.playback.is_some() {
        if keys.just_pressed(KeyCode::KeyP) {
            pilot.flight.paused = !pilot.flight.paused;
        }
        return;
    }
    if keys.just_pressed(KeyCode::F6) {
        save_game(pilot);
    }
    if keys.just_pressed(KeyCode::F7) {
        load_game(pilot);
    }
    if keys.just_pressed(KeyCode::F8) {
        if let Some(path) = pilot.flight.recording.take() {
            pilot.flight.session.finish_stream();
            pilot.notice.0 = format!("Recording finished: {}", path.display());
        } else {
            pilot.notice.0 = "No recording active; start with --record <file>".into();
        }
    }
    if keys.just_pressed(KeyCode::F9) {
        if let Some((profile, path)) = profiling.0.take() {
            profile.write(&path);
            pilot.notice.0 = format!("CPU profile finished: {}", path.display());
        } else {
            pilot.notice.0 = "No CPU profile active; start with --profile <file>".into();
        }
    }
    if keys.just_pressed(KeyCode::KeyP) {
        pilot.flight.paused = !pilot.flight.paused;
        if pilot.flight.paused {
            neutral_pilot(&mut pilot.flight.session);
        }
    }
    if keys.just_pressed(KeyCode::KeyR) {
        let initial = pilot.flight.session.recording_initial().clone();
        pilot.flight.session.execute(Action::ResetWorld {
            initial: Box::new(initial),
        });
        let flight = &mut *pilot.flight;
        flight.paused = true;
        flight.rate = 0;
        flight.ground_spawns = 0;
        pilot.visuals.rebuild = true;
        pilot.forecast.coast = None;
        pilot.docking.own = None;
        pilot.docking.target = None;
        pilot.notice.0.clear();
    }
    if keys.just_pressed(KeyCode::Tab)
        && keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight])
    {
        let bodies = pilot.flight.session.sim().fleet.ephemeris.bodies();
        let body = match pilot.flight.session.sim().presentation.focus_body {
            None => Some(0),
            Some(i) if i + 1 < bodies.len() => Some(i + 1),
            Some(_) => None,
        };
        pilot.flight.session.execute(Action::View {
            command: ViewCommand::Focus { body },
        });
    } else if keys.just_pressed(KeyCode::Tab) {
        let old = pilot.flight.session.sim().selected.clone();
        let ids = pilot.flight.session.sim().fleet.vessel_ids();
        let i = ids
            .iter()
            .position(|id| *id == old)
            .expect("selected vessel");
        select_pilot(pilot, &ids[(i + 1) % ids.len()]);
        pilot.forecast.coast = None;
    }
    if keys.just_pressed(KeyCode::KeyO) {
        let sim = pilot.flight.session.sim();
        let body = sim.fleet.ephemeris.bodies()[sim.observation_body()]
            .id
            .clone();
        let craft = pilot.flight.craft.clone();
        let Outcome::Spawned(id) = pilot.flight.session.execute(Action::LaunchOrbitAt {
            body,
            craft,
            offset: DVec3::ZERO,
        }) else {
            unreachable!()
        };
        select_pilot(pilot, &id);
    }
    if keys.just_pressed(KeyCode::KeyN) {
        let flight = &mut *pilot.flight;
        flight.ground_spawns += 1;
        let site = nearby_site(
            flight.session.sim().launch_site,
            30.0 * flight.ground_spawns as f64,
            flight.session.sim().planet.terrain.radius_meters,
        );
        let craft = flight.craft.clone();
        flight.session.execute(Action::LaunchGround { craft, site });
    }
    if keys.just_pressed(KeyCode::KeyF) {
        crew_transfer(pilot);
        return;
    }
    let warp_was_active = pilot.flight.session.sim().maneuver_warp.active();
    let id = pilot.flight.session.sim().selected.clone();
    let paused = pilot.flight.paused;
    let commanded = pilot.flight.session.sim().fleet.has_command(&id);
    if !commanded
        && keys.any_pressed([
            KeyCode::KeyW,
            KeyCode::KeyA,
            KeyCode::KeyS,
            KeyCode::KeyD,
            KeyCode::KeyQ,
            KeyCode::KeyE,
            KeyCode::ShiftLeft,
            KeyCode::ShiftRight,
        ])
    {
        pilot.notice.0 = if pilot.flight.session.sim().fleet.requires_crew(&id) {
            "Control refused: healthy pilot must occupy a healthy seat".into()
        } else {
            "Control refused: command capability unavailable or thermally failed".into()
        };
    }
    let profile = pilot.flight.session.sim().fleet.control_profile(&id);
    let vehicle = profile == Some(void_assembly::ControlProfile::Rover);
    let eva = profile == Some(void_assembly::ControlProfile::Eva);
    let session = &mut pilot.flight.session;
    if vehicle {
        let previous = session
            .sim()
            .fleet
            .vehicle_control(&id)
            .expect("wheel controls");
        let drive = if commanded && !paused {
            axis(&keys, KeyCode::KeyW, KeyCode::KeyS)
        } else {
            0.0
        };
        let steer = if commanded && !paused {
            axis(&keys, KeyCode::KeyD, KeyCode::KeyA)
        } else {
            0.0
        };
        let brake = if keys.pressed(KeyCode::Space) || !commanded {
            1.0
        } else if keys.just_pressed(KeyCode::KeyX) {
            if previous.brake > 0.0 { 0.0 } else { 1.0 }
        } else if drive != 0.0 {
            0.0
        } else {
            previous.brake
        };
        let control = void_assembly::VehicleControl {
            drive,
            steer,
            brake,
        };
        if control != previous {
            session.execute(Action::Vehicle { control });
        }
    }
    if eva {
        let pack = session.sim().fleet.rcs_control(&id).enabled;
        let control = if commanded && !paused && !pack {
            void_assembly::EvaControl {
                forward: axis(&keys, KeyCode::KeyW, KeyCode::KeyS),
                strafe: axis(&keys, KeyCode::KeyD, KeyCode::KeyA),
                yaw: axis(&keys, KeyCode::KeyE, KeyCode::KeyQ),
            }
        } else {
            void_assembly::EvaControl::default()
        };
        if session.sim().fleet.eva_control(&id) != Some(control)
            && let Outcome::Refused(reason) = session.execute(Action::Eva { control })
        {
            pilot.notice.0 = reason;
        }
        if keys.just_pressed(KeyCode::Space)
            && let Outcome::Refused(reason) = session.execute(Action::EvaJump)
        {
            pilot.notice.0 = reason;
        }
    }
    if keys.just_pressed(KeyCode::KeyT) && commanded && !eva {
        let enabled = session.sim().fleet.sas_phase(&id) == void_vessels::SasPhase::Off;
        if enabled && !session.sim().fleet.has_reaction_wheel(&id) {
            pilot.notice.0 =
                "SAS unavailable: aircraft has aerodynamic controls, no reaction wheel".into();
        } else {
            session.execute(Action::Sas { enabled });
        }
    }
    let mut c = session.sim().fleet.control(&id);
    let dt = time.delta_secs_f64().min(0.05);
    let throttle_axis = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]) as i32
        - keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]) as i32;
    if !session.sim().fleet.command_failed(&id)
        && !keys.just_pressed(KeyCode::Tab)
        && !keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight])
    {
        c.throttle = (c.throttle + f64::from(throttle_axis) * dt * 0.5).clamp(0.0, 1.0);
    }
    if keys.just_pressed(KeyCode::KeyX) {
        c.throttle = 0.0;
    }
    let turn = if commanded
        && !paused
        && !vehicle
        && (!eva || session.sim().fleet.rcs_control(&id).enabled)
    {
        DVec3::new(
            axis(&keys, KeyCode::KeyS, KeyCode::KeyW),
            if profile == Some(void_assembly::ControlProfile::Aircraft) {
                // +Z nose / +Y top: player right is -X, hence negative yaw about +Y.
                axis(&keys, KeyCode::KeyQ, KeyCode::KeyE)
            } else {
                axis(&keys, KeyCode::KeyE, KeyCode::KeyQ)
            },
            axis(&keys, KeyCode::KeyD, KeyCode::KeyA),
        )
    } else {
        DVec3::ZERO
    };
    let mut rcs = session.sim().fleet.rcs_control(&id);
    if keys.just_pressed(KeyCode::KeyH) && commanded {
        let available = session.sim().fleet.part_snapshots(&id).iter().any(|p| {
            p.definition
                .modules
                .iter()
                .any(|m| matches!(m, Module::Rcs { .. }))
        });
        if available {
            rcs.enabled = !rcs.enabled;
        } else {
            pilot.notice.0 = "RCS unavailable: selected vessel has no nozzles".into();
        }
    }
    let translate = keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]);
    rcs.force = if commanded && !paused && rcs.enabled && translate {
        DVec3::new(
            axis(&keys, KeyCode::KeyD, KeyCode::KeyA),
            axis(&keys, KeyCode::KeyE, KeyCode::KeyQ),
            axis(&keys, KeyCode::KeyW, KeyCode::KeyS),
        ) * 80.0
    } else {
        DVec3::ZERO
    };
    if eva {
        rcs.force.x = -rcs.force.x;
    }
    let pack_turn = if eva {
        DVec3::new(turn.x, -turn.y, turn.z)
    } else {
        turn
    };
    rcs.torque = if rcs.enabled && !translate {
        pack_turn * 30.0
    } else {
        DVec3::ZERO
    };
    if rcs.torque != DVec3::ZERO
        && session.sim().fleet.sas_phase(&id) != void_vessels::SasPhase::Off
    {
        session.execute(Action::Sas { enabled: false });
        pilot.notice.0 = "Manual RCS torque disengaged SAS reaction wheel".into();
    }
    c.turn = if rcs.enabled || translate {
        DVec3::ZERO
    } else {
        turn
    };
    if rcs != session.sim().fleet.rcs_control(&id) {
        session.execute(Action::Rcs { control: rcs });
    }
    if profile == Some(void_assembly::ControlProfile::Aircraft)
        && session.sim().fleet.has_wheels(&id)
    {
        let wheel_control = void_assembly::VehicleControl {
            drive: 0.0,
            steer: if paused { 0.0 } else { -turn.y },
            brake: if paused || keys.pressed(KeyCode::KeyB) {
                1.0
            } else {
                0.0
            },
        };
        if session.sim().fleet.vehicle_control(&id) != Some(wheel_control) {
            session.execute(Action::Vehicle {
                control: wheel_control,
            });
        }
    }
    let previous = session.sim().fleet.control(&id);
    if previous.throttle != c.throttle || previous.turn != c.turn {
        session.execute(Action::Control {
            throttle: c.throttle,
            turn: c.turn,
        });
    }
    if profile == Some(void_assembly::ControlProfile::Flight)
        && !keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight])
    {
        docking_controls(pilot, &keys);
    }
    if keys.just_pressed(KeyCode::Space) && !vehicle && !eva {
        pilot.flight.session.execute(Action::Stage);
        pilot.docking.own = None;
        pilot.docking.target = None;
        pilot.forecast.coast = None;
    }
    if keys.just_pressed(KeyCode::Period) {
        pilot.flight.rate = (pilot.flight.rate + 1).min(RATES.len() - 1);
        pilot.notice.0.clear();
    }
    if keys.just_pressed(KeyCode::Comma) {
        pilot.flight.rate = pilot.flight.rate.saturating_sub(1);
        pilot.notice.0.clear();
    }
    if keys.just_pressed(KeyCode::KeyC) {
        pilot.forecast.coast = Some(pilot.flight.session.predict(600.0));
    }
    if profile == Some(void_assembly::ControlProfile::Flight) {
        plan_controls(pilot, &keys);
    }
    if warp_was_active && !pilot.flight.session.sim().maneuver_warp.active() {
        pilot.flight.rate = 0;
    }
    plot_controls(pilot, &keys);
    if keys.just_pressed(KeyCode::F1) {
        next_body_view(pilot);
    }
    if keys.just_pressed(KeyCode::Home) && !keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight])
    {
        pilot.flight.session.execute(Action::View {
            command: ViewCommand::Focus { body: None },
        });
    }
    if keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]) {
        for (key, factor) in [(KeyCode::F10, 0.5), (KeyCode::F11, 2.0)] {
            if keys.just_pressed(key) {
                let value =
                    (pilot.flight.session.sim().presentation.exposure * factor).clamp(0.001, 100.0);
                pilot.flight.session.execute(Action::View {
                    command: ViewCommand::Exposure { value },
                });
            }
        }
    }
    view_controls(
        &mut pilot.flight.session,
        camera.pointer_over_label,
        &keys,
        buttons,
        motion,
        scroll,
    );
}
fn plot_controls(pilot: &mut Pilot, keys: &ButtonInput<KeyCode>) {
    use void_orbit::FrameSpec;
    let sim = pilot.flight.session.sim();
    let bodies = sim.fleet.ephemeris.bodies();
    let current = sim.presentation.plotting_frame;
    let (mut primary, mut secondary, mut mode) = match current {
        FrameSpec::Barycentric => (sim.home, None, 0),
        FrameSpec::BodyInertial { body } => (body, None, 1),
        FrameSpec::BodySurface { body } => (body, None, 2),
        FrameSpec::TwoBodyRotating { primary, secondary } => (primary, Some(secondary), 3),
    };
    let mut changed = false;
    for (key, value) in [
        (KeyCode::Digit1, 0),
        (KeyCode::Digit2, 1),
        (KeyCode::Digit3, 2),
        (KeyCode::Digit4, 3),
    ] {
        if keys.just_pressed(key) {
            mode = value;
            changed = true;
        }
    }
    if keys.just_pressed(KeyCode::KeyG) {
        mode = (mode + 1) % 4;
        changed = true;
    }
    if keys.just_pressed(KeyCode::KeyJ) {
        if keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]) {
            let candidates: Vec<_> = bodies
                .iter()
                .filter(|b| {
                    b.index != primary
                        && sim.fleet.ephemeris.system_of(b.index)
                            == sim.fleet.ephemeris.system_of(primary)
                })
                .map(|b| b.index)
                .collect();
            if candidates.is_empty() {
                pilot.notice.0 = "Two-body plot requires two bodies".into();
                return;
            }
            let next = secondary
                .and_then(|s| candidates.iter().position(|&i| i == s))
                .map_or(0, |i| (i + 1) % candidates.len());
            secondary = Some(candidates[next]);
            mode = 3;
        } else {
            primary = (primary + 1) % bodies.len();
            if secondary.is_some_and(|s| {
                s == primary
                    || sim.fleet.ephemeris.system_of(s) != sim.fleet.ephemeris.system_of(primary)
            }) {
                secondary = None;
            }
        }
        changed = true;
    }
    if !changed {
        return;
    }
    let frame = match mode {
        0 => FrameSpec::Barycentric,
        1 => FrameSpec::BodyInertial { body: primary },
        2 => FrameSpec::BodySurface { body: primary },
        3 => {
            let secondary = secondary
                .or_else(|| {
                    bodies
                        .iter()
                        .find(|b| b.parent_index == Some(primary))
                        .map(|b| b.index)
                })
                .or(bodies[primary].parent_index)
                .or_else(|| {
                    bodies
                        .iter()
                        .find(|b| {
                            b.index != primary
                                && sim.fleet.ephemeris.system_of(b.index)
                                    == sim.fleet.ephemeris.system_of(primary)
                        })
                        .map(|b| b.index)
                });
            let Some(secondary) = secondary else {
                pilot.notice.0 = "Two-body plot requires two bodies".into();
                return;
            };
            FrameSpec::TwoBodyRotating { primary, secondary }
        }
        _ => unreachable!(),
    };
    pilot.flight.session.execute(Action::View {
        command: ViewCommand::PlotFrame { frame },
    });
    pilot.notice.0 = "Plotting frame changed; physics frame unchanged".into();
}

fn plan_controls(pilot: &mut Pilot, keys: &ButtonInput<KeyCode>) {
    use void_orbit::{ManeuverSpec, ReferenceMode};
    let id = pilot.flight.session.sim().selected.clone();
    let run = |pilot: &mut Pilot, action: Action| match pilot.flight.session.execute(action) {
        Outcome::Applied => pilot.notice.0.clear(),
        Outcome::Refused(reason) => pilot.notice.0 = reason,
        other => panic!("unexpected maneuver outcome {other:?}"),
    };
    if keys.just_pressed(KeyCode::KeyM) {
        let sim = pilot.flight.session.sim();
        let fleet = &sim.fleet;
        let ship = fleet.snapshot(&id);
        let mut positions = vec![DVec3::ZERO; fleet.ephemeris.bodies().len()];
        fleet.ephemeris.positions_at(fleet.time(), &mut positions);
        let reference = void_orbit::DominanceTree::new(fleet.ephemeris.bodies())
            .dominant(&positions, ship.position);
        let start_time = sim
            .plans
            .get(&id)
            .and_then(|p| p.plan.burns().last())
            .map_or(fleet.time() + 60.0, |b| b.end_time + 60.0);
        run(
            pilot,
            Action::AddManeuver {
                spec: ManeuverSpec {
                    start_time,
                    reference_body: reference,
                    reference_mode: ReferenceMode::Auto,
                    prograde: 100.0,
                    normal: 0.0,
                    radial: 0.0,
                },
            },
        );
    }
    let Some(p) = pilot.flight.session.sim().plans.get(&id) else {
        return;
    };
    if p.plan.count() == 0 {
        return;
    }
    let selected = p.selected;
    let count = p.plan.count();
    let mut spec = p.plan.maneuver(selected);
    if keys.just_pressed(KeyCode::BracketLeft) {
        run(
            pilot,
            Action::SelectManeuver {
                index: selected.saturating_sub(1),
            },
        );
    }
    if keys.just_pressed(KeyCode::BracketRight) {
        run(
            pilot,
            Action::SelectManeuver {
                index: (selected + 1).min(count - 1),
            },
        );
    }
    let step = if keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]) {
        10.0
    } else {
        1.0
    };
    let prograde = axis(keys, KeyCode::ArrowUp, KeyCode::ArrowDown) * step;
    let normal = axis(keys, KeyCode::ArrowRight, KeyCode::ArrowLeft) * step;
    let radial = axis(keys, KeyCode::PageUp, KeyCode::PageDown) * step;
    let seconds = (f64::from(keys.pressed(KeyCode::End))
        - f64::from(
            keys.pressed(KeyCode::Home) && keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]),
        ))
        * step;
    if prograde != 0.0 || normal != 0.0 || radial != 0.0 || seconds != 0.0 {
        spec.prograde += prograde;
        spec.normal += normal;
        spec.radial += radial;
        spec.start_time += seconds;
        run(
            pilot,
            Action::EditManeuver {
                index: selected,
                spec,
            },
        );
    }
    if keys.just_pressed(KeyCode::KeyV) {
        let sim = pilot.flight.session.sim();
        if spec.reference_mode == ReferenceMode::Auto {
            spec.reference_mode = ReferenceMode::Fixed;
        } else if spec.reference_body + 1 < sim.fleet.ephemeris.bodies().len() {
            spec.reference_body += 1;
        } else {
            spec.reference_mode = ReferenceMode::Auto;
            spec.reference_body = sim.home;
        }
        run(
            pilot,
            Action::EditManeuver {
                index: selected,
                spec,
            },
        );
    }
    if keys.just_pressed(KeyCode::Delete) {
        run(pilot, Action::RemoveManeuver { index: selected });
    }
    if keys.just_pressed(KeyCode::KeyY) {
        run(
            pilot,
            Action::PlaceManeuverAtApsis {
                index: selected,
                apsis: void_orbit::ApsisKind::Periapsis,
            },
        );
    }
    if keys.just_pressed(KeyCode::KeyU) {
        run(
            pilot,
            Action::PlaceManeuverAtApsis {
                index: selected,
                apsis: void_orbit::ApsisKind::Apoapsis,
            },
        );
    }
    if keys.just_pressed(KeyCode::KeyZ) {
        if pilot.flight.session.sim().maneuver_warp.active() {
            run(pilot, Action::CancelManeuverWarp);
            pilot.flight.rate = 0;
        } else {
            run(pilot, Action::BeginManeuverWarp);
            if pilot.flight.session.sim().maneuver_warp.active() {
                pilot.flight.rate = RATES.len() - 1;
                pilot.flight.paused = false;
            }
        }
    }
    if keys.just_pressed(KeyCode::KeyB) {
        run(pilot, Action::ExecuteManeuver);
    }
    if keys.just_pressed(KeyCode::Escape) {
        run(pilot, Action::AbortManeuver);
    }
}

fn view_controls(
    session: &mut FlightSession,
    pointer_over_label: bool,
    keys: &ButtonInput<KeyCode>,
    buttons: &ButtonInput<MouseButton>,
    motion: &AccumulatedMouseMotion,
    scroll: &AccumulatedMouseScroll,
) {
    for (key, setting) in [
        (KeyCode::F2, Toggle::Wire),
        (KeyCode::F3, Toggle::Bounds),
        (KeyCode::F4, Toggle::Colliders),
        (KeyCode::F5, Toggle::Terrain),
        (KeyCode::KeyK, Toggle::AltitudeAgl),
        (KeyCode::KeyL, Toggle::SpeedSurface),
    ] {
        if keys.just_pressed(key) {
            session.execute(Action::View {
                command: ViewCommand::Toggle { setting },
            });
        }
    }
    if buttons.pressed(MouseButton::Left) && !pointer_over_label && motion.delta != Vec2::ZERO {
        session.execute(Action::View {
            command: ViewCommand::Drag {
                x: f64::from(motion.delta.x),
                y: f64::from(motion.delta.y),
            },
        });
    }
    let pixels = f64::from(scroll.delta.y)
        * match scroll.unit {
            bevy::input::mouse::MouseScrollUnit::Line => 40.0,
            bevy::input::mouse::MouseScrollUnit::Pixel => 1.0,
        };
    if pixels != 0.0 {
        session.execute(Action::View {
            command: ViewCommand::Zoom { pixels },
        });
    }
}

fn begin_profile_frame(time: Res<Time>, mut profiling: ResMut<Profiling>) {
    if let Some((profile, _)) = &mut profiling.0 {
        profile.sample("frame_interval", time.delta_secs_f64() * 1000.0);
    }
}
fn simulate(
    time: Res<Time>,
    window: Single<&Window>,
    mut profiling: ResMut<Profiling>,
    mut pilot: Pilot,
) {
    let started = std::time::Instant::now();
    simulate_inner(&time, &window, &mut pilot);
    let flight = &mut *pilot.flight;
    let sim = flight.session.sim();
    let t = sim.fleet.time();
    let clear = sim
        .fleet
        .clearance(&sim.selected, sim.nearby_body(&sim.selected));
    let forecast = &mut *pilot.forecast;
    if clear > 20.0 && (forecast.coast.is_none() || t < forecast.at || t - forecast.at >= 2.0) {
        forecast.coast = Some(flight.session.predict(6000.0));
        forecast.at = t;
        forecast.generation += 1;
    } else if clear <= 20.0 {
        forecast.coast = None;
    }
    if let Some((profile, _)) = &mut profiling.0 {
        profile.span("simulation", started, std::time::Instant::now());
    }
}
fn simulate_inner(time: &Time, window: &Window, pilot: &mut Pilot) {
    let flight = &mut *pilot.flight;
    if flight.paused || !window.focused {
        if flight.playback.is_none() {
            flight.session.execute(Action::EndFrame {
                paused: flight.paused,
                rate: flight.rate,
            });
        }
        return;
    }
    if let Some(mut playback) = flight.playback.take() {
        let terrain_before = flight.session.sim().planet.terrain.clone();
        if playback.next_frame(&mut flight.session) {
            flight.playback = Some(playback);
        } else {
            flight.paused = true;
            pilot.notice.0 = "Replay complete: all world marks verified".into();
        }
        if !std::sync::Arc::ptr_eq(&terrain_before, &flight.session.sim().planet.terrain) {
            pilot.visuals.rebuild = true;
            pilot.forecast.coast = None;
            flight.craft = flight.session.recording_initial().craft.clone();
        }
        return;
    }
    let maneuver_warp = flight.session.sim().maneuver_warp.active();
    if maneuver_warp {
        flight.rate = RATES.len() - 1;
    }
    if flight.rate > 2 {
        let sim = flight.session.sim();
        let f = &sim.fleet;
        while flight.rate > 2
            && f.vessel_ids().iter().any(|id| {
                f.snapshot(id).mode == void_vessels::VesselMode::Orbit
                    && f.clearance(id, sim.nearby_body(id))
                        < sim.terrains[&sim.nearby_body(id)].radius_meters
                            * crate::flight::rails_min_clearance_radii(RATES[flight.rate])
            })
        {
            flight.rate -= 1;
            pilot.notice.0 = "Warp limited by orbital vessel clearance".into();
        }
    }
    let rate = RATES[flight.rate];
    flight.frames += 1;
    let outcome = flight.session.execute(Action::Advance {
        seconds: time.delta_secs_f64().min(0.05) * rate,
        rails: rate > 4.0,
    });
    if maneuver_warp && !flight.session.sim().maneuver_warp.active() {
        flight.rate = 0;
        if let void_fleet_flight::warp::ManeuverWarp::Stopped { message } =
            &flight.session.sim().maneuver_warp
        {
            pilot.notice.0 = message.clone();
        }
    }
    match outcome {
        Outcome::Advanced(true) => {}
        Outcome::Advanced(false) => {
            flight.rate = 0;
            pilot.notice.0 = "Rails stopped at an encounter or ground band".into();
        }
        Outcome::Refused(reason) => {
            flight.rate = 0;
            pilot.notice.0 = format!("Warp refused: {reason}");
        }
        other => panic!("unexpected advance outcome: {other:?}"),
    }
    flight.session.execute(Action::EndFrame {
        paused: flight.paused,
        rate: flight.rate,
    });
    if flight.frames.is_multiple_of(60) {
        flight.session.mark();
    }
}
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn draw(
    mut commands: Commands,
    mut pilot: Pilot,
    mut camera_view: ResMut<CameraView>,
    mut profiling: ResMut<Profiling>,
    assets: Res<RenderAssets>,
    mut ground: ResMut<Ground>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut parts: Query<
        (&Visual, &mut Transform, &mut Visibility),
        (Without<Tile>, Without<MainCamera>),
    >,
    mut tiles: Query<&mut Transform, (With<Tile>, Without<Visual>, Without<MainCamera>)>,
    mut tile_visibility: Query<&mut Visibility, (With<Tile>, Without<Visual>)>,
    mut camera: Single<&mut Transform, With<MainCamera>>,
    projection: Single<&Projection, With<MainCamera>>,
    mut hud: Single<&mut Text, With<Hud>>,
    window: Single<&Window>,
    mut gizmos: Gizmos,
) {
    let started = std::time::Instant::now();
    let pilot = &mut pilot;
    refresh_ports(&mut pilot.docking, &pilot.flight.session);
    let visuals = &mut *pilot.visuals;
    if visuals.rebuild {
        for (_, entities) in visuals.parts.drain() {
            for e in entities {
                commands.entity(e).despawn();
            }
        }
        visuals.collision.clear();
        visuals.rebuild = false;
    }
    let ground = &mut ground.0;
    let session = &pilot.flight.session;
    let sim = session.sim();
    let f = &sim.fleet;
    let render_body = sim.observation_body();
    let surface = f.body_frames(render_body).1;
    // All world meshes/shaders use the observed body's axes, camera-relative.
    let q = surface_axes(f, render_body);
    let selected = f.snapshot(&sim.selected);
    let sample = sim.presentation.sample(sim);
    ground.prepare(sim, &sample, q, &mut commands, &mut meshes);
    let mut to_camera = HashMap::new();
    let mut to_camera = |from: void_frames::FrameId| {
        *to_camera
            .entry(from)
            .or_insert_with(|| sample.to_camera(f, from, q))
    };
    let focus = to_camera(sample.focus_frame).apply_point(sample.focus_local);
    let eye = f
        .frames()
        .transform(sample.focus_frame, surface)
        .apply_point(sample.camera(f, q).translation);
    let camera_up = q.conjugate() * sample.view.up;
    **camera = Transform::default().looking_to(focus.as_vec3(), camera_up.as_vec3());
    camera_view.focus = sample.focus;
    camera_view.view = Some(sample.view);
    camera_view.eye = eye;
    let snapshots = f
        .vessel_ids()
        .iter()
        .flat_map(|id| f.part_snapshots(id))
        .collect::<Vec<_>>();
    let live = snapshots
        .iter()
        .map(|p| p.id.clone())
        .collect::<HashSet<_>>();
    visuals.parts.retain(|id, entities| {
        if !live.contains(id) {
            for e in entities {
                commands.entity(*e).despawn();
            }
            false
        } else {
            true
        }
    });
    for p in &snapshots {
        if !visuals.parts.contains_key(&p.id) {
            let root = part_transform(&mut to_camera, p);
            let mut entities: Vec<Entity> = assets.parts[&p.definition.id]
                .iter()
                .map(|piece| {
                    commands
                        .spawn((
                            Visual {
                                id: p.id.clone(),
                                local: piece.local,
                                flame: piece.flame,
                                wheel: None,
                            },
                            Mesh3d(piece.mesh.clone()),
                            MeshMaterial3d(piece.material.clone()),
                            root.mul_transform(piece.local),
                            if piece.flame && !p.firing {
                                Visibility::Hidden
                            } else {
                                Visibility::Inherited
                            },
                        ))
                        .id()
                })
                .collect();
            for module in &p.definition.modules {
                let Module::Wheel {
                    id: mid,
                    parameters: d,
                } = module
                else {
                    continue;
                };
                let axle = (-d.suspension_direction).cross(d.forward);
                let tire = meshes.add(Cylinder::new(d.radius_meters as f32, 0.2));
                let stripe = meshes.add(Cuboid::new(0.03, 0.21, (d.radius_meters * 1.8) as f32));
                let local =
                    Transform::from_rotation(DQuat::from_rotation_arc(DVec3::Y, axle).as_quat());
                for (mesh, material, shape) in [
                    (
                        tire,
                        assets.parts[&p.definition.id][0].material.clone(),
                        local,
                    ),
                    (stripe, assets.center.clone(), Transform::IDENTITY),
                ] {
                    entities.push(
                        commands
                            .spawn((
                                Visual {
                                    id: p.id.clone(),
                                    local: shape,
                                    flame: false,
                                    wheel: Some(mid.clone()),
                                },
                                Mesh3d(mesh),
                                MeshMaterial3d(material),
                                root,
                                Visibility::Inherited,
                            ))
                            .id(),
                    );
                }
            }
            visuals.parts.insert(p.id.clone(), entities);
        }
    }
    for (visual, mut transform, mut visibility) in &mut parts {
        if let Some(p) = snapshots.iter().find(|p| p.id == visual.id) {
            let mut root = part_transform(&mut to_camera, p);
            if let Some(mid) = &visual.wheel {
                let d = p
                    .definition
                    .modules
                    .iter()
                    .find_map(|m| match m {
                        Module::Wheel { id, parameters } if id == mid => Some(parameters),
                        _ => None,
                    })
                    .expect("wheel definition");
                let void_assembly::ModuleState::Wheel { state, .. } = p.modules[mid] else {
                    panic!("wheel visual state")
                };
                let up = -d.suspension_direction;
                let axle = up.cross(d.forward);
                let rotation = DQuat::from_axis_angle(up, -state.steer_radians)
                    * DQuat::from_axis_angle(axle, state.spin_radians);
                root = root.mul_transform(
                    Transform::from_translation(
                        (d.suspension_origin
                            + d.suspension_direction * state.suspension_length_meters)
                            .as_vec3(),
                    )
                    .with_rotation(rotation.as_quat()),
                );
            }
            *transform = root.mul_transform(visual.local);
            *visibility = if visual.flame && !p.firing {
                Visibility::Hidden
            } else {
                Visibility::Inherited
            };
        }
    }
    let observers = snapshots
        .iter()
        .map(|p| {
            f.frames()
                .transform(p.frame, surface)
                .apply_point(p.local_position)
        })
        .collect();
    ground.finish_builds();
    let max_level = ground.max_level();
    ground.select(&LodView {
        camera: Some(LodCamera {
            position: eye,
            distance_scale: 1.0,
            max_level,
            focal_pixels: match *projection {
                Projection::Perspective(p) => {
                    f64::from(window.physical_height().max(1))
                        / (2.0 * (f64::from(p.fov) / 2.0).tan())
                }
                _ => panic!("main camera must be perspective"),
            },
            min_observer_cell_pixels: 3.0,
        }),
        observer_positions: observers,
        distance_scale: 1.0,
        horizon_culling: true,
    });
    ground.set_wireframe(&mut commands, sim.presentation.wire);
    ground.draw(&mut commands, &mut meshes, &mut tiles);
    for mut v in &mut tile_visibility {
        *v = if sim.presentation.terrain {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
    if sim.presentation.bounds {
        for line in ground.boundaries() {
            gizmos.linestrip(line, Color::srgb(1.0, 0.2, 0.2));
        }
    }
    let terrain_tiles = f.terrain_tiles();
    let live = terrain_tiles
        .iter()
        .map(|t| (t.scene, t.tile.clone()))
        .collect::<HashSet<_>>();
    visuals.collision.retain(|key, _| live.contains(key));
    if sim.presentation.colliders {
        for collider in f.vessel_collider_meshes() {
            let into = to_camera(collider.frame);
            let vertices = &collider.mesh.vertices;
            let indices = unique_edges(&collider.mesh.triangles);
            for edge in indices.as_chunks::<2>().0 {
                let point = |i: u32| {
                    into.apply_point(Vec3::from_array(vertices[i as usize]).as_dvec3())
                        .as_vec3()
                };
                gizmos.line(point(edge[0]), point(edge[1]), Color::srgb(0.2, 1.0, 0.4));
            }
        }
        for tile in terrain_tiles {
            let key = (tile.scene, tile.tile.clone());
            let vertices = visuals.collision.entry(key).or_insert_with(|| {
                let (vertices, triangles) = f.terrain_geometry(tile.scene, &tile.tile);
                unique_edges(&triangles)
                    .iter()
                    .map(|i| Vec3::from_array(vertices[*i as usize]))
                    .collect()
            });
            let into = to_camera(tile.frame);
            let point = |v: Vec3| {
                into.apply_point(tile.local_position + v.as_dvec3())
                    .as_vec3()
            };
            for edge in vertices.as_chunks::<2>().0 {
                gizmos.line(point(edge[0]), point(edge[1]), Color::srgb(0.2, 1.0, 0.4));
            }
        }
    }
    let into = to_camera(f.vessel_frame(&sim.selected));
    let allocation = f.rcs_allocation(&sim.selected);
    for nozzle in allocation.nozzles.iter().filter(|n| n.throttle > 0.001) {
        let start = into.apply_point(nozzle.point);
        let end =
            into.apply_point(nozzle.point - nozzle.full_force.normalize() * nozzle.throttle * 1.5);
        gizmos.line(start.as_vec3(), end.as_vec3(), Color::srgb(0.4, 0.8, 1.0));
    }
    for (port, color) in [
        (&pilot.docking.own, Color::srgb(0.1, 1.0, 0.3)),
        (&pilot.docking.target, Color::srgb(1.0, 0.5, 0.1)),
    ] {
        if let Some(port) = port {
            let (mount, normal) = port_pose(session, port);
            let local = mount + f.centre_of_mass_local(&port.vessel);
            let into = to_camera(f.vessel_frame(&port.vessel));
            gizmos.line(
                into.apply_point(local).as_vec3(),
                into.apply_point(local + normal).as_vec3(),
                color,
            );
        }
    }
    hud.0 = diagnostics_text(&pilot.flight, &pilot.notice, &pilot.docking, &selected);
    if let Some((profile, _)) = &mut profiling.0 {
        profile.span("draw_lod_overlays", started, std::time::Instant::now());
    }
}
/// The DEV panel's full status text.
fn diagnostics_text(
    flight: &Flight,
    notice: &Notice,
    docking: &Docking,
    selected: &void_vessels::VesselSnapshot,
) -> String {
    let session = &flight.session;
    let sim = session.sim();
    let f = &sim.fleet;
    let p = f.thrust(&sim.selected);
    let navigation = sim.navigation_body(&sim.selected);
    let body = &f.ephemeris.bodies()[navigation];
    let relative = f
        .frames()
        .transform(f.vessel_frame(&sim.selected), f.body_frames(navigation).0)
        .apply_state(void_frames::State {
            position: f.centre_of_mass_local(&sim.selected),
            velocity: DVec3::ZERO,
        });
    let axes = f
        .frames()
        .transform(f.body_frames(navigation).0, f.origin_frame())
        .rotation();
    let r = axes * relative.position;
    let v = axes * relative.velocity;
    let orbital = void_orbit::osculating_orbit(r, v, body.gm);
    let surface = f
        .frames()
        .transform(f.vessel_frame(&sim.selected), f.body_frames(navigation).1)
        .apply_state(void_frames::State {
            position: f.centre_of_mass_local(&sim.selected),
            velocity: DVec3::ZERO,
        });
    let agl = sim.presentation.altitude_agl && sim.terrains.contains_key(&navigation);
    let altitude = if agl {
        f.clearance(&sim.selected, navigation)
    } else {
        r.length() - body.radius_meters
    };
    let speed = if sim.presentation.speed_surface {
        surface.velocity.length()
    } else {
        v.length()
    };
    let fuel: f64 = f
        .part_snapshots(&sim.selected)
        .iter()
        .map(|p| p.fuel_kg)
        .sum();
    let aircraft_data = if f.control_profile(&sim.selected)
        == Some(void_assembly::ControlProfile::Aircraft)
    {
        let data = f.air_data(&sim.selected);
        let control = f.control(&sim.selected).turn;
        let airflow = if data.dynamic_pressure_pa < 1.0 {
            "low airflow; AoA/stall unavailable".to_owned()
        } else {
            format!(
                "max section AoA {:.1}° stall {:.0}%",
                data.maximum_angle_radians.to_degrees(),
                data.maximum_stall * 100.0
            )
        };
        format!(
            "\nAIR: {:.1}m/s q {:.1}kPa {} | pitch {:.0}% yaw {:.0}% roll {:.0}%\nB hold brakes | Q/E nose steering | aerodynamic control, no reaction wheel",
            data.airspeed_mps,
            data.dynamic_pressure_pa / 1000.0,
            airflow,
            control.x * 100.0,
            control.y * 100.0,
            control.z * 100.0
        )
    } else {
        String::new()
    };
    let stellar_status = sim
        .world
        .stellar
        .as_ref()
        .map(|stellar| {
            let system = f.vessel_system(&sim.selected).0;
            let name = if system == 0 {
                stellar.home.id.as_str()
            } else {
                stellar.neighbors[system - 1].placement.id.as_str()
            };
            format!(" · {name} system · stellar distances in light years")
        })
        .unwrap_or_default();
    let (paused, rate) = if flight.playback.is_some() {
        (sim.presentation.paused, sim.presentation.rate)
    } else {
        (flight.paused, flight.rate)
    };
    let docking_text =
        if f.control_profile(&sim.selected) == Some(void_assembly::ControlProfile::Flight) {
            docking_description(session, docking)
        } else {
            String::new()
        };
    format!(
        "VOID{}\n{} ({}) | {:?} | {} | {}x\nT+{:.2}s {} {:.1}m {} {:.1}m/s | {}\nmass {:.1}kg fuel {:.1}kg throttle {:.0}% force {:.1}kN SAS {:?}\nPe {:.1}km Ap {:.1}km | {} vessels\n{}\nTab vessel | Shift+Tab body focus | click map labels | 1–4/G plot frame | J body | Shift+J pair\nN craft beside launch site | O orbital craft | R reset | , . warp | K altitude | L speed\nF1 near/orbit/far | Home ship | Alt+F10/F11 exposure\nF2 wire | F3 boundaries | F4 actual colliders | F5 terrain\nF6 save | F7 load (paused) | F8 finish recording | F9 finish CPU profile\n{}{}\n{}\n{}\n{}{} | Water {:.0} N\n{}\n{}",
        stellar_status,
        selected.name,
        sim.selected,
        selected.mode,
        if paused { "paused" } else { "running" },
        RATES[rate],
        f.time(),
        if agl { "AGL" } else { "ALT" },
        altitude,
        if sim.presentation.speed_surface {
            "surface"
        } else {
            "orbit"
        },
        speed,
        body.name,
        selected.mass_kg,
        fuel,
        f.control(&sim.selected).throttle * 100.0,
        p.force.length() / 1000.0,
        f.sas_phase(&sim.selected),
        (orbital.periapsis_radius_meters - body.radius_meters) / 1000.0,
        (orbital.apoapsis_radius_meters - body.radius_meters) / 1000.0,
        f.vessel_ids().len(),
        pilot_description(sim),
        notice.0,
        aircraft_data,
        scenery_description(sim),
        plotting_description(sim),
        thermal_description(session),
        vehicle_description(sim),
        f.water_wrench(&sim.selected).force.length(),
        docking_text,
        plan_description(session),
    )
}

#[derive(Component)]
struct Sky;
#[allow(clippy::too_many_arguments)]
fn setup_scenery(
    mut commands: Commands,
    flight: NonSend<Flight>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut standard: ResMut<Assets<StandardMaterial>>,
    mut grounds: ResMut<Assets<crate::scenery::GroundMaterial>>,
    mut stars: ResMut<Assets<crate::scenery::StarMaterial>>,
    camera: Single<Entity, With<MainCamera>>,
) {
    let world = crate::world_scenery::WorldScenery::new(
        &mut commands,
        flight.session.sim(),
        &mut grounds,
        &mut images,
        &mut meshes,
        &mut standard,
    );
    commands.insert_resource(Ground(Box::new(world)));
    let (positions, colors) = void_scenery::generate_stars(&void_scenery::DEFAULT_STARS);
    commands.spawn((
        Sky,
        Visibility::Inherited,
        Mesh3d(meshes.add(crate::scenery::star_mesh(positions, &colors))),
        MeshMaterial3d(stars.add(crate::scenery::StarMaterial { brightness: 0.08 })),
        Transform::default(),
        bevy::camera::visibility::NoFrustumCulling,
    ));
    commands.entity(*camera).insert((
        Camera3d {
            depth_texture_usages: (bevy::render::render_resource::TextureUsages::RENDER_ATTACHMENT
                | bevy::render::render_resource::TextureUsages::TEXTURE_BINDING)
                .into(),
            ..default()
        },
        bevy::camera::Hdr,
        bevy::core_pipeline::tonemapping::DebandDither::Disabled,
        Projection::Perspective(PerspectiveProjection {
            fov: 58.0_f32.to_radians(),
            far: 1e20,
            ..default()
        }),
        Msaa::Off,
        bevy::core_pipeline::tonemapping::Tonemapping::None,
        crate::air::AirLayers(vec![]),
        crate::air::AirSettings::new(&void_scenery::earth_like_atmosphere(
            flight.session.sim().planet.terrain.radius_meters,
        )),
    ));
}
/// A loaded save or reset can bring another world; scenery is rebuilt only when it differs.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn refresh_scenery(
    mut commands: Commands,
    flight: NonSend<Flight>,
    visuals: Res<PartVisuals>,
    mut ground: ResMut<Ground>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut standard: ResMut<Assets<StandardMaterial>>,
    mut grounds: ResMut<Assets<crate::scenery::GroundMaterial>>,
    far: Query<(Entity, &crate::world_scenery::FarBody)>,
    markers: Query<Entity, With<crate::map::MapMarker>>,
) {
    let world = &mut ground.0;
    if !visuals.rebuild
        || serde_json::to_value(&world.world).unwrap()
            == serde_json::to_value(&flight.session.sim().world).unwrap()
    {
        return;
    }
    world.unload(&mut commands, &mut meshes, &mut grounds, &mut images);
    for (entity, body) in &far {
        commands.entity(entity).despawn();
        meshes.remove(body.1);
        standard.remove(body.2);
    }
    for e in &markers {
        commands.entity(e).despawn();
    }
    crate::map::spawn_map_labels(&mut commands, flight.session.sim().fleet.ephemeris.bodies());
    **world = crate::world_scenery::WorldScenery::new(
        &mut commands,
        flight.session.sim(),
        &mut grounds,
        &mut images,
        &mut meshes,
        &mut standard,
    );
}
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn update_scenery(
    flight: NonSend<Flight>,
    ground: Res<Ground>,
    window: Single<&Window>,
    mut grounds: ResMut<Assets<crate::scenery::GroundMaterial>>,
    mut camera: Query<
        (
            &Transform,
            &mut crate::air::AirSettings,
            &Projection,
            &mut crate::air::AirLayers,
        ),
        With<MainCamera>,
    >,
    mut sky: Query<(&mut Transform, &mut Visibility), (With<Sky>, Without<MainCamera>)>,
    mut light: Query<&mut Transform, (With<SceneSun>, Without<Sky>, Without<MainCamera>)>,
    mut far: Query<
        (
            &crate::world_scenery::FarBody,
            &mut Transform,
            &mut Visibility,
        ),
        (Without<Sky>, Without<SceneSun>, Without<MainCamera>),
    >,
) {
    let world = &ground.0;
    let sim = flight.session.sim();
    let fleet = &sim.fleet;
    let q = surface_axes(fleet, sim.observation_body());
    let sample = sim.presentation.sample(sim);
    for (transform, mut air, projection, mut layers) in &mut camera {
        let Projection::Perspective(p) = projection else {
            panic!("world camera requires perspective");
        };
        let focal =
            f64::from(window.physical_height().max(1)) / (2.0 * (f64::from(p.fov) / 2.0).tan());
        let (settings, volumes, sun) = world.update_air(
            sim,
            &sample,
            q,
            crate::world_scenery::AirView {
                camera: transform,
                projection: p,
                focal,
            },
            &mut grounds,
        );
        *air = settings;
        *layers = volumes;
        for mut t in &mut light {
            *t = Transform::default().looking_to(
                (-sun).as_vec3(),
                if sun.z.abs() < 0.99 { Vec3::Z } else { Vec3::Y },
            );
        }
    }
    for (mut t, mut visibility) in &mut sky {
        *t = Transform::from_rotation(q.conjugate().as_quat());
        *visibility = if sim.presentation.visual_stars {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
    for (body, mut t, mut visibility) in &mut far {
        let into = sample.to_camera(fleet, fleet.body_frames(body.0).1, q);
        *t = Transform::from_translation(into.apply_point(DVec3::ZERO).as_vec3())
            .with_rotation(into.rotation().as_quat())
            .with_scale(Vec3::splat(
                fleet.ephemeris.bodies()[body.0].radius_meters as f32,
            ));
        let solid = world.bodies.get(&body.0);
        *visibility = if solid.is_some()
            && (!sim.presentation.terrain
                || (body.0 == world.active
                    && !body.3
                    && solid.is_some_and(|b| b.field.drawn_count() > 0)))
        {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn draw_map(
    flight: NonSend<Flight>,
    camera_view: Res<CameraView>,
    forecast: Res<Forecast>,
    mut plots: ResMut<MapPlots>,
    camera: Single<(&Camera, &GlobalTransform), With<MainCamera>>,
    mut markers: Query<(
        &crate::map::MapMarker,
        &mut Node,
        &mut Visibility,
        &ComputedNode,
    )>,
    mut texts: Query<(&mut Text, &mut Visibility), Without<crate::map::MapMarker>>,
    mut gizmos: Gizmos,
) {
    let plots = &mut *plots;
    let view = camera_view.view.expect("main camera state");
    let sim = flight.session.sim();
    let fleet = &sim.fleet;
    let home = sim.observation_body();
    let bodies = fleet.ephemeris.bodies();
    let mut positions = vec![DVec3::ZERO; bodies.len()];
    let mut velocities = positions.clone();
    fleet
        .ephemeris
        .states_at(fleet.time(), &mut positions, Some(&mut velocities));
    let ship = fleet.snapshot(&sim.selected);
    let reference = void_orbit::DominanceTree::new(bodies).dominant(&positions, ship.position);
    let q = surface_axes(fleet, home);
    let frame = void_view::MapFrame {
        time: fleet.time(),
        positions: &positions,
        velocities: &velocities,
        origin: camera_view.focus,
        vessel: ship.position,
        vessel_velocity: ship.velocity,
        plotting: void_view::PlottingFrame {
            kind: sim.presentation.path_frame(),
            reference,
        },
        // Simulation time makes refresh cadence independent of replay rendering speed.
        wall_ms: fleet.time() * 1000.0,
    };
    let spec = sim.presentation.plotting_frame;
    let apsis_reference = match spec {
        void_orbit::FrameSpec::BodyInertial { body }
        | void_orbit::FrameSpec::BodySurface { body } => body,
        void_orbit::FrameSpec::TwoBodyRotating { primary, .. } => primary,
        void_orbit::FrameSpec::Barycentric => reference,
    };
    if let Some(prediction) = &forecast.coast {
        plots.coast.update(
            &fleet.ephemeris,
            &prediction.trajectory,
            spec,
            forecast.generation,
            fleet.time(),
            frame.origin,
            apsis_reference,
        );
    } else {
        plots.coast = Default::default();
    }
    if plots.plan_vessel != sim.selected {
        plots.plan = Default::default();
        plots.plan_vessel = sim.selected.clone();
    }
    if let Some(p) = sim.plans.get(&plots.plan_vessel) {
        plots.plan.update(
            &fleet.ephemeris,
            &p.plan.trajectory,
            spec,
            p.plan.generation,
            fleet.time(),
            frame.origin,
            apsis_reference,
        );
    } else {
        plots.plan = Default::default();
    }
    let eye_inertial = fleet
        .frames()
        .transform(fleet.body_frames(home).1, fleet.origin_frame())
        .apply_point(camera_view.eye);
    let render = |v: DVec3| (q.conjugate() * (v + frame.origin - eye_inertial)).as_vec3();
    if view.map_weight > 0.0 {
        for (body, points) in bodies.iter().zip(plots.bodies.update(
            &fleet.ephemeris,
            spec,
            fleet.time(),
            frame.origin,
        )) {
            gizmos.linestrip(
                points.into_iter().map(&render),
                crate::map::color(&body.color).with_alpha(view.map_weight as f32),
            );
        }
        for (path, color) in [
            (&plots.coast, crate::map::color(crate::map::PATH_COLOR)),
            (&plots.plan, Color::srgb(1.0, 0.6, 0.15)),
        ] {
            gizmos.linestrip(
                path.points.iter().copied().map(&render),
                color.with_alpha(view.map_weight as f32),
            );
        }
    }
    let wanted = void_view::map_labels(
        bodies,
        &frame,
        sim.presentation.focus_body,
        &plots.coast.apsides,
    );
    let (camera, transform) = *camera;
    crate::map::place_map_labels(
        camera,
        transform,
        &mut markers,
        &mut texts,
        &wanted,
        view.map_weight,
        &render,
    );
}
