//! Native flight HUD, mapped from the archived src/main.ts and style.css.
use super::*;

#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub(super) enum Readout {
    Clock,
    Stages,
    Throttle,
    Altitude,
    Speed,
    SpeedReference,
    Orbit,
    Maneuver,
    Status,
    ViewDiagnostics,
}
#[derive(Component)]
pub(super) struct Panel;
#[derive(Component)]
pub(super) struct Dev;
#[derive(Component)]
pub(super) struct Help;
#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub(super) enum Field {
    Start,
    Prograde,
    Normal,
    Radial,
}
#[derive(Component)]
pub(super) struct FieldText(Field);
#[derive(Component)]
pub(super) struct StageList;
#[derive(Component)]
pub(super) struct OrbitFade;
#[derive(Component)]
pub(super) struct StageItem(String);
#[derive(Component)]
pub(super) enum Gauge {
    Throttle,
    Fuel(String),
}
#[derive(Resource, Default)]
pub(super) struct StageCache {
    keys: Vec<String>,
}
#[derive(Component, Clone, Copy)]
pub(super) enum Click {
    Key(KeyCode),
    Warp(usize),
    Pause,
    Dev,
    Help,
    Body,
    Field(Field),
    Toggle(Toggle),
}
#[derive(Resource)]
pub(super) struct HudFont {
    mono: Handle<Font>,
    cjk: Handle<Font>,
}
#[derive(Resource, Default)]
pub(super) struct PendingClicks(Vec<Click>);

#[derive(Resource, Default)]
pub(super) struct UiState {
    pub pointer: bool,
    dragging: bool,
    pub editing: bool,
    field: Option<Field>,
    draft: String,
    dev: bool,
    help: bool,
}
const INK: Color = Color::srgb(0.81, 0.84, 0.89);
const BG: Color = Color::srgba(0.031, 0.039, 0.063, 0.82);
fn panel(commands: &mut Commands, node: Node) -> Entity {
    commands
        .spawn((
            Panel,
            node,
            Interaction::None,
            BackgroundColor(BG),
            BorderColor::all(Color::srgb(0.165, 0.188, 0.25)),
            ZIndex(10),
        ))
        .id()
}
fn base() -> Node {
    Node {
        position_type: PositionType::Absolute,
        padding: UiRect::axes(px(10), px(6)),
        border: UiRect::all(px(1)),
        border_radius: BorderRadius::all(px(6)),
        row_gap: px(4),
        column_gap: px(5),
        flex_direction: FlexDirection::Column,
        ..default()
    }
}
fn text(commands: &mut Commands, parent: Entity, value: &str, size: f32) -> Entity {
    let e = commands
        .spawn((
            Text::new(value),
            TextFont {
                font_size: FontSize::Px(size),
                ..default()
            },
            TextColor(INK),
        ))
        .id();
    commands.entity(parent).add_child(e);
    e
}
fn readout(commands: &mut Commands, parent: Entity, kind: Readout, size: f32) -> Entity {
    let e = text(commands, parent, "", size);
    commands.entity(e).insert(kind);
    e
}
fn button(commands: &mut Commands, parent: Entity, label: &str, click: Click) {
    let e = commands
        .spawn((
            Button,
            click,
            Node {
                padding: UiRect::axes(px(5), px(3)),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(6)),
                ..default()
            },
            BackgroundColor(Color::srgb(0.078, 0.098, 0.145)),
            BorderColor::all(Color::srgb(0.22, 0.27, 0.36)),
        ))
        .id();
    observe_button(commands, e, click);
    text(commands, e, label, 11.);
    commands.entity(parent).add_child(e);
}
fn mode_button(commands: &mut Commands, parent: Entity, label: &str, key: KeyCode) {
    let action = Click::Key(key);
    let entity = commands
        .spawn((
            Button,
            action,
            Node {
                padding: UiRect::axes(px(4), px(1)),
                ..default()
            },
            BackgroundColor(Color::NONE),
        ))
        .id();
    let caption = text(commands, entity, label, 11.);
    commands
        .entity(caption)
        .insert(TextColor(Color::srgb(0.53, 0.57, 0.65)));
    observe_button(commands, entity, action);
    commands.entity(parent).add_child(entity);
}

fn observe_button(commands: &mut Commands, entity: Entity, action: Click) {
    commands.entity(entity).observe(
        move |mut event: On<Pointer<bevy::picking::events::Click>>,
              mut queue: ResMut<PendingClicks>| {
            if event.button == bevy::picking::pointer::PointerButton::Primary {
                queue.0.push(action);
            }
            event.propagate(false);
        },
    );
}

fn row(commands: &mut Commands, parent: Entity) -> Entity {
    let e = commands
        .spawn(Node {
            column_gap: px(5),
            flex_wrap: FlexWrap::Wrap,
            ..default()
        })
        .id();
    commands.entity(parent).add_child(e);
    e
}
pub(super) fn spawn(
    commands: &mut Commands,
    images: &mut Assets<Image>,
    fonts: &mut Assets<Font>,
    scale: f64,
) {
    let mono = fonts.add(Font::from_bytes(
        include_bytes!("fonts/DejaVuSansMono.ttf").to_vec(),
    ));
    let cjk = fonts.add(Font::from_bytes(
        include_bytes!("fonts/DroidSansFallbackFull.ttf").to_vec(),
    ));
    commands.insert_resource(HudFont { mono, cjk });
    commands.insert_resource(UiState::default());
    commands.insert_resource(PendingClicks::default());
    commands.insert_resource(StageCache::default());
    let clock = panel(
        commands,
        Node {
            left: px(12),
            top: px(12),
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            ..base()
        },
    );
    readout(commands, clock, Readout::Clock, 14.);
    let rates = row(commands, clock);
    for (i, r) in RATES.iter().enumerate() {
        let label = if *r >= 1000. {
            format!("{}k×", r / 1000.)
        } else {
            format!("{r}×")
        };
        button(commands, rates, &label, Click::Warp(i));
    }
    button(commands, rates, "Pause", Click::Pause);
    let stages = panel(
        commands,
        Node {
            left: px(12),
            bottom: px(12),
            width: px(330),
            ..base()
        },
    );
    readout(commands, stages, Readout::Stages, 12.);
    let list = commands
        .spawn((
            StageList,
            ScrollPosition::default(),
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: px(4),
                max_height: vh(30),
                overflow: Overflow::scroll_y(),
                ..default()
            },
        ))
        .id();
    commands.entity(stages).add_child(list);
    button(
        commands,
        stages,
        "Space · stage",
        Click::Key(KeyCode::Space),
    );
    let flight = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: percent(50),
                bottom: px(12),
                column_gap: px(8),
                align_items: AlignItems::Stretch,
                ..default()
            },
            UiTransform::from_translation(bevy::ui::Val2::percent(-50, 0)),
            ZIndex(10),
        ))
        .id();
    let sas = panel(
        commands,
        Node {
            position_type: PositionType::Relative,
            justify_content: JustifyContent::Center,
            ..base()
        },
    );
    commands.entity(flight).add_child(sas);
    button(commands, sas, "SAS", Click::Key(KeyCode::KeyT));
    let throttle = panel(
        commands,
        Node {
            position_type: PositionType::Relative,
            width: px(64),
            align_items: AlignItems::Center,
            ..base()
        },
    );
    commands.entity(flight).add_child(throttle);
    text(commands, throttle, "THR", 11.);
    let track = commands
        .spawn((
            Node {
                width: px(10),
                height: px(54),
                position_type: PositionType::Relative,
                ..default()
            },
            BackgroundColor(Color::srgb(0.1, 0.125, 0.188)),
        ))
        .id();
    commands.entity(throttle).add_child(track);
    let fill = commands
        .spawn((
            Gauge::Throttle,
            Node {
                position_type: PositionType::Absolute,
                bottom: px(0),
                width: percent(100),
                height: percent(0),
                ..default()
            },
            BackgroundColor(Color::srgb(0.42, 0.55, 0.78)),
        ))
        .id();
    commands.entity(track).add_child(fill);
    readout(commands, throttle, Readout::Throttle, 12.);
    let speed = panel(
        commands,
        Node {
            position_type: PositionType::Relative,
            width: px(184),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..base()
        },
    );
    commands.entity(flight).add_child(speed);
    mode_button(commands, speed, "AGL", KeyCode::KeyK);
    let altitude = readout(commands, speed, Readout::Altitude, 20.);
    commands.entity(altitude).insert(TextColor(Color::WHITE));
    let divider = commands
        .spawn((
            Node {
                width: percent(100),
                height: px(1),
                margin: UiRect::axes(px(0), px(4)),
                ..default()
            },
            BackgroundColor(Color::srgb(0.165, 0.188, 0.25)),
        ))
        .id();
    commands.entity(speed).add_child(divider);
    mode_button(commands, speed, "SURFACE", KeyCode::KeyL);
    let velocity = readout(commands, speed, Readout::Speed, 22.);
    commands
        .entity(velocity)
        .insert(TextColor(Color::srgb(0.49, 1., 0.69)));
    let reference = readout(commands, speed, Readout::SpeedReference, 11.);
    commands
        .entity(reference)
        .insert(TextColor(Color::srgb(0.53, 0.57, 0.65)));
    let nav = panel(
        commands,
        Node {
            position_type: PositionType::Relative,
            align_items: AlignItems::Center,
            padding: UiRect::all(px(6)),
            ..base()
        },
    );
    commands.entity(flight).add_child(nav);
    let ball = crate::navball::spawn_navball(commands, images, 150., scale);
    commands.entity(nav).add_child(ball);
    let heading = text(commands, nav, "", 11.);
    commands.entity(heading).insert(NavballHeading);
    let orbit = panel(
        commands,
        Node {
            right: px(12),
            top: percent(35),
            width: px(210),
            ..base()
        },
    );
    commands.entity(orbit).insert((Readout::Orbit, OrbitFade));
    readout(commands, orbit, Readout::Orbit, 12.);
    button(
        commands,
        orbit,
        "PATH · cycle frame",
        Click::Key(KeyCode::KeyG),
    );
    let maneuver = panel(
        commands,
        Node {
            right: px(12),
            bottom: px(45),
            width: px(265),
            max_height: vh(43),
            overflow: Overflow::scroll_y(),
            ..base()
        },
    );
    commands
        .entity(maneuver)
        .insert((Readout::Maneuver, ScrollPosition::default()));
    text(commands, maneuver, "MANEUVER", 11.);
    readout(commands, maneuver, Readout::Maneuver, 11.);
    for (field, label) in [
        (Field::Start, "Start T+ s"),
        (Field::Prograde, "Prograde m/s"),
        (Field::Normal, "Normal m/s"),
        (Field::Radial, "Radial m/s"),
    ] {
        let r = row(commands, maneuver);
        text(commands, r, label, 11.);
        let e = commands
            .spawn((
                Button,
                Click::Field(field),
                Node {
                    padding: UiRect::axes(px(5), px(2)),
                    min_width: px(95),
                    ..default()
                },
                BackgroundColor(Color::srgb(0.078, 0.098, 0.145)),
            ))
            .id();
        observe_button(commands, e, Click::Field(field));
        let t = text(commands, e, "—", 11.);
        commands.entity(t).insert(FieldText(field));
        commands.entity(r).add_child(e);
    }
    text(
        commands,
        maneuver,
        "Click value · type · Enter commit · Esc cancel",
        10.,
    );
    for controls in [
        vec![
            ("Add", KeyCode::KeyM),
            ("Remove", KeyCode::Delete),
            ("Warp", KeyCode::KeyZ),
        ],
        vec![
            ("Previous", KeyCode::BracketLeft),
            ("Next", KeyCode::BracketRight),
            ("Reference", KeyCode::KeyV),
        ],
        vec![
            ("At Pe", KeyCode::KeyY),
            ("At Ap", KeyCode::KeyU),
            ("Execute", KeyCode::KeyB),
            ("Abort", KeyCode::Escape),
        ],
    ] {
        let r = row(commands, maneuver);
        for (label, key) in controls {
            button(commands, r, label, Click::Key(key));
        }
    }
    let dev = panel(
        commands,
        Node {
            right: px(12),
            top: px(12),
            ..base()
        },
    );
    button(commands, dev, "DEV `", Click::Dev);
    let body = commands
        .spawn((
            Dev,
            ScrollPosition::default(),
            Node {
                display: Display::None,
                width: px(360),
                max_height: vh(53),
                overflow: Overflow::scroll_y(),
                row_gap: px(4),
                flex_direction: FlexDirection::Column,
                ..default()
            },
        ))
        .id();
    commands.entity(dev).add_child(body);
    button(commands, body, "Planet · focus next", Click::Body);
    for (label, key) in [
        ("Near / orbit / far", KeyCode::F1),
        ("Terrain", KeyCode::F5),
        ("Mesh edges", KeyCode::F2),
        ("Tile bounds", KeyCode::F3),
        ("Colliders", KeyCode::F4),
        ("Exposure −", KeyCode::F10),
        ("Exposure +", KeyCode::F11),
    ] {
        button(commands, body, label, Click::Key(key));
    }
    for (label, setting) in [
        ("Atmosphere · visual", Toggle::VisualAir),
        ("Clouds", Toggle::VisualClouds),
        ("Ocean · visual", Toggle::VisualOcean),
        ("Stars", Toggle::VisualStars),
    ] {
        button(commands, body, label, Click::Toggle(setting));
    }
    readout(commands, body, Readout::ViewDiagnostics, 11.);
    let diagnostics = text(commands, body, "", 11.);
    commands.entity(diagnostics).insert(Hud);
    let help = panel(
        commands,
        Node {
            right: px(12),
            bottom: px(12),
            ..base()
        },
    );
    button(commands, help, "? keys", Click::Help);
    let help_body = panel(
        commands,
        Node {
            display: Display::None,
            right: px(12),
            bottom: px(50),
            width: px(380),
            ..base()
        },
    );
    commands.entity(help_body).insert(Help);
    text(
        commands,
        help_body,
        "Space stage · Shift/Ctrl throttle · X cut\nW/S pitch · A/D yaw · Q/E roll · T SAS\n,/. time rate · P pause · R reset\nDrag orbit camera · wheel zoom into map\nTab vessel · Shift+Tab focus body · click labels\nK ALT/AGL · L SURFACE/ORBIT · G plot frame\nF1 near/orbit/far · ` DEV · ? help\nF6 save · F7 load paused · F8 finish recording\nM maneuver · B execute · Esc abort\nVehicle / EVA / docking controls: DEV status",
        11.,
    );
    let status = panel(
        commands,
        Node {
            left: px(12),
            top: px(88),
            max_width: px(440),
            ..base()
        },
    );
    readout(commands, status, Readout::Status, 11.);
}

#[allow(clippy::too_many_arguments)]
pub(super) fn interactions(
    keys: Res<ButtonInput<KeyCode>>,
    mut keyboard: MessageReader<bevy::input::keyboard::KeyboardInput>,
    buttons: Res<ButtonInput<MouseButton>>,
    all: Query<&Interaction, With<Panel>>,
    mut clicks: Query<(&Interaction, &Click, &mut BackgroundColor), With<Button>>,
    mut state: ResMut<UiState>,
    mut pending: Option<ResMut<PendingClicks>>,
    mut lab: NonSendMut<Lab>,
    mut nodes: Query<(&mut Node, Option<&Dev>, Option<&Help>)>,
) {
    let queued = pending
        .as_mut()
        .map_or(Vec::new(), |p| std::mem::take(&mut p.0));
    let was_editing = state.field.is_some();
    state.pointer = all.iter().any(|i| *i != Interaction::None)
        || clicks.iter().any(|(i, _, _)| *i != Interaction::None);
    if buttons.just_pressed(MouseButton::Left) && state.pointer {
        state.dragging = true;
    }
    if !buttons.pressed(MouseButton::Left) {
        state.dragging = false;
    }
    state.pointer |= state.dragging || !queued.is_empty();
    if keys.just_pressed(KeyCode::Backquote) && state.field.is_none() {
        state.dev = !state.dev;
    }
    for (interaction, action, mut background) in &mut clicks {
        background.0 = if matches!(action, Click::Key(KeyCode::KeyK | KeyCode::KeyL))
            && *interaction != Interaction::Hovered
        {
            Color::NONE
        } else if *interaction == Interaction::Hovered {
            Color::srgb(0.16, 0.21, 0.31)
        } else {
            Color::srgb(0.078, 0.098, 0.145)
        };
    }
    for click in &queued {
        if !matches!(click, Click::Field(_)) {
            state.field = None;
        }
        match click {
            Click::Dev => state.dev = !state.dev,
            Click::Help => state.help = !state.help,
            _ if lab.playback.is_some() => {
                lab.notice = "UI controls unavailable during playback".into()
            }
            Click::Pause => lab.paused = !lab.paused,
            Click::Warp(i) => {
                lab.rate = *i;
                lab.notice.clear();
            }
            Click::Field(field) => {
                if let Some(p) = lab
                    .session
                    .sim()
                    .plans
                    .get(&lab.session.sim().selected)
                    .filter(|p| p.plan.count() > 0)
                {
                    let spec = p.plan.maneuver(p.selected);
                    state.field = Some(*field);
                    state.draft = field_value(*field, &spec).to_string();
                } else {
                    lab.notice = "Add a maneuver before editing values".into();
                }
            }
            Click::Toggle(setting) => {
                lab.session.execute(Action::View {
                    command: ViewCommand::Toggle { setting: *setting },
                });
            }
            Click::Body => {
                let p = &lab.session.sim().presentation;
                let next = p.focus_body.map_or(0, |b| {
                    (b + 1) % lab.session.sim().fleet.ephemeris.bodies().len()
                });
                lab.session.execute(Action::View {
                    command: ViewCommand::Focus { body: Some(next) },
                });
            }
            Click::Key(key) => dispatch(&mut lab, *key),
        }
    }
    for event in keyboard.read() {
        if !event.state.is_pressed() {
            continue;
        }
        let Some(field) = state.field else {
            continue;
        };
        match event.key_code {
            KeyCode::Escape => state.field = None,
            KeyCode::Enter => match state.draft.parse::<f64>() {
                Ok(value) if value.is_finite() => {
                    let selected = lab.session.sim().selected.clone();
                    if let Some(p) = lab
                        .session
                        .sim()
                        .plans
                        .get(&selected)
                        .filter(|p| p.plan.count() > 0)
                    {
                        let index = p.selected;
                        let mut spec = p.plan.maneuver(index);
                        match field {
                            Field::Start => spec.start_time = value,
                            Field::Prograde => spec.prograde = value,
                            Field::Normal => spec.normal = value,
                            Field::Radial => spec.radial = value,
                        }
                        match lab.session.execute(Action::EditManeuver { index, spec }) {
                            Outcome::Refused(r) => lab.notice = r,
                            _ => lab.notice.clear(),
                        }
                    }
                    state.field = None;
                }
                _ => lab.notice = "Enter a finite number".into(),
            },
            KeyCode::Backspace => {
                state.draft.pop();
            }
            KeyCode::KeyA if keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]) => {
                state.draft.clear()
            }
            _ => {
                if let Some(value) = &event.text {
                    state.draft.extend(
                        value
                            .chars()
                            .filter(|c| c.is_ascii_digit() || ".-+eE".contains(*c)),
                    );
                }
            }
        }
    }
    state.editing = was_editing || state.field.is_some();
    for (mut node, dev, help) in &mut nodes {
        if dev.is_some() {
            node.display = if state.dev {
                Display::Flex
            } else {
                Display::None
            };
        }
        if help.is_some() {
            node.display = if state.help {
                Display::Flex
            } else {
                Display::None
            };
        }
    }
}
fn dispatch(lab: &mut Lab, key: KeyCode) {
    let mut keys = ButtonInput::default();
    keys.press(key);
    match key {
        KeyCode::KeyT => {
            let enabled = lab
                .session
                .sim()
                .fleet
                .sas_phase(&lab.session.sim().selected)
                != void_sas::SasPhase::Off;
            match lab.session.execute(Action::Sas { enabled: !enabled }) {
                Outcome::Applied => lab.notice.clear(),
                Outcome::Refused(reason) => lab.notice = reason,
                other => panic!("unexpected SAS outcome: {other:?}"),
            }
        }
        KeyCode::Space => match lab.session.execute(Action::Stage) {
            Outcome::Refused(r) => lab.notice = r,
            _ => {
                lab.prediction = None;
                lab.own_port = None;
                lab.target_port = None;
            }
        },
        KeyCode::KeyX => {
            let c = lab.session.sim().fleet.control(&lab.session.sim().selected);
            lab.session.execute(Action::Control {
                throttle: 0.,
                turn: c.turn,
            });
        }
        KeyCode::KeyG => plot_controls(lab, &keys),
        KeyCode::F1 => {
            let body = lab.session.sim().observation_body();
            let radius = lab.session.sim().fleet.ephemeris.bodies()[body].radius_meters;
            let ratio = lab.session.sim().presentation.distance / radius;
            scenery_preset(
                lab,
                body,
                if ratio < 1.1 {
                    "orbit"
                } else if ratio < 10. {
                    "far"
                } else {
                    "near"
                },
            );
        }
        KeyCode::F10 | KeyCode::F11 => {
            let value = (lab.session.sim().presentation.exposure
                * if key == KeyCode::F10 { 0.9 } else { 1.1 })
            .clamp(0.1, 100.);
            lab.session.execute(Action::View {
                command: ViewCommand::Exposure { value },
            });
        }
        KeyCode::F2 | KeyCode::F3 | KeyCode::F4 | KeyCode::F5 | KeyCode::KeyK | KeyCode::KeyL => {
            view_controls(
                lab,
                &keys,
                &ButtonInput::default(),
                &AccumulatedMouseMotion::default(),
                &AccumulatedMouseScroll::default(),
            )
        }
        _ => {
            if key == KeyCode::Home {
                keys.press(KeyCode::AltLeft);
            }
            plan_controls(lab, &keys);
        }
    }
}

#[allow(clippy::type_complexity)]
pub(super) fn refresh(
    lab: NonSend<Lab>,
    mut texts: Query<
        (&Readout, &mut Text),
        (Without<crate::navball::NavballLabel>, Without<FieldText>),
    >,
    mut orbit: Query<(&Readout, &mut Node), With<Panel>>,
    state: Res<UiState>,
    ground: Res<Ground>,
    window: Single<&Window>,
    mut fields: Query<(&FieldText, &mut Text), Without<Readout>>,
) {
    for (field, mut text) in &mut fields {
        text.0 = if state.field == Some(field.0) {
            format!("{}▏", state.draft)
        } else {
            lab.session
                .sim()
                .plans
                .get(&lab.session.sim().selected)
                .filter(|p| p.plan.count() > 0)
                .map_or("—".into(), |p| {
                    format!("{:.2}", field_value(field.0, &p.plan.maneuver(p.selected)))
                })
        };
    }
    let sim = lab.session.sim();
    let f = &sim.fleet;
    let id = &sim.selected;
    let ship = f.snapshot(id);
    let nav = sim.navigation_body(id);
    let body = &f.ephemeris.bodies()[nav];
    let view = &sim.presentation;
    let inertial = f
        .frames()
        .transform(f.vessel_frame(id), f.body_frames(nav).0)
        .apply_state(void_frames::State {
            position: f.centre_of_mass_local(id),
            velocity: DVec3::ZERO,
        });
    let surface = f
        .frames()
        .transform(f.vessel_frame(id), f.body_frames(nav).1)
        .apply_state(void_frames::State {
            position: f.centre_of_mass_local(id),
            velocity: DVec3::ZERO,
        });
    let altitude = if view.altitude_agl && sim.terrains.contains_key(&nav) {
        f.clearance(id, nav)
    } else {
        inertial.position.length() - body.radius_meters
    };
    let speed = if view.speed_surface {
        surface.velocity.length()
    } else {
        inertial.velocity.length()
    };
    let orbital = void_orbit::osculating_orbit(inertial.position, inertial.velocity, body.gm);
    let parts = f.part_snapshots(id);
    let c = f.control(id);
    let whole = f.time().floor() as u64;
    let map = lab.view.map_or(0., |v| v.map_weight);
    for (kind, mut node) in &mut orbit {
        if *kind == Readout::Maneuver {
            node.display = if sim.plans.contains_key(id)
                || (f.control_profile(id) == Some(void_assembly::ControlProfile::Flight)
                    && sim.plan_engine(id).is_ok())
            {
                Display::Flex
            } else {
                Display::None
            };
        }
        if *kind == Readout::Orbit {
            node.display = if map > 0. {
                Display::Flex
            } else {
                Display::None
            };
        }
    }
    for (kind, mut text) in &mut texts {
        text.0 = match kind {
            Readout::Clock => format!(
                "T+ {}{:02}:{:02}:{:02} · {}× {}",
                if whole >= 86400 {
                    format!("{}d ", whole / 86400)
                } else {
                    String::new()
                },
                whole / 3600 % 24,
                whole / 60 % 60,
                whole % 60,
                RATES[if lab.playback.is_some() {
                    view.rate
                } else {
                    lab.rate
                }],
                if lab.paused { "PAUSED" } else { "" }
            ),
            Readout::Stages => format!("STAGES · {} · {:?}", ship.name, ship.mode),
            Readout::Throttle => format!(
                "{:3.0}%\n{}",
                c.throttle * 100.,
                if parts.iter().any(|p| p.firing) {
                    "firing"
                } else if parts.iter().any(|p| p.lit) {
                    "staged"
                } else {
                    "unlit"
                }
            ),
            Readout::Altitude => distance(altitude),
            Readout::Speed => format!("{speed:.1} m/s"),
            Readout::SpeedReference => format!(
                "{} {}",
                if view.speed_surface { "over" } else { "about" },
                body.name
            ),
            Readout::Orbit => format!(
                "ORBIT · {}\nAp {}\nPe {}\nimpact {}\nmap {:.0}%",
                body.name,
                if orbital.apoapsis_radius_meters.is_finite() {
                    distance(orbital.apoapsis_radius_meters - body.radius_meters)
                } else {
                    "escape".into()
                },
                distance(orbital.periapsis_radius_meters - body.radius_meters),
                lab.prediction
                    .as_ref()
                    .and_then(|p| p.impact.as_ref())
                    .map_or("—".into(), |p| format!(
                        "in {:.0}s",
                        (p.0 - f.time()).max(0.)
                    )),
                map * 100.
            ),
            Readout::Maneuver => plan_description(&lab)
                .split("\nM add")
                .next()
                .unwrap()
                .to_owned(),
            Readout::ViewDiagnostics => {
                let sample = view.sample(sim);
                let state = sample.view;
                let observed = sim.observation_body();
                let (spin_body, spin_weight) =
                    void_view::camera_spin(&state, view.path_frame(), observed, nav);
                let focus = if view.focus_body.is_some() {
                    f.ephemeris.bodies()[observed].name.as_str()
                } else {
                    ship.name.as_str()
                };
                let (building, requests, drawn, bytes) = ground.readiness();
                let sea = sim
                    .world
                    .bodies
                    .get(&f.ephemeris.bodies()[observed].id)
                    .and_then(|b| b.sea_level_meters)
                    .map_or("—".into(), distance);
                format!(
                    "focus   {} · reference {}\ncamera  {} · map {:.0}% · up {:.0}%\nco-rotate {:.0}% {}\nrange   {} – {}\ntiles   {} drawn · {} building · {} new requests\ncache   {:.1} MiB\nscenery {}×{} · sea {} · visual air/water",
                    focus,
                    body.name,
                    distance(view.distance),
                    state.map_weight * 100.,
                    state.up_weight * 100.,
                    spin_weight * 100.,
                    f.ephemeris.bodies()[spin_body].name,
                    distance(state.min_distance),
                    distance(state.max_distance),
                    drawn,
                    building,
                    requests,
                    bytes as f64 / 1048576.,
                    window.physical_width(),
                    window.physical_height(),
                    sea
                )
            }
            Readout::Status => format!(
                "{} · {}{}",
                ship.name,
                f.control_profile(id)
                    .map_or("Passive", |profile| match profile {
                        void_assembly::ControlProfile::Flight => "Flight",
                        void_assembly::ControlProfile::Aircraft => "Aircraft",
                        void_assembly::ControlProfile::Rover => "Rover",
                        void_assembly::ControlProfile::Eva => "EVA",
                    }),
                if lab.notice.is_empty() {
                    String::new()
                } else {
                    format!("\n{}", lab.notice)
                }
            ),
        };
    }
}
fn distance(m: f64) -> String {
    if m.abs() >= 1e9 {
        format!("{:.3} Gm", m / 1e9)
    } else if m.abs() >= 1e4 {
        format!("{:.1} km", m / 1e3)
    } else {
        format!("{m:.1} m")
    }
}

fn field_value(field: Field, spec: &void_orbit::ManeuverSpec) -> f64 {
    match field {
        Field::Start => spec.start_time,
        Field::Prograde => spec.prograde,
        Field::Normal => spec.normal,
        Field::Radial => spec.radial,
    }
}

/// Current connected-stack vacuum estimate. Does not assume future decoupling or sum shared fuel twice.
fn engine_delta_v(f: &void_vessels::Fleet, id: &str, p: &void_vessels::PartSnapshot) -> String {
    let members = f
        .part_snapshots(id)
        .into_iter()
        .map(|p| p.id)
        .collect::<Vec<_>>();
    let ratings = f
        .parts()
        .part(&p.id)
        .engines()
        .filter(|(_, r)| r.jet.is_none())
        .collect::<Vec<_>>();
    if ratings.is_empty() {
        return "—".into();
    }
    if ratings.len() != 1 {
        return "— (multiple engines)".into();
    }
    let (_, r) = ratings[0];
    let fuel: f64 = f
        .parts()
        .resource_tanks(&members, &p.id, r.resource)
        .iter()
        .map(|t| f.parts().part(t).resource(r.resource))
        .sum();
    let mass = f.snapshot(id).mass_kg;
    assert!(
        mass > fuel && mass.is_finite() && fuel.is_finite(),
        "HUD invalid mass"
    );
    format!(
        "{:.0}m/s (stack vac)",
        r.isp_seconds * void_assembly::G0 * (mass / (mass - fuel)).ln()
    )
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(super) fn stages(
    mut commands: Commands,
    lab: NonSend<Lab>,
    mut cache: Option<ResMut<StageCache>>,
    list: Query<Entity, With<StageList>>,
    mut stage_text: Query<(&StageItem, &mut Text, &mut TextColor)>,
    mut gauges: Query<(&Gauge, &mut Node, &mut BackgroundColor)>,
) {
    let Some(cache) = cache.as_mut() else {
        return;
    };
    let Ok(parent) = list.single() else {
        return;
    };
    let sim = lab.session.sim();
    let f = &sim.fleet;
    let id = &sim.selected;
    let parts = f.part_snapshots(id);
    let displayed = parts
        .iter()
        .filter(|p| p.stage.is_some() || !p.module_stages.is_empty())
        .collect::<Vec<_>>();
    let keys = displayed.iter().map(|p| p.id.clone()).collect::<Vec<_>>();
    if cache.keys != keys {
        // Gauge rows have their own hierarchy; recreate list children together.
        commands.entity(parent).despawn_children();
        cache.keys = keys;
        for p in &displayed {
            let r = commands
                .spawn(Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: px(2),
                    ..default()
                })
                .id();
            commands.entity(parent).add_child(r);
            let t = text(&mut commands, r, "", 11.);
            commands.entity(t).insert(StageItem(p.id.clone()));
            if engine_fuel(f, id, p).1 > 0. {
                let track = commands
                    .spawn((
                        Node {
                            height: px(6),
                            width: percent(100),
                            ..default()
                        },
                        BackgroundColor(Color::srgb(0.1, 0.125, 0.188)),
                    ))
                    .id();
                commands.entity(r).add_child(track);
                let fill = commands
                    .spawn((
                        Gauge::Fuel(p.id.clone()),
                        Node {
                            height: percent(100),
                            width: percent(0),
                            ..default()
                        },
                        BackgroundColor(Color::srgb(0.42, 0.55, 0.78)),
                    ))
                    .id();
                commands.entity(track).add_child(fill);
            }
        }
    }
    for (item, mut text, mut color) in &mut stage_text {
        let Some(p) = parts.iter().find(|p| p.id == item.0) else {
            continue;
        };
        let (fuel, capacity) = engine_fuel(f, id, p);
        color.0 = if p.lit {
            Color::srgb(0.49, 1., 0.69)
        } else if p.staged {
            Color::srgba(0.81, 0.84, 0.89, 0.4)
        } else if p
            .module_stages
            .values()
            .flatten()
            .any(|stage| Some(stage) == f.stages_left(id).first())
        {
            Color::srgb(1., 0.83, 0.47)
        } else {
            INK
        };
        text.0 = format!(
            "{} · {}\n{:.0}/{:.0} kg · Δv {}",
            p.definition.name.split('/').next().unwrap().trim(),
            if p.firing {
                "firing"
            } else if p.lit {
                "active"
            } else if p.staged {
                "consumed"
            } else {
                if p.module_stages
                    .values()
                    .flatten()
                    .any(|stage| Some(stage) == f.stages_left(id).first())
                {
                    "next"
                } else {
                    "waiting"
                }
            },
            fuel,
            capacity,
            engine_delta_v(f, id, p)
        );
        if capacity == 0. {
            text.0 = format!(
                "{} · stage {} · {}",
                p.definition.name,
                p.stage
                    .or_else(|| p.module_stages.values().flatten().copied().min())
                    .map_or("—".into(), |stage| stage.to_string()),
                if p.staged {
                    "consumed"
                } else if p
                    .module_stages
                    .values()
                    .flatten()
                    .any(|stage| Some(stage) == f.stages_left(id).first())
                {
                    "next"
                } else {
                    "waiting"
                }
            );
        }
    }
    for (gauge, mut node, mut background) in &mut gauges {
        match gauge {
            Gauge::Throttle => {
                node.height = percent((f.control(id).throttle * 100.) as f32);
                background.0 = if parts.iter().any(|p| p.firing) {
                    Color::srgb(1., 0.70, 0.36)
                } else {
                    Color::srgb(0.42, 0.55, 0.78)
                };
            }
            Gauge::Fuel(key) => {
                let Some(p) = parts.iter().find(|p| &p.id == key) else {
                    continue;
                };
                let (fuel, capacity) = engine_fuel(f, id, p);
                node.width = percent(if capacity > 0. {
                    (fuel / capacity * 100.) as f32
                } else {
                    0.
                });
                background.0 = if p.lit {
                    Color::srgb(0.49, 1., 0.69)
                } else {
                    Color::srgb(0.42, 0.55, 0.78)
                };
            }
        }
    }
}
fn engine_fuel(f: &void_vessels::Fleet, id: &str, p: &void_vessels::PartSnapshot) -> (f64, f64) {
    let members = f
        .part_snapshots(id)
        .into_iter()
        .map(|p| p.id)
        .collect::<Vec<_>>();
    let mut tanks = std::collections::BTreeSet::new();
    for (_, r) in f.parts().part(&p.id).engines() {
        for t in f.parts().resource_tanks(&members, &p.id, r.resource) {
            tanks.insert((t, r.resource));
        }
    }
    if tanks.is_empty() {
        return (p.fuel_kg, void_assembly::tank_capacity(p.definition));
    }
    let mut fuel = 0.;
    let mut capacity = 0.;
    for (t, r) in tanks {
        let tank = f.parts().part(&t);
        fuel += tank.resource(r);
        capacity += void_assembly::full_resources(tank.definition)
            .get(&r)
            .copied()
            .expect("resource tank capacity");
    }
    (fuel, capacity)
}

#[allow(clippy::type_complexity)]
pub(super) fn indicators(
    lab: NonSend<Lab>,
    children: Query<&Children>,
    roots: Query<Entity, With<OrbitFade>>,
    mut colors: Query<
        (
            Option<&mut TextColor>,
            Option<&mut BackgroundColor>,
            Option<&mut BorderColor>,
        ),
        Without<Button>,
    >,
    mut buttons: Query<(Entity, &Click, &mut BackgroundColor), With<Button>>,
    mut labels: Query<&mut Text>,
) {
    let sim = lab.session.sim();
    let p = &sim.presentation;
    // Reference orbit panel fades with the same map weight as the trajectories.
    let alpha = lab.view.map_or(0., |s| s.map_weight) as f32;
    let mut pending = roots.iter().collect::<Vec<_>>();
    let mut descendants = Vec::new();
    while let Some(e) = pending.pop() {
        descendants.push(e);
        if let Ok(c) = children.get(e) {
            pending.extend(c.iter());
        }
    }
    // Button and generic colour queries are disjoint through Without<Button>.
    for (entity, click, mut bg) in &mut buttons {
        let active = match click {
            Click::Toggle(t) => match t {
                Toggle::VisualAir => p.visual_air,
                Toggle::VisualClouds => p.visual_clouds,
                Toggle::VisualOcean => p.visual_ocean,
                Toggle::VisualStars => p.visual_stars,
                _ => false,
            },
            Click::Warp(i) => {
                *i == if lab.playback.is_some() {
                    p.rate
                } else {
                    lab.rate
                }
            }
            Click::Pause => lab.paused,
            Click::Key(KeyCode::KeyT) => {
                sim.fleet.sas_phase(&sim.selected) != void_sas::SasPhase::Off
            }
            _ => false,
        };
        if active {
            bg.0 = Color::srgb(0.12, 0.35, 0.18);
        }
        if let Click::Warp(i) = click
            && RATES[*i] > 4.0
            && sim.fleet.rails_blocker().is_some()
        {
            bg.0 = Color::srgb(0.06, 0.07, 0.09);
        }
        if descendants.contains(&entity) {
            bg.0.set_alpha(alpha);
        }
        if let Click::Key(KeyCode::KeyK | KeyCode::KeyL) = click
            && let Ok(children) = children.get(entity)
        {
            for child in children.iter() {
                if let Ok(mut text) = labels.get_mut(child) {
                    text.0 = if matches!(click, Click::Key(KeyCode::KeyK)) {
                        if p.altitude_agl { "AGL" } else { "ALT" }
                    } else if p.speed_surface {
                        "SURFACE"
                    } else {
                        "ORBIT"
                    }
                    .into();
                }
            }
        }
        if let Click::Toggle(t) = click
            && let Ok(children) = children.get(entity)
        {
            for child in children.iter() {
                if let Ok(mut text) = labels.get_mut(child) {
                    let name = match t {
                        Toggle::VisualAir => "Atmosphere · visual",
                        Toggle::VisualClouds => "Clouds",
                        Toggle::VisualOcean => "Ocean · visual",
                        Toggle::VisualStars => "Stars",
                        _ => "",
                    };
                    text.0 = format!("{} {}", if active { "☑" } else { "☐" }, name);
                }
            }
        }
    }
    for e in descendants {
        if let Ok((text, bg, border)) = colors.get_mut(e) {
            if let Some(mut text) = text {
                text.0.set_alpha(alpha);
            }
            if let Some(mut bg) = bg {
                bg.0.set_alpha(0.82 * alpha);
            }
            if let Some(mut border) = border {
                border.top.set_alpha(alpha);
                border.bottom.set_alpha(alpha);
                border.left.set_alpha(alpha);
                border.right.set_alpha(alpha);
            }
        }
    }
}

pub(super) fn scroll_panels(
    scroll: Res<AccumulatedMouseScroll>,
    window: Single<&Window>,
    mut panels: Query<(&ComputedNode, &UiGlobalTransform, &mut ScrollPosition)>,
) {
    let Some(cursor) = window.physical_cursor_position() else {
        return;
    };
    if scroll.delta.y == 0. {
        return;
    }
    let delta = -scroll.delta.y
        * match scroll.unit {
            bevy::input::mouse::MouseScrollUnit::Line => 24.,
            bevy::input::mouse::MouseScrollUnit::Pixel => 1.,
        };
    for (node, transform, mut pos) in &mut panels {
        if node.contains_point(*transform, cursor) {
            let max = ((node.content_size() - node.size()) * node.inverse_scale_factor())
                .y
                .max(0.);
            pos.y = (pos.y + delta).clamp(0., max);
        }
    }
}

/// Keep font bytes in the executable so arbitrary working directories render all HUD glyphs.
pub(super) fn apply_font(
    font: Option<Res<HudFont>>,
    mut texts: Query<&mut TextFont, Without<crate::navball::NavballLabel>>,
    assets: Res<Assets<Font>>,
    context: Option<ResMut<bevy::text::FontCx>>,
    mut registered: Local<bool>,
) {
    let Some(font) = font else {
        return;
    };
    if !*registered && let Some(mut context) = context {
        let fallback = assets.get(&font.cjk).expect("bundled CJK font asset");
        let families = context
            .collection
            .register_fonts(fallback.data.clone(), None)
            .into_iter()
            .map(|(id, _)| id)
            .collect::<Vec<_>>();
        assert!(!families.is_empty(), "bundled CJK font registration failed");
        for tag in ["Hani", "Hira", "Kana", "Hang"] {
            let script: fontique::Script = tag.parse().expect("valid Unicode script");
            assert!(
                context
                    .collection
                    .set_fallbacks(script, families.iter().copied()),
                "CJK default fallback"
            );
            if tag == "Hani" {
                for locale in ["zh-Hans", "zh-Hant", "ja"] {
                    context
                        .collection
                        .set_fallbacks((script, locale), families.iter().copied());
                }
            }
        }
        *registered = true;
    }
    for mut text in &mut texts {
        if text.font != font.mono.clone().into() {
            text.font = font.mono.clone().into();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn released_quick_pointer_click_is_queued_and_applied_once() {
        let mut app = super::super::tests::initialized_scene(true);
        app.insert_resource(ButtonInput::<KeyCode>::default())
            .insert_resource(ButtonInput::<MouseButton>::default())
            .add_message::<bevy::input::keyboard::KeyboardInput>()
            .add_systems(Update, interactions.before(super::super::draw));
        let button = app
            .world_mut()
            .query::<(Entity, &Click)>()
            .iter(app.world())
            .find_map(|(e, c)| matches!(c, Click::Toggle(Toggle::VisualAir)).then_some(e))
            .unwrap();
        let window = app
            .world_mut()
            .query_filtered::<Entity, With<Window>>()
            .single(app.world())
            .unwrap();
        let camera = app
            .world_mut()
            .query_filtered::<Entity, With<LabCamera>>()
            .single(app.world())
            .unwrap();
        let event = Pointer::new(
            bevy::picking::pointer::PointerId::Mouse,
            bevy::picking::pointer::Location {
                target: bevy::camera::NormalizedRenderTarget::Window(
                    bevy::window::WindowRef::Entity(window)
                        .normalize(None)
                        .unwrap(),
                ),
                position: Vec2::ZERO,
            },
            bevy::picking::events::Click {
                button: bevy::picking::pointer::PointerButton::Primary,
                hit: bevy::picking::backend::HitData::new(camera, 0., None, None),
                duration: std::time::Duration::from_millis(5),
                count: 1,
            },
            button,
        );
        app.world_mut().trigger(event);
        assert_eq!(app.world().resource::<PendingClicks>().0.len(), 1);
        assert!(
            !app.world()
                .resource::<ButtonInput<MouseButton>>()
                .pressed(MouseButton::Left)
        );
        app.update();
        assert!(
            !app.world()
                .non_send::<Lab>()
                .session
                .sim()
                .presentation
                .visual_air
        );
        assert!(app.world().resource::<PendingClicks>().0.is_empty());
        app.update();
        assert!(
            !app.world()
                .non_send::<Lab>()
                .session
                .sim()
                .presentation
                .visual_air,
            "same click must not double-toggle"
        );
    }
    #[test]
    fn numeric_commit_and_cancel_capture_the_current_keyboard_frame() {
        for key_code in [KeyCode::Enter, KeyCode::Escape] {
            let mut app = super::super::tests::initialized_scene(true);
            app.insert_resource(ButtonInput::<KeyCode>::default())
                .insert_resource(ButtonInput::<MouseButton>::default())
                .insert_resource(AccumulatedMouseMotion::default())
                .insert_resource(AccumulatedMouseScroll::default())
                .add_message::<bevy::input::keyboard::KeyboardInput>()
                .add_systems(
                    Update,
                    (interactions, super::super::controls)
                        .chain()
                        .before(super::super::draw),
                );
            {
                let mut state = app.world_mut().resource_mut::<UiState>();
                state.field = Some(Field::Start);
                state.draft = "123".into();
            }
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(key_code);
            let window = app
                .world_mut()
                .query_filtered::<Entity, With<Window>>()
                .single(app.world())
                .unwrap();
            app.world_mut()
                .write_message(bevy::input::keyboard::KeyboardInput {
                    key_code,
                    logical_key: if key_code == KeyCode::Enter {
                        bevy::input::keyboard::Key::Enter
                    } else {
                        bevy::input::keyboard::Key::Escape
                    },
                    state: bevy::input::ButtonState::Pressed,
                    text: None,
                    repeat: false,
                    window,
                });
            let before =
                void_fleet_flight::session::world_mark(app.world().non_send::<Lab>().session.sim());
            app.update();
            let state = app.world().resource::<UiState>();
            assert!(state.field.is_none());
            assert!(
                state.editing,
                "commit/cancel frame must still capture game keys"
            );
            assert_eq!(
                before,
                void_fleet_flight::session::world_mark(app.world().non_send::<Lab>().session.sim())
            );
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .clear();
            app.update();
            assert!(!app.world().resource::<UiState>().editing);
        }
    }
    #[test]
    fn edited_maneuver_value_reaches_the_live_plan_and_replays() {
        let mut app = super::super::tests::initialized_scene(true);
        app.insert_resource(ButtonInput::<KeyCode>::default())
            .insert_resource(ButtonInput::<MouseButton>::default())
            .insert_resource(AccumulatedMouseMotion::default())
            .insert_resource(AccumulatedMouseScroll::default())
            .add_message::<bevy::input::keyboard::KeyboardInput>()
            .add_systems(
                Update,
                (interactions, super::super::controls)
                    .chain()
                    .before(super::super::draw),
            );
        {
            let mut lab = app.world_mut().non_send_mut::<Lab>();
            let craft = lab.craft.clone();
            let Outcome::Spawned(id) = lab.session.execute(Action::LaunchOrbit {
                craft,
                offset: DVec3::ZERO,
            }) else {
                panic!("orbit fixture");
            };
            lab.session.execute(Action::Select { vessel: id });
            lab.session.execute(Action::Stage);
        }
        app.world_mut()
            .resource_mut::<PendingClicks>()
            .0
            .push(Click::Key(KeyCode::KeyM));
        app.update();
        app.world_mut()
            .resource_mut::<PendingClicks>()
            .0
            .push(Click::Field(Field::Prograde));
        app.update();
        let window = app
            .world_mut()
            .query_filtered::<Entity, With<Window>>()
            .single(app.world())
            .unwrap();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::ControlLeft);
        app.world_mut()
            .write_message(bevy::input::keyboard::KeyboardInput {
                key_code: KeyCode::KeyA,
                logical_key: bevy::input::keyboard::Key::Character("a".into()),
                state: bevy::input::ButtonState::Pressed,
                text: Some("a".into()),
                repeat: false,
                window,
            });
        app.update();
        assert!(app.world().resource::<UiState>().draft.is_empty());
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        app.world_mut()
            .write_message(bevy::input::keyboard::KeyboardInput {
                key_code: KeyCode::Digit1,
                logical_key: bevy::input::keyboard::Key::Character("12.5".into()),
                state: bevy::input::ButtonState::Pressed,
                text: Some("12.5".into()),
                repeat: false,
                window,
            });
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::Enter);
        app.world_mut()
            .write_message(bevy::input::keyboard::KeyboardInput {
                key_code: KeyCode::Enter,
                logical_key: bevy::input::keyboard::Key::Enter,
                state: bevy::input::ButtonState::Pressed,
                text: None,
                repeat: false,
                window,
            });
        app.update();
        let mut lab = app.world_mut().non_send_mut::<Lab>();
        let sim = lab.session.sim();
        let plan = &sim.plans[&sim.selected];
        assert_eq!(plan.plan.maneuver(plan.selected).prograde, 12.5);
        assert!(!plan.executing, "numeric Enter must not execute or dock");
        lab.session.mark();
        let recording = lab.session.recording();
        assert_eq!(
            recording
                .entries
                .iter()
                .filter(|entry| matches!(entry.action, Action::EditManeuver { .. }))
                .count(),
            1
        );
        assert_eq!(
            void_fleet_flight::session::world_mark(FlightSession::from_recording(recording).sim()),
            void_fleet_flight::session::world_mark(lab.session.sim()),
        );
    }
    #[test]
    fn sas_button_reports_the_core_refusal_for_a_passive_stage() {
        let mut app = super::super::tests::initialized_scene(true);
        let mut lab = app.world_mut().non_send_mut::<Lab>();
        assert!(matches!(
            lab.session.execute(Action::Stage),
            Outcome::Staged(_)
        ));
        assert!(matches!(
            lab.session.execute(Action::Stage),
            Outcome::Staged(_)
        ));
        let passive = lab
            .session
            .sim()
            .fleet
            .vessel_ids()
            .into_iter()
            .find(|id| !lab.session.sim().fleet.has_command(id))
            .expect("separated booster without a command part");
        lab.session.execute(Action::Select { vessel: passive });
        dispatch(&mut lab, KeyCode::KeyT);
        assert!(lab.notice.contains("functioning command part"));
    }
}
