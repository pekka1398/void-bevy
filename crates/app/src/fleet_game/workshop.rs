//! Main-game craft construction. Authored Craft is compiled before every edit;
//! launching transfers it through the existing journalled Fleet action.
use super::*;
use void_assembly::{
    ResourceId, actionable, add_part, capacity, catalog, compile, definition, export_craft,
    fresh_craft, remove_subtree, set_attachment_twist,
};

#[derive(Resource)]
pub(super) struct Workshop {
    pub open: bool,
    pub capture_frame: bool,
    craft: Craft,
    selected: String,
    pending: Option<String>,
    child_node: usize,
    path: String,
    notice: String,
    revision: u64,
    drawn: u64,
    editing: Option<Edit>,
    draft: String,
    queue: Vec<Command>,
    previous_pause: bool,
    catalog_open: bool,
}
#[derive(Clone, Copy)]
enum Edit {
    Name,
    Path,
}
#[derive(Clone)]
enum Command {
    Close,
    New,
    Demo,
    Preset(u8),
    Catalog(String),
    CatalogPanel,
    Select(String),
    Socket(String, String),
    ChildNode,
    Remove,
    Rotate(f64),
    Move(usize, f64),
    ModuleStage(String, String, i32),
    Stage(i32),
    Fuel(ResourceId, f64),
    Save,
    Load,
    Launch,
    Edit(Edit),
}
#[derive(Component)]
pub(super) struct WorkshopRoot;
#[derive(Component)]
pub(super) struct WorkshopBody;
impl Default for Workshop {
    fn default() -> Self {
        let craft = fresh_craft();
        let selected = compile(&craft).expect("fresh craft").root_id;
        Self {
            open: false,
            capture_frame: false,
            craft,
            selected,
            pending: None,
            child_node: 0,
            path: "craft-workshop.json".into(),
            notice: "Choose a catalog part, then an available attachment socket.".into(),
            revision: 1,
            drawn: 0,
            editing: None,
            draft: String::new(),
            queue: vec![],
            previous_pause: false,
            catalog_open: false,
        }
    }
}
impl Workshop {
    pub(super) fn review_preset(&mut self, name: &str) -> Result<(), String> {
        self.edit(match name {
            "rocket" => Command::Demo,
            "rover" => Command::Preset(1),
            "aircraft" => Command::Preset(0),
            _ => return Err("Unknown UI review craft".into()),
        })?;
        self.revision += 1;
        Ok(())
    }
    pub(super) fn toggle(&mut self, lab: &mut Lab) {
        if !self.open {
            if lab.playback.is_some() {
                lab.notice = "Workshop unavailable during replay.".into();
                return;
            }
            self.previous_pause = lab.paused;
            lab.paused = true;
            self.open = true;
        } else {
            self.open = false;
            lab.paused = self.previous_pause;
        }
        self.editing = None;
        self.revision += 1;
    }
    fn replace(&mut self, craft: Craft) -> Result<(), String> {
        let compiled = compile(&craft)?;
        if !craft.parts.iter().any(|p| p.id == self.selected) {
            self.selected = compiled.root_id;
        }
        self.craft = craft;
        Ok(())
    }
    fn edit(&mut self, command: Command) -> Result<(), String> {
        match command {
            Command::New => {
                self.pending = None;
                self.replace(fresh_craft())?;
            }
            Command::Demo => {
                self.pending = None;
                self.replace(void_assembly::flight_rocket())?;
            }
            Command::Preset(which) => {
                let c = match which {
                    0 => void_assembly::aircraft(),
                    1 => void_assembly::crew_rover(),
                    2 => void_assembly::crewed_flight_rocket(),
                    _ => unreachable!("authored preset"),
                };
                self.pending = None;
                self.replace(c)?;
            }
            Command::CatalogPanel => self.catalog_open = !self.catalog_open,
            Command::Catalog(id) => {
                definition(&id)?;
                self.pending = Some(id);
                self.catalog_open = false;
                self.child_node = 0;
            }
            Command::Select(id) => {
                if !self.craft.parts.iter().any(|p| p.id == id) {
                    return Err("Unknown selected part".into());
                }
                self.selected = id;
            }
            Command::ChildNode => {
                let id = self.pending.as_ref().ok_or("Choose a catalog part first")?;
                let nodes = &definition(id)?.nodes;
                if nodes.is_empty() {
                    return Err("Part has no attachment sockets".into());
                }
                self.child_node = (self.child_node + 1) % nodes.len();
            }
            Command::Socket(parent, socket) => {
                let id = self.pending.as_ref().ok_or("Choose a catalog part first")?;
                let d = definition(id)?;
                let child = d
                    .nodes
                    .get(self.child_node)
                    .ok_or("Part has no attachment sockets")?;
                let next = add_part(&self.craft, id, &parent, &socket, &child.id)?;
                self.selected = next.parts.last().expect("added part").id.clone();
                self.replace(next)?;
                self.pending = None;
            }
            Command::Remove => {
                self.replace(remove_subtree(&self.craft, &self.selected)?)?;
            }
            Command::Rotate(delta) => {
                let p = self
                    .craft
                    .parts
                    .iter()
                    .find(|p| p.id == self.selected)
                    .expect("selected craft part");
                let a = p
                    .attachment
                    .as_ref()
                    .ok_or("Root cannot rotate around an attachment socket")?;
                let angle = a.twist_radians + delta;
                let mut c = self.craft.clone();
                c.version = 3;
                self.replace(set_attachment_twist(&c, &self.selected, angle)?)?;
            }
            Command::ModuleStage(part, module, delta) => {
                let mut c = self.craft.clone();
                let p = c
                    .parts
                    .iter_mut()
                    .find(|p| p.id == part)
                    .ok_or("Stage target part no longer exists")?;
                let inherited =
                    void_assembly::default_module_stages(definition(&p.definition_id)?, p.stage);
                let inherited_stage = *inherited
                    .get(&module)
                    .ok_or("Requested stage module is not actionable on target part")?;
                let current = p
                    .module_stages
                    .get(&module)
                    .copied()
                    .unwrap_or(inherited_stage);
                let n = current.map_or(-1, |n| n as i32) + delta;
                if n > 99 {
                    return Err("Stage must be between 0 and 99".into());
                }
                p.module_stages.insert(module, (n >= 0).then_some(n as u32));
                self.replace(c)?;
            }
            Command::Move(axis, delta) => {
                let compiled = compile(&self.craft)?;
                let selected = compiled.part(&self.selected);
                let a = selected
                    .instance
                    .attachment
                    .as_ref()
                    .ok_or("Root cannot move on surface")?;
                let parent = compiled.part(&a.parent_id);
                if !void_assembly::node(parent.definition, &a.parent_node_id)?.surface {
                    return Err("Placement offsets require an authored surface socket".into());
                }
                let mut pose = a.pose.unwrap_or(void_assembly::PartPose {
                    position: parent.pose.rotation.inverse()
                        * (selected.pose.position - parent.pose.position),
                    rotation: parent.pose.rotation.inverse() * selected.pose.rotation,
                });
                pose.position[axis] += delta;
                let mut c = self.craft.clone();
                c.version = 3;
                self.replace(void_assembly::set_attachment_pose(
                    &c,
                    &self.selected,
                    pose,
                )?)?;
            }
            Command::Stage(delta) => {
                let mut c = self.craft.clone();
                let p = c
                    .parts
                    .iter_mut()
                    .find(|p| p.id == self.selected)
                    .expect("selected craft part");
                if !actionable(definition(&p.definition_id)?) {
                    return Err("Selected part has no engine or decoupler stage".into());
                }
                let n = p.stage.map_or(-1, |n| n as i32) + delta;
                if n > 99 {
                    return Err("Stage must be between 0 and 99".into());
                }
                p.stage = (n >= 0).then_some(n as u32);
                p.module_stages.clear();
                self.replace(c)?;
            }
            Command::Fuel(resource, fraction) => {
                let mut c = self.craft.clone();
                let p = c
                    .parts
                    .iter_mut()
                    .find(|p| p.id == self.selected)
                    .expect("selected craft part");
                let cap = capacity(definition(&p.definition_id)?, resource);
                let quantity = p
                    .resources
                    .get_mut(&resource)
                    .ok_or("Selected part has no requested resource tank")?;
                *quantity = (*quantity + cap * fraction).clamp(0., cap);
                self.replace(c)?;
            }
            Command::Save => {
                let text = export_craft(&self.craft)?;
                // Publish an entirely written file without replacing an existing save.
                use std::io::Write;
                let path = std::path::Path::new(&self.path);
                let temp = path.with_file_name(format!(
                    ".void-craft-{}-{}.tmp",
                    std::process::id(),
                    self.revision
                ));
                let mut f = std::fs::OpenOptions::new()
                    .create_new(true)
                    .write(true)
                    .open(&temp)
                    .map_err(|e| format!("Save temporary file: {e}"))?;
                let result = (|| -> Result<(), String> {
                    f.write_all(text.as_bytes())
                        .and_then(|_| f.sync_all())
                        .map_err(|e| format!("Save: {e}"))?;
                    std::fs::hard_link(&temp, path).map_err(|e| {
                        format!(
                            "Save {}: {e}; choose a new path to preserve existing files",
                            self.path
                        )
                    })?;
                    Ok(())
                })();
                if temp.exists() {
                    std::fs::remove_file(&temp)
                        .map_err(|e| format!("Remove save temporary file: {e}"))?;
                }
                result?;
                self.notice = format!("Saved {}", self.path);
            }
            Command::Load => {
                let text = std::fs::read_to_string(&self.path)
                    .map_err(|e| format!("Load {}: {e}", self.path))?;
                self.replace(import_craft(&text)?)?;
                self.pending = None;
                self.notice = format!("Loaded {}", self.path);
            }
            Command::Edit(field) => {
                self.draft = match field {
                    Edit::Name => self.craft.name.clone(),
                    Edit::Path => self.path.clone(),
                };
                self.editing = Some(field);
            }
            Command::Close | Command::Launch => unreachable!("handled by integration"),
        }
        Ok(())
    }
}
pub(super) fn spawn(commands: &mut Commands) {
    commands.insert_resource(Workshop::default());
    commands.spawn((
        WorkshopRoot,
        ui::Panel,
        Interaction::None,
        Node {
            display: Display::None,
            position_type: PositionType::Absolute,
            left: percent(2),
            top: percent(4),
            width: percent(96),
            height: percent(90),
            padding: UiRect::all(px(12)),
            flex_direction: FlexDirection::Column,
            row_gap: px(8),
            overflow: Overflow::clip(),
            ..default()
        },
        ScrollPosition::default(),
        BackgroundColor(Color::srgb(0.035, 0.045, 0.065)),
        ZIndex(100),
    ));
}
fn button(commands: &mut Commands, parent: Entity, label: &str, command: Command) {
    let e = commands
        .spawn((
            Button,
            Node {
                padding: UiRect::axes(px(7), px(4)),
                ..default()
            },
            BackgroundColor(Color::srgb(0.12, 0.17, 0.23)),
        ))
        .id();
    commands.entity(e).observe(
        move |mut event: On<Pointer<bevy::picking::events::Click>>, mut state: ResMut<Workshop>| {
            if event.button == bevy::picking::pointer::PointerButton::Primary {
                state.queue.push(command.clone());
            }
            event.propagate(false);
        },
    );
    ui::text(commands, e, label, 12.);
    commands.entity(parent).add_child(e);
}
#[allow(clippy::too_many_arguments)]
pub(super) fn process(
    mut state: ResMut<Workshop>,
    mut lab: NonSendMut<Lab>,
    keys: Res<ButtonInput<KeyCode>>,
    mut keyboard: MessageReader<bevy::input::keyboard::KeyboardInput>,
) {
    let events: Vec<_> = keyboard.read().cloned().collect();
    state.capture_frame = state.open;
    if !state.open {
        return;
    }
    lab.paused = true;
    for command in std::mem::take(&mut state.queue) {
        state.notice.clear();
        match command {
            Command::Close => state.toggle(&mut lab),
            Command::Launch => {
                let checked = compile(&state.craft).map(|_| ());
                if let Err(e) = checked {
                    state.notice = e;
                    continue;
                }
                let body = lab.session.sim().fleet.ephemeris.bodies()
                    [lab.session.sim().observation_body()]
                .id
                .clone();
                let site = match lab.session.sim().world.daylight_terrain_site(&body) {
                    Ok(site) => site,
                    Err(e) => {
                        state.notice = format!("Launch site: {e}");
                        continue;
                    }
                };
                let result = lab.session.execute(Action::LaunchGroundAt {
                    body,
                    craft: state.craft.clone(),
                    site,
                });
                match result {
                    Outcome::Spawned(id) => {
                        lab.craft = state.craft.clone();
                        select_pilot(&mut lab, &id);
                        lab.prediction = None;
                        lab.dirty = true;
                        state.toggle(&mut lab);
                        lab.paused = true;
                        lab.notice = "Workshop craft launched. Resume and stage to fly.".into();
                    }
                    other => state.notice = format!("Launch refused: {other:?}"),
                }
            }
            other => {
                if let Err(e) = state.edit(other) {
                    state.notice = e;
                }
            }
        }
        state.revision += 1;
        if !state.open {
            return;
        }
    }
    if state.editing.is_none() && keys.just_pressed(KeyCode::Escape) {
        state.toggle(&mut lab);
        return;
    }
    if state.editing.is_some() {
        for event in events {
            if event.state != bevy::input::ButtonState::Pressed {
                continue;
            }
            match event.key_code {
                KeyCode::Escape => state.editing = None,
                KeyCode::Enter => {
                    let draft = state.draft.clone();
                    match state.editing.take().expect("active editor") {
                        Edit::Path => {
                            if draft.trim().is_empty() {
                                state.notice = "Path cannot be empty".into();
                            } else {
                                state.path = draft;
                            }
                        }
                        Edit::Name => {
                            let mut c = state.craft.clone();
                            c.name = draft;
                            if let Err(e) = state.replace(c) {
                                state.notice = e;
                            }
                        }
                    }
                }
                KeyCode::Backspace => {
                    state.draft.pop();
                }
                KeyCode::KeyA
                    if keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]) =>
                {
                    state.draft.clear()
                }
                _ if !keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]) => {
                    if let Some(text) = event.text {
                        for c in text.chars().filter(|c| !c.is_control()) {
                            state.draft.push(c);
                        }
                    }
                }
                _ => {}
            }
            state.revision += 1;
            if state.editing.is_none() {
                break;
            }
        }
    }
}
pub(super) fn refresh(
    mut commands: Commands,
    mut state: ResMut<Workshop>,
    roots: Query<(Entity, Option<&Children>), With<WorkshopRoot>>,
    scroll: Query<&ScrollPosition, With<WorkshopBody>>,
) {
    if state.drawn == state.revision {
        return;
    }
    state.drawn = state.revision;
    let Ok((root, children)) = roots.single() else {
        return;
    };
    if let Some(children) = children {
        for child in children.iter() {
            commands.entity(child).despawn();
        }
    }
    commands.entity(root).insert(Node {
        display: if state.open {
            Display::Flex
        } else {
            Display::None
        },
        position_type: PositionType::Absolute,
        left: percent(2),
        top: percent(4),
        width: percent(96),
        height: percent(90),
        padding: UiRect::all(px(12)),
        flex_direction: FlexDirection::Column,
        row_gap: px(8),
        overflow: Overflow::clip(),
        ..default()
    });
    if !state.open {
        return;
    }
    ui::text(
        &mut commands,
        root,
        "WORKSHOP · edits affect the blueprint; existing vessels remain in flight",
        17.,
    );
    let r = ui::row(&mut commands, root);
    for (label, action) in [
        ("Return", Command::Close),
        ("New command pod", Command::New),
        ("Rocket blueprint", Command::Demo),
        ("Aircraft", Command::Preset(0)),
        ("Crew rover", Command::Preset(1)),
        ("Crew rocket", Command::Preset(2)),
        ("Name", Command::Edit(Edit::Name)),
        ("File path", Command::Edit(Edit::Path)),
        ("Save new file", Command::Save),
        ("Load", Command::Load),
        ("Launch on viewed body", Command::Launch),
    ] {
        button(&mut commands, r, label, action);
    }
    ui::text(
        &mut commands,
        root,
        &format!("{} · {}", state.craft.name, state.path),
        13.,
    );
    if state.editing.is_some() {
        ui::text(
            &mut commands,
            root,
            &format!(
                "> {}▏  Enter accepts · Esc cancels · Ctrl+A clears",
                state.draft
            ),
            14.,
        );
    }
    if !state.notice.is_empty() {
        ui::text(&mut commands, root, &state.notice, 13.);
    }
    let retained_scroll = scroll
        .single()
        .map_or(ScrollPosition::default(), |p| p.clone());
    let body = commands
        .spawn((
            WorkshopBody,
            ui::Panel,
            Interaction::None,
            retained_scroll,
            Node {
                width: percent(100),
                flex_grow: 1.,
                flex_shrink: 1.,
                min_height: px(0),
                flex_direction: FlexDirection::Column,
                row_gap: px(8),
                overflow: Overflow::scroll_y(),
                ..default()
            },
        ))
        .id();
    commands.entity(root).add_child(body);
    let root = body;
    let compiled = compile(&state.craft).expect("validated blueprint");
    // PartGraph is the mass authority, including every mass-bearing resource and crew.
    let mut graph = void_assembly::PartGraph::new();
    let ids = graph.add(&compiled, "workshop");
    let dry: f64 = graph
        .parts()
        .map(|p| p.definition.dry_mass_kg + p.crew_mass_kg())
        .sum();
    ui::text(
        &mut commands,
        root,
        &format!(
            "{} parts · mass {:.1} kg · dry+crew {:.1} kg · resources {:.1} kg",
            ids.len(),
            graph.mass(&ids),
            dry,
            graph.mass(&ids) - dry
        ),
        13.,
    );
    let thrust: f64 = compiled
        .parts
        .iter()
        .flat_map(|p| p.definition.modules.iter())
        .filter_map(|m| match m {
            void_assembly::Module::Engine {
                thrust_newtons,
                jet: None,
                ..
            } => Some(*thrust_newtons),
            _ => None,
        })
        .fold(0.0, |total, thrust| total + thrust);
    ui::text(
        &mut commands,
        root,
        &format!(
            "Installed rocket engines · nominal vacuum maximum {:.1} kN (not staged thrust; jets excluded)",
            thrust / 1000.
        ),
        12.,
    );
    ui::text(
        &mut commands,
        root,
        "BLUEPRINT · X/Y front · oriented projected bounds · green attachment sockets",
        12.,
    );
    let view = commands
        .spawn((
            Node {
                width: px(580),
                height: px(220),
                min_height: px(220),
                flex_shrink: 0.,
                position_type: PositionType::Relative,
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(Color::srgb(0.015, 0.025, 0.04)),
        ))
        .id();
    commands.entity(root).add_child(view);
    let mut bounds = Vec::new();
    for part in &compiled.parts {
        let size = match part.definition.shape {
            void_assembly::Shape::Box => void_assembly::part_box_size(part.definition),
            void_assembly::Shape::Cylinder | void_assembly::Shape::Cone => DVec3::new(
                2.0 * part.definition.radius,
                part.definition.height,
                2.0 * part.definition.radius,
            ),
        };
        let half = size / 2.;
        let mut min = DVec3::splat(f64::INFINITY);
        let mut max = DVec3::splat(f64::NEG_INFINITY);
        for x in [-1., 1.] {
            for y in [-1., 1.] {
                for z in [-1., 1.] {
                    let point =
                        part.pose.position + part.pose.rotation * (half * DVec3::new(x, y, z));
                    min = min.min(point);
                    max = max.max(point);
                }
            }
        }
        bounds.push((part, min, max));
    }
    let low = bounds
        .iter()
        .map(|(_, min, _)| min.y)
        .fold(f64::INFINITY, f64::min);
    let high = bounds
        .iter()
        .map(|(_, _, max)| max.y)
        .fold(f64::NEG_INFINITY, f64::max);
    let left = bounds
        .iter()
        .map(|(_, min, _)| min.x)
        .fold(f64::INFINITY, f64::min);
    let right = bounds
        .iter()
        .map(|(_, _, max)| max.x)
        .fold(f64::NEG_INFINITY, f64::max);
    let scale = (190. / (high - low).max(1.)).min(500. / (right - left).max(1.));
    for (part, min, max) in bounds {
        let e = commands
            .spawn((
                Button,
                Node {
                    position_type: PositionType::Absolute,
                    left: px((290. + (min.x - (left + right) / 2.) * scale) as f32),
                    top: px((15. + (high - max.y) * scale) as f32),
                    width: px(((max.x - min.x) * scale).max(3.) as f32),
                    height: px(((max.y - min.y) * scale).max(3.) as f32),
                    border: UiRect::all(px(2)),
                    ..default()
                },
                BackgroundColor(if part.instance.id == state.selected {
                    Color::srgb(0.20, 0.55, 0.70)
                } else {
                    Color::srgb(0.30, 0.35, 0.40)
                }),
                BorderColor::all(Color::srgb(0.7, 0.85, 0.95)),
            ))
            .id();
        let id = part.instance.id.clone();
        commands.entity(e).observe(
            move |mut event: On<Pointer<bevy::picking::events::Click>>,
                  mut state: ResMut<Workshop>| {
                if event.button == bevy::picking::pointer::PointerButton::Primary {
                    state.queue.push(Command::Select(id.clone()));
                }
                event.propagate(false);
            },
        );
        commands.entity(view).add_child(e);
        ui::text(&mut commands, e, &part.instance.id, 10.);
    }
    for socket in compiled.free_nodes() {
        let point = socket.pose.position;
        let e = commands
            .spawn((
                Button,
                Node {
                    position_type: PositionType::Absolute,
                    left: px((290. + (point.x - (left + right) / 2.) * scale - 4.) as f32),
                    top: px((15. + (high - point.y) * scale - 4.) as f32),
                    width: px(8),
                    height: px(8),
                    border_radius: BorderRadius::all(px(4)),
                    ..default()
                },
                BackgroundColor(Color::srgb(0.25, 0.9, 0.45)),
                ZIndex(2),
            ))
            .id();
        let parent = socket.part_id.clone();
        let id = socket.node.id.clone();
        commands.entity(e).observe(
            move |mut event: On<Pointer<bevy::picking::events::Click>>,
                  mut state: ResMut<Workshop>| {
                if event.button == bevy::picking::pointer::PointerButton::Primary {
                    state
                        .queue
                        .push(Command::Socket(parent.clone(), id.clone()));
                }
                event.propagate(false);
            },
        );
        commands.entity(view).add_child(e);
    }
    ui::text(
        &mut commands,
        root,
        "CATALOG — choose a part; attachment validation checks size and occupied nodes",
        13.,
    );
    let r = ui::row(&mut commands, root);
    button(
        &mut commands,
        r,
        if state.catalog_open {
            "Hide catalog"
        } else {
            "Add part · show catalog"
        },
        Command::CatalogPanel,
    );
    if state.catalog_open {
        for d in catalog() {
            button(
                &mut commands,
                r,
                &format!("{} ({:.0} kg)", d.name, d.dry_mass_kg),
                Command::Catalog(d.id.clone()),
            );
        }
    }
    if let Some(id) = &state.pending {
        let d = definition(id).expect("catalog selection");
        let label = d
            .nodes
            .get(state.child_node)
            .map_or("none", |n| n.id.as_str());
        let r = ui::row(&mut commands, root);
        button(
            &mut commands,
            r,
            &format!("Pending {} · child socket {label} · cycle", d.name),
            Command::ChildNode,
        );
    }
    ui::text(
        &mut commands,
        root,
        "CRAFT — select part to edit; coordinates are blueprint metres",
        13.,
    );
    for p in &compiled.parts {
        let r = ui::row(&mut commands, root);
        button(
            &mut commands,
            r,
            &format!(
                "{}{} · {} · stage {} · ({:.2}, {:.2}, {:.2})",
                if p.instance.id == state.selected {
                    "▶ "
                } else {
                    ""
                },
                p.instance.id,
                p.definition.name,
                p.instance.stage.map_or("none".into(), |n| n.to_string()),
                p.pose.position.x,
                p.pose.position.y,
                p.pose.position.z
            ),
            Command::Select(p.instance.id.clone()),
        );
        for n in compiled
            .free_nodes()
            .iter()
            .filter(|n| n.part_id == p.instance.id)
        {
            button(
                &mut commands,
                r,
                &format!("Attach {} [size {}]", n.node.id, n.node.size),
                Command::Socket(p.instance.id.clone(), n.node.id.clone()),
            );
        }
    }
    let r = ui::row(&mut commands, root);
    for (label, action) in [
        ("Remove subtree", Command::Remove),
        ("Rotate −15°", Command::Rotate(-std::f64::consts::PI / 12.)),
        ("Rotate +15°", Command::Rotate(std::f64::consts::PI / 12.)),
        ("All module stages −", Command::Stage(-1)),
        ("All module stages +", Command::Stage(1)),
    ] {
        button(&mut commands, r, label, action);
    }
    let selected = compiled.part(&state.selected);
    for (module, stage) in
        void_assembly::default_module_stages(selected.definition, selected.instance.stage)
    {
        let stage = selected
            .instance
            .module_stages
            .get(&module)
            .copied()
            .unwrap_or(stage);
        let r = ui::row(&mut commands, root);
        ui::text(
            &mut commands,
            r,
            &format!(
                "Module {module} · stage {}",
                stage.map_or("none".into(), |n| n.to_string())
            ),
            12.,
        );
        button(
            &mut commands,
            r,
            "−",
            Command::ModuleStage(state.selected.clone(), module.clone(), -1),
        );
        button(
            &mut commands,
            r,
            "+",
            Command::ModuleStage(state.selected.clone(), module, 1),
        );
    }
    let r = ui::row(&mut commands, root);
    ui::text(
        &mut commands,
        r,
        "Surface offsets · parent-local 0.1 m; valid only on authored surface mounts",
        12.,
    );
    for (axis, name) in [(0, "X"), (1, "Y"), (2, "Z")] {
        button(
            &mut commands,
            r,
            &format!("{name}−"),
            Command::Move(axis, -0.1),
        );
        button(
            &mut commands,
            r,
            &format!("{name}+"),
            Command::Move(axis, 0.1),
        );
    }
    for (&resource, &amount) in &selected.instance.resources {
        let r = ui::row(&mut commands, root);
        ui::text(
            &mut commands,
            r,
            &format!(
                "{resource:?} {:.1}/{:.1} kg",
                amount,
                capacity(selected.definition, resource)
            ),
            12.,
        );
        button(&mut commands, r, "−10%", Command::Fuel(resource, -0.1));
        button(&mut commands, r, "+10%", Command::Fuel(resource, 0.1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_blueprint_edits_preserve_authoritative_craft() {
        let mut state = Workshop::default();
        let original = state.craft.clone();
        assert!(state.edit(Command::Remove).is_err());
        assert_eq!(state.craft, original);
        assert!(state.edit(Command::Rotate(0.5)).is_err());
        assert_eq!(state.craft, original);
        state.edit(Command::Catalog("tank-small".into())).unwrap();
        state
            .edit(Command::Socket("p1".into(), "bottom".into()))
            .unwrap();
        let built = state.craft.clone();
        assert!(
            state
                .edit(Command::Socket("p1".into(), "bottom".into()))
                .is_err()
        );
        assert_eq!(state.craft, built);
    }
    #[test]
    fn removal_and_load_keep_stable_selection_and_compile() {
        let mut state = Workshop::default();
        state.edit(Command::Demo).unwrap();
        state.selected = state.craft.parts.last().unwrap().id.clone();
        state.edit(Command::Remove).unwrap();
        assert!(state.craft.parts.iter().any(|p| p.id == state.selected));
        compile(&state.craft).unwrap();
        state.edit(Command::New).unwrap();
        assert_eq!(state.selected, "p1");
    }
    #[test]
    fn twist_is_an_explicit_version_three_edit_and_survives_roundtrip() {
        let mut state = Workshop::default();
        state.edit(Command::Demo).unwrap();
        state.selected = state
            .craft
            .parts
            .iter()
            .find(|p| p.attachment.is_some())
            .unwrap()
            .id
            .clone();
        state.edit(Command::Rotate(0.25)).unwrap();
        assert_eq!(state.craft.version, 3);
        assert_eq!(
            import_craft(&export_craft(&state.craft).unwrap()).unwrap(),
            state.craft
        );
    }
    #[test]
    fn module_stage_widget_keeps_its_target_and_rejects_stale_ids() {
        let mut state = Workshop::default();
        state.edit(Command::Demo).unwrap();
        let (part, module) = state
            .craft
            .parts
            .iter()
            .find_map(|p| {
                let stages = void_assembly::default_module_stages(
                    definition(&p.definition_id).unwrap(),
                    p.stage,
                );
                stages.keys().next().map(|m| (p.id.clone(), m.clone()))
            })
            .unwrap();
        state.selected = compile(&state.craft).unwrap().root_id;
        assert_ne!(part, state.selected);
        state
            .edit(Command::ModuleStage(part.clone(), module.clone(), 1))
            .unwrap();
        assert!(
            state
                .craft
                .parts
                .iter()
                .find(|p| p.id == part)
                .unwrap()
                .module_stages
                .contains_key(&module)
        );
        let before = state.craft.clone();
        assert!(
            state
                .edit(Command::ModuleStage(part, "missing module".into(), 1))
                .is_err()
        );
        assert_eq!(state.craft, before);
        assert!(
            state
                .edit(Command::ModuleStage("removed part".into(), module, 1))
                .is_err()
        );
        assert_eq!(state.craft, before);
    }
    #[test]
    fn module_stage_edit_preserves_other_assignments_and_resources() {
        let mut state = Workshop::default();
        state.edit(Command::Demo).unwrap();
        let p = state
            .craft
            .parts
            .iter()
            .find(|p| {
                definition(&p.definition_id)
                    .unwrap()
                    .modules
                    .iter()
                    .any(|m| matches!(m, void_assembly::Module::Engine { .. }))
            })
            .unwrap();
        state.selected = p.id.clone();
        let module = definition(&p.definition_id)
            .unwrap()
            .modules
            .iter()
            .find(|m| matches!(m, void_assembly::Module::Engine { .. }))
            .unwrap()
            .id()
            .to_string();
        let resources = p.resources.clone();
        state
            .edit(Command::ModuleStage(
                state.selected.clone(),
                module.clone(),
                1,
            ))
            .unwrap();
        let p = state
            .craft
            .parts
            .iter()
            .find(|p| p.id == state.selected)
            .unwrap();
        assert_eq!(p.resources, resources);
        assert!(p.module_stages[&module].is_some());
        compile(&state.craft).unwrap();
    }
    #[test]
    fn craft_save_is_atomic_and_refuses_overwrite() {
        let directory =
            std::env::temp_dir().join(format!("void-workshop-test-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("craft.json");
        let _ = std::fs::remove_file(&path);
        let mut state = Workshop {
            path: path.to_string_lossy().into_owned(),
            ..Default::default()
        };
        state.edit(Command::Save).unwrap();
        let saved = std::fs::read(&path).unwrap();
        state.edit(Command::Demo).unwrap();
        assert!(state.edit(Command::Save).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), saved);
        state.edit(Command::Load).unwrap();
        assert_eq!(state.craft.parts.len(), 1);
        std::fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn actual_workshop_refresh_handles_cone_rocket_rover_and_aircraft_shapes() {
        let mut app = super::super::tests::initialized_scene(true);
        app.add_systems(Update, refresh.before(super::super::draw));
        app.world_mut().resource_mut::<Workshop>().open = true;
        app.update(); // Fresh command pod is a cone, not a box.
        for preset in ["rocket", "rover", "aircraft"] {
            app.world_mut()
                .resource_mut::<Workshop>()
                .review_preset(preset)
                .unwrap();
            app.update();
            let workshop = app.world().resource::<Workshop>();
            assert_eq!(workshop.drawn, workshop.revision);
            assert!(workshop.open);
            assert!(compile(&workshop.craft).unwrap().parts.len() > 1);
            if preset == "aircraft" {
                assert!(
                    app.world_mut()
                        .query::<&Text>()
                        .iter(app.world())
                        .any(|text| text.0.contains("maximum 0.0 kN"))
                );
            }
        }
    }
    #[test]
    fn workshop_launch_preserves_existing_vessels_and_replays_edited_blueprint() {
        let mut app = super::super::tests::initialized_scene(true);
        app.insert_resource(ButtonInput::<KeyCode>::default())
            .add_message::<bevy::input::keyboard::KeyboardInput>()
            .add_systems(Update, process.before(super::super::draw));
        let existing = app
            .world()
            .non_send::<Lab>()
            .session
            .sim()
            .fleet
            .vessel_ids();
        let blueprint;
        {
            let mut workshop = app.world_mut().resource_mut::<Workshop>();
            workshop.edit(Command::Demo).unwrap();
            let engine = workshop
                .craft
                .parts
                .iter()
                .find(|p| {
                    definition(&p.definition_id)
                        .unwrap()
                        .modules
                        .iter()
                        .any(|m| matches!(m, void_assembly::Module::Engine { .. }))
                })
                .unwrap();
            workshop.selected = engine.id.clone();
            workshop.edit(Command::Stage(1)).unwrap();
            blueprint = workshop.craft.clone();
            workshop.open = true;
            workshop.queue.push(Command::Launch);
            workshop.queue.push(Command::Launch); // A queued double click must launch once.
        }
        app.update();
        let mut lab = app.world_mut().non_send_mut::<Lab>();
        let sim = lab.session.sim();
        assert_eq!(sim.fleet.vessel_ids().len(), existing.len() + 1);
        for id in existing {
            assert!(sim.fleet.vessel_ids().contains(&id));
        }
        assert_eq!(lab.craft, blueprint);
        let selected = sim.selected.clone();
        let parts = sim.fleet.part_snapshots(&selected);
        assert_eq!(parts.len(), blueprint.parts.len());
        for authored in &blueprint.parts {
            let part = parts
                .iter()
                .find(|p| p.id.ends_with(&format!("/{}", authored.id)))
                .expect("stable launched part ID");
            assert_eq!(part.resources, authored.resources);
            assert_eq!(part.stage, authored.stage);
            let mut expected_stages = void_assembly::default_module_stages(
                definition(&authored.definition_id).unwrap(),
                authored.stage,
            );
            expected_stages.extend(authored.module_stages.clone());
            assert_eq!(part.module_stages, expected_stages);
        }
        lab.session.mark();
        let recording = lab.session.recording();
        assert_eq!(
            recording
                .entries
                .iter()
                .filter(|e| matches!(e.action, Action::LaunchGroundAt { .. }))
                .count(),
            1
        );
        assert_eq!(
            void_fleet_flight::session::world_mark(FlightSession::from_recording(recording).sim()),
            void_fleet_flight::session::world_mark(lab.session.sim())
        );
    }
}
