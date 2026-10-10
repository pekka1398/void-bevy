//! Headless checks of the main game's basic flow: no window, no renderer.
use super::ui::{Click, Field, PendingClicks, UiState};
use super::*;
use void_fleet_flight::session::world_mark;

fn input_resources(app: &mut App) {
    app.insert_resource(ButtonInput::<KeyCode>::default())
        .insert_resource(ButtonInput::<MouseButton>::default())
        .insert_resource(AccumulatedMouseMotion::default())
        .insert_resource(AccumulatedMouseScroll::default())
        .add_message::<bevy::input::keyboard::KeyboardInput>();
}
/// The main game with `craft`, drawn once with every overlay on.
fn scene(craft: Craft) -> App {
    let mut flight = Flight::new(
        FlightSession::new(void_fleet_flight::world::main_game(&craft)).with_recording(),
        craft,
    );
    flight.session.execute(Action::View {
        command: ViewCommand::Configure { main_camera: true },
    });
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()))
        .insert_resource(Assets::<Mesh>::default())
        .insert_resource(Assets::<bevy::mesh::skinning::SkinnedMeshInverseBindposes>::default())
        .insert_resource(Assets::<StandardMaterial>::default())
        .insert_resource(Assets::<Image>::default())
        .insert_resource(Assets::<Font>::default())
        .insert_resource(Assets::<crate::scenery::GroundMaterial>::default())
        .insert_resource(Assets::<crate::scenery::StarMaterial>::default())
        .add_plugins(bevy::gizmos::GizmoPlugin);
    insert_game(&mut app, flight);
    app.add_systems(
        Update,
        (refresh_scenery, draw, draw_map, instruments, update_scenery).chain(),
    );
    // A Window component supplies dimensions; no WindowPlugin or OS window is created.
    app.world_mut().spawn(Window::default());
    app.update();
    for setting in [
        Toggle::Colliders,
        Toggle::Bounds,
        Toggle::Wire,
        Toggle::Terrain,
    ] {
        session(&mut app).execute(Action::View {
            command: ViewCommand::Toggle { setting },
        });
    }
    app.update();
    let visuals = app.world().resource::<PartVisuals>();
    assert!(!visuals.parts.is_empty());
    assert!(!visuals.collision.is_empty());
    app
}
pub(super) fn initialized_scene() -> App {
    scene(void_assembly::rcs_flight_rocket())
}
fn session(app: &mut App) -> Mut<'_, FlightSession> {
    app.world_mut()
        .non_send_mut::<Flight>()
        .map_unchanged(|f| &mut f.session)
}
fn sim(app: &App) -> &void_fleet_flight::FleetFlight {
    app.world().non_send::<Flight>().session.sim()
}
fn press(app: &mut App, keys: &[KeyCode]) {
    let mut input = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
    input.reset_all();
    for key in keys {
        input.press(*key);
    }
}
fn launch_orbit(app: &mut App) -> String {
    let craft = app.world().non_send::<Flight>().craft.clone();
    let Outcome::Spawned(id) = session(app).execute(Action::LaunchOrbit {
        craft,
        offset: DVec3::ZERO,
    }) else {
        panic!("orbit launch")
    };
    id
}
fn window(app: &mut App) -> Entity {
    app.world_mut()
        .query_filtered::<Entity, With<Window>>()
        .single(app.world())
        .unwrap()
}

#[test]
fn arguments_accept_only_the_kept_entries() {
    let parse = |args: &[&str]| Arguments::parse(args.iter().map(|s| s.to_string()));
    assert!(parse(&[]).is_ok());
    assert!(parse(&["--craft", "c.json", "--record", "r"]).is_ok());
    assert!(parse(&["--load", "s.json", "--profile", "p"]).is_ok());
    assert!(parse(&["--recover-recording", "a", "--output", "b"]).is_ok());
    for bad in [
        &["--reentry"][..],
        &["--planet", "luna"],
        &["--craft"],
        &["--load", "a", "--craft", "b"],
        &["--replay", "a", "--record", "b"],
        &["--output", "b"],
        &["--craft", "a", "--craft", "b"],
    ] {
        assert!(parse(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn main_scene_draws_scenery_and_navball_without_a_window_or_renderer() {
    let mut app = initialized_scene();
    assert!(matches!(
        app.world_mut()
            .query::<&Msaa>()
            .single(app.world())
            .unwrap(),
        Msaa::Off
    ));
    assert_eq!(
        app.world_mut()
            .query::<&bevy::camera::Hdr>()
            .iter(app.world())
            .count(),
        1
    );
    let home = sim(&app).home;
    let world = &app.world().resource::<Ground>().0;
    let material = world.bodies[&home].material.clone();
    let sea = sim(&app).planet.terrain.radius_meters + void_terrain::SEA_LEVEL;
    assert_eq!(
        app.world()
            .resource::<Assets<crate::scenery::GroundMaterial>>()
            .get(&material)
            .unwrap()
            .ground
            .bottom_radius,
        sea as f32
    );
    let layers = app
        .world_mut()
        .query::<&crate::air::AirLayers>()
        .single(app.world())
        .unwrap();
    assert_eq!(layers.0.len(), 3);
    assert_eq!(
        app.world_mut()
            .query::<&crate::navball::Navball>()
            .iter(app.world())
            .count(),
        1
    );
}

#[test]
fn solar_renderer_switches_bodies_and_reuses_assets_after_checkpoint_restore() {
    let mut app = initialized_scene();
    let images = app.world().resource::<Assets<Image>>().len();
    let materials = app
        .world()
        .resource::<Assets<crate::scenery::GroundMaterial>>()
        .len();
    let ids: Vec<_> = sim(&app).world.bodies.keys().cloned().collect();
    let before = sim(&app).fleet.snapshot(&sim(&app).selected);
    for id in ids {
        let body = sim(&app).world.body_index(&id);
        let radius = sim(&app).fleet.ephemeris.bodies()[body].radius_meters;
        session(&mut app).execute(Action::View {
            command: ViewCommand::BodyPreset {
                body,
                direction: DVec3::new(1.0, 0.2, 0.3).normalize(),
                distance: radius * 3.5,
            },
        });
        app.update();
        let after = sim(&app).fleet.snapshot(&sim(&app).selected);
        assert_eq!(before.position, after.position);
        assert_eq!(before.velocity, after.velocity);
        let world = &app.world().resource::<Ground>().0;
        assert_eq!(world.active, sim(&app).observation_body());
        assert_eq!(world.bodies.len(), 5);
        assert_eq!(world.atmospheres.len(), 3);
    }
    for _ in 0..3 {
        {
            let mut flight = app.world_mut().non_send_mut::<Flight>();
            let checkpoint = void_fleet_flight::checkpoint::FlightCheckpoint::capture(
                flight.session.sim(),
                flight.session.recording_initial().clone(),
            );
            flight.session = FlightSession::from_checkpoint(checkpoint).with_recording();
        }
        app.world_mut().resource_mut::<PartVisuals>().rebuild = true;
        app.update();
        assert_eq!(app.world().resource::<Assets<Image>>().len(), images);
        assert_eq!(
            app.world()
                .resource::<Assets<crate::scenery::GroundMaterial>>()
                .len(),
            materials
        );
        assert!(!app.world().resource::<PartVisuals>().parts.is_empty());
    }
}

#[test]
fn orbital_forecast_replays_and_resumes_without_observation_side_effects() {
    let mut app = initialized_scene();
    app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
        std::time::Duration::from_millis(31),
    ))
    .add_systems(Update, simulate.before(draw));
    let id = launch_orbit(&mut app);
    session(&mut app).execute(Action::Select { vessel: id });
    app.world_mut().non_send_mut::<Flight>().paused = false;
    for _ in 0..5 {
        app.world_mut().resource_mut::<Forecast>().at = f64::NEG_INFINITY;
        app.update();
    }
    let forecast = app.world().resource::<Forecast>();
    assert_eq!(forecast.generation, 5);
    assert!(forecast.coast.as_ref().unwrap().points.len() > 3);
    let mut flight = app.world_mut().non_send_mut::<Flight>();
    let mut replay = FlightSession::from_recording(flight.session.recording()).with_recording();
    let mut loaded =
        FlightSession::from_checkpoint(void_fleet_flight::checkpoint::FlightCheckpoint::capture(
            flight.session.sim(),
            flight.session.recording_initial().clone(),
        ));
    let expected = world_mark(flight.session.sim());
    assert_eq!(world_mark(replay.sim()), expected);
    assert_eq!(world_mark(loaded.sim()), expected);
    for session in [&mut flight.session, &mut replay, &mut loaded] {
        session.execute(Action::Advance {
            seconds: 0.219,
            rails: false,
        });
    }
    let expected = world_mark(flight.session.sim());
    assert_eq!(world_mark(replay.sim()), expected);
    assert_eq!(world_mark(loaded.sim()), expected);
}

#[test]
fn paused_window_inputs_replay_and_rendering_does_not_change_marks() {
    let mut app = initialized_scene();
    let initial_direction = sim(&app).presentation.direction;
    input_resources(&mut app);
    app.insert_resource(AccumulatedMouseMotion {
        delta: Vec2::new(14.0, -8.0),
    })
    .insert_resource(AccumulatedMouseScroll {
        unit: bevy::input::mouse::MouseScrollUnit::Line,
        delta: Vec2::new(0.0, -2.0),
    })
    .add_systems(Update, (controls, simulate).chain().before(draw));
    app.world_mut().non_send_mut::<Flight>().paused = true;
    press(&mut app, &[KeyCode::KeyL]);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    app.update();
    let expected = {
        let mut flight = app.world_mut().non_send_mut::<Flight>();
        assert_ne!(
            flight.session.sim().presentation.direction,
            initial_direction
        );
        assert!(!flight.session.sim().presentation.speed_surface);
        assert_eq!(flight.session.sim().fleet.time(), 0.0);
        let recording = flight.session.recording();
        assert!(matches!(
            recording.entries.last().unwrap().action,
            Action::EndFrame { paused: true, .. }
        ));
        let expected = world_mark(flight.session.sim());
        assert_eq!(
            world_mark(FlightSession::from_recording(recording.clone()).sim()),
            expected
        );
        // Presentation-only updates from here: a paused playback host takes no input.
        let (playback, _) = Playback::new(recording);
        flight.playback = Some(playback);
        expected
    };
    press(&mut app, &[]);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .reset_all();
    app.world_mut()
        .resource_mut::<AccumulatedMouseMotion>()
        .delta = Vec2::ZERO;
    app.world_mut()
        .resource_mut::<AccumulatedMouseScroll>()
        .delta = Vec2::ZERO;
    for _ in 0..3 {
        app.update();
    }
    assert_eq!(world_mark(sim(&app)), expected);
}

#[test]
fn focus_pause_and_vessel_handoff_neutralize_held_requests() {
    let mut app = initialized_scene();
    input_resources(&mut app);
    app.add_systems(Update, controls.before(draw));
    launch_orbit(&mut app);
    app.world_mut().non_send_mut::<Flight>().paused = false;
    let old = sim(&app).selected.clone();
    press(&mut app, &[KeyCode::KeyH, KeyCode::AltLeft, KeyCode::KeyW]);
    app.update();
    assert_eq!(sim(&app).fleet.rcs_control(&old).force, DVec3::Z * 80.0);
    press(&mut app, &[KeyCode::Tab]);
    app.update();
    assert_ne!(sim(&app).selected, old);
    assert_eq!(sim(&app).fleet.rcs_control(&old).force, DVec3::ZERO);
    let selected = sim(&app).selected.clone();
    press(&mut app, &[KeyCode::KeyH, KeyCode::AltLeft, KeyCode::KeyW]);
    app.update();
    assert_eq!(
        sim(&app).fleet.rcs_control(&selected).force,
        DVec3::Z * 80.0
    );
    session(&mut app).execute(Action::Sas { enabled: true });
    {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.clear();
        keys.release(KeyCode::AltLeft);
    }
    app.update();
    assert_eq!(
        sim(&app).fleet.sas_phase(&selected),
        void_vessels::SasPhase::Off
    );
    assert!(
        app.world()
            .resource::<Notice>()
            .0
            .contains("disengaged SAS")
    );
    let w = window(&mut app);
    app.world_mut().get_mut::<Window>(w).unwrap().focused = false;
    app.update();
    assert_eq!(sim(&app).fleet.rcs_control(&selected).force, DVec3::ZERO);
    app.world_mut().get_mut::<Window>(w).unwrap().focused = true;
    press(&mut app, &[KeyCode::KeyP]);
    app.update();
    assert!(app.world().non_send::<Flight>().paused);
    assert_eq!(sim(&app).fleet.rcs_control(&selected).force, DVec3::ZERO);
}

#[test]
fn maneuver_keys_arm_a_plan_and_switch_ship_without_aborting_it() {
    let mut app = initialized_scene();
    let id = launch_orbit(&mut app);
    session(&mut app).execute(Action::Select { vessel: id.clone() });
    session(&mut app).execute(Action::Stage);
    input_resources(&mut app);
    app.add_systems(Update, controls.before(draw));
    for key in [KeyCode::KeyM, KeyCode::KeyB, KeyCode::Tab] {
        press(&mut app, &[key]);
        app.update();
    }
    let sim = sim(&app);
    assert_ne!(sim.selected, id);
    assert!(sim.plans[&id].executing);
    assert_eq!(
        sim.fleet.guidance(&id).unwrap().status,
        void_vessels::GuidanceStatus::Armed
    );
}

#[test]
fn unfocused_window_does_not_inject_control_changes_into_replay() {
    let mut app = initialized_scene();
    let initial = app
        .world()
        .non_send::<Flight>()
        .session
        .recording_initial()
        .clone();
    let mut flown = FlightSession::new(initial).with_recording();
    flown.execute(Action::Control {
        throttle: 0.0,
        turn: DVec3::X * 0.25,
    });
    for _ in 0..2 {
        flown.execute(Action::Advance {
            seconds: 0.113,
            rails: false,
        });
        flown.mark();
    }
    let (mut replay, mut replayed) = Playback::new(flown.recording());
    assert!(replay.next_frame(&mut replayed));
    let before = world_mark(replayed.sim());
    {
        let mut flight = app.world_mut().non_send_mut::<Flight>();
        flight.session = replayed;
        flight.playback = Some(replay);
    }
    let w = window(&mut app);
    app.world_mut().get_mut::<Window>(w).unwrap().focused = false;
    input_resources(&mut app);
    app.add_systems(Update, controls.before(draw));
    app.update();
    assert_eq!(before, world_mark(sim(&app)));
}

fn ui_app(craft: Craft) -> App {
    let mut app = scene(craft);
    input_resources(&mut app);
    app.add_systems(Update, (ui::interactions, controls).chain().before(draw));
    app
}
fn click(app: &mut App, click: Click) {
    app.world_mut()
        .resource_mut::<PendingClicks>()
        .0
        .push(click);
    app.update();
}
fn type_value(app: &mut App, field: Field, value: &str) {
    click(app, Click::Field(field));
    let w = window(app);
    app.world_mut()
        .write_message(bevy::input::keyboard::KeyboardInput {
            key_code: KeyCode::Digit1,
            logical_key: bevy::input::keyboard::Key::Character(value.into()),
            state: bevy::input::ButtonState::Pressed,
            text: Some(value.into()),
            repeat: false,
            window: w,
        });
    // The click put the field's current value in the draft; Ctrl+A clears it first.
    app.world_mut().resource_mut::<UiState>().draft.clear();
    app.update();
    press(app, &[KeyCode::Enter]);
    app.world_mut()
        .write_message(bevy::input::keyboard::KeyboardInput {
            key_code: KeyCode::Enter,
            logical_key: bevy::input::keyboard::Key::Enter,
            state: bevy::input::ButtonState::Pressed,
            text: None,
            repeat: false,
            window: w,
        });
    app.update();
    press(app, &[]);
    app.update();
}

#[test]
fn edited_maneuver_value_reaches_the_live_plan_and_replays() {
    let mut app = ui_app(void_assembly::rcs_flight_rocket());
    let id = launch_orbit(&mut app);
    session(&mut app).execute(Action::Select { vessel: id });
    session(&mut app).execute(Action::Stage);
    click(&mut app, Click::Key(KeyCode::KeyM));
    type_value(&mut app, Field::Prograde, "12.5");
    let mut flight = app.world_mut().non_send_mut::<Flight>();
    let sim = flight.session.sim();
    let plan = &sim.plans[&sim.selected];
    assert_eq!(plan.plan.maneuver(plan.selected).prograde, 12.5);
    assert!(!plan.executing, "numeric Enter must not execute or dock");
    let recording = flight.session.recording();
    assert_eq!(
        recording
            .entries
            .iter()
            .filter(|entry| matches!(entry.action, Action::EditManeuver { .. }))
            .count(),
        1
    );
    assert_eq!(
        world_mark(FlightSession::from_recording(recording).sim()),
        world_mark(flight.session.sim()),
    );
}

#[test]
fn sas_button_reports_the_core_refusal_for_a_passive_stage() {
    let mut app = ui_app(void_assembly::flight_rocket());
    for _ in 0..2 {
        assert!(matches!(
            session(&mut app).execute(Action::Stage),
            Outcome::Staged(_)
        ));
    }
    let passive = sim(&app)
        .fleet
        .vessel_ids()
        .into_iter()
        .find(|id| !sim(&app).fleet.has_command(id))
        .expect("separated booster without a command part");
    session(&mut app).execute(Action::Select { vessel: passive });
    click(&mut app, Click::Key(KeyCode::KeyT));
    assert!(
        app.world()
            .resource::<Notice>()
            .0
            .contains("functioning command part")
    );
}

/// The owner's reentry recipe, entirely through DEV panel clicks and typed values.
#[test]
fn dev_panel_places_a_reentry_and_the_journal_and_save_reproduce_it() {
    use place::{PlaceClick as P, PlaceField as F};
    let mut app = ui_app(void_assembly::reentry_capsule());
    click(&mut app, Click::Place(P::Velocity)); // Surface -> Orbital
    click(&mut app, Click::Place(P::Velocity)); // Orbital -> Landed
    click(&mut app, Click::Place(P::Velocity)); // Landed -> Surface
    click(&mut app, Click::Place(P::Attitude)); // Upright -> Prograde
    click(&mut app, Click::Place(P::Attitude)); // Prograde -> Retrograde
    type_value(&mut app, Field::Place(F::Altitude), "110000");
    type_value(&mut app, Field::Place(F::Speed), "7500");
    type_value(&mut app, Field::Place(F::Path), "-2");
    click(&mut app, Click::Place(P::Apply));
    assert!(
        app.world().resource::<Notice>().0.contains("placed"),
        "{}",
        app.world().resource::<Notice>().0
    );
    let flight = app.world().non_send::<Flight>();
    assert!(flight.paused);
    let sim = flight.session.sim();
    let body = sim.home;
    let state = sim.fleet.body_fixed_state(&sim.selected, body);
    let altitude = state.position.length() - sim.fleet.ephemeris.bodies()[body].radius_meters;
    assert!(altitude > 110_000.0 && altitude < 120_000.0, "{altitude}");
    assert!((state.velocity.length() - 7500.0).abs() < 1.0);
    // Heat shield (-Y) forward.
    let shield = sim.fleet.snapshot(&sim.selected).rotation * -DVec3::Y;
    let flow = sim
        .fleet
        .frames()
        .transform(sim.fleet.body_frames(body).1, sim.fleet.origin_frame())
        .apply_direction(state.velocity.normalize());
    assert!(shield.dot(flow) > 0.999);
    let mut flight = app.world_mut().non_send_mut::<Flight>();
    let mark = world_mark(flight.session.sim());
    assert_eq!(
        world_mark(FlightSession::from_recording(flight.session.recording()).sim()),
        mark
    );
    let mut loaded =
        FlightSession::from_checkpoint(void_fleet_flight::checkpoint::FlightCheckpoint::capture(
            flight.session.sim(),
            flight.session.recording_initial().clone(),
        ));
    for session in [&mut flight.session, &mut loaded] {
        session.execute(Action::Advance {
            seconds: 1.0,
            rails: false,
        });
    }
    assert_eq!(world_mark(loaded.sim()), world_mark(flight.session.sim()));
}

#[test]
fn dev_panel_places_a_second_ship_ahead_of_the_target() {
    use place::PlaceClick as P;
    let mut app = ui_app(void_assembly::rcs_flight_rocket());
    let target = launch_orbit(&mut app);
    click(&mut app, Click::Place(P::Target));
    assert_eq!(
        app.world().resource::<place::PlaceDraft>().target,
        Some(target.clone())
    );
    click(&mut app, Click::Place(P::ApplyNear));
    let sim = sim(&app);
    let relative = sim.fleet.relative(&target, &sim.selected);
    assert!(relative.velocity.length() < 1e-3);
    assert!(relative.position.length() < 60.0);
}

fn aircraft_world_torque(app: &App) -> DVec3 {
    let sim = sim(app);
    let load = sim.fleet.aerodynamic_wrench(&sim.selected);
    sim.fleet
        .frames()
        .transform(load.frame, sim.fleet.origin_frame())
        .apply_direction(load.torque)
}
/// An aircraft placed in level flight over the normal world answers the keys in the player's
/// directions, judged from the actual aerodynamic loads.
#[test]
fn aircraft_keyboard_pitches_up_and_banks_and_yaws_to_player_right() {
    for key in [KeyCode::KeyW, KeyCode::KeyD, KeyCode::KeyE] {
        let mut app = ui_app(void_assembly::aircraft());
        let mut draft = app.world().resource::<place::PlaceDraft>().clone();
        draft.altitude = 1000.0;
        draft.speed = 100.0;
        let placement = draft.placement();
        assert_eq!(
            session(&mut app).execute(Action::Place { placement }),
            Outcome::Applied
        );
        app.world_mut().non_send_mut::<Flight>().paused = false;
        let q = sim(&app).fleet.snapshot(&sim(&app).selected).rotation;
        let nose = q * DVec3::Z;
        let top = q * DVec3::Y;
        let right = nose.cross(top);
        let neutral = aircraft_world_torque(&app);
        press(&mut app, &[key]);
        app.update();
        let change = aircraft_world_torque(&app) - neutral;
        let response = match key {
            KeyCode::KeyW => change.cross(nose).dot(top),
            KeyCode::KeyD => change.cross(top).dot(right),
            KeyCode::KeyE => change.cross(nose).dot(right),
            _ => unreachable!(),
        };
        assert!(
            response > 100.0,
            "{key:?}: world torque {change:?}, response {response}"
        );
    }
}
