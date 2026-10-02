//! Distant-system two-vessel encounter. P pause, N fixed step, F +10 s, J join,
//! T +40 s, V 1x/4x, R reset, O origin/30,000 ly placement, 1 ships, 2 planet, 3 system, 4 cluster.
//! Drag to orbit; wheel to zoom. Render positions are split differences before f32.
use bevy::{
    input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit},
    prelude::*,
};
use glam::DVec3;
use std::{cell::RefCell, rc::Rc};
use void_assembly_lab::parts::RenderAssets;
use void_multiscale::*;
use void_multiscale_lab::Encounter;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "VOID | multiscale encounter".into(),
                resolution: (1280, 900).into(),
                ..default()
            }),
            ..default()
        }))
        .insert_resource(ClearColor(Color::BLACK))
        .insert_resource(GlobalAmbientLight {
            brightness: 400.0,
            ..default()
        })
        .insert_non_send(Lab::new(true))
        .add_systems(Startup, setup)
        .add_systems(Update, (input, simulate, rebuild, draw).chain())
        .run();
}
struct Lab {
    encounter: Encounter,
    distant: bool,
    running: bool,
    rate: f64,
    rebuild: bool,
    focus: u8,
    distance: f64,
    azimuth: f64,
    elevation: f64,
    notice: String,
}
impl Lab {
    fn new(distant: bool) -> Self {
        let galaxy = if distant {
            default_galaxy()
        } else {
            SplitPosition::at(DVec3::ZERO)
        };
        Self {
            encounter: Encounter::new(Rc::new(RefCell::new(wide_world(galaxy))), "Aster"),
            distant,
            running: false,
            rate: 1.0,
            rebuild: true,
            focus: 1,
            distance: 25.0,
            azimuth: 0.4,
            elevation: 0.25,
            notice: String::new(),
        }
    }
    fn centre(&self) -> SplitPosition {
        let e = &self.encounter;
        let world = e.world.borrow();
        let states = world.at(e.time());
        match self.focus {
            1 => states[world.system_index(&e.system)]
                .origin
                .translate(e.fleet.snapshot(&e.first).position),
            2 => world.body_position(e.planet_index, &states),
            3 | 4 => states[world.system_index(&e.system)].origin,
            _ => panic!("unknown focus"),
        }
    }
    fn nodes(&self) -> Option<(DVec3, DVec3)> {
        if self.encounter.fleet.vessel_ids().len() == 1 {
            return None;
        }
        Some((
            self.encounter
                .fleet
                .node_frame(&format!("{}/p2", self.encounter.first), "bottom")
                .0,
            self.encounter
                .fleet
                .node_frame(&format!("{}/p2", self.encounter.second), "bottom")
                .0,
        ))
    }
}
#[derive(Component)]
enum Visual {
    Part { id: String, local: Transform },
    Body(usize),
}
#[derive(Resource)]
struct BodyAssets {
    sphere: Handle<Mesh>,
    materials: Vec<Handle<StandardMaterial>>,
}
#[derive(Component)]
struct Hud;
fn setup(
    lab: NonSend<Lab>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(RenderAssets::new(&mut meshes, &mut materials));
    let sphere = meshes.add(Sphere::new(1.0).mesh().ico(4).expect("sphere mesh"));
    let body_materials = lab
        .encounter
        .world
        .borrow()
        .bodies
        .iter()
        .map(|body| {
            let colour = Srgba::hex(&body.color).expect("body colour");
            materials.add(StandardMaterial {
                base_color: colour.into(),
                unlit: body.parent_index.is_none(),
                ..default()
            })
        })
        .collect();
    commands.insert_resource(BodyAssets {
        sphere,
        materials: body_materials,
    });
    commands.spawn((
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection {
            fov: 58f32.to_radians(),
            far: 1e6,
            ..default()
        }),
        Transform::default(),
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: 12000.0,
            ..default()
        },
        Transform::default().looking_to(Vec3::new(-0.3, -0.7, -0.4), Vec3::Z),
    ));
    commands.spawn((
        Hud,
        Text::new(""),
        TextFont {
            font_size: FontSize::Px(16.0),
            ..default()
        },
        Node {
            position_type: PositionType::Absolute,
            top: px(10),
            left: px(10),
            ..default()
        },
    ));
}
fn input(
    mut lab: NonSendMut<Lab>,
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    window: Single<&Window>,
) {
    if !window.focused {
        return;
    }
    if keys.just_pressed(KeyCode::KeyR) {
        *lab = Lab::new(lab.distant);
    }
    if keys.just_pressed(KeyCode::KeyO) {
        *lab = Lab::new(!lab.distant);
    }
    if keys.just_pressed(KeyCode::KeyP) {
        lab.running = !lab.running;
    }
    if keys.just_pressed(KeyCode::KeyN) {
        lab.encounter.advance(1.0 / 60.0);
    }
    if keys.just_pressed(KeyCode::KeyF) {
        lab.encounter.advance(10.0);
    }
    if keys.just_pressed(KeyCode::KeyT) {
        lab.encounter.advance(40.0);
    }
    if keys.just_pressed(KeyCode::KeyV) {
        lab.rate = if lab.rate == 1.0 { 4.0 } else { 1.0 };
    }
    if keys.just_pressed(KeyCode::KeyJ) {
        if let Some((a, b)) = lab.nodes() {
            let distance = (a - b).length();
            if distance <= 0.25 {
                lab.encounter.join();
                lab.rebuild = true;
                lab.notice = "Joined using the actual Fleet nodes".into();
            } else {
                lab.notice = format!("Join rejected: nodes {distance:.3} m apart (limit 0.25 m)");
            }
        } else {
            lab.notice = "Already joined".into();
        }
    }
    for (key, focus, distance) in [
        (KeyCode::Digit1, 1, 25.0),
        (KeyCode::Digit2, 2, 2e7),
        (KeyCode::Digit3, 3, 4.0 * AU),
        (KeyCode::Digit4, 4, 8.0 * LIGHT_YEAR),
    ] {
        if keys.just_pressed(key) {
            lab.focus = focus;
            lab.distance = distance;
        }
    }
    if buttons.pressed(MouseButton::Left) || buttons.pressed(MouseButton::Right) {
        lab.azimuth -= motion.delta.x as f64 * 0.005;
        lab.elevation = (lab.elevation + motion.delta.y as f64 * 0.005).clamp(-1.5, 1.5);
    }
    let wheel = scroll.delta.y as f64
        * if scroll.unit == MouseScrollUnit::Line {
            100.0
        } else {
            1.0
        };
    lab.distance = (lab.distance * (-wheel * 0.001).exp()).clamp(2.0, 100.0 * LIGHT_YEAR);
}
fn simulate(mut lab: NonSendMut<Lab>, time: Res<Time>) {
    if lab.running {
        let seconds = time.delta_secs_f64() * lab.rate;
        lab.encounter.advance(seconds);
    }
}
fn rebuild(
    mut commands: Commands,
    mut lab: NonSendMut<Lab>,
    existing: Query<Entity, With<Visual>>,
    assets: Res<RenderAssets>,
    bodies: Res<BodyAssets>,
) {
    if !lab.rebuild {
        return;
    }
    for entity in &existing {
        commands.entity(entity).despawn();
    }
    for id in lab.encounter.fleet.vessel_ids() {
        for part in lab.encounter.fleet.part_snapshots(&id) {
            for piece in &assets.parts[&part.definition.id] {
                if piece.flame {
                    continue;
                }
                commands.spawn((
                    Visual::Part {
                        id: part.id.clone(),
                        local: piece.local,
                    },
                    Mesh3d(piece.mesh.clone()),
                    MeshMaterial3d(piece.material.clone()),
                    Transform::default(),
                ));
            }
        }
    }
    for i in 0..bodies.materials.len() {
        commands.spawn((
            Visual::Body(i),
            Mesh3d(bodies.sphere.clone()),
            MeshMaterial3d(bodies.materials[i].clone()),
            Transform::default(),
        ));
    }
    lab.rebuild = false;
}
fn draw(
    lab: NonSend<Lab>,
    mut camera: Single<&mut Transform, (With<Camera3d>, Without<Visual>)>,
    mut visuals: Query<(&Visual, &mut Transform), Without<Camera3d>>,
    mut hud: Single<&mut Text, With<Hud>>,
    mut gizmos: Gizmos,
) {
    let e = &lab.encounter;
    let centre = lab.centre();
    let scale = (lab.distance / 1000.0).max(1.0);
    let direction = DVec3::new(
        lab.elevation.cos() * lab.azimuth.cos(),
        lab.elevation.cos() * lab.azimuth.sin(),
        lab.elevation.sin(),
    );
    **camera = Transform::from_translation((direction * (lab.distance / scale)).as_vec3())
        .looking_at(Vec3::ZERO, Vec3::Z);
    let world = e.world.borrow();
    let states = world.at(e.time());
    let origin = states[world.system_index(&e.system)].origin;
    for (visual, mut transform) in &mut visuals {
        *transform = match visual {
            Visual::Part { id, local } => {
                let owner = e.fleet.vessel_of_part(id);
                let p = e
                    .fleet
                    .part_snapshots(&owner)
                    .into_iter()
                    .find(|p| &p.id == id)
                    .expect("rendered part exists");
                let position = origin.translate(p.position).relative(&centre);
                Transform {
                    translation: ((position + p.rotation * local.translation.as_dvec3()) / scale)
                        .as_vec3(),
                    rotation: p.rotation.as_quat() * local.rotation,
                    scale: local.scale / scale as f32,
                }
            }
            Visual::Body(i) => Transform::from_translation(
                (world.body_position(*i, &states).relative(&centre) / scale).as_vec3(),
            )
            .with_scale(Vec3::splat((world.bodies[*i].radius_meters / scale) as f32)),
        };
    }
    for (i, body) in world.bodies.iter().enumerate() {
        if body.radius_meters / scale < 1.0 {
            let p = (world.body_position(i, &states).relative(&centre) / scale).as_vec3();
            let colour = Color::from(Srgba::hex(&body.color).expect("body colour"));
            for axis in [Vec3::X, Vec3::Y, Vec3::Z] {
                gizmos.line(p - axis, p + axis, colour);
            }
        }
    }
    if let Some((a, b)) = lab.nodes() {
        let point = |p| (origin.translate(p).relative(&centre) / scale).as_vec3();
        gizmos.line(point(a), point(b), Color::srgb(0.2, 1.0, 0.5));
    }
    let mut text = format!(
        "MULTISCALE / FLEET ENCOUNTER\nPlacement: {}\nTime: {:.3} s | {}\n{} vessels | camera {:.3e} m\n",
        if lab.distant {
            "30,000 light-years out"
        } else {
            "near origin"
        },
        e.time(),
        if lab.running { "running" } else { "paused" },
        e.fleet.vessel_ids().len(),
        lab.distance
    );
    for id in e.fleet.vessel_ids() {
        let s = e.fleet.snapshot(&id);
        let relative = e.fleet.relative(&id, &e.first);
        text.push_str(&format!(
            "{id}: {:?} / scene {:?} / {:.1} kg / {:.6} m / {:.6} m/s\n",
            s.mode,
            s.scene,
            s.mass_kg,
            relative.position.length(),
            relative.velocity.length()
        ));
    }
    if let Some((a, b)) = lab.nodes() {
        text.push_str(&format!("Node distance: {:.6} m\n", (a - b).length()));
    }
    let split = e.position(&e.first);
    text.push_str(&format!("Split cells: {:?}\nOffset: {:?} m\nP pause | N step | F +10 s | T +40 s | V 1x/4x | J join | R reset | O placement\n1 ships | 2 planet | 3 system | 4 cluster\nDrag orbit | wheel zoom\n{}", split.cell, split.offset, lab.notice));
    text.push_str(&format!("\nRate: {}x", lab.rate));
    for event in e.fleet.events.iter().rev().take(6).rev() {
        text.push_str(&format!(
            "\n{:.2} s {} {:?} -> {:?}",
            event.time, event.vessel, event.from, event.to
        ));
    }
    **hud = Text::new(text);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn systems_have_disjoint_queries() {
        let mut world = World::new();
        let mut schedule = Schedule::default();
        schedule.add_systems((input, simulate, rebuild, draw).chain());
        schedule
            .initialize(&mut world)
            .expect("encounter system access");
    }
}
