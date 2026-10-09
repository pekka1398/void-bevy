//! Contextual cockpit controls and explicit navigation selection.
use super::ui::{base, panel, row, text};
use super::*;

#[derive(Resource, Default)]
pub(super) struct Cockpit {
    pub held: Vec<KeyCode>,
    pub tapped: Vec<KeyCode>,
    queue: Vec<Command>,
    navigation: bool,
    signature: String,
}
impl Cockpit {
    pub(super) fn open_navigation(&mut self) {
        self.navigation = true;
    }
}
#[derive(Clone)]
enum Command {
    Workshop,
    Navigation,
    Key(KeyCode),
    Dock(KeyCode),
    Crew,
    Vessel(String),
    Body(Option<String>),
    Throttle(f64),
}
#[derive(Component)]
pub(super) struct Navigation;
#[derive(Component)]
pub(super) struct Vehicle;
#[derive(Component)]
pub(super) struct Hold(Vec<KeyCode>);
#[derive(Component)]
struct ContextHelp;
#[derive(Component)]
pub(super) struct CockpitReadout;
fn button(commands: &mut Commands, parent: Entity, label: &str, action: Command) {
    let e = commands
        .spawn((
            Button,
            Node {
                padding: UiRect::axes(px(6), px(4)),
                ..default()
            },
            BackgroundColor(Color::srgb(0.08, 0.12, 0.18)),
        ))
        .id();
    text(commands, e, label, 11.);
    commands.entity(e).observe(
        move |mut event: On<Pointer<bevy::picking::events::Click>>, mut state: ResMut<Cockpit>| {
            if event.button == bevy::picking::pointer::PointerButton::Primary {
                state.queue.push(action.clone());
            }
            event.propagate(false);
        },
    );
    commands.entity(parent).add_child(e);
}
fn hold(commands: &mut Commands, parent: Entity, label: &str, keys: &[KeyCode]) {
    let e = commands
        .spawn((
            Button,
            Hold(keys.to_vec()),
            Node {
                padding: UiRect::axes(px(6), px(4)),
                ..default()
            },
            BackgroundColor(Color::srgb(0.08, 0.12, 0.18)),
        ))
        .id();
    text(commands, e, label, 11.);
    commands.entity(parent).add_child(e);
}
pub(super) fn spawn(commands: &mut Commands) -> Entity {
    commands.insert_resource(Cockpit::default());
    let toolbar = panel(
        commands,
        Node {
            right: px(12),
            top: px(70),
            flex_direction: FlexDirection::Row,
            ..base()
        },
    );
    button(
        commands,
        toolbar,
        "Navigation / vessels",
        Command::Navigation,
    );
    button(commands, toolbar, "Workshop / assembly", Command::Workshop);
    let nav = panel(
        commands,
        Node {
            display: Display::None,
            left: px(12),
            top: px(140),
            width: px(310),
            max_height: vh(34),
            overflow: Overflow::scroll_y(),
            ..base()
        },
    );
    commands
        .entity(nav)
        .insert((Navigation, ScrollPosition::default()));
    let vehicle = panel(
        commands,
        Node {
            left: px(12),
            top: px(140),
            width: px(310),
            max_height: vh(34),
            overflow: Overflow::scroll_y(),
            ..base()
        },
    );
    commands
        .entity(vehicle)
        .insert((Vehicle, ScrollPosition::default()));
    toolbar
}
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(super) fn update(
    mut commands: Commands,
    mut state: ResMut<Cockpit>,
    mut lab: NonSendMut<Lab>,
    holds: Query<(&Interaction, &Hold)>,
    mut navs: Query<(Entity, &mut Node), (With<Navigation>, Without<Vehicle>)>,
    mut vehicles: Query<(Entity, &mut Node), (With<Vehicle>, Without<Navigation>)>,
    windows: Query<&Window>,
    mut workshop: Option<ResMut<workshop::Workshop>>,
    menu: Option<Res<menus::MenuState>>,
    ui_state: Option<Res<ui::UiState>>,
    mut readouts: Query<&mut Text, With<CockpitReadout>>,
) {
    state.held.clear();
    state.tapped.clear();
    let blocking = menu.as_ref().is_some_and(|s| s.blocking || s.capture_frame)
        || workshop.as_ref().is_some_and(|s| s.open || s.capture_frame);
    if blocking || ui_state.as_ref().is_some_and(|s| s.editing) {
        state.queue.clear();
        return;
    }
    if windows.iter().any(|w| w.focused) {
        for (interaction, hold) in &holds {
            if *interaction == Interaction::Pressed {
                state.held.extend(hold.0.iter().copied());
            }
        }
    }
    for action in std::mem::take(&mut state.queue) {
        if matches!(action, Command::Navigation) {
            state.navigation = !state.navigation;
            continue;
        }
        if lab.playback.is_some() {
            lab.notice = "Controls unavailable during playback".into();
            continue;
        }
        match action {
            Command::Navigation => unreachable!(),
            Command::Workshop => {
                if let Some(w) = workshop.as_mut() {
                    w.toggle(&mut lab);
                }
                state.held.clear();
                break;
            }
            Command::Key(k) => state.tapped.push(k),
            Command::Dock(k) => {
                let mut keys = ButtonInput::default();
                keys.press(k);
                docking_controls(&mut lab, &keys);
            }
            Command::Crew => crew_transfer(&mut lab),
            Command::Vessel(id) => {
                if lab.session.sim().fleet.vessel_ids().contains(&id) {
                    select_pilot(&mut lab, &id);
                    lab.prediction = None;
                } else {
                    lab.notice = "Selected vessel no longer exists".into();
                }
            }
            Command::Body(id) => {
                let body = match id {
                    None => None,
                    Some(id) => match lab
                        .session
                        .sim()
                        .fleet
                        .ephemeris
                        .bodies()
                        .iter()
                        .find(|b| b.id == id)
                    {
                        Some(body) => Some(body.index),
                        None => {
                            lab.notice = "Selected body no longer exists in this world".into();
                            continue;
                        }
                    },
                };
                lab.session.execute(Action::View {
                    command: ViewCommand::Focus { body },
                });
            }
            Command::Throttle(delta) => {
                let c = lab.session.sim().fleet.control(&lab.session.sim().selected);
                lab.session.execute(Action::Control {
                    throttle: (c.throttle + delta).clamp(0., 1.),
                    turn: c.turn,
                });
            }
        }
    }
    let sim = lab.session.sim();
    let profile = sim.fleet.control_profile(&sim.selected);
    let ids = sim.fleet.vessel_ids();
    for mut t in &mut readouts {
        let control = sim.fleet.control(&sim.selected);
        let rcs = sim.fleet.rcs_control(&sim.selected);
        let vehicle = sim
            .fleet
            .vehicle_control(&sim.selected)
            .map_or(String::new(), |c| {
                format!(
                    "\nDrive {:.0}% · steer {:.0}% · brake {:.0}%",
                    c.drive * 100.,
                    c.steer * 100.,
                    c.brake * 100.
                )
            });
        let port = |p: &Option<PortAddress>| {
            p.as_ref()
                .map_or("none".into(), |p| format!("{}/{}", p.part, p.module))
        };
        let dock = if profile == Some(void_assembly::ControlProfile::Flight)
            && !ports(&lab, true).is_empty()
        {
            format!(
                "\nOwn {} · target {}",
                port(&lab.own_port),
                port(&lab.target_port)
            )
        } else {
            String::new()
        };
        **t = format!(
            "{}\nThrottle {:.0}% · RCS {}{}{}",
            menu.as_ref()
                .map_or_else(|| "Default controls".into(), |m| m.binding_summary()),
            control.throttle * 100.,
            rcs.enabled,
            vehicle,
            dock
        );
    }
    let signature = format!(
        "{:?}|{:?}|{}|{:?}|{:?}",
        profile,
        ids,
        state.navigation,
        sim.selected,
        sim.fleet
            .ephemeris
            .bodies()
            .iter()
            .map(|b| &b.id)
            .collect::<Vec<_>>()
    );
    for (_, mut node) in &mut navs {
        node.display = if state.navigation {
            Display::Flex
        } else {
            Display::None
        };
    }
    if signature == state.signature {
        return;
    }
    state.signature = signature;
    for (entity, _) in &mut navs {
        commands.entity(entity).despawn_children();
        text(&mut commands, entity, "VESSELS · select pilot", 12.);
        for id in &ids {
            button(
                &mut commands,
                entity,
                &format!("{} {id}", if *id == sim.selected { "●" } else { "○" }),
                Command::Vessel(id.clone()),
            );
        }
        text(&mut commands, entity, "CAMERA FOCUS · preserves world", 12.);
        button(
            &mut commands,
            entity,
            "Selected vessel",
            Command::Body(None),
        );
        for b in sim.fleet.ephemeris.bodies() {
            button(
                &mut commands,
                entity,
                &b.id,
                Command::Body(Some(b.id.clone())),
            );
        }
        button(
            &mut commands,
            entity,
            "Near / orbit / far",
            Command::Key(KeyCode::F1),
        );
        button(
            &mut commands,
            entity,
            "Cycle plotting frame",
            Command::Key(KeyCode::KeyG),
        );
    }
    for (entity, mut node) in &mut vehicles {
        node.display = if state.navigation {
            Display::None
        } else {
            Display::Flex
        };
        commands.entity(entity).despawn_children();
        if state.navigation {
            continue;
        }
        text(
            &mut commands,
            entity,
            match profile {
                Some(void_assembly::ControlProfile::Flight) => "PILOT · Flight",
                Some(void_assembly::ControlProfile::Aircraft) => "PILOT · Aircraft",
                Some(void_assembly::ControlProfile::Rover) => "PILOT · Rover",
                Some(void_assembly::ControlProfile::Eva) => "PILOT · EVA",
                None => "Unpiloted vessel",
            },
            12.,
        );
        let info = text(&mut commands, entity, "", 11.);
        commands.entity(info).insert(CockpitReadout);
        let snapshots = sim.fleet.part_snapshots(&sim.selected);
        let has_engine = snapshots.iter().any(|p| {
            p.definition
                .modules
                .iter()
                .any(|m| matches!(m, Module::Engine { .. }))
        });
        let has_rcs = snapshots.iter().any(|p| {
            p.definition
                .modules
                .iter()
                .any(|m| matches!(m, Module::Rcs { .. }))
        });
        let has_seat = snapshots.iter().any(|p| {
            p.definition
                .modules
                .iter()
                .any(|m| matches!(m, Module::Seat { .. }))
        }) || profile == Some(void_assembly::ControlProfile::Eva);
        let has_port = snapshots.iter().any(|p| {
            p.definition
                .modules
                .iter()
                .any(|m| matches!(m, Module::DockingPort { .. }))
        });
        if has_seat {
            button(
                &mut commands,
                entity,
                "Exit seat / board nearest",
                Command::Crew,
            );
        }
        if has_engine {
            let r = row(&mut commands, entity);
            for (label, delta) in [("Throttle −", -0.1), ("Throttle +", 0.1)] {
                button(&mut commands, r, label, Command::Throttle(delta));
            }
            button(&mut commands, r, "Cut", Command::Key(KeyCode::KeyX));
        }
        use void_assembly::ControlProfile;
        let r = row(&mut commands, entity);
        let bindings: &[(&str, KeyCode)] = match profile {
            Some(ControlProfile::Flight) => &[
                ("Pitch · W", KeyCode::KeyW),
                ("Pitch · S", KeyCode::KeyS),
                ("Yaw · A", KeyCode::KeyA),
                ("Yaw · D", KeyCode::KeyD),
                ("Roll · Q", KeyCode::KeyQ),
                ("Roll · E", KeyCode::KeyE),
            ],
            Some(ControlProfile::Aircraft) => &[
                ("Pitch · W", KeyCode::KeyW),
                ("Pitch · S", KeyCode::KeyS),
                ("Roll · A", KeyCode::KeyA),
                ("Roll · D", KeyCode::KeyD),
                ("Yaw · Q", KeyCode::KeyQ),
                ("Yaw · E", KeyCode::KeyE),
            ],
            Some(ControlProfile::Rover) => &[
                ("Forward", KeyCode::KeyW),
                ("Back", KeyCode::KeyS),
                ("Steer left", KeyCode::KeyA),
                ("Steer right", KeyCode::KeyD),
            ],
            Some(ControlProfile::Eva) => &[
                ("Forward", KeyCode::KeyW),
                ("Back", KeyCode::KeyS),
                ("Left", KeyCode::KeyA),
                ("Right", KeyCode::KeyD),
                ("Turn · Q", KeyCode::KeyQ),
                ("Turn · E", KeyCode::KeyE),
            ],
            None => &[],
        };
        for &(label, k) in bindings {
            hold(&mut commands, r, label, &[k]);
        }
        match profile {
            Some(ControlProfile::Rover) => {
                hold(&mut commands, entity, "Hold brake", &[KeyCode::Space]);
                button(
                    &mut commands,
                    entity,
                    "Parking brake / cut",
                    Command::Key(KeyCode::KeyX),
                );
            }
            Some(ControlProfile::Eva) => {
                button(&mut commands, entity, "Jump", Command::Key(KeyCode::Space))
            }
            Some(ControlProfile::Aircraft) => {
                hold(&mut commands, entity, "Hold wheel brakes", &[KeyCode::KeyB])
            }
            _ => {}
        }
        if has_rcs {
            button(
                &mut commands,
                entity,
                "RCS / EVA pack",
                Command::Key(KeyCode::KeyH),
            );
            let r = row(&mut commands, entity);
            for (label, k) in [
                (
                    if profile == Some(ControlProfile::Eva) {
                        "Translate −X"
                    } else {
                        "Translate +X"
                    },
                    KeyCode::KeyD,
                ),
                (
                    if profile == Some(ControlProfile::Eva) {
                        "+X"
                    } else {
                        "−X"
                    },
                    KeyCode::KeyA,
                ),
                ("+Y", KeyCode::KeyE),
                ("−Y", KeyCode::KeyQ),
                ("+Z", KeyCode::KeyW),
                ("−Z", KeyCode::KeyS),
            ] {
                hold(&mut commands, r, label, &[KeyCode::AltLeft, k]);
            }
        }
        if profile == Some(ControlProfile::Flight) && has_port {
            text(&mut commands, entity, "DOCKING", 12.);
            for (label, k) in [
                ("Own port", KeyCode::F10),
                ("Target port", KeyCode::F11),
                ("Arm selected ports", KeyCode::F12),
                ("Dock", KeyCode::Enter),
                ("Undock", KeyCode::Backspace),
            ] {
                button(&mut commands, entity, label, Command::Dock(k));
            }
        }
        let help = match profile {
            Some(ControlProfile::Rover) => {
                "Default keys: W/S drive · A/D steer · Space brake · X parking brake\nCrew requires healthy pilot and seat."
            }
            Some(ControlProfile::Eva) => {
                "Default keys: W/S walk · A/D strafe · Q/E turn · Space jump\nH pack · Alt + movement translates · F boards nearest seat."
            }
            Some(ControlProfile::Aircraft) => {
                "Default keys: W/S pitch · A/D roll · Q/E yaw · B wheel brakes\nShift/Ctrl throttle · no reaction wheel SAS without installed module."
            }
            _ => {
                "Default keys: W/S pitch · A/D yaw · Q/E roll · Space stage\nT SAS · H RCS · Alt + movement translates\nSelect own and target ports, arm, then dock."
            }
        };
        let e = text(&mut commands, entity, help, 11.);
        commands.entity(e).insert(ContextHelp);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn navigation_hides_the_vehicle_surface_and_rejects_stale_identity() {
        let mut app = super::super::tests::initialized_scene(true);
        app.add_systems(Update, update.before(super::super::draw));
        app.update();
        app.world_mut().resource_mut::<Cockpit>().open_navigation();
        app.update();
        let nav = app
            .world_mut()
            .query_filtered::<&Node, With<Navigation>>()
            .single(app.world())
            .unwrap();
        assert_eq!(nav.display, Display::Flex);
        let vehicle = app
            .world_mut()
            .query_filtered::<&Node, With<Vehicle>>()
            .single(app.world())
            .unwrap();
        assert_eq!(
            vehicle.display,
            Display::None,
            "No empty cockpit can overlay navigation headings"
        );
        let focus = app
            .world()
            .non_send::<Lab>()
            .session
            .sim()
            .presentation
            .focus_body;
        app.world_mut()
            .resource_mut::<Cockpit>()
            .queue
            .push(Command::Body(Some("removed body".into())));
        app.update();
        assert_eq!(
            app.world()
                .non_send::<Lab>()
                .session
                .sim()
                .presentation
                .focus_body,
            focus
        );
        assert!(
            app.world()
                .non_send::<Lab>()
                .notice
                .contains("no longer exists")
        );
    }
    #[test]
    fn held_cockpit_input_releases_on_pointer_release_focus_loss_and_modal() {
        let mut app = super::super::tests::initialized_scene(true);
        app.init_resource::<ButtonInput<KeyCode>>()
            .init_resource::<ButtonInput<MouseButton>>()
            .init_resource::<AccumulatedMouseMotion>()
            .init_resource::<AccumulatedMouseScroll>()
            .add_systems(
                Update,
                (update, super::super::controls)
                    .chain()
                    .before(super::super::draw),
            );
        app.world_mut().non_send_mut::<Lab>().paused = false;
        let held = app
            .world_mut()
            .spawn((Hold(vec![KeyCode::KeyW]), Interaction::Pressed))
            .id();
        let window = app
            .world_mut()
            .query_filtered::<Entity, With<Window>>()
            .single(app.world())
            .unwrap();
        app.world_mut().get_mut::<Window>(window).unwrap().focused = true;
        let turn = |app: &App| {
            let l = app.world().non_send::<Lab>();
            l.session
                .sim()
                .fleet
                .control(&l.session.sim().selected)
                .turn
        };
        app.update();
        assert!(turn(&app).x < 0.);
        *app.world_mut().get_mut::<Interaction>(held).unwrap() = Interaction::None;
        app.update();
        assert_eq!(turn(&app), DVec3::ZERO);
        *app.world_mut().get_mut::<Interaction>(held).unwrap() = Interaction::Pressed;
        app.update();
        assert!(turn(&app).x < 0.);
        app.world_mut().get_mut::<Window>(window).unwrap().focused = false;
        app.update();
        assert_eq!(turn(&app), DVec3::ZERO);
        app.world_mut().get_mut::<Window>(window).unwrap().focused = true;
        app.update();
        assert!(turn(&app).x < 0.);
        app.world_mut().resource_mut::<menus::MenuState>().blocking = true;
        app.update();
        assert_eq!(turn(&app), DVec3::ZERO);
        assert!(app.world().resource::<Cockpit>().held.is_empty());
    }
}
