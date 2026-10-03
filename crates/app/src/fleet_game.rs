//! Shared Fleet flight scene used by the independent integration lab and the main game.
use crate::{
    flight::game_planet_by_id,
    overlay::unique_edges,
    tiles::{Tile, TileField},
};
use bevy::{
    input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll},
    pbr::wireframe::WireframePlugin,
    prelude::*,
    render::settings::{WgpuFeatures, WgpuSettings},
};
use glam::DVec3;
use std::collections::{HashMap, HashSet};
use void_assembly::{Craft, Module, demo_craft, import_craft};
use void_assembly_lab::parts::RenderAssets;
use void_fleet_flight::session::{
    Action, FlightSession, InitialWorld, Outcome, Playback, Recording,
};
use void_landing::{FrameState, PlanetFrame, demo_rocket, landing_lod_options};
use void_lod::{LodCamera, LodView};
use void_vessels::nearby_site;

const RATES: [f64; 9] = crate::flight::TIME_RATES;
struct Lab {
    main_game: bool,
    pointer_over_label: bool,
    speed_surface: bool,
    altitude_agl: bool,
    camera: void_view::OrbitCamera,
    view: Option<void_view::ViewState>,
    focus_body: Option<usize>,
    path_frame: void_view::PathFrameKind,
    eye: DVec3,
    focus_position: DVec3,
    last_view_time: f64,
    orbits: void_view::MapOrbits,
    path: void_view::MapPath,
    plan_path: void_view::MapPath,
    plan_vessel: String,
    prediction_at: f64,
    prediction_generation: u64,
    session: FlightSession,
    save_path: std::path::PathBuf,
    record_path: Option<std::path::PathBuf>,
    frames: usize,
    playback: Option<Playback>,
    profile: Option<(void_diagnostics::Profiler, std::path::PathBuf)>,
    craft: Craft,
    paused: bool,
    rate: usize,
    yaw: f64,
    pitch: f64,
    distance: f64,
    colliders: bool,
    bounds: bool,
    wire: bool,
    terrain: bool,
    dirty: bool,
    notice: String,
    spawned: u32,
    parts: HashMap<String, Vec<Entity>>,
    collision: HashMap<(u64, String), Vec<Vec3>>,
    prediction: Option<void_landing::CoastPrediction>,
}
#[derive(Resource)]
enum Ground {
    Plain(Box<TileField>, Handle<StandardMaterial>),
    Shaded(
        Box<TileField<crate::scenery::GroundMaterial>>,
        Handle<crate::scenery::GroundMaterial>,
    ),
}
macro_rules! ground_call {
    ($self:expr, $field:ident => $body:expr) => {
        match $self {
            Ground::Plain($field, _) => $body,
            Ground::Shaded($field, _) => $body,
        }
    };
}
impl Ground {
    fn reset(&mut self, planet: &void_landing::LandingPlanet) {
        let demo = demo_rocket(&planet.terrain);
        let options = landing_lod_options(&planet.terrain, &demo.options.contact);
        let terrain: Option<std::sync::Arc<dyn void_lod::SurfaceSampler + Send + Sync>> =
            Some(planet.terrain.clone());
        match self {
            Ground::Plain(field, material) => {
                **field = TileField::new(options, terrain, material.clone())
            }
            Ground::Shaded(field, material) => {
                **field = TileField::new(options, terrain, material.clone());
                field.no_frustum_culling = true;
                field.wireframe_color = scene_color(Color::WHITE);
            }
        }
    }
    fn finish_builds(&mut self) {
        ground_call!(self, f => f.finish_builds());
    }
    fn max_level(&self) -> u32 {
        ground_call!(self, f => f.lod.options.max_level)
    }
    fn select(&mut self, view: &LodView) {
        ground_call!(self, f => f.select(view));
    }
    fn set_wireframe(&mut self, commands: &mut Commands, on: bool) {
        ground_call!(self, f => f.set_wireframe(commands, on));
    }
    fn boundaries(&self, eye: DVec3) -> Vec<Vec<Vec3>> {
        ground_call!(self, f => f.boundaries(eye))
    }
    fn draw<F: bevy::ecs::query::QueryFilter>(
        &mut self,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        tiles: &mut Query<&mut Transform, F>,
        eye: DVec3,
    ) {
        ground_call!(self, f => f.draw(commands, meshes, tiles, eye));
    }
}
const EXPOSURE: f32 = 6.309_573;
fn scene_color(color: Color) -> Color {
    let c = color.to_linear();
    Color::linear_rgba(
        c.red / EXPOSURE,
        c.green / EXPOSURE,
        c.blue / EXPOSURE,
        c.alpha,
    )
}
impl Drop for Lab {
    fn drop(&mut self) {
        if !std::thread::panicking()
            && let Some((profile, path)) = &self.profile
        {
            profile.write(path);
        }
        if !std::thread::panicking()
            && let Some(path) = self.record_path.take()
        {
            self.session.save(&path);
            eprintln!("Fleet recording saved: {}", path.display());
        }
    }
}
#[derive(Component)]
struct LabCamera;
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
}
fn argument(name: &str) -> Option<String> {
    let args: Vec<_> = std::env::args().collect();
    args.iter()
        .position(|s| s == name)
        .map(|i| args.get(i + 1).expect("argument needs a value").clone())
}
pub fn run(main_game: bool) {
    if let Some(path) = argument("--verify-save") {
        let session = FlightSession::load_checkpoint(&path);
        println!(
            "Verified Fleet world save: T+{:.6} s, {} vessels, selected {}",
            session.sim().fleet.time(),
            session.sim().fleet.vessel_ids().len(),
            session.sim().selected
        );
        return;
    }
    if let Some(path) = argument("--verify") {
        let mut profile = void_diagnostics::Profiler::new();
        let started = std::time::Instant::now();
        let session = FlightSession::load(&path);
        profile.span("headless_verify", started, std::time::Instant::now());
        if let Some(output) = argument("--profile") {
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
    let id = argument("--planet").unwrap_or("aurelia".into());
    let planet = game_planet_by_id(&id, argument("--terrain").as_deref());
    let craft = argument("--craft").map_or_else(demo_craft, |path| {
        import_craft(&std::fs::read_to_string(path).expect("read craft")).expect("invalid craft")
    });
    let site = planet
        .launch_site
        .unwrap_or_else(|| demo_rocket(&planet.planet.terrain).launch_site.normalize());
    let air =
        planet.planet.air_density_scale.is_some() && !std::env::args().any(|a| a == "--vacuum");
    let replay_path = argument("--replay");
    assert!(
        replay_path.is_none() || (argument("--load").is_none() && argument("--record").is_none()),
        "--replay cannot be combined with --load or --record"
    );
    let session = argument("--load").map_or_else(
        || FlightSession::new(InitialWorld::new(&planet.planet, &craft, site, air)),
        FlightSession::load_checkpoint,
    );
    let craft = session.recording_initial().craft.clone();
    let mut lab = new_lab(session, craft);
    lab.main_game = main_game;
    lab.paused = !main_game || argument("--load").is_some();
    if let Some(path) = replay_path {
        let (playback, session) = Playback::new(Recording::read(path));
        lab.session = session;
        lab.craft = lab.session.recording_initial().craft.clone();
        lab.playback = Some(playback);
        lab.paused = false;
    }
    lab.save_path = argument("--save")
        .unwrap_or("lab-log/fleet-save.json".into())
        .into();
    lab.record_path = argument("--record").map(Into::into);
    lab.profile =
        argument("--profile").map(|path| (void_diagnostics::Profiler::new(), path.into()));
    let mut app = App::new();
    app.add_plugins((
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: if main_game {
                        "VOID"
                    } else {
                        "VOID Fleet flight integration"
                    }
                    .into(),
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
    ))
    .insert_resource(ClearColor(if main_game {
        Color::BLACK
    } else {
        Color::srgb(0.02, 0.025, 0.04)
    }))
    .insert_resource(GlobalAmbientLight {
        brightness: if main_game { 40.0 } else { 100.0 },
        color: Color::srgb_u8(0xcb, 0xe7, 0xff),
        ..default()
    })
    .insert_non_send(lab)
    .add_systems(Startup, (setup, setup_scenery).chain())
    .add_systems(
        Update,
        (
            begin_profile_frame,
            controls,
            simulate,
            refresh_scenery,
            draw,
            draw_map,
            instruments,
            update_scenery,
        )
            .chain(),
    )
    .run();
}
fn new_lab(session: FlightSession, craft: Craft) -> Lab {
    let f = &session.sim().fleet;
    let ship = f.snapshot(&session.sim().selected);
    let radial =
        (ship.position - f.ephemeris.body_position(session.sim().home, f.time())).normalize();
    let side = if radial.x.hypot(radial.y) > 1e-9 {
        DVec3::new(-radial.y, radial.x, 0.0).normalize()
    } else {
        DVec3::X
    };
    let camera = void_view::OrbitCamera::new((side + 0.3 * radial).normalize(), 40.0);
    let orbits = void_view::MapOrbits::new(f.ephemeris.bodies());
    Lab {
        main_game: false,
        pointer_over_label: false,
        speed_surface: true,
        altitude_agl: true,
        camera,
        view: None,
        focus_body: None,
        path_frame: void_view::PathFrameKind::Inertial,
        eye: DVec3::ZERO,
        focus_position: DVec3::ZERO,
        last_view_time: 0.0,
        orbits,
        path: void_view::MapPath::new(),
        plan_path: void_view::MapPath::new(),
        plan_vessel: String::new(),
        prediction_at: f64::NEG_INFINITY,
        prediction_generation: 0,
        session,
        save_path: "lab-log/fleet-save.json".into(),
        record_path: None,
        frames: 0,
        playback: None,
        profile: None,
        craft,
        paused: true,
        rate: 0,
        yaw: 0.4,
        pitch: 0.25,
        distance: 40.0,
        colliders: false,
        bounds: false,
        wire: false,
        terrain: true,
        dirty: false,
        notice: String::new(),
        spawned: 0,
        parts: HashMap::new(),
        collision: HashMap::new(),
        prediction: None,
    }
}
fn setup(
    mut commands: Commands,
    lab: NonSend<Lab>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    window: Single<&Window>,
) {
    let assets = RenderAssets::new(&mut meshes, &mut materials);
    commands.insert_resource(assets);
    let demo = demo_rocket(&lab.session.sim().planet.terrain);
    let material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.32, 0.42, 0.28),
        perceptual_roughness: 1.0,
        ..default()
    });
    commands.insert_resource(Ground::Plain(
        Box::new(TileField::new(
            landing_lod_options(&lab.session.sim().planet.terrain, &demo.options.contact),
            Some(lab.session.sim().planet.terrain.clone()),
            material.clone(),
        )),
        material,
    ));
    commands.spawn((Camera3d::default(), Transform::default(), LabCamera));
    commands.spawn((
        SceneSun,
        DirectionalLight {
            illuminance: if lab.main_game { 1000.0 } else { 8000.0 },
            ..default()
        },
        Transform::default().looking_to(Vec3::new(-1.0, -0.4, -0.7), Vec3::Z),
    ));
    commands.spawn((
        Hud,
        Text::new(""),
        TextFont {
            font_size: FontSize::Px(14.0),
            ..default()
        },
        Node {
            position_type: PositionType::Absolute,
            left: px(12),
            top: px(12),
            ..default()
        },
    ));
    if lab.main_game {
        spawn_bodies(
            &mut commands,
            &mut meshes,
            &mut materials,
            lab.session.sim().fleet.ephemeris.bodies(),
            lab.session.sim().home,
        );
        crate::map::spawn_map_labels(&mut commands, lab.session.sim().fleet.ephemeris.bodies());
        let ball = crate::navball::spawn_navball(
            &mut commands,
            &mut images,
            150.0,
            f64::from(window.scale_factor()),
        );
        commands.entity(ball).insert((
            Node {
                position_type: PositionType::Absolute,
                bottom: px(40),
                left: percent(50),
                width: px(150),
                height: px(150),
                ..default()
            },
            UiTransform::from_translation(bevy::ui::Val2::percent(-50, 0)),
        ));
        commands.spawn((
            NavballHeading,
            Text::new(""),
            TextFont {
                font_size: FontSize::Px(13.0),
                ..default()
            },
            Node {
                position_type: PositionType::Absolute,
                bottom: px(12),
                left: percent(50),
                ..default()
            },
            UiTransform::from_translation(bevy::ui::Val2::percent(-50, 0)),
        ));
    }
}

#[allow(clippy::type_complexity)]
fn instruments(
    lab: NonSend<Lab>,
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
    let sim = lab.session.sim();
    let fleet = &sim.fleet;
    let ship = fleet.snapshot(&sim.selected);
    let mut positions = vec![DVec3::ZERO; fleet.ephemeris.bodies().len()];
    fleet.ephemeris.positions_at(fleet.time(), &mut positions);
    let reference = void_orbit::DominanceTree::new(fleet.ephemeris.bodies())
        .dominant(&positions, ship.position);
    let frame = PlanetFrame::new(&fleet.ephemeris, reference);
    let local = frame.to_body_fixed(
        &fleet.ephemeris,
        fleet.time(),
        FrameState {
            position: ship.position,
            velocity: ship.velocity,
        },
    );
    let axes = void_orbit::body_orientation(&frame.body.rotation, fleet.time());
    let q = glam::DQuat::from_mat3(&glam::DMat3::from_cols(axes[0], axes[1], axes[2])).normalize();
    let input = void_navball::NavballInput {
        nose: (ship.rotation * DVec3::Y).normalize(),
        top: (ship.rotation * DVec3::Z).normalize(),
        up: (ship.position - fleet.ephemeris.body_position(reference, fleet.time())).normalize(),
        pole: frame.body.rotation.axis(),
        prime_meridian: axes[0],
        velocity: if lab.speed_surface {
            q * local.velocity
        } else {
            ship.velocity
                - fleet
                    .ephemeris
                    .body_state(void_frames::BodyId(reference), fleet.time())
                    .1
        },
    };
    for mut ball in &mut balls {
        let reading = crate::navball::draw_navball(&mut ball, &input, &mut images, &mut labels);
        for mut text in &mut heading {
            text.0 = format!(
                "HDG {:03} | {:+.0} deg",
                reading.heading.round() as i64 % 360,
                reading.pitch
            );
        }
    }
}
fn axis(keys: &ButtonInput<KeyCode>, plus: KeyCode, minus: KeyCode) -> f64 {
    keys.pressed(plus) as i32 as f64 - keys.pressed(minus) as i32 as f64
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
    mut lab: NonSendMut<Lab>,
) {
    let lab = &mut *lab;
    let (over_label, clicked) =
        crate::map::label_click(&markers, &buttons, lab.view.map_or(0.0, |s| s.map_weight));
    if let Some(kind) = clicked {
        let bodies = lab.session.sim().fleet.ephemeris.bodies();
        lab.focus_body = match kind {
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
        lab.camera.distance = lab
            .focus_body
            .map_or(40.0, |i| bodies[i].radius_meters * 4.0);
    }
    lab.pointer_over_label = over_label;
    if !window.focused {
        if lab.playback.is_none() {
            let control = lab.session.sim().fleet.control(&lab.session.sim().selected);
            if control.turn != DVec3::ZERO {
                lab.session.execute(Action::Control {
                    throttle: control.throttle,
                    turn: DVec3::ZERO,
                });
            }
        }
        return;
    }
    if lab.playback.is_some() {
        if keys.just_pressed(KeyCode::KeyP) {
            lab.paused = !lab.paused;
        }
        view_controls(lab, &keys, &buttons, &motion, &scroll);
        return;
    }
    if keys.just_pressed(KeyCode::F6) {
        lab.session.save_checkpoint(&lab.save_path);
        lab.notice = format!("Saved {}", lab.save_path.display());
    }
    if keys.just_pressed(KeyCode::F7) {
        lab.session = FlightSession::load_checkpoint(&lab.save_path);
        lab.craft = lab.session.recording_initial().craft.clone();
        lab.dirty = true;
        lab.prediction = None;
        lab.paused = true;
        lab.rate = 0;
        lab.notice = format!("Loaded {}", lab.save_path.display());
    }
    if keys.just_pressed(KeyCode::F8) {
        if let Some(path) = lab.record_path.take() {
            lab.session.save(&path);
            lab.notice = format!("Recording finished: {}", path.display());
        } else {
            lab.notice = "No recording active; start with --record <file>".into();
        }
    }
    if keys.just_pressed(KeyCode::F9) {
        if let Some((profile, path)) = lab.profile.take() {
            profile.write(&path);
            lab.notice = format!("CPU profile finished: {}", path.display());
        } else {
            lab.notice = "No CPU profile active; start with --profile <file>".into();
        }
    }
    if keys.just_pressed(KeyCode::KeyP) {
        lab.paused = !lab.paused;
    }
    if keys.just_pressed(KeyCode::KeyR) {
        lab.session = FlightSession::new(lab.session.recording_initial().clone());
        lab.dirty = true;
        lab.prediction = None;
        lab.paused = true;
        lab.rate = 0;
        lab.spawned = 0;
        lab.notice.clear();
    }
    if keys.just_pressed(KeyCode::Tab)
        && lab.main_game
        && keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight])
    {
        let bodies = lab.session.sim().fleet.ephemeris.bodies();
        lab.focus_body = match lab.focus_body {
            None => Some(0),
            Some(i) if i + 1 < bodies.len() => Some(i + 1),
            Some(_) => None,
        };
        lab.camera.distance = lab
            .focus_body
            .map_or(40.0, |i| bodies[i].radius_meters * 4.0);
    } else if keys.just_pressed(KeyCode::Tab) {
        let old = lab.session.sim().selected.clone();
        let ids = lab.session.sim().fleet.vessel_ids();
        let i = ids
            .iter()
            .position(|id| *id == old)
            .expect("selected vessel");
        let mut c = lab.session.sim().fleet.control(&old);
        c.turn = DVec3::ZERO;
        if c.turn != lab.session.sim().fleet.control(&old).turn {
            lab.session.execute(Action::Control {
                throttle: c.throttle,
                turn: c.turn,
            });
        }
        lab.session.execute(Action::Select {
            vessel: ids[(i + 1) % ids.len()].clone(),
        });
        lab.prediction = None;
        lab.focus_body = None;
        lab.camera.distance = 40.0;
    }
    if keys.just_pressed(KeyCode::KeyO) {
        let Outcome::Spawned(id) = lab.session.execute(Action::LaunchOrbit {
            craft: lab.craft.clone(),
            offset: DVec3::ZERO,
        }) else {
            unreachable!()
        };
        lab.session.execute(Action::Select { vessel: id });
        lab.distance = 40.0;
        lab.camera.distance = 40.0;
        lab.focus_body = None;
    }
    if keys.just_pressed(KeyCode::KeyN) {
        lab.spawned += 1;
        let site = nearby_site(
            lab.session.sim().launch_site,
            30.0 * lab.spawned as f64,
            lab.session.sim().planet.terrain.radius_meters,
        );
        lab.session.execute(Action::LaunchGround {
            craft: lab.craft.clone(),
            site,
        });
    }
    let warp_was_active = lab.session.sim().maneuver_warp.active();
    let id = lab.session.sim().selected.clone();
    let commanded = lab.session.sim().fleet.part_snapshots(&id).iter().any(|p| {
        p.definition
            .modules
            .iter()
            .any(|m| matches!(m, Module::Command))
    });
    if keys.just_pressed(KeyCode::KeyT) && commanded {
        let enabled = lab.session.sim().fleet.sas_phase(&id) == void_vessels::SasPhase::Off;
        lab.session.execute(Action::Sas { enabled });
    }
    let mut c = lab.session.sim().fleet.control(&id);
    let dt = time.delta_secs_f64().min(0.05);
    let throttle_axis = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]) as i32
        - keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]) as i32;
    if !keys.just_pressed(KeyCode::Tab) {
        c.throttle = (c.throttle + f64::from(throttle_axis) * dt * 0.5).clamp(0.0, 1.0);
    }
    if keys.just_pressed(KeyCode::KeyX) {
        c.throttle = 0.0;
    }
    c.turn = if commanded {
        DVec3::new(
            axis(&keys, KeyCode::KeyS, KeyCode::KeyW),
            axis(&keys, KeyCode::KeyE, KeyCode::KeyQ),
            axis(&keys, KeyCode::KeyD, KeyCode::KeyA),
        )
    } else {
        DVec3::ZERO
    };
    let previous = lab.session.sim().fleet.control(&id);
    if previous.throttle != c.throttle || previous.turn != c.turn {
        lab.session.execute(Action::Control {
            throttle: c.throttle,
            turn: c.turn,
        });
    }
    if keys.just_pressed(KeyCode::Space) {
        lab.session.execute(Action::Stage);
        lab.prediction = None;
    }
    if keys.just_pressed(KeyCode::Period) {
        lab.rate = (lab.rate + 1).min(RATES.len() - 1);
        lab.notice.clear();
    }
    if keys.just_pressed(KeyCode::Comma) {
        lab.rate = lab.rate.saturating_sub(1);
        lab.notice.clear();
    }
    if keys.just_pressed(KeyCode::KeyC) {
        lab.prediction = Some(lab.session.predict(600.0));
    }
    plan_controls(lab, &keys);
    if warp_was_active && !lab.session.sim().maneuver_warp.active() {
        lab.rate = 0;
    }
    view_controls(lab, &keys, &buttons, &motion, &scroll);
}
fn plan_controls(lab: &mut Lab, keys: &ButtonInput<KeyCode>) {
    use void_orbit::{ManeuverSpec, ReferenceMode};
    let id = lab.session.sim().selected.clone();
    let run = |lab: &mut Lab, action: Action| match lab.session.execute(action) {
        Outcome::Applied => lab.notice.clear(),
        Outcome::Refused(reason) => lab.notice = reason,
        other => panic!("unexpected maneuver outcome {other:?}"),
    };
    if keys.just_pressed(KeyCode::KeyM) {
        let sim = lab.session.sim();
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
            lab,
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
    let Some(p) = lab.session.sim().plans.get(&id) else {
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
            lab,
            Action::SelectManeuver {
                index: selected.saturating_sub(1),
            },
        );
    }
    if keys.just_pressed(KeyCode::BracketRight) {
        run(
            lab,
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
    let seconds = axis(keys, KeyCode::End, KeyCode::Home) * step;
    if prograde != 0.0 || normal != 0.0 || radial != 0.0 || seconds != 0.0 {
        spec.prograde += prograde;
        spec.normal += normal;
        spec.radial += radial;
        spec.start_time += seconds;
        run(
            lab,
            Action::EditManeuver {
                index: selected,
                spec,
            },
        );
    }
    if keys.just_pressed(KeyCode::KeyV) {
        if spec.reference_mode == ReferenceMode::Auto {
            spec.reference_mode = ReferenceMode::Fixed;
        } else if spec.reference_body + 1 < lab.session.sim().fleet.ephemeris.bodies().len() {
            spec.reference_body += 1;
        } else {
            spec.reference_mode = ReferenceMode::Auto;
            spec.reference_body = lab.session.sim().home;
        }
        run(
            lab,
            Action::EditManeuver {
                index: selected,
                spec,
            },
        );
    }
    if keys.just_pressed(KeyCode::Delete) {
        run(lab, Action::RemoveManeuver { index: selected });
    }
    if keys.just_pressed(KeyCode::KeyY) {
        run(
            lab,
            Action::PlaceManeuverAtApsis {
                index: selected,
                apsis: void_orbit::ApsisKind::Periapsis,
            },
        );
    }
    if keys.just_pressed(KeyCode::KeyU) {
        run(
            lab,
            Action::PlaceManeuverAtApsis {
                index: selected,
                apsis: void_orbit::ApsisKind::Apoapsis,
            },
        );
    }
    if keys.just_pressed(KeyCode::KeyZ) {
        if lab.session.sim().maneuver_warp.active() {
            run(lab, Action::CancelManeuverWarp);
            lab.rate = 0;
        } else {
            run(lab, Action::BeginManeuverWarp);
            if lab.session.sim().maneuver_warp.active() {
                lab.rate = RATES.len() - 1;
                lab.paused = false;
            }
        }
    }
    if keys.just_pressed(KeyCode::KeyB) {
        run(lab, Action::ExecuteManeuver);
    }
    if keys.just_pressed(KeyCode::Escape) {
        run(lab, Action::AbortManeuver);
    }
}
fn plan_description(lab: &Lab) -> String {
    let sim = lab.session.sim();
    let Some(p) = sim.plans.get(&sim.selected) else {
        return "M add maneuver | Z warp before burn | B execute first | Esc abort".into();
    };
    let mut text = format!(
        "Plan: {} maneuvers, {} completed | {}",
        p.plan.count(),
        p.plan.completed_count,
        p.message
    );
    if p.plan.count() > 0 {
        let spec = p.plan.maneuver(p.selected);
        let status = match p.plan.status(p.selected) {
            Ok(burn) => format!("burn {:.2}s", burn.end_time - burn.start_time),
            Err(reason) => format!("unavailable: {reason}"),
        };
        text.push_str(&format!(
            "\n[{}] T+{:.2}s Δv {:+.1}/{:+.1}/{:+.1}m/s {:?} {} | {}",
            p.selected + 1,
            spec.start_time,
            spec.prograde,
            spec.normal,
            spec.radial,
            spec.reference_mode,
            sim.fleet.ephemeris.bodies()[spec.reference_body].name,
            status
        ));
    }
    text.push_str("\nM add | [] select | arrows prograde/normal | PgUp/Dn radial | Home/End time | Y/U apsis | V reference | Del remove | Z warp | B execute first | Esc abort");
    text
}

fn view_controls(
    lab: &mut Lab,
    keys: &ButtonInput<KeyCode>,
    buttons: &ButtonInput<MouseButton>,
    motion: &AccumulatedMouseMotion,
    scroll: &AccumulatedMouseScroll,
) {
    if keys.just_pressed(KeyCode::F2) {
        lab.wire = !lab.wire;
    }
    if keys.just_pressed(KeyCode::F3) {
        lab.bounds = !lab.bounds;
    }
    if keys.just_pressed(KeyCode::F4) {
        lab.colliders = !lab.colliders;
    }
    if keys.just_pressed(KeyCode::F5) {
        lab.terrain = !lab.terrain;
    }
    if lab.main_game {
        if keys.just_pressed(KeyCode::KeyK) {
            lab.altitude_agl = !lab.altitude_agl;
        }
        if keys.just_pressed(KeyCode::KeyL) {
            lab.speed_surface = !lab.speed_surface;
        }
        if let Some(state) = lab.view {
            if buttons.pressed(MouseButton::Left) && !lab.pointer_over_label {
                lab.camera.drag(
                    f64::from(motion.delta.x),
                    f64::from(motion.delta.y),
                    state.up,
                );
            }
            let pixels = match scroll.unit {
                bevy::input::mouse::MouseScrollUnit::Line => f64::from(scroll.delta.y) * 40.0,
                bevy::input::mouse::MouseScrollUnit::Pixel => f64::from(scroll.delta.y),
            };
            lab.camera.zoom(
                (-pixels * 0.002).exp(),
                state.min_distance,
                state.max_distance,
            );
        }
        if keys.just_pressed(KeyCode::KeyG) {
            lab.path_frame = if lab.path_frame == void_view::PathFrameKind::Inertial {
                void_view::PathFrameKind::Surface
            } else {
                void_view::PathFrameKind::Inertial
            };
        }
        return;
    }
    if buttons.pressed(MouseButton::Left) {
        lab.yaw -= f64::from(motion.delta.x) * 0.006;
        lab.pitch = (lab.pitch + f64::from(motion.delta.y) * 0.006).clamp(-1.5, 1.5);
    }
    lab.distance = (lab.distance * (-f64::from(scroll.delta.y) * 0.12).exp()).clamp(2.0, 2e8);
}

fn begin_profile_frame(time: Res<Time>, mut lab: NonSendMut<Lab>) {
    if let Some((profile, _)) = &mut lab.profile {
        profile.sample("frame_interval", time.delta_secs_f64() * 1000.0);
    }
}
fn simulate(time: Res<Time>, window: Single<&Window>, mut lab: NonSendMut<Lab>) {
    let started = std::time::Instant::now();
    simulate_inner(&time, &window, &mut lab);
    if lab.main_game {
        let sim = lab.session.sim();
        let t = sim.fleet.time();
        let clear = sim.fleet.clearance(&sim.selected, sim.home);
        if clear > 20.0
            && (lab.prediction.is_none() || t < lab.prediction_at || t - lab.prediction_at >= 2.0)
        {
            lab.prediction = Some(lab.session.predict(6000.0));
            lab.prediction_at = t;
            lab.prediction_generation += 1;
        } else if clear <= 20.0 {
            lab.prediction = None;
        }
    }
    if let Some((profile, _)) = &mut lab.profile {
        profile.span("simulation", started, std::time::Instant::now());
    }
}
fn simulate_inner(time: &Time, window: &Window, lab: &mut Lab) {
    if lab.paused || !window.focused {
        return;
    }
    if let Some(mut playback) = lab.playback.take() {
        if playback.next_frame(&mut lab.session) {
            lab.playback = Some(playback);
        } else {
            lab.paused = true;
            lab.notice = "Replay complete: all world marks verified".into();
        }
        return;
    }
    let maneuver_warp = lab.session.sim().maneuver_warp.active();
    if maneuver_warp {
        lab.rate = RATES.len() - 1;
    }
    if lab.rate > 2 {
        let f = &lab.session.sim().fleet;
        let radius = lab.session.sim().planet.terrain.radius_meters;
        while lab.rate > 2
            && f.vessel_ids().iter().any(|id| {
                f.snapshot(id).mode == void_vessels::VesselMode::Orbit
                    && f.clearance(id, lab.session.sim().home)
                        < radius * crate::flight::rails_min_clearance_radii(RATES[lab.rate])
            })
        {
            lab.rate -= 1;
            lab.notice = "Warp limited by orbital vessel clearance".into();
        }
    }
    let rate = RATES[lab.rate];
    lab.frames += 1;
    let outcome = lab.session.execute(Action::Advance {
        seconds: time.delta_secs_f64().min(0.05) * rate,
        rails: rate > 4.0,
    });
    if maneuver_warp && !lab.session.sim().maneuver_warp.active() {
        lab.rate = 0;
        if let void_fleet_flight::warp::ManeuverWarp::Stopped { message } =
            &lab.session.sim().maneuver_warp
        {
            lab.notice = message.clone();
        }
    }
    if lab.frames.is_multiple_of(60) {
        lab.session.mark();
    }
    match outcome {
        Outcome::Advanced(true) => {}
        Outcome::Advanced(false) => {
            lab.rate = 0;
            lab.notice = "Rails stopped at an encounter or ground band".into();
        }
        Outcome::Refused(reason) => {
            lab.rate = 0;
            lab.notice = format!("Warp refused: {reason}");
        }
        other => panic!("unexpected advance outcome: {other:?}"),
    }
}
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn draw(
    mut commands: Commands,
    mut lab: NonSendMut<Lab>,
    assets: Res<RenderAssets>,
    mut ground: ResMut<Ground>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut parts: Query<
        (&Visual, &mut Transform, &mut Visibility),
        (Without<Tile>, Without<LabCamera>),
    >,
    mut tiles: Query<&mut Transform, (With<Tile>, Without<Visual>, Without<LabCamera>)>,
    mut tile_visibility: Query<&mut Visibility, (With<Tile>, Without<Visual>)>,
    tile_entities: Query<Entity, With<Tile>>,
    mut camera: Single<&mut Transform, With<LabCamera>>,
    projection: Single<&Projection, With<LabCamera>>,
    mut hud: Single<&mut Text, With<Hud>>,
    window: Single<&Window>,
    mut gizmos: Gizmos,
) {
    let started = std::time::Instant::now();
    let lab = &mut *lab;
    if lab.dirty {
        for (_, entities) in lab.parts.drain() {
            for e in entities {
                commands.entity(e).despawn();
            }
        }
        lab.collision.clear();
        lab.orbits = void_view::MapOrbits::new(lab.session.sim().fleet.ephemeris.bodies());
        lab.path = void_view::MapPath::new();
        lab.focus_body = None;
        lab.last_view_time = lab.session.sim().fleet.time();
        for entity in &tile_entities {
            commands.entity(entity).despawn();
        }
        ground.reset(&lab.session.sim().planet);
        lab.dirty = false;
    }
    let f = &lab.session.sim().fleet;
    let frame = PlanetFrame::new(&f.ephemeris, lab.session.sim().home);
    let a = void_orbit::body_orientation(&frame.body.rotation, f.time());
    let q = glam::DQuat::from_mat3(&glam::DMat3::from_cols(a[0], a[1], a[2])).normalize();
    let selected = f.snapshot(&lab.session.sim().selected);
    let state = frame.to_body_fixed(
        &f.ephemeris,
        f.time(),
        FrameState {
            position: selected.position,
            velocity: selected.velocity,
        },
    );
    let up = state.position.normalize();
    let east = if up.x.hypot(up.y) > 1e-9 {
        DVec3::new(-up.y, up.x, 0.0).normalize()
    } else {
        DVec3::X
    };
    let north = up.cross(east);
    let direction = east * (lab.yaw.cos() * lab.pitch.cos())
        + north * (lab.yaw.sin() * lab.pitch.cos())
        + up * lab.pitch.sin();
    let eye = if lab.main_game {
        let mut positions = vec![DVec3::ZERO; f.ephemeris.bodies().len()];
        f.ephemeris.positions_at(f.time(), &mut positions);
        let bodies = f.ephemeris.bodies();
        let reference = lab.focus_body.unwrap_or_else(|| {
            void_orbit::DominanceTree::new(bodies).dominant(&positions, selected.position)
        });
        let body = &bodies[reference];
        let radius = body.radius_meters;
        let radial = (selected.position - positions[reference]).normalize();
        let geometry = void_view::FocusGeometry {
            kind: if lab.focus_body.is_some() {
                void_view::FocusKind::Body
            } else {
                void_view::FocusKind::Vessel
            },
            radial: lab.focus_body.is_none().then_some(radial),
            north: body.rotation.axis(),
            reference_radius: radius,
            altitude: if lab.focus_body.is_none() {
                (selected.position - positions[reference]).length() - radius
            } else {
                0.0
            },
            focus_radius: if lab.focus_body.is_some() {
                radius
            } else {
                0.0
            },
        };
        let view = void_view::view_state(
            void_view::ViewMode::Single,
            false,
            &geometry,
            lab.camera.distance,
        );
        lab.camera.distance =
            lab.camera
                .clamp_distance(lab.camera.distance, view.min_distance, view.max_distance);
        let navigation =
            void_orbit::DominanceTree::new(bodies).dominant(&positions, selected.position);
        let spin = void_view::camera_spin(&view, lab.path_frame, reference, navigation);
        let elapsed = f.time() - lab.last_view_time;
        if elapsed >= 0.0 && spin.1 > 0.0 {
            lab.camera.corotate(
                bodies[spin.0].rotation.axis(),
                bodies[spin.0].rotation.rate() * elapsed * spin.1,
            );
        }
        lab.last_view_time = f.time();
        lab.camera.clamp_to_up(view.up);
        lab.focus_position = lab.focus_body.map_or(selected.position, |i| positions[i]);
        let eye_inertial = lab.focus_position + lab.camera.direction * lab.camera.distance;
        let eye = frame
            .to_body_fixed(
                &f.ephemeris,
                f.time(),
                FrameState {
                    position: eye_inertial,
                    velocity: DVec3::ZERO,
                },
            )
            .position;
        let focus = frame
            .to_body_fixed(
                &f.ephemeris,
                f.time(),
                FrameState {
                    position: lab.focus_position,
                    velocity: DVec3::ZERO,
                },
            )
            .position;
        **camera = Transform::default()
            .looking_to((focus - eye).as_vec3(), (q.conjugate() * view.up).as_vec3());
        lab.view = Some(view);
        lab.distance = lab.camera.distance;
        eye
    } else {
        let eye = state.position + direction * lab.distance;
        **camera = Transform::default().looking_to((state.position - eye).as_vec3(), up.as_vec3());
        eye
    };
    lab.eye = eye;
    let snapshots = f
        .vessel_ids()
        .iter()
        .flat_map(|id| f.part_snapshots(id))
        .collect::<Vec<_>>();
    let live = snapshots
        .iter()
        .map(|p| p.id.clone())
        .collect::<HashSet<_>>();
    lab.parts.retain(|id, entities| {
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
        if !lab.parts.contains_key(&p.id) {
            let origin = frame
                .to_body_fixed(
                    &f.ephemeris,
                    f.time(),
                    FrameState {
                        position: p.position,
                        velocity: DVec3::ZERO,
                    },
                )
                .position;
            let root = Transform::from_translation((origin - eye).as_vec3())
                .with_rotation((q.conjugate() * p.rotation).as_quat());
            let entities = assets.parts[&p.definition.id]
                .iter()
                .map(|piece| {
                    commands
                        .spawn((
                            Visual {
                                id: p.id.clone(),
                                local: piece.local,
                                flame: piece.flame,
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
            lab.parts.insert(p.id.clone(), entities);
        }
    }
    for (visual, mut transform, mut visibility) in &mut parts {
        if let Some(p) = snapshots.iter().find(|p| p.id == visual.id) {
            let origin = frame
                .to_body_fixed(
                    &f.ephemeris,
                    f.time(),
                    FrameState {
                        position: p.position,
                        velocity: DVec3::ZERO,
                    },
                )
                .position;
            *transform = Transform::from_translation((origin - eye).as_vec3())
                .with_rotation((q.conjugate() * p.rotation).as_quat())
                .mul_transform(visual.local);
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
            frame
                .to_body_fixed(
                    &f.ephemeris,
                    f.time(),
                    FrameState {
                        position: p.position,
                        velocity: DVec3::ZERO,
                    },
                )
                .position
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
                _ => panic!("Fleet scene requires a perspective camera"),
            },
            min_observer_cell_pixels: 3.0,
        }),
        observer_positions: observers,
        distance_scale: 1.0,
        horizon_culling: true,
    });
    ground.set_wireframe(&mut commands, lab.wire);
    ground.draw(&mut commands, &mut meshes, &mut tiles, eye);
    for mut v in &mut tile_visibility {
        *v = if lab.terrain {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
    if lab.bounds {
        for line in ground.boundaries(eye) {
            gizmos.linestrip(line, Color::srgb(1.0, 0.2, 0.2));
        }
    }
    let terrain_tiles = f.terrain_tiles();
    let live = terrain_tiles
        .iter()
        .map(|t| (t.scene, t.tile.clone()))
        .collect::<HashSet<_>>();
    lab.collision.retain(|key, _| live.contains(key));
    if lab.colliders {
        for collider in f.vessel_collider_meshes() {
            let origin = frame
                .to_body_fixed(
                    &f.ephemeris,
                    f.time(),
                    FrameState {
                        position: collider.position,
                        velocity: DVec3::ZERO,
                    },
                )
                .position;
            let rotation = q.conjugate() * collider.rotation;
            let vertices = &collider.mesh.vertices;
            let indices = unique_edges(&collider.mesh.triangles);
            for edge in indices.as_chunks::<2>().0 {
                let point = |i: u32| {
                    (origin - eye + rotation * Vec3::from_array(vertices[i as usize]).as_dvec3())
                        .as_vec3()
                };
                gizmos.line(point(edge[0]), point(edge[1]), Color::srgb(0.2, 1.0, 0.4));
            }
        }
        for tile in terrain_tiles {
            let key = (tile.scene, tile.tile.clone());
            let vertices = lab.collision.entry(key).or_insert_with(|| {
                let (vertices, triangles) = f.terrain_geometry(tile.scene, &tile.tile);
                unique_edges(&triangles)
                    .iter()
                    .map(|i| Vec3::from_array(vertices[*i as usize]))
                    .collect()
            });
            let origin = frame
                .to_body_fixed(
                    &f.ephemeris,
                    f.time(),
                    FrameState {
                        position: tile.position,
                        velocity: DVec3::ZERO,
                    },
                )
                .position;
            let rot = q.conjugate() * tile.rotation;
            for edge in vertices.as_chunks::<2>().0 {
                gizmos.line(
                    (origin - eye + rot * edge[0].as_dvec3()).as_vec3(),
                    (origin - eye + rot * edge[1].as_dvec3()).as_vec3(),
                    Color::srgb(0.2, 1.0, 0.4),
                );
            }
        }
    }
    if let Some(prediction) = &lab.prediction
        && !lab.main_game
    {
        gizmos.linestrip(
            prediction.points.iter().map(|(_, p)| (*p - eye).as_vec3()),
            Color::srgb(0.2, 0.9, 1.0),
        );
    }
    if !lab.main_game
        && let Some(plan) = lab.session.sim().plans.get(&lab.session.sim().selected)
    {
        let trajectory = &plan.plan.trajectory;
        gizmos.linestrip(
            (0..trajectory.count()).map(|i| {
                let local = frame.to_body_fixed(
                    &f.ephemeris,
                    f.time(),
                    FrameState {
                        position: trajectory.position(i),
                        velocity: trajectory.velocity(i),
                    },
                );
                (local.position - eye).as_vec3()
            }),
            Color::srgb(1.0, 0.6, 0.15),
        );
    }
    let p = f.thrust(&lab.session.sim().selected);
    let mut positions = vec![DVec3::ZERO; f.ephemeris.bodies().len()];
    let mut velocities = positions.clone();
    f.ephemeris
        .states_at(f.time(), &mut positions, Some(&mut velocities));
    let navigation = void_orbit::DominanceTree::new(f.ephemeris.bodies())
        .dominant(&positions, selected.position);
    let body = &f.ephemeris.bodies()[navigation];
    let r = selected.position - positions[navigation];
    let v = selected.velocity - velocities[navigation];
    let orbital = void_orbit::osculating_orbit(r, v, body.gm);
    let surface = PlanetFrame::new(&f.ephemeris, navigation).to_body_fixed(
        &f.ephemeris,
        f.time(),
        FrameState {
            position: selected.position,
            velocity: selected.velocity,
        },
    );
    let altitude = if lab.altitude_agl && navigation == lab.session.sim().home {
        f.clearance(&lab.session.sim().selected, navigation)
    } else {
        r.length() - body.radius_meters
    };
    let speed = if lab.speed_surface {
        surface.velocity.length()
    } else {
        v.length()
    };
    let fuel: f64 = f
        .part_snapshots(&lab.session.sim().selected)
        .iter()
        .map(|p| p.fuel_kg)
        .sum();
    **hud = Text::new(format!(
        "{}\n{} ({}) | {:?} | {} | {}x\nT+{:.2}s {} {:.1}m {} {:.1}m/s | {}\nmass {:.1}kg fuel {:.1}kg throttle {:.0}% force {:.1}kN SAS {:?}\nPe {:.1}km Ap {:.1}km | {} vessels\nP pause | Space stage | Shift/Ctrl throttle | X cut | WASD QE turn | T SAS\nTab vessel | Shift+Tab body focus | click map labels | G path frame\nN nearby craft | O orbital craft | R reset | , . warp | K altitude | L speed\nF2 wire | F3 boundaries | F4 actual colliders | F5 terrain\nF6 save | F7 load (paused) | F8 finish recording | F9 finish CPU profile\n{}",
        if lab.main_game {
            "VOID"
        } else {
            "FLEET FLIGHT INTEGRATION"
        },
        selected.name,
        lab.session.sim().selected,
        selected.mode,
        if lab.paused { "paused" } else { "running" },
        RATES[lab.rate],
        f.time(),
        if lab.altitude_agl && navigation == lab.session.sim().home {
            "AGL"
        } else {
            "ALT"
        },
        altitude,
        if lab.speed_surface {
            "surface"
        } else {
            "orbit"
        },
        speed,
        body.name,
        selected.mass_kg,
        fuel,
        f.control(&lab.session.sim().selected).throttle * 100.0,
        p.force.length() / 1000.0,
        f.sas_phase(&lab.session.sim().selected),
        (orbital.periapsis_radius_meters - body.radius_meters) / 1000.0,
        (orbital.apoapsis_radius_meters - body.radius_meters) / 1000.0,
        f.vessel_ids().len(),
        format_args!("{}\n{}", lab.notice, plan_description(lab)),
    ));
    if let Some((profile, _)) = &mut lab.profile {
        profile.span("draw_lod_overlays", started, std::time::Instant::now());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn initialized_scene(main_game: bool) -> App {
        let planet = game_planet_by_id(if main_game { "aurelia" } else { "pebble" }, None);
        let craft = demo_craft();
        let site = demo_rocket(&planet.planet.terrain).launch_site.normalize();
        let sim = FlightSession::new(InitialWorld::new(&planet.planet, &craft, site, false));
        let mut lab = new_lab(sim, craft);
        lab.main_game = main_game;
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()))
            .insert_resource(Assets::<Mesh>::default())
            .insert_resource(Assets::<bevy::mesh::skinning::SkinnedMeshInverseBindposes>::default())
            .insert_resource(Assets::<StandardMaterial>::default())
            .insert_resource(Assets::<Image>::default())
            .insert_resource(Assets::<crate::scenery::GroundMaterial>::default())
            .insert_resource(Assets::<crate::scenery::StarMaterial>::default())
            .insert_non_send(lab)
            .add_plugins(bevy::gizmos::GizmoPlugin)
            .add_systems(Startup, (setup, setup_scenery).chain())
            .add_systems(
                Update,
                (refresh_scenery, draw, draw_map, instruments, update_scenery).chain(),
            );
        // A Window component supplies dimensions; no WindowPlugin or OS window is created.
        app.world_mut().spawn(Window::default());
        app.update();
        {
            let mut lab = app.world_mut().non_send_mut::<Lab>();
            lab.colliders = true;
            lab.bounds = true;
            lab.wire = true;
            lab.terrain = false;
        }
        app.update();
        let lab = app.world().non_send::<Lab>();
        assert!(!lab.parts.is_empty());
        assert!(!lab.collision.is_empty());
        app
    }
    #[test]
    fn integration_scene_initializes_and_draws_without_a_window_or_renderer() {
        let _ = initialized_scene(false);
    }
    #[test]
    fn main_scene_draws_fleet_scenery_and_navball_without_a_window_or_renderer() {
        let mut app = initialized_scene(true);
        assert!(matches!(
            app.world().resource::<Ground>(),
            Ground::Shaded(..)
        ));
        assert_eq!(
            app.world_mut()
                .query::<&crate::navball::Navball>()
                .iter(app.world())
                .count(),
            1
        );
        assert!(app.world().contains_resource::<crate::air::AirTextures>());
        // Loading a world with a different terrain replaces shader inputs and the terrain source.
        let planet = game_planet_by_id("luna", None);
        let craft = demo_craft();
        let site = demo_rocket(&planet.planet.terrain).launch_site.normalize();
        {
            let mut lab = app.world_mut().non_send_mut::<Lab>();
            lab.session =
                FlightSession::new(InitialWorld::new(&planet.planet, &craft, site, false));
            lab.dirty = true;
        }
        app.update();
        assert_eq!(
            app.world()
                .resource::<SceneryState>()
                .uniforms
                .bottom_radius,
            planet.planet.terrain.radius_meters as f32
        );
    }

    #[test]
    fn warp_key_and_window_step_resume_one_x_before_ignition() {
        let planet = void_landing::earth_size();
        let pod = void_vessels::pod_tank("Resting pod");
        let craft = demo_craft();
        let mut session = FlightSession::new(InitialWorld::new(
            &planet,
            &pod,
            void_vessels::flat_site(&planet),
            false,
        ));
        session.execute(Action::Advance {
            seconds: 20.0,
            rails: false,
        });
        let Outcome::Spawned(id) = session.execute(Action::LaunchOrbit {
            craft: craft.clone(),
            offset: DVec3::ZERO,
        }) else {
            panic!("launch")
        };
        session.execute(Action::Select { vessel: id });
        session.execute(Action::Stage);
        let start = session.sim().fleet.time() + 200.037;
        session.execute(Action::AddManeuver {
            spec: void_orbit::ManeuverSpec {
                start_time: start,
                reference_body: session.sim().home,
                reference_mode: void_orbit::ReferenceMode::Fixed,
                prograde: 10.0,
                normal: 0.0,
                radial: 0.0,
            },
        });
        let mut lab = new_lab(session, craft);
        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::KeyZ);
        plan_controls(&mut lab, &keys);
        assert!(lab.session.sim().maneuver_warp.active());
        assert!(!lab.paused);
        let mut time = Time::<()>::default();
        time.advance_by(std::time::Duration::from_secs(1));
        simulate_inner(&time, &Window::default(), &mut lab);
        assert_eq!(lab.rate, 0);
        assert_eq!(lab.session.sim().fleet.time(), start - 30.0);
        assert!(lab.notice.contains("30 seconds"));
    }

    #[test]
    fn maneuver_keys_arm_a_plan_and_switch_ship_without_aborting_it() {
        let mut app = initialized_scene(true);
        let id = {
            let mut lab = app.world_mut().non_send_mut::<Lab>();
            let craft = lab.craft.clone();
            let Outcome::Spawned(id) = lab.session.execute(Action::LaunchOrbit {
                craft,
                offset: DVec3::ZERO,
            }) else {
                panic!("launch")
            };
            lab.session.execute(Action::Select { vessel: id.clone() });
            lab.session.execute(Action::Stage);
            id
        };
        app.insert_resource(ButtonInput::<KeyCode>::default())
            .insert_resource(ButtonInput::<MouseButton>::default())
            .insert_resource(AccumulatedMouseMotion::default())
            .insert_resource(AccumulatedMouseScroll::default())
            .add_systems(Update, controls.before(draw));
        for key in [KeyCode::KeyM, KeyCode::KeyB, KeyCode::Tab] {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.reset_all();
            keys.press(key);
            app.update();
        }
        let lab = app.world().non_send::<Lab>();
        assert_ne!(lab.session.sim().selected, id);
        assert!(lab.session.sim().plans[&id].executing);
        assert_eq!(
            lab.session.sim().fleet.guidance(&id).unwrap().status,
            void_vessels::GuidanceStatus::Armed
        );
    }

    #[test]
    fn unfocused_window_does_not_inject_control_changes_into_replay() {
        let mut app = initialized_scene(false);
        let initial = app
            .world()
            .non_send::<Lab>()
            .session
            .recording_initial()
            .clone();
        let mut flown = FlightSession::new(initial);
        flown.execute(Action::Control {
            throttle: 0.0,
            turn: DVec3::X * 0.25,
        });
        flown.execute(Action::Advance {
            seconds: 0.113,
            rails: false,
        });
        flown.mark();
        flown.execute(Action::Advance {
            seconds: 0.113,
            rails: false,
        });
        let (mut replay, mut session) = Playback::new(flown.recording());
        assert!(replay.next_frame(&mut session));
        let before = void_fleet_flight::session::world_mark(session.sim());
        {
            let mut lab = app.world_mut().non_send_mut::<Lab>();
            lab.session = session;
            lab.playback = Some(replay);
        }
        app.world_mut()
            .query::<&mut Window>()
            .single_mut(app.world_mut())
            .unwrap()
            .focused = false;
        app.insert_resource(ButtonInput::<KeyCode>::default())
            .insert_resource(ButtonInput::<MouseButton>::default())
            .insert_resource(AccumulatedMouseMotion::default())
            .insert_resource(AccumulatedMouseScroll::default())
            .add_systems(Update, controls.before(draw));
        app.update();
        assert_eq!(
            before,
            void_fleet_flight::session::world_mark(app.world().non_send::<Lab>().session.sim())
        );
    }
}

#[derive(Component)]
struct Sky;
#[derive(Resource)]
struct SceneryState {
    terrain: std::sync::Arc<void_terrain::Terrain>,
    uniforms: crate::scenery::GroundUniforms,
    ground: Handle<crate::scenery::GroundMaterial>,
    stars: Handle<crate::scenery::StarMaterial>,
}

#[allow(clippy::too_many_arguments)]
fn build_scenery(
    commands: &mut Commands,
    lab: &Lab,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    grounds: &mut Assets<crate::scenery::GroundMaterial>,
    star_materials: &mut Assets<crate::scenery::StarMaterial>,
    camera: Entity,
) {
    use crate::scenery::*;
    use void_scenery::atmosphere::*;
    use void_scenery::clouds::*;
    use void_scenery::tables::*;
    let planet = &lab.session.sim().planet;
    let params = void_scenery::earth_like_atmosphere(planet.terrain.radius_meters);
    let transmittance = build_transmittance_table(&params);
    let multiple = build_multiple_scattering_table(&params, &transmittance, 64, 20);
    let irradiance = build_irradiance_table(&params, &transmittance, &multiple, 128, 24);
    let layered = matches!(
        planet.terrain_config,
        void_terrain::TerrainConfig::Layered(_)
    );
    let air = planet.air_density_scale.is_some();
    let max = planet.terrain.max_height_meters;
    let mut uniforms = GroundUniforms::new(
        &params,
        if layered {
            void_terrain::SEA_LEVEL
        } else if air {
            1800.0
        } else {
            0.0
        },
        if layered {
            void_terrain::SEA_LEVEL + 2600.0
        } else {
            max * 0.5625
        },
        if layered {
            void_terrain::SEA_LEVEL + 4800.0
        } else {
            max * 0.75
        },
    );
    uniforms.ocean_enabled = f32::from(u8::from(layered));
    uniforms.atmosphere_enabled = f32::from(u8::from(air));
    let transmittance = images.add(table_image(
        &transmittance,
        TRANSMITTANCE_WIDTH,
        TRANSMITTANCE_HEIGHT,
    ));
    let irradiance = images.add(table_image(
        &irradiance,
        IRRADIANCE_WIDTH,
        IRRADIANCE_HEIGHT,
    ));
    let ground = grounds.add(GroundMaterial {
        ground: uniforms,
        transmittance: transmittance.clone(),
        irradiance: irradiance.clone(),
    });
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    commands.insert_resource(crate::air::AirTextures {
        transmittance,
        multiple: images.add(table_image(
            &multiple,
            MULTIPLE_SCATTERING_SIZE,
            MULTIPLE_SCATTERING_SIZE,
        )),
        irradiance,
        weather: images.add(crate::air::weather_image(
            build_cloud_weather(threads),
            WEATHER_WIDTH,
            WEATHER_HEIGHT,
        )),
        shape: images.add(crate::air::noise_volume_image(
            build_cloud_noise(SHAPE_SIZE, false),
            SHAPE_SIZE,
        )),
        detail: images.add(crate::air::noise_volume_image(
            build_cloud_noise(DETAIL_SIZE, true),
            DETAIL_SIZE,
        )),
    });
    let demo = demo_rocket(&planet.terrain);
    let mut field = TileField::new(
        landing_lod_options(&planet.terrain, &demo.options.contact),
        Some(planet.terrain.clone()),
        ground.clone(),
    );
    field.no_frustum_culling = true;
    field.wireframe_color = scene_color(Color::WHITE);
    commands.insert_resource(Ground::Shaded(Box::new(field), ground.clone()));
    let (positions, colors) = void_scenery::generate_stars(&void_scenery::DEFAULT_STARS);
    let stars = star_materials.add(StarMaterial { brightness: 0.08 });
    commands.spawn((
        Sky,
        Mesh3d(meshes.add(star_mesh(positions, &colors))),
        MeshMaterial3d(stars.clone()),
        Transform::default(),
        bevy::camera::visibility::NoFrustumCulling,
    ));
    let mut settings = crate::air::AirSettings::new(&params);
    settings.exposure = EXPOSURE;
    settings.enabled = f32::from(u8::from(air));
    settings.clouds_enabled = f32::from(u8::from(air));
    settings.sea_level = uniforms.sea_level;
    settings.sun_disc_enabled = f32::from(u8::from(
        lab.session.sim().fleet.ephemeris.bodies()[lab.session.sim().home]
            .parent_index
            .is_none(),
    ));
    commands.entity(camera).insert((
        Camera3d {
            depth_texture_usages: (bevy::render::render_resource::TextureUsages::RENDER_ATTACHMENT
                | bevy::render::render_resource::TextureUsages::TEXTURE_BINDING)
                .into(),
            ..default()
        },
        settings,
        bevy::camera::Hdr,
        bevy::render::view::Msaa::Off,
        bevy::core_pipeline::tonemapping::Tonemapping::None,
        bevy::core_pipeline::tonemapping::DebandDither::Disabled,
        Projection::Perspective(PerspectiveProjection {
            fov: 58.0_f32.to_radians(),
            far: 1e14,
            ..default()
        }),
    ));
    commands.insert_resource(SceneryState {
        terrain: planet.terrain.clone(),
        uniforms,
        ground,
        stars,
    });
}

#[allow(clippy::too_many_arguments)]
fn setup_scenery(
    mut commands: Commands,
    lab: NonSend<Lab>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    grounds: Option<ResMut<Assets<crate::scenery::GroundMaterial>>>,
    stars: Option<ResMut<Assets<crate::scenery::StarMaterial>>>,
    camera: Single<Entity, With<LabCamera>>,
) {
    if !lab.main_game {
        return;
    }
    build_scenery(
        &mut commands,
        &lab,
        &mut meshes,
        &mut images,
        grounds.expect("main scenery materials").into_inner(),
        stars.expect("main star materials").into_inner(),
        *camera,
    );
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn refresh_scenery(
    mut commands: Commands,
    lab: NonSend<Lab>,
    state: Option<Res<SceneryState>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    grounds: Option<ResMut<Assets<crate::scenery::GroundMaterial>>>,
    stars: Option<ResMut<Assets<crate::scenery::StarMaterial>>>,
    camera: Single<Entity, With<LabCamera>>,
    old_scene: Query<Entity, Or<(With<Sky>, With<BodySphere>, With<crate::map::MapMarker>)>>,
) {
    if !lab.main_game {
        return;
    }
    if state
        .as_ref()
        .is_some_and(|s| std::sync::Arc::ptr_eq(&s.terrain, &lab.session.sim().planet.terrain))
    {
        return;
    }
    for e in &old_scene {
        commands.entity(e).despawn();
    }
    spawn_bodies(
        &mut commands,
        &mut meshes,
        &mut materials,
        lab.session.sim().fleet.ephemeris.bodies(),
        lab.session.sim().home,
    );
    crate::map::spawn_map_labels(&mut commands, lab.session.sim().fleet.ephemeris.bodies());
    build_scenery(
        &mut commands,
        &lab,
        &mut meshes,
        &mut images,
        grounds.expect("main scenery materials").into_inner(),
        stars.expect("main star materials").into_inner(),
        *camera,
    );
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn update_scenery(
    lab: NonSend<Lab>,
    state: Option<ResMut<SceneryState>>,
    window: Single<&Window>,
    grounds: Option<ResMut<Assets<crate::scenery::GroundMaterial>>>,
    stars: Option<ResMut<Assets<crate::scenery::StarMaterial>>>,
    mut camera: Query<(&Transform, &mut crate::air::AirSettings, &Projection), With<LabCamera>>,
    mut sky: Query<&mut Transform, (With<Sky>, Without<LabCamera>)>,
    mut light: Query<&mut Transform, (With<SceneSun>, Without<Sky>, Without<LabCamera>)>,
) {
    let Some(mut state) = state else {
        return;
    };
    let sim = lab.session.sim();
    let f = &sim.fleet;
    let frame = PlanetFrame::new(&f.ephemeris, sim.home);
    let axes = void_orbit::body_orientation(&frame.body.rotation, f.time());
    let q = glam::DQuat::from_mat3(&glam::DMat3::from_cols(axes[0], axes[1], axes[2])).normalize();
    let eye = lab.eye;
    let bodies = f.ephemeris.bodies();
    let root = bodies
        .iter()
        .find(|b| b.parent_index.is_none())
        .expect("system root");
    // Lone-planet lab presets deliberately have no luminous body; retain their fixed inertial sun.
    let inertial_sun = if root.index == sim.home {
        DVec3::X
    } else {
        let d = f.ephemeris.body_position(root.index, f.time())
            - f.ephemeris.body_position(sim.home, f.time());
        assert!(
            d.is_finite() && d.length_squared() > 0.0,
            "coincident sun and home planet"
        );
        d.normalize()
    };
    let sun = q.conjugate() * inertial_sun;
    for mut transform in &mut light {
        let up = if sun.z.abs() < 0.9999 {
            Vec3::Z
        } else {
            Vec3::Y
        };
        *transform = Transform::default().looking_to((-sun).as_vec3(), up);
    }
    for (transform, mut air, projection) in &mut camera {
        if let Projection::Perspective(p) = projection {
            let focal =
                f64::from(window.physical_height().max(1)) / (2.0 * (f64::from(p.fov) / 2.0).tan());
            air.update(eye, transform.rotation, p, focal, sun);
        }
    }
    crate::scenery::update_ground(&mut state.uniforms, eye, sun, f.time());
    grounds
        .expect("main scenery materials")
        .get_mut(&state.ground)
        .expect("ground material")
        .ground = state.uniforms;
    for mut transform in &mut sky {
        transform.rotation = q.conjugate().as_quat();
    }
    let altitude = eye.length() - frame.body.radius_meters;
    let smooth = |a: f64, b: f64, x: f64| {
        let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    };
    let daylight = smooth(-0.18, 0.02, eye.normalize().dot(sun))
        * (1.0 - smooth(0.0, 60e3, altitude))
        * f64::from(u8::from(sim.planet.air_density_scale.is_some()));
    stars
        .expect("main star materials")
        .get_mut(&state.stars)
        .expect("star material")
        .brightness = (0.08 * (1.0 - daylight)) as f32;
}

#[derive(Component)]
struct BodySphere(usize);
fn spawn_bodies(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    bodies: &[void_orbit::CelestialBody],
    home: usize,
) {
    let sphere = meshes.add(Sphere::new(1.0).mesh().uv(64, 32));
    for body in bodies.iter().filter(|b| b.index != home) {
        let material = materials.add(StandardMaterial {
            base_color: crate::map::color(&body.color),
            unlit: body.parent_index.is_none(),
            perceptual_roughness: 0.9,
            ..default()
        });
        commands.spawn((
            BodySphere(body.index),
            Mesh3d(sphere.clone()),
            MeshMaterial3d(material),
            Transform::default(),
        ));
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn draw_map(
    mut lab: NonSendMut<Lab>,
    mut spheres: Query<(&BodySphere, &mut Transform), Without<LabCamera>>,
    camera: Single<(&Camera, &GlobalTransform), With<LabCamera>>,
    mut markers: Query<(
        &crate::map::MapMarker,
        &mut Node,
        &mut Visibility,
        &ComputedNode,
    )>,
    mut texts: Query<(&mut Text, &mut Visibility), Without<crate::map::MapMarker>>,
    mut gizmos: Gizmos,
) {
    let lab = &mut *lab;
    if !lab.main_game {
        return;
    }
    let view = lab.view.expect("main camera state");
    let fleet = &lab.session.sim().fleet;
    let home = lab.session.sim().home;
    let bodies = fleet.ephemeris.bodies();
    let mut positions = vec![DVec3::ZERO; bodies.len()];
    let mut velocities = positions.clone();
    fleet
        .ephemeris
        .states_at(fleet.time(), &mut positions, Some(&mut velocities));
    let ship = fleet.snapshot(&lab.session.sim().selected);
    let reference = void_orbit::DominanceTree::new(bodies).dominant(&positions, ship.position);
    let axes = void_orbit::body_orientation(&bodies[home].rotation, fleet.time());
    let q = glam::DQuat::from_mat3(&glam::DMat3::from_cols(axes[0], axes[1], axes[2])).normalize();
    let frame = void_view::MapFrame {
        time: fleet.time(),
        positions: &positions,
        velocities: &velocities,
        origin: lab.focus_position,
        vessel: ship.position,
        vessel_velocity: ship.velocity,
        plotting: void_view::PlottingFrame {
            kind: lab.path_frame,
            reference,
        },
        // Simulation time makes refresh cadence independent of replay rendering speed.
        wall_ms: fleet.time() * 1000.0,
    };
    lab.orbits.update(bodies, &frame);
    if let Some(prediction) = &lab.prediction {
        lab.path.update(
            &fleet.ephemeris,
            &prediction.trajectory,
            lab.prediction_generation,
            &frame,
            true,
        );
    } else {
        lab.path.hide();
    }
    if lab.plan_vessel != lab.session.sim().selected {
        lab.plan_path = void_view::MapPath::new();
        lab.plan_vessel = lab.session.sim().selected.clone();
    }
    if let Some(p) = lab.session.sim().plans.get(&lab.plan_vessel) {
        lab.plan_path.update(
            &fleet.ephemeris,
            &p.plan.trajectory,
            p.plan.generation,
            &frame,
            true,
        );
    } else {
        lab.plan_path.hide();
    }
    let home_frame = PlanetFrame::new(&fleet.ephemeris, home);
    let eye_inertial = home_frame
        .to_inertial(
            &fleet.ephemeris,
            fleet.time(),
            FrameState {
                position: lab.eye,
                velocity: DVec3::ZERO,
            },
        )
        .position;
    let render = |v: DVec3| (q.conjugate() * (v + frame.origin - eye_inertial)).as_vec3();
    for (body, mut transform) in &mut spheres {
        transform.translation = render(positions[body.0] - frame.origin);
        transform.scale = Vec3::splat(bodies[body.0].radius_meters as f32);
    }
    crate::map::draw_map_lines(
        &mut gizmos,
        bodies,
        &lab.orbits,
        &[
            (&lab.path, crate::map::color(crate::map::PATH_COLOR)),
            (&lab.plan_path, Color::srgb(1.0, 0.6, 0.15)),
        ],
        &frame,
        view.map_weight as f32,
        &render,
    );
    let wanted = void_view::map_labels(
        bodies,
        &frame,
        lab.focus_body,
        &lab.path.apsis_positions(&frame),
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
