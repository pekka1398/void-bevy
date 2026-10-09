//! Session menu. Settings change presentation; world changes remain journalled.
use super::*;
#[derive(Resource, Default)]
pub(super) struct MenuState {
    pub blocking: bool,
    pub capture_frame: bool,
    root: Option<Entity>,
    confirm_restart: bool,
    bindings: Vec<(KeyCode, KeyCode)>,
    capture: Option<KeyCode>,
    path_draft: Option<String>,
}
impl MenuState {
    pub(super) fn binding_summary(&self) -> String {
        if self.bindings.is_empty() {
            "Default controls · Esc menu / key settings".into()
        } else {
            format!(
                "Custom controls: {} · Esc menu",
                self.bindings
                    .iter()
                    .map(|(action, key)| format!("{action:?}={key:?}"))
                    .collect::<Vec<_>>()
                    .join(" · ")
            )
        }
    }
}
#[derive(Resource, Default)]
pub(super) struct Pending(Vec<MenuAction>);
#[derive(Clone, Copy)]
enum MenuAction {
    Open,
    Close,
    Pause,
    Save,
    Load,
    Finish,
    Restart,
    Confirm,
    Cancel,
    Scale(f32),
    Toggle(Toggle),
    Exposure(f32),
    Bind(KeyCode),
    ResetBindings,
    EditPath,
    Fullscreen,
    Vsync,
}
#[derive(Component)]
pub(super) struct MenuReadout;
fn button(c: &mut Commands, parent: Entity, label: &str, action: MenuAction) {
    let e = c
        .spawn((
            Button,
            Node {
                padding: UiRect::axes(px(8), px(5)),
                flex_shrink: 0.,
                ..default()
            },
            BackgroundColor(Color::srgb(0.08, 0.12, 0.18)),
        ))
        .id();
    c.entity(e).observe(
        move |mut event: On<Pointer<bevy::picking::events::Click>>,
              mut pending: ResMut<Pending>| {
            if event.button == bevy::picking::pointer::PointerButton::Primary {
                pending.0.push(action);
            }
            event.propagate(false);
        },
    );
    ui::text(c, e, label, 13.);
    c.entity(parent).add_child(e);
}
pub(super) fn spawn(c: &mut Commands, toolbar: Entity) {
    c.init_resource::<MenuState>();
    c.init_resource::<Pending>();
    button(c, toolbar, "Menu · Esc", MenuAction::Open);
}
fn build(c: &mut Commands, state: &mut MenuState) {
    if let Some(e) = state.root.take() {
        c.entity(e).despawn();
    }
    if !state.blocking {
        return;
    }
    let root = ui::panel(
        c,
        Node {
            position_type: PositionType::Absolute,
            left: percent(22),
            top: percent(9),
            width: percent(56),
            max_height: percent(83),
            overflow: Overflow::scroll_y(),
            ..ui::base()
        },
    );
    c.entity(root)
        .insert(BackgroundColor(Color::srgb(0.035, 0.045, 0.065)));
    c.entity(root)
        .insert((ZIndex(80), ScrollPosition::default()));
    state.root = Some(root);
    ui::text(c, root, "VOID · SESSION & SETTINGS", 20.);
    let status = ui::text(c, root, "", 12.);
    c.entity(status).insert(MenuReadout);
    if state.confirm_restart {
        ui::text(
            c,
            root,
            "Restart this session from its initial world? Current progress is discarded. Save first if needed.",
            13.,
        );
        button(c, root, "RESTART CURRENT WORLD", MenuAction::Confirm);
        button(c, root, "Cancel", MenuAction::Cancel);
        return;
    }
    button(c, root, "Close menu", MenuAction::Close);
    let row = ui::row(c, root);
    button(c, row, "Pause / Resume", MenuAction::Pause);
    button(c, row, "Save · F6", MenuAction::Save);
    button(c, row, "Load saved world · F7", MenuAction::Load);
    button(c, root, "Edit save/load path…", MenuAction::EditPath);
    let row = ui::row(c, root);
    button(c, row, "Restart current world…", MenuAction::Restart);
    button(c, row, "Finish recording · F8", MenuAction::Finish);
    ui::text(
        c,
        root,
        "New craft: open ASSEMBLY, choose a preset, then Launch in the current world. Authored rover/aircraft/planet fixtures are startup options.",
        11.,
    );
    ui::text(c, root, "Display · changes apply immediately", 14.);
    let row = ui::row(c, root);
    button(c, row, "Window / Fullscreen", MenuAction::Fullscreen);
    button(c, row, "VSync on / off", MenuAction::Vsync);
    let row = ui::row(c, root);
    for (label, value) in [
        ("UI 80%", 0.8),
        ("UI 100%", 1.),
        ("UI 125%", 1.25),
        ("UI 150%", 1.5),
    ] {
        button(c, row, label, MenuAction::Scale(value));
    }
    let row = ui::row(c, root);
    for (label, t) in [
        ("Air", Toggle::VisualAir),
        ("Clouds", Toggle::VisualClouds),
        ("Ocean", Toggle::VisualOcean),
        ("Stars", Toggle::VisualStars),
        ("Terrain", Toggle::Terrain),
    ] {
        button(c, row, label, MenuAction::Toggle(t));
    }
    let row = ui::row(c, root);
    button(c, row, "Exposure / 2", MenuAction::Exposure(0.5));
    button(c, row, "Exposure × 2", MenuAction::Exposure(2.));
    ui::text(
        c,
        root,
        "Keyboard · session bindings · click an action then press a key (Esc cancels)",
        13.,
    );
    let row = ui::row(c, root);
    for (label, key) in [
        ("Pause", KeyCode::KeyP),
        ("Stage/brake", KeyCode::Space),
        ("Throttle +", KeyCode::ShiftLeft),
        ("Throttle −", KeyCode::ControlLeft),
        ("Cut/park", KeyCode::KeyX),
        ("SAS", KeyCode::KeyT),
        ("RCS/pack", KeyCode::KeyH),
    ] {
        let physical = state
            .bindings
            .iter()
            .find(|(k, _)| *k == key)
            .map_or(key, |(_, v)| *v);
        button(
            c,
            row,
            &format!("{label}: {physical:?}"),
            MenuAction::Bind(key),
        );
    }
    button(
        c,
        root,
        "Reset keyboard bindings",
        MenuAction::ResetBindings,
    );
    ui::text(
        c,
        root,
        "Controls (defaults; remapped actions shown above)\nFlight: Space stage · Shift/Ctrl throttle · X cut · W/S pitch · A/D yaw · Q/E roll · T SAS\nRCS: H enable · Alt+W/S forward/back · Alt+D/A right/left · Alt+E/Q up/down\nRover: W/S drive · A/D steer · Space brake · X parking · F exit\nAircraft: W/S pitch · A/D roll · Q/E yaw · B brake\nEVA: WASD walk · Q/E turn · Space jump · H pack · F board\nView: Tab ship · F1 view · Home ship · 1–4/G plot frames · J/Shift+J reference\nDocking: F10 own port · F11 target · F12 arm · Enter dock · Backspace undock\nP pause · ,/. warp · N launch craft · O orbital craft · R reset\nMenu opening pauses and clears pilot input. Closing keeps pause state; Resume is explicit.",
        11.,
    );
}
pub(super) fn save(lab: &mut Lab) {
    let checkpoint = void_fleet_flight::checkpoint::FlightCheckpoint::capture(
        lab.session.sim(),
        lab.session.recording_initial().clone(),
    );
    lab.notice = match checkpoint.write_external(&lab.save_path) {
        Ok(()) => format!("Saved {}", lab.save_path.display()),
        Err(e) => format!("Save failed: {e}"),
    };
}
pub(super) fn load(lab: &mut Lab) {
    if lab.playback.is_some() {
        lab.notice =
            "Load refused during replay; finish playback before replacing the world".into();
        return;
    }
    match lab.session.load_external_checkpoint(&lab.save_path) {
        Ok(()) => {
            reset_presentation(lab);
            lab.notice = format!("Loaded {} (paused)", lab.save_path.display());
        }
        Err(e) => lab.notice = format!("Load failed: {e}"),
    }
}
fn reset_presentation(lab: &mut Lab) {
    lab.craft = lab.session.recording_initial().craft.clone();
    lab.dirty = true;
    lab.prediction = None;
    lab.paused = true;
    lab.rate = 0;
    lab.own_port = None;
    lab.target_port = None;
    lab.rendezvous = false;
    lab.reentry = false;
    lab.water_review = None;
    lab.spawned = 0;
    neutral_pilot(lab);
}
#[allow(clippy::too_many_arguments)]
pub(super) fn update(
    mut c: Commands,
    mut lab: NonSendMut<Lab>,
    mut state: ResMut<MenuState>,
    mut pending: ResMut<Pending>,
    keys: Res<ButtonInput<KeyCode>>,
    mut keyboard: MessageReader<bevy::input::keyboard::KeyboardInput>,
    mut scale: ResMut<UiScale>,
    hud: Option<Res<ui::UiState>>,
    workshop: Option<Res<workshop::Workshop>>,
    mut text: Query<&mut Text, With<MenuReadout>>,
    mut windows: Query<&mut Window>,
) {
    state.capture_frame = false;
    if let Some(mut draft) = state.path_draft.take() {
        state.capture_frame = true;
        let mut done = false;
        for event in keyboard.read().filter(|e| e.state.is_pressed()) {
            match &event.logical_key {
                bevy::input::keyboard::Key::Enter => {
                    if draft.trim().is_empty() {
                        lab.notice = "Save path cannot be empty".into();
                    } else {
                        lab.save_path = draft.clone().into();
                        lab.notice = format!("Save path: {}", lab.save_path.display());
                    }
                    done = true;
                }
                bevy::input::keyboard::Key::Escape => {
                    done = true;
                    lab.notice = "Path edit cancelled".into();
                }
                bevy::input::keyboard::Key::Backspace => {
                    draft.pop();
                }
                bevy::input::keyboard::Key::Character(value) => {
                    if keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight])
                        && value.eq_ignore_ascii_case("a")
                    {
                        draft.clear();
                    } else if !keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]) {
                        draft.push_str(value);
                    }
                }
                _ => {}
            }
        }
        if !done {
            lab.notice = format!("Path: {draft} · Enter apply / Esc cancel / Ctrl+A clear");
            state.path_draft = Some(draft);
        }
        for mut t in &mut text {
            *t = Text::new(lab.notice.clone());
        }
        return;
    }
    keyboard.clear();
    if let Some(canonical) = state.capture {
        state.capture_frame = true;
        if let Some(physical) = keys.get_just_pressed().next().copied() {
            state.capture = None;
            if physical != KeyCode::Escape {
                if binding_conflict(canonical, physical, &state.bindings) {
                    lab.notice = "Key reserved by another control; choose an unused key (for example I, Digit5–0 or F13–24)".into();
                } else {
                    state.bindings.retain(|(k, _)| *k != canonical);
                    state.bindings.push((canonical, physical));
                    lab.notice = format!("Bound {canonical:?} to {physical:?}");
                }
            }
            build(&mut c, &mut state);
        }
        return;
    }
    if keys.just_pressed(KeyCode::Escape)
        && !hud.as_ref().is_some_and(|s| s.text_editing())
        && !workshop.as_ref().is_some_and(|s| s.open)
    {
        state.capture_frame = true;
        pending.0.push(if state.blocking {
            MenuAction::Close
        } else {
            MenuAction::Open
        });
    }
    let mut rebuild = false;
    for action in std::mem::take(&mut pending.0) {
        state.capture_frame = true;
        match action {
            MenuAction::Open => {
                state.blocking = true;
                lab.paused = true;
                if lab.playback.is_none() {
                    neutral_pilot(&mut lab);
                }
                rebuild = true;
            }
            MenuAction::Close => {
                state.blocking = false;
                state.confirm_restart = false;
                rebuild = true;
            }
            MenuAction::Pause => {
                lab.paused = !lab.paused;
                if lab.playback.is_none() {
                    neutral_pilot(&mut lab);
                }
                if !lab.paused {
                    state.blocking = false;
                    rebuild = true;
                }
            }
            MenuAction::Save => save(&mut lab),
            MenuAction::Load => load(&mut lab),
            MenuAction::Finish => {
                if let Some(path) = lab.record_path.take() {
                    lab.session.finish_stream();
                    lab.notice = format!("Recording finished: {}", path.display());
                } else {
                    lab.notice =
                        "Recording inactive; launch with --record <journal> to record".into();
                }
            }
            MenuAction::Restart => {
                state.confirm_restart = true;
                rebuild = true;
            }
            MenuAction::Cancel => {
                state.confirm_restart = false;
                rebuild = true;
            }
            MenuAction::Confirm => {
                if lab.playback.is_some() {
                    lab.notice = "Restart refused during replay".into();
                } else {
                    restart_session(&mut lab);
                    lab.notice = format!("Restarted current session (paused). {}", lab.notice);
                }
                state.confirm_restart = false;
                rebuild = true;
            }
            MenuAction::Scale(value) => scale.0 = value,
            MenuAction::Fullscreen => {
                if let Ok(mut window) = windows.single_mut() {
                    window.mode = if window.mode == bevy::window::WindowMode::Windowed {
                        bevy::window::WindowMode::BorderlessFullscreen(
                            bevy::window::MonitorSelection::Current,
                        )
                    } else {
                        bevy::window::WindowMode::Windowed
                    };
                }
            }
            MenuAction::Vsync => {
                if let Ok(mut window) = windows.single_mut() {
                    window.present_mode =
                        if window.present_mode == bevy::window::PresentMode::AutoNoVsync {
                            bevy::window::PresentMode::AutoVsync
                        } else {
                            bevy::window::PresentMode::AutoNoVsync
                        };
                }
            }
            MenuAction::EditPath => {
                state.path_draft = Some(lab.save_path.to_string_lossy().into_owned());
                lab.notice = "Type save/load path; Ctrl+A clear, Enter apply, Esc cancel".into();
            }
            MenuAction::Bind(key) => {
                state.capture = Some(key);
                lab.notice = format!("Press a key for {key:?}; Esc cancels");
            }
            MenuAction::ResetBindings => {
                state.bindings.clear();
                rebuild = true;
            }
            MenuAction::Toggle(setting) => {
                if lab.playback.is_some() {
                    lab.notice = "Visual changes are locked during replay".into();
                } else {
                    lab.session.execute(Action::View {
                        command: ViewCommand::Toggle { setting },
                    });
                }
            }
            MenuAction::Exposure(factor) => {
                if lab.playback.is_none() {
                    let value =
                        (lab.session.sim().presentation.exposure * factor).clamp(0.001, 100.);
                    lab.session.execute(Action::View {
                        command: ViewCommand::Exposure { value },
                    });
                } else {
                    lab.notice = "Exposure is locked during replay".into();
                }
            }
        }
    }
    if rebuild || (state.blocking && state.root.is_none()) {
        build(&mut c, &mut state);
    }
    let p = &lab.session.sim().presentation;
    let window_status = windows
        .single()
        .map(|w| format!("{:?} · {:?}", w.mode, w.present_mode))
        .unwrap_or_else(|_| "No desktop window".into());
    for mut t in &mut text {
        *t = Text::new(format!(
            "{} · {}\nSave/load path: {}\nRecording: {} · UI {:.0}% · exposure {:.3}\nAir {} · clouds {} · ocean {} · stars {}\n{window_status}\n{}",
            if lab.playback.is_some() {
                "REPLAY"
            } else {
                "LIVE"
            },
            if lab.paused { "PAUSED" } else { "RUNNING" },
            lab.save_path.display(),
            lab.record_path
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "inactive".into()),
            scale.0 * 100.,
            p.exposure,
            p.visual_air,
            p.visual_clouds,
            p.visual_ocean,
            p.visual_stars,
            lab.notice
        ));
    }
}

/// Transform physical keyboard state only. Cockpit virtual buttons are applied
/// after this mapping, so their semantic actions stay independent of bindings.
pub(super) fn effective_keys(
    physical: &ButtonInput<KeyCode>,
    state: &MenuState,
) -> ButtonInput<KeyCode> {
    let mut result = physical.clone();
    for (canonical, _) in &state.bindings {
        result.reset(*canonical);
        if *canonical == KeyCode::ShiftLeft {
            result.reset(KeyCode::ShiftRight);
        }
        if *canonical == KeyCode::ControlLeft {
            result.reset(KeyCode::ControlRight);
        }
    }
    for (canonical, key) in &state.bindings {
        if physical.pressed(*key) {
            result.press(*canonical);
            if !physical.just_pressed(*key) {
                result.clear_just_pressed(*canonical);
            }
        }
        if physical.just_released(*key) {
            result.press(*canonical);
            result.clear_just_pressed(*canonical);
            result.release(*canonical);
        }
        if *key != *canonical && !state.bindings.iter().any(|(k, _)| k == key) {
            result.reset(*key);
        }
    }
    result
}

fn binding_conflict(
    canonical: KeyCode,
    physical: KeyCode,
    bindings: &[(KeyCode, KeyCode)],
) -> bool {
    if physical == canonical
        || (canonical == KeyCode::ShiftLeft && physical == KeyCode::ShiftRight)
        || (canonical == KeyCode::ControlLeft && physical == KeyCode::ControlRight)
    {
        return false;
    }
    if bindings
        .iter()
        .any(|(k, v)| *k != canonical && *v == physical)
    {
        return true;
    }
    // These physical keys retain other game/menu functions even after a remap.
    const RESERVED: &[KeyCode] = &[
        KeyCode::AltLeft,
        KeyCode::AltRight,
        KeyCode::ArrowDown,
        KeyCode::ArrowLeft,
        KeyCode::ArrowRight,
        KeyCode::ArrowUp,
        KeyCode::Backspace,
        KeyCode::BracketLeft,
        KeyCode::BracketRight,
        KeyCode::Comma,
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
        KeyCode::Delete,
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::End,
        KeyCode::Enter,
        KeyCode::Escape,
        KeyCode::F1,
        KeyCode::F2,
        KeyCode::F3,
        KeyCode::F4,
        KeyCode::F5,
        KeyCode::F6,
        KeyCode::F7,
        KeyCode::F8,
        KeyCode::F9,
        KeyCode::F10,
        KeyCode::F11,
        KeyCode::F12,
        KeyCode::Home,
        KeyCode::KeyA,
        KeyCode::KeyB,
        KeyCode::KeyC,
        KeyCode::KeyD,
        KeyCode::KeyE,
        KeyCode::KeyF,
        KeyCode::KeyG,
        KeyCode::KeyH,
        KeyCode::KeyJ,
        KeyCode::KeyK,
        KeyCode::KeyL,
        KeyCode::KeyM,
        KeyCode::KeyN,
        KeyCode::KeyO,
        KeyCode::KeyP,
        KeyCode::KeyQ,
        KeyCode::KeyR,
        KeyCode::KeyS,
        KeyCode::KeyT,
        KeyCode::KeyU,
        KeyCode::KeyV,
        KeyCode::KeyW,
        KeyCode::KeyX,
        KeyCode::KeyY,
        KeyCode::KeyZ,
        KeyCode::PageDown,
        KeyCode::PageUp,
        KeyCode::Period,
        KeyCode::PrintScreen,
        KeyCode::ShiftLeft,
        KeyCode::ShiftRight,
        KeyCode::Space,
        KeyCode::Tab,
    ];
    RESERVED.contains(&physical)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture_app(kind: usize) -> App {
        let planet = if kind == 2 {
            let mut planet = void_landing::earth_size();
            planet.sea_level = Some(1800.);
            planet
        } else {
            void_landing::aurelia()
        };
        let craft = if kind == 0 {
            void_assembly::rcs_flight_rocket()
        } else {
            void_assembly::reentry_capsule()
        };
        let initial = InitialWorld::new(
            &planet,
            &craft,
            if kind == 2 {
                DVec3::X
            } else {
                void_vessels::flat_site(&planet)
            },
            true,
        );
        let mut lab = new_lab(FlightSession::new(initial).with_recording(), craft);
        lab.main_game = true;
        lab.rendezvous = kind == 0;
        lab.reentry = kind == 1;
        lab.water_review = (kind == 2).then_some(WATER_REVIEW_CASES[5]);
        lab.spawned = 9;
        if kind == 0 {
            lab.craft = void_assembly::crew_rover();
        } // A workshop launch changed the next-launch blueprint.
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_non_send(lab)
            .init_resource::<MenuState>()
            .init_resource::<Pending>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<AccumulatedMouseMotion>()
            .init_resource::<AccumulatedMouseScroll>()
            .init_resource::<UiScale>()
            .add_message::<bevy::input::keyboard::KeyboardInput>()
            .add_systems(Update, (update, super::super::controls).chain());
        app.world_mut().spawn(Window {
            focused: true,
            ..default()
        });
        app
    }
    #[test]
    fn confirmed_restart_matches_keyboard_for_rendezvous_reentry_and_water() {
        for (kind, label) in ["Rendezvous", "Reentry", "Splashdown"].iter().enumerate() {
            let mut keyboard = fixture_app(kind);
            keyboard
                .world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::KeyR);
            keyboard.update();
            let mut menu = fixture_app(kind);
            menu.world_mut()
                .resource_mut::<Pending>()
                .0
                .push(MenuAction::Confirm);
            menu.update();
            let expected = void_fleet_flight::session::world_mark(
                keyboard.world().non_send::<Lab>().session.sim(),
            );
            let mut lab = menu.world_mut().non_send_mut::<Lab>();
            assert_eq!(
                expected,
                void_fleet_flight::session::world_mark(lab.session.sim())
            );
            assert!(lab.notice.contains(label));
            assert!(lab.paused);
            assert_eq!((lab.rate, lab.spawned), (0, 0));
            if kind == 2 {
                assert_eq!(lab.water_review, Some(WATER_REVIEW_CASES[5]));
            }
            let replay = FlightSession::from_recording(lab.session.recording());
            assert_eq!(
                expected,
                void_fleet_flight::session::world_mark(replay.sim())
            );
        }
    }
    #[test]
    fn checked_load_clears_startup_fixtures_and_restart_keeps_loaded_world() {
        let planet = void_landing::aurelia();
        let craft = void_assembly::crew_rover();
        let initial = InitialWorld::new(&planet, &craft, void_vessels::flat_site(&planet), true);
        let source = FlightSession::new(initial.clone());
        let checkpoint =
            void_fleet_flight::checkpoint::FlightCheckpoint::capture(source.sim(), initial);
        let path =
            std::env::temp_dir().join(format!("void-ui-fixture-load-{}.json", std::process::id()));
        checkpoint.write_external(&path).unwrap();
        let mut app = fixture_app(0);
        {
            let mut lab = app.world_mut().non_send_mut::<Lab>();
            lab.reentry = true;
            lab.water_review = Some(WATER_REVIEW_CASES[5]);
            lab.save_path = path.clone();
            load(&mut lab);
            assert!(lab.notice.starts_with("Loaded"));
            assert!(!lab.rendezvous && !lab.reentry && lab.water_review.is_none());
            assert_eq!(lab.craft, craft);
            restart_session(&mut lab);
            assert_eq!(lab.session.sim().fleet.vessel_ids().len(), 1);
            assert_eq!(
                lab.session
                    .sim()
                    .fleet
                    .control_profile(&lab.session.sim().selected),
                Some(void_assembly::ControlProfile::Rover)
            );
        }
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn remapping_preserves_edges_and_blocks_conflicts() {
        let mut physical = ButtonInput::default();
        physical.press(KeyCode::KeyI);
        let state = MenuState {
            bindings: vec![(KeyCode::Space, KeyCode::KeyI)],
            ..default()
        };
        let mapped = effective_keys(&physical, &state);
        assert!(mapped.pressed(KeyCode::Space));
        assert!(mapped.just_pressed(KeyCode::Space));
        assert!(!mapped.pressed(KeyCode::KeyI));
        physical.clear();
        let mapped = effective_keys(&physical, &state);
        assert!(mapped.pressed(KeyCode::Space));
        assert!(!mapped.just_pressed(KeyCode::Space));
        physical.release(KeyCode::KeyI);
        let mapped = effective_keys(&physical, &state);
        assert!(mapped.just_released(KeyCode::Space));
        assert!(!mapped.pressed(KeyCode::Space));
        assert!(binding_conflict(KeyCode::Space, KeyCode::KeyW, &[]));
        assert!(!binding_conflict(KeyCode::Space, KeyCode::KeyI, &[]));
        assert!(binding_conflict(
            KeyCode::KeyP,
            KeyCode::KeyI,
            &state.bindings
        ));
        let mut physical = ButtonInput::default();
        physical.press(KeyCode::ShiftRight);
        physical.press(KeyCode::ControlRight);
        let remapped = MenuState {
            bindings: vec![
                (KeyCode::ShiftLeft, KeyCode::Digit5),
                (KeyCode::ControlLeft, KeyCode::Digit6),
            ],
            ..default()
        };
        let effective = effective_keys(&physical, &remapped);
        assert!(!effective.pressed(KeyCode::ShiftRight));
        assert!(!effective.pressed(KeyCode::ControlRight));
        assert!(effective_keys(&physical, &MenuState::default()).pressed(KeyCode::ShiftRight));
    }
    #[test]
    fn replay_menu_pause_and_close_preserve_authoritative_state() {
        let planet = void_landing::aurelia();
        let craft = void_vessels::pod_tank("menu replay");
        let initial = InitialWorld::new(&planet, &craft, void_vessels::flat_site(&planet), true);
        let mut session = FlightSession::new(initial).with_recording();
        session.execute(Action::Control {
            throttle: 0.4,
            turn: DVec3::X,
        });
        let (playback, _) = Playback::new(session.recording());
        let mut lab = new_lab(session, craft);
        lab.playback = Some(playback);
        lab.paused = false;
        let before = void_fleet_flight::session::world_mark(lab.session.sim());
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_non_send(lab)
            .init_resource::<MenuState>()
            .init_resource::<Pending>()
            .init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<UiScale>()
            .add_message::<bevy::input::keyboard::KeyboardInput>()
            .add_systems(Update, update);
        app.world_mut()
            .resource_mut::<Pending>()
            .0
            .push(MenuAction::Open);
        app.update();
        assert!(app.world().non_send::<Lab>().paused);
        assert_eq!(
            void_fleet_flight::session::world_mark(app.world().non_send::<Lab>().session.sim()),
            before
        );
        app.world_mut()
            .resource_mut::<Pending>()
            .0
            .push(MenuAction::Close);
        app.update();
        assert!(!app.world().resource::<MenuState>().blocking);
        assert!(app.world().resource::<MenuState>().capture_frame);
        assert_eq!(
            void_fleet_flight::session::world_mark(app.world().non_send::<Lab>().session.sim()),
            before
        );
    }
}
