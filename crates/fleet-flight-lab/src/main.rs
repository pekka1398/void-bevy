//! Integration acceptance: assembly craft + Fleet + game terrain + air, kept outside void-app.
use bevy::{
    input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll},
    pbr::wireframe::WireframePlugin,
    prelude::*,
    render::settings::{WgpuFeatures, WgpuSettings},
};
use glam::DVec3;
use std::collections::{HashMap, HashSet};
use void_app::{
    flight::game_planet_by_id,
    overlay::unique_edges,
    tiles::{Tile, TileField},
};
use void_assembly::{Craft, Module, demo_craft, import_craft};
use void_assembly_lab::parts::RenderAssets;
use void_fleet_flight::FleetFlight;
use void_landing::{FrameState, PlanetFrame, demo_rocket, landing_lod_options};
use void_lod::{LodCamera, LodView};
use void_vessels::{VesselControl, nearby_site};

const RATES: [f64; 6] = [1.0, 2.0, 4.0, 20.0, 100.0, 1000.0];
struct Lab {
    sim: FleetFlight,
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
struct Ground(TileField);
#[derive(Component)]
struct LabCamera;
#[derive(Component)]
struct Hud;
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
fn main() {
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
    let sim = FleetFlight::new(planet.planet, &craft, site, air);
    App::new()
        .add_plugins((
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "VOID Fleet flight integration".into(),
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
        .insert_resource(ClearColor(Color::srgb(0.02, 0.025, 0.04)))
        .insert_resource(GlobalAmbientLight {
            brightness: 100.0,
            ..default()
        })
        .insert_non_send(new_lab(sim, craft))
        .add_systems(Startup, setup)
        .add_systems(Update, (controls, simulate, draw).chain())
        .run();
}
fn new_lab(sim: FleetFlight, craft: Craft) -> Lab {
    Lab {
        sim,
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
) {
    let assets = RenderAssets::new(&mut meshes, &mut materials);
    commands.insert_resource(assets);
    let demo = demo_rocket(&lab.sim.planet.terrain);
    let material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.32, 0.42, 0.28),
        perceptual_roughness: 1.0,
        ..default()
    });
    commands.insert_resource(Ground(TileField::new(
        landing_lod_options(&lab.sim.planet.terrain, &demo.options.contact),
        Some(lab.sim.planet.terrain.clone()),
        material,
    )));
    commands.spawn((Camera3d::default(), Transform::default(), LabCamera));
    commands.spawn((
        DirectionalLight {
            illuminance: 8000.0,
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
    mut lab: NonSendMut<Lab>,
) {
    let lab = &mut *lab;
    if !window.focused {
        lab.sim.control(VesselControl {
            turn: DVec3::ZERO,
            ..lab.sim.fleet.control(&lab.sim.selected)
        });
        return;
    }
    if keys.just_pressed(KeyCode::KeyP) {
        lab.paused = !lab.paused;
    }
    if keys.just_pressed(KeyCode::KeyR) {
        let air = lab.sim.planet.air_density_scale.is_some()
            && !std::env::args().any(|a| a == "--vacuum");
        lab.sim = FleetFlight::new(lab.sim.planet.clone(), &lab.craft, lab.sim.launch_site, air);
        lab.dirty = true;
        lab.prediction = None;
        lab.paused = true;
        lab.rate = 0;
        lab.spawned = 0;
        lab.notice.clear();
    }
    if keys.just_pressed(KeyCode::Tab) {
        let old = lab.sim.selected.clone();
        let ids = lab.sim.fleet.vessel_ids();
        let i = ids
            .iter()
            .position(|id| *id == old)
            .expect("selected vessel");
        let mut c = lab.sim.fleet.control(&old);
        c.turn = DVec3::ZERO;
        lab.sim.fleet.set_control(&old, c);
        lab.sim.select(&ids[(i + 1) % ids.len()]);
        lab.prediction = None;
    }
    if keys.just_pressed(KeyCode::KeyO) {
        let id = lab.sim.launch_orbital(&lab.craft, DVec3::ZERO);
        lab.sim.select(&id);
        lab.distance = 40.0;
    }
    if keys.just_pressed(KeyCode::KeyN) {
        lab.spawned += 1;
        let site = nearby_site(
            lab.sim.launch_site,
            30.0 * lab.spawned as f64,
            lab.sim.planet.terrain.radius_meters,
        );
        lab.sim.fleet.launch_landed(&lab.craft, lab.sim.home, site);
        lab.sim.fleet.advance(0.0);
    }
    let id = lab.sim.selected.clone();
    let commanded = lab.sim.fleet.part_snapshots(&id).iter().any(|p| {
        p.definition
            .modules
            .iter()
            .any(|m| matches!(m, Module::Command))
    });
    if keys.just_pressed(KeyCode::KeyT) && commanded {
        lab.sim
            .sas(lab.sim.fleet.sas_phase(&id) == void_vessels::SasPhase::Off);
    }
    let mut c = lab.sim.fleet.control(&id);
    let dt = time.delta_secs_f64().min(0.05);
    let throttle_axis = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]) as i32
        - keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]) as i32;
    c.throttle = (c.throttle + f64::from(throttle_axis) * dt * 0.5).clamp(0.0, 1.0);
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
    lab.sim.control(c);
    if keys.just_pressed(KeyCode::Space) {
        lab.sim.stage();
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
        lab.prediction = Some(lab.sim.predict(600.0));
    }
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
    if buttons.pressed(MouseButton::Left) {
        lab.yaw -= f64::from(motion.delta.x) * 0.006;
        lab.pitch = (lab.pitch + f64::from(motion.delta.y) * 0.006).clamp(-1.5, 1.5);
    }
    lab.distance = (lab.distance * (-f64::from(scroll.delta.y) * 0.12).exp()).clamp(2.0, 2e8);
}
fn simulate(time: Res<Time>, window: Single<&Window>, mut lab: NonSendMut<Lab>) {
    if lab.paused || !window.focused {
        return;
    }
    let rate = RATES[lab.rate];
    match lab
        .sim
        .advance(time.delta_secs_f64().min(0.05) * rate, rate > 4.0)
    {
        Ok(true) => {}
        Ok(false) => {
            lab.rate = 0;
            lab.notice = "Rails stopped at an encounter or ground band".into();
        }
        Err(reason) => {
            lab.rate = 0;
            lab.notice = format!("Warp refused: {reason}");
        }
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
    mut camera: Single<&mut Transform, With<LabCamera>>,
    mut hud: Single<&mut Text, With<Hud>>,
    window: Single<&Window>,
    mut gizmos: Gizmos,
) {
    let lab = &mut *lab;
    if lab.dirty {
        for (_, entities) in lab.parts.drain() {
            for e in entities {
                commands.entity(e).despawn();
            }
        }
        lab.collision.clear();
        lab.dirty = false;
    }
    let f = &lab.sim.fleet;
    let frame = PlanetFrame::new(&f.ephemeris, lab.sim.home);
    let a = void_orbit::body_orientation(&frame.body.rotation, f.time());
    let q = glam::DQuat::from_mat3(&glam::DMat3::from_cols(a[0], a[1], a[2])).normalize();
    let selected = f.snapshot(&lab.sim.selected);
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
    let eye = state.position + direction * lab.distance;
    **camera = Transform::default().looking_to((state.position - eye).as_vec3(), up.as_vec3());
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
    ground.0.finish_builds();
    let max_level = ground.0.lod.options.max_level;
    ground.0.select(&LodView {
        camera: Some(LodCamera {
            position: eye,
            distance_scale: 1.0,
            max_level,
            focal_pixels: f64::from(window.height()) / (2.0 * (std::f64::consts::PI / 8.0).tan()),
            min_observer_cell_pixels: 3.0,
        }),
        observer_positions: observers,
        distance_scale: 1.0,
        horizon_culling: true,
    });
    ground.0.set_wireframe(&mut commands, lab.wire);
    ground.0.draw(&mut commands, &mut meshes, &mut tiles, eye);
    for mut v in &mut tile_visibility {
        *v = if lab.terrain {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
    if lab.bounds {
        for line in ground.0.boundaries(eye) {
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
    if let Some(prediction) = &lab.prediction {
        gizmos.linestrip(
            prediction.points.iter().map(|(_, p)| (*p - eye).as_vec3()),
            Color::srgb(0.2, 0.9, 1.0),
        );
    }
    let p = f.thrust(&lab.sim.selected);
    **hud = Text::new(format!(
        "ASSEMBLY / FLEET FLIGHT INTEGRATION\n{} | {:?} | {} | {}x\nT+{:.2}s AGL {:.1}m surface {:.1}m/s mass {:.1}kg\nthrottle {:.0}% force {:.1}kN flow {:.2}kg/s SAS {:?}\n{} vessels | ground {} bubble {} | collision tiles {}\nP pause | Space stage | Shift/Ctrl throttle | X cut | WASD QE turn | T SAS\nTab vessel | N nearby ground craft | O orbital craft | R reset\n, . warp | C vacuum prediction (600s snapshot)\nF2 wire | F3 boundaries | F4 collision terrain | F5 terrain\n{}",
        lab.sim.selected,
        selected.mode,
        if lab.paused { "paused" } else { "running" },
        RATES[lab.rate],
        f.time(),
        f.clearance(&lab.sim.selected, lab.sim.home),
        state.velocity.length(),
        selected.mass_kg,
        f.control(&lab.sim.selected).throttle * 100.0,
        p.force.length() / 1000.0,
        p.flow_kg_per_second,
        f.sas_phase(&lab.sim.selected),
        f.vessel_ids().len(),
        f.ground_count(),
        f.bubble_count(),
        lab.collision.len(),
        lab.notice
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn integration_scene_initializes_and_draws_without_a_window_or_renderer() {
        let planet = game_planet_by_id("pebble", None);
        let craft = demo_craft();
        let site = demo_rocket(&planet.planet.terrain).launch_site.normalize();
        let sim = FleetFlight::new(planet.planet, &craft, site, false);
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()))
            .insert_resource(Assets::<Mesh>::default())
            .insert_resource(Assets::<bevy::mesh::skinning::SkinnedMeshInverseBindposes>::default())
            .insert_resource(Assets::<StandardMaterial>::default())
            .insert_non_send(new_lab(sim, craft))
            .add_plugins(bevy::gizmos::GizmoPlugin)
            .add_systems(Startup, setup)
            .add_systems(Update, draw);
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
    }
}
