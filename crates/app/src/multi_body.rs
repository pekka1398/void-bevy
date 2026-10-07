//! Independent two-body acceptance scene. Uses the production Fleet, world and scenery shaders.
use crate::{
    air::{AirLayers, AirSettings, AirTextures},
    scenery::{
        GroundMaterial, GroundUniforms, SceneryPlugin, StarMaterial, star_mesh, table_image,
    },
    tiles::{Tile, TileField},
};
use bevy::{
    input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll},
    pbr::wireframe::WireframePlugin,
    prelude::*,
};
use glam::DVec3;
use std::collections::HashMap;
use void_assembly_lab::parts::RenderAssets;
use void_fleet_flight::{
    presentation::{Toggle, ViewCommand},
    session::{Action, FlightSession, InitialWorld, Outcome, Playback, Recording},
};
const EXPOSURE: f32 = 6.309_573;
#[derive(Component)]
struct Camera;
#[derive(Component)]
struct Sun;
#[derive(Component)]
struct Hud;
#[derive(Component)]
struct FarBody(usize, AssetId<Mesh>, AssetId<StandardMaterial>, bool);
#[derive(Component)]
struct StarSky;
#[derive(Component)]
struct Piece {
    id: String,
    local: Transform,
    flame: bool,
}
struct BodyScene {
    field: TileField<GroundMaterial>,
    material: Handle<GroundMaterial>,
    color: Option<[f32; 3]>,
}
struct AtmosphericBody {
    top_radius: f64,
    air: AirSettings,
    textures: AirTextures,
}
struct Appearance {
    terrain: std::sync::Arc<void_terrain::Terrain>,
    color: Option<[f32; 3]>,
}
impl void_lod::SurfaceSampler for Appearance {
    fn sample(&self, direction: DVec3, cell: f64) -> void_lod::SurfaceSample {
        let (height, color) = self.terrain.sample(direction, Some(cell));
        void_lod::SurfaceSample {
            height_meters: height,
            color: self.color.unwrap_or(color.map(|v| v as f32)),
        }
    }
}
struct Scene {
    session: FlightSession,
    playback: Option<Playback>,
    craft: void_assembly::Craft,
    bodies: HashMap<usize, BodyScene>,
    atmospheres: HashMap<usize, AtmosphericBody>,
    render_world: serde_json::Value,
    terrain_identity: std::collections::BTreeMap<usize, std::sync::Arc<void_terrain::Terrain>>,
    owned_images: Vec<AssetId<Image>>,
    parts: HashMap<String, (String, Vec<Entity>)>,
    active: usize,
    generation: u64,
    refresh: bool,
    paused: bool,
    rate: usize,
    frames: usize,
    recording: bool,
    notices: String,
}
fn arg(name: &str) -> Option<String> {
    let args: Vec<_> = std::env::args().collect();
    args.iter()
        .position(|a| a == name)
        .map(|i| args.get(i + 1).expect("argument value").clone())
}
fn solar_mode() -> bool {
    std::env::args().any(|a| a == "--solar")
        || std::env::args()
            .next()
            .expect("executable")
            .contains("solar_scenery")
}
fn selene_terrain(sim: &void_fleet_flight::FleetFlight) -> Option<usize> {
    sim.world.bodies.get("selene")?.terrain.as_ref()?;
    Some(sim.world.body_index("selene"))
}
fn preset(s: &mut FlightSession, id: &str, view: &str) {
    let body = s.sim().world.body_index(id);
    assert!(
        s.sim().world.bodies.contains_key(id),
        "body has no authored scenery"
    );
    let fleet = &s.sim().fleet;
    let radius = fleet.ephemeris.bodies()[body].radius_meters;
    let surface = fleet.body_frames(body).1;
    let direction = if id == "sol" {
        DVec3::new(1.0, 0.2, 0.3).normalize()
    } else {
        let sun = illuminated_site(s.sim(), body);
        let east = if sun.z.abs() < 0.99 {
            DVec3::Z.cross(sun).normalize()
        } else {
            DVec3::X.cross(sun).normalize()
        };
        (sun + east * 0.7 + DVec3::Z * 0.15).normalize()
    };
    let direction = fleet
        .frames()
        .transform(surface, fleet.origin_frame())
        .apply_direction(direction);
    let distance = radius
        * match view {
            "near" => 1.025,
            "orbit" => {
                if id == "halo" {
                    6.0
                } else {
                    3.5
                }
            }
            "far" => 12.0,
            _ => panic!("view must be near/orbit/far"),
        };
    s.execute(Action::View {
        command: ViewCommand::BodyPreset {
            body,
            direction,
            distance,
        },
    });
}
fn initial() -> InitialWorld {
    if let Some(path) = arg("--world") {
        return serde_json::from_reader(std::fs::File::open(path).expect("open authored world"))
            .expect("parse authored InitialWorld");
    }
    let mut initial = initial_with_atmospheres(std::env::args().any(|a| a == "--two-atmospheres"));
    if solar_mode() {
        initial.world = void_fleet_flight::world::solar_scenery(
            &crate::flight::game_planet_by_id("aurelia", Some("layered")).planet,
        );
    }
    initial
}
fn initial_with_atmospheres(two: bool) -> InitialWorld {
    let p = crate::flight::game_planet_by_id("aurelia", Some("layered"));
    let mut world = void_fleet_flight::world::aurelia_selene(&p.planet);
    if two {
        // Deliberately fictional optical/physical fixture, never a claim that Selene has air.
        let moon = world.bodies.get_mut("selene").expect("moon");
        moon.label = "SELENE · TWO-ATMOSPHERE TEST FIXTURE".into();
        moon.air_density_scale = Some(0.02);
        moon.visual.atmosphere = true;
        moon.visual.scattering = Some(void_scenery::atmosphere_scene::AtmosphereProfile::Custom {
            height_meters: 50000.0,
            rayleigh_scattering: [8e-6, 2e-6, 1e-6],
            rayleigh_scale_height: 7000.0,
            mie_scattering: 1e-6,
            mie_extinction: 1.2e-6,
            mie_scale_height: 2000.0,
            mie_anisotropy: 0.6,
            ozone_absorption: [0.0; 3],
            ozone_center_height: 0.0,
            ozone_width: 1.0,
        });
    }
    InitialWorld {
        world,
        launch_body: "aurelia".into(),
        craft: void_assembly::flight_rocket(),
        launch_site: p.launch_site.expect("layered launch site"),
    }
}
fn fixture() -> FlightSession {
    let i = initial();
    let mut s = FlightSession::new(i.clone());
    let moon = i.world.body_index("selene");
    let site = illuminated_site(s.sim(), moon);
    s.execute(Action::LaunchGroundAt {
        body: "selene".into(),
        craft: i.craft,
        site,
    });
    s.execute(Action::View {
        command: ViewCommand::Configure { main_camera: true },
    });
    if solar_mode() {
        preset(
            &mut s,
            &arg("--body").unwrap_or_else(|| "selene".into()),
            &arg("--view").unwrap_or_else(|| "orbit".into()),
        );
    }
    s.execute(Action::EndFrame {
        paused: true,
        rate: 0,
    });
    s
}
/// Acceptance fixtures need visible terrain; choose the subsolar point in the body's own frame.
fn illuminated_site(sim: &void_fleet_flight::FleetFlight, body: usize) -> DVec3 {
    let fleet = &sim.fleet;
    let sun = fleet
        .ephemeris
        .bodies()
        .iter()
        .find(|b| b.parent_index.is_none())
        .expect("sun");
    fleet
        .frames()
        .transform(fleet.body_frames(sun.index).0, fleet.body_frames(body).1)
        .apply_point(DVec3::ZERO)
        .normalize()
}
pub fn run() {
    if let Some(path) = arg("--export-world") {
        let config = initial();
        config.world.build();
        serde_json::to_writer_pretty(
            std::fs::File::create(path).expect("create world config"),
            &config,
        )
        .expect("write world config");
        println!("authored world exported");
        return;
    }
    if let Some(path) = arg("--verify-save") {
        FlightSession::load_checkpoint(path);
        println!("multi-body checkpoint verified");
        return;
    }
    if let Some(path) = arg("--verify") {
        FlightSession::load(path);
        println!("multi-body recording verified");
        return;
    }
    let (session, playback) = if let Some(path) = arg("--replay") {
        assert!(
            arg("--record").is_none() && arg("--load").is_none(),
            "replay is exclusive"
        );
        let (p, s) = Playback::new(Recording::read(path));
        (s, Some(p))
    } else {
        (
            arg("--load").map_or_else(fixture, FlightSession::load_checkpoint),
            None,
        )
    };
    let craft = session.recording_initial().craft.clone();
    let active = session.sim().observation_body();
    let paused = session.sim().presentation.paused;
    let rate = session.sim().presentation.rate;
    let mut scene = Scene {
        session,
        playback,
        craft,
        bodies: HashMap::new(),
        atmospheres: HashMap::new(),
        parts: HashMap::new(),
        active,
        terrain_identity: std::collections::BTreeMap::new(),
        render_world: serde_json::Value::Null,
        owned_images: vec![],
        generation: 0,
        refresh: true,
        paused,
        rate,
        frames: 0,
        recording: false,
        notices: String::new(),
    };
    if let Some(path) = arg("--record") {
        scene.session.begin_stream(path);
        scene.recording = true;
    }
    App::new()
        .add_plugins((
            DefaultPlugins
                .set(bevy::render::RenderPlugin {
                    render_creation: bevy::render::settings::WgpuSettings {
                        features: bevy::render::settings::WgpuFeatures::POLYGON_MODE_LINE,
                        ..default()
                    }
                    .into(),
                    ..default()
                })
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: if solar_mode() {
                            "VOID · solar scenery".into()
                        } else {
                            "VOID · multi-body world".into()
                        },
                        ..default()
                    }),
                    ..default()
                }),
            WireframePlugin::default(),
            SceneryPlugin,
        ))
        .insert_non_send(scene)
        .insert_resource(ClearColor(Color::BLACK))
        .add_systems(Startup, setup)
        .add_systems(Update, (controls, draw).chain())
        .run();
}
impl Drop for Scene {
    fn drop(&mut self) {
        if self.recording && !std::thread::panicking() {
            self.session.finish_stream();
        }
    }
}
#[allow(clippy::too_many_arguments)]
fn setup(
    mut commands: Commands,
    mut scene: NonSendMut<Scene>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut standard: ResMut<Assets<StandardMaterial>>,
    mut grounds: ResMut<Assets<GroundMaterial>>,
    mut stars: ResMut<Assets<StarMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    commands.insert_resource(RenderAssets::new(&mut meshes, &mut standard));
    let sim = scene.session.sim();
    let (scenes, atmospheres, owned_images) =
        build_scenes(&mut commands, sim, &mut grounds, &mut images);
    spawn_far(&mut commands, sim, &mut meshes, &mut standard);
    let (p, c) = void_scenery::generate_stars(&void_scenery::DEFAULT_STARS);
    commands.spawn((
        StarSky,
        Mesh3d(meshes.add(star_mesh(p, &c))),
        MeshMaterial3d(stars.add(StarMaterial { brightness: 0.08 })),
        Transform::default(),
        bevy::camera::visibility::NoFrustumCulling,
    ));
    let air = preview_air(sim, scene.active);
    commands.spawn((
        Camera,
        Camera3d {
            depth_texture_usages: (bevy::render::render_resource::TextureUsages::RENDER_ATTACHMENT
                | bevy::render::render_resource::TextureUsages::TEXTURE_BINDING)
                .into(),
            ..default()
        },
        air,
        AirLayers(vec![]),
        bevy::camera::Hdr,
        Msaa::Off,
        bevy::core_pipeline::tonemapping::Tonemapping::None,
        bevy::core_pipeline::tonemapping::DebandDither::Disabled,
        Projection::Perspective(PerspectiveProjection {
            far: 1e14,
            fov: 58_f32.to_radians(),
            ..default()
        }),
        Transform::default(),
    ));
    commands.spawn((
        Sun,
        DirectionalLight {
            illuminance: 1000.0,
            ..default()
        },
        Transform::default(),
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
    scene.terrain_identity = scene.session.sim().terrains.clone();
    scene.render_world = serde_json::to_value(&scene.session.sim().world).unwrap();
    scene.owned_images = owned_images;
    scene.bodies = scenes;
    scene.atmospheres = atmospheres;
}
type BuiltScenes = (
    HashMap<usize, BodyScene>,
    HashMap<usize, AtmosphericBody>,
    Vec<AssetId<Image>>,
);
fn build_scenes(
    commands: &mut Commands,
    sim: &void_fleet_flight::FleetFlight,
    grounds: &mut Assets<GroundMaterial>,
    images: &mut Assets<Image>,
) -> BuiltScenes {
    use void_scenery::atmosphere::*;
    use void_scenery::clouds::*;
    use void_scenery::tables::*;
    let world = sim.world.clone();
    let before = images
        .iter()
        .map(|(id, _)| id)
        .collect::<std::collections::HashSet<_>>();
    let mut scenes = HashMap::new();
    let mut atmospheres = HashMap::new();
    // Shared deterministic noise assets; per-body coverage/optics remain independent.
    let weather = images.add(crate::air::weather_image(
        build_cloud_weather(2),
        WEATHER_WIDTH,
        WEATHER_HEIGHT,
    ));
    let shape = images.add(crate::air::noise_volume_image(
        build_cloud_noise(SHAPE_SIZE, false),
        SHAPE_SIZE,
    ));
    let detail = images.add(crate::air::noise_volume_image(
        build_cloud_noise(DETAIL_SIZE, true),
        DETAIL_SIZE,
    ));
    let mut resolve_textures = None;
    for (id, d) in &world.bodies {
        let body = world.body_index(id);
        let radius = sim.fleet.ephemeris.bodies()[body].radius_meters + d.air_datum_meters;
        let params = if let Some(profile) = &d.visual.scattering {
            profile.parameters(radius)
        } else {
            // Explicit vacuum tables for mandatory ground/resolve bindings, never Earth air.
            let mut p = void_scenery::earth_like_atmosphere(radius);
            p.rayleigh_scattering = [0.0; 3];
            p.ozone_absorption = [0.0; 3];
            p.mie_scattering = 0.0;
            p.mie_extinction = 0.0;
            p
        };
        let trans = build_transmittance_table(&params);
        let multiple = build_multiple_scattering_table(&params, &trans, 64, 20);
        let irradiance = build_irradiance_table(&params, &trans, &multiple, 128, 24);
        let trans = images.add(table_image(
            &trans,
            TRANSMITTANCE_WIDTH,
            TRANSMITTANCE_HEIGHT,
        ));
        let irradiance = images.add(table_image(
            &irradiance,
            IRRADIANCE_WIDTH,
            IRRADIANCE_HEIGHT,
        ));
        let textures = AirTextures {
            transmittance: trans.clone(),
            irradiance: irradiance.clone(),
            multiple: images.add(table_image(
                &multiple,
                MULTIPLE_SCATTERING_SIZE,
                MULTIPLE_SCATTERING_SIZE,
            )),
            weather: weather.clone(),
            shape: shape.clone(),
            detail: detail.clone(),
        };
        if resolve_textures.is_none() {
            resolve_textures = Some(textures.clone());
        }
        let mut air = AirSettings::new(&params);
        air.enabled = f32::from(u8::from(d.visual.atmosphere));
        air.clouds_enabled = f32::from(u8::from(d.visual.clouds));
        if let Some(clouds) = &d.visual.cloud_profile {
            air.cloud_bottom = clouds.bottom_meters as f32;
            air.cloud_top = clouds.top_meters as f32;
            air.cloud_extinction = clouds.extinction_per_meter as f32;
            air.coverage = clouds.coverage as f32;
        }
        air.exposure = 1.0;
        air.tone_mapping = crate::air::ToneMapping::None as u8 as f32;
        air.sun_disc_enabled = 0.0;
        air.sea_level = (d.visual.color_datum_meters - d.air_datum_meters) as f32;
        if d.visual.atmosphere {
            atmospheres.insert(
                body,
                AtmosphericBody {
                    top_radius: params.top_radius,
                    air,
                    textures,
                },
            );
        }
        if d.terrain.is_none() {
            continue;
        }
        let terrain = sim.terrains[&body].clone();
        let mut uniforms = GroundUniforms::new(
            &params,
            d.visual.color_datum_meters,
            d.visual.rock_height_meters,
            d.visual.snow_height_meters,
        );
        uniforms.ocean_enabled = f32::from(u8::from(d.visual.ocean));
        uniforms.atmosphere_enabled = f32::from(u8::from(d.visual.atmosphere));
        let material = grounds.add(GroundMaterial {
            ground: uniforms,
            transmittance: trans,
            irradiance,
        });
        let demo = void_landing::demo_rocket(&terrain);
        let mut field = TileField::new(
            void_landing::landing_lod_options(&terrain, &demo.options.contact),
            Some(std::sync::Arc::new(Appearance {
                terrain,
                color: d.visual.surface_color,
            })),
            material.clone(),
        );
        field.no_frustum_culling = true;
        scenes.insert(
            body,
            BodyScene {
                field,
                material,
                color: d.visual.surface_color,
            },
        );
    }
    commands.insert_resource(resolve_textures.expect("multi-body lab needs a configured body"));
    let owned = images
        .iter()
        .map(|(id, _)| id)
        .filter(|id| !before.contains(id))
        .collect();
    (scenes, atmospheres, owned)
}
fn preview_air(sim: &void_fleet_flight::FleetFlight, body: usize) -> AirSettings {
    let id = &sim.fleet.ephemeris.bodies()[body].id;
    let visual = &sim.world.bodies[id].visual;
    let radius = sim.fleet.ephemeris.bodies()[body].radius_meters;
    let mut air = if let Some(profile) = &visual.scattering {
        AirSettings::new(&profile.parameters(radius + sim.world.bodies[id].air_datum_meters))
    } else {
        // Mandatory resolve uniforms, explicitly disabled vacuum.
        AirSettings::new(&void_scenery::earth_like_atmosphere(radius))
    };
    air.enabled = 0.0; // Optical transport comes exclusively from AirLayers.
    air.clouds_enabled = 0.0;
    air.exposure = arg("--exposure").map_or(EXPOSURE, |v| {
        let exposure: f32 = v.parse().expect("numeric exposure");
        assert!(
            exposure.is_finite() && exposure > 0.0,
            "positive finite exposure"
        );
        exposure
    });
    air.tone_mapping = crate::air::ToneMapping::AcesFilmic as u8 as f32;
    air.sun_disc_enabled = 0.0;
    air
}
fn spawn_far(
    commands: &mut Commands,
    sim: &void_fleet_flight::FleetFlight,
    meshes: &mut Assets<Mesh>,
    standard: &mut Assets<StandardMaterial>,
) {
    use void_scenery::solar::SurfaceRecipe;
    for body in sim.fleet.ephemeris.bodies() {
        let descriptor = sim.world.bodies.get(&body.id);
        let mut mesh = Sphere::new(1.0).mesh().uv(256, 128);
        let mut positions = match mesh
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .expect("sphere positions")
        {
            bevy::mesh::VertexAttributeValues::Float32x3(p) => p.clone(),
            _ => panic!("sphere position format"),
        };
        // Bevy sphere uses y poles; body-fixed spin axis is z.
        for p in &mut positions {
            *p = [p[0], p[2], -p[1]];
        }
        let colors = positions
            .iter_mut()
            .map(|p| {
                let d = DVec3::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2])).normalize();
                if let Some(descriptor) = descriptor {
                    match &descriptor.visual.surface {
                        SurfaceRecipe::SolidSurface => {
                            let terrain = &sim.terrains[&body.index];
                            let (h, c) = terrain.sample(d, None);
                            *p = (d * (1.0 + h / body.radius_meters)).as_vec3().to_array();
                            let c = descriptor
                                .visual
                                .surface_color
                                .unwrap_or(c.map(|v| v as f32));
                            [c[0], c[1], c[2], 1.0]
                        }
                        recipe => recipe.color(d),
                    }
                } else {
                    // System bodies without authored scenery keep the pre-existing map sphere.
                    let c = crate::map::color(&body.color).to_linear();
                    [c.red, c.green, c.blue, 1.0]
                }
            })
            .collect::<Vec<_>>();
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, colors);
        mesh.compute_smooth_normals();
        let is_star = descriptor
            .is_some_and(|d| matches!(d.visual.surface, SurfaceRecipe::EmissiveStar { .. }))
            || body.parent_index.is_none();
        let material = StandardMaterial {
            base_color: Color::WHITE,
            unlit: is_star,
            perceptual_roughness: 1.0,
            cull_mode: None,
            ..default()
        };
        let mesh = meshes.add(mesh);
        let material = standard.add(material);
        commands.spawn((
            FarBody(body.index, mesh.id(), material.id(), false),
            Mesh3d(mesh),
            MeshMaterial3d(material),
            Transform::default(),
        ));
        if descriptor
            .is_some_and(|d| matches!(d.visual.surface, SurfaceRecipe::EmissiveStar { .. }))
        {
            // Thin optically transparent emission shells: a visual corona, no solid/collider.
            for layer in 0..48 {
                let radius = 1.006 + f32::from(layer as u8) * 0.006;
                let mesh = meshes.add(Sphere::new(radius).mesh().uv(64, 32));
                let alpha = 0.004 * (1.0 - f32::from(layer as u8) / 48.0);
                let material = standard.add(StandardMaterial {
                    base_color: Color::linear_rgba(1.0, 0.48, 0.12, alpha),
                    unlit: true,
                    alpha_mode: AlphaMode::Add,
                    ..default()
                });
                commands.spawn((
                    FarBody(body.index, mesh.id(), material.id(), true),
                    Mesh3d(mesh),
                    MeshMaterial3d(material),
                    Transform::default(),
                ));
            }
        }
        if let Some(ring) = descriptor.and_then(|d| d.visual.rings.as_ref()) {
            let mesh = meshes.add(crate::solar_mesh::rings(ring));
            let material = standard.add(StandardMaterial {
                base_color: Color::WHITE,
                alpha_mode: AlphaMode::Blend,
                cull_mode: None,
                perceptual_roughness: 1.0,
                ..default()
            });
            commands.spawn((
                FarBody(body.index, mesh.id(), material.id(), true),
                Mesh3d(mesh),
                MeshMaterial3d(material),
                Transform::default(),
            ));
        }
    }
}
fn controls(
    mut scene: NonSendMut<Scene>,
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    buttons: Res<ButtonInput<MouseButton>>,
) {
    if let Some(mut p) = scene.playback.take() {
        if !p.next_frame(&mut scene.session) {
            scene.notices = "Replay complete".into();
            scene.paused = true;
        } else {
            scene.playback = Some(p);
        }
        return;
    }
    if keys.just_pressed(KeyCode::Tab) {
        let ids = scene.session.sim().fleet.vessel_ids();
        let n = ids
            .iter()
            .position(|i| *i == scene.session.sim().selected)
            .expect("selected");
        let vessel = ids[(n + 1) % ids.len()].clone();
        scene.session.execute(Action::Select { vessel });
        scene.session.execute(Action::View {
            command: ViewCommand::Focus { body: None },
        });
    }
    if solar_mode() {
        let ids: Vec<_> = scene.session.sim().world.bodies.keys().cloned().collect();
        let current = &scene.session.sim().fleet.ephemeris.bodies()
            [scene.session.sim().observation_body()]
        .id;
        let index = ids
            .iter()
            .position(|id| id == current)
            .expect("configured focus");
        if keys.just_pressed(KeyCode::BracketRight) || keys.just_pressed(KeyCode::BracketLeft) {
            let offset = if keys.just_pressed(KeyCode::BracketRight) {
                1
            } else {
                ids.len() - 1
            };
            preset(
                &mut scene.session,
                &ids[(index + offset) % ids.len()],
                "orbit",
            );
        }
        for (key, view) in [
            (KeyCode::Digit7, "near"),
            (KeyCode::Digit8, "orbit"),
            (KeyCode::Digit9, "far"),
        ] {
            if keys.just_pressed(key) {
                preset(&mut scene.session, &ids[index], view);
            }
        }
    }
    for (slot, key) in [KeyCode::Digit1, KeyCode::Digit2].into_iter().enumerate() {
        if keys.just_pressed(key) {
            let body = if slot == 0 {
                scene.session.sim().home
            } else if let Some(body) = selene_terrain(scene.session.sim()) {
                body
            } else {
                scene.notices = "Selene terrain is not configured".into();
                continue;
            };
            scene.session.execute(Action::View {
                command: ViewCommand::Focus { body: Some(body) },
            });
        }
    }
    if keys.just_pressed(KeyCode::Home) {
        scene.session.execute(Action::View {
            command: ViewCommand::Focus { body: None },
        });
    }
    if keys.just_pressed(KeyCode::KeyP) {
        scene.paused = !scene.paused;
    }
    if keys.just_pressed(KeyCode::Comma) {
        scene.rate = scene.rate.saturating_sub(1);
    }
    if keys.just_pressed(KeyCode::Period) {
        scene.rate = (scene.rate + 1).min(8);
    }
    if keys.just_pressed(KeyCode::KeyR) {
        let s = fixture();
        let base = void_fleet_flight::checkpoint::FlightCheckpoint::capture(
            s.sim(),
            s.recording_initial().clone(),
        );
        scene.session.execute(Action::LoadWorld {
            checkpoint: Box::new(base),
        });
        scene.refresh = true;
        scene.generation += 1;
        scene.paused = scene.session.sim().presentation.paused;
        scene.rate = scene.session.sim().presentation.rate;
        scene.craft = scene.session.recording_initial().craft.clone();
    }
    if keys.just_pressed(KeyCode::F6) {
        let (paused, rate) = (scene.paused, scene.rate);
        scene.session.execute(Action::EndFrame { paused, rate });
        scene
            .session
            .save_checkpoint(arg("--save").unwrap_or("lab-log/multi-body-save.json".into()));
        scene.notices = "Saved complete world and view".into();
    }
    if keys.just_pressed(KeyCode::F7) {
        let base = void_fleet_flight::checkpoint::FlightCheckpoint::read(
            arg("--save").unwrap_or("lab-log/multi-body-save.json".into()),
        );
        scene.session.execute(Action::LoadWorld {
            checkpoint: Box::new(base),
        });
        scene.refresh = true;
        scene.generation += 1;
        scene.paused = scene.session.sim().presentation.paused;
        scene.rate = scene.session.sim().presentation.rate;
        scene.craft = scene.session.recording_initial().craft.clone();
    }
    if keys.just_pressed(KeyCode::F8) && scene.recording {
        scene.session.finish_stream();
        scene.recording = false;
        scene.notices = "Recording stopped".into();
    }
    if keys.just_pressed(KeyCode::KeyT) {
        let enabled = scene
            .session
            .sim()
            .fleet
            .sas_phase(&scene.session.sim().selected)
            == void_vessels::SasPhase::Off;
        scene.session.execute(Action::Sas { enabled });
    }
    if keys.just_pressed(KeyCode::Space) {
        scene.session.execute(Action::Stage);
    }
    if keys.just_pressed(KeyCode::KeyM) {
        let sim = scene.session.sim();
        let body = sim.nearby_body(&sim.selected);
        let spec = void_orbit::ManeuverSpec {
            start_time: sim.fleet.time() + 120.0,
            prograde: 20.0,
            normal: 0.0,
            radial: 0.0,
            reference_body: body,
            reference_mode: void_orbit::ReferenceMode::Fixed,
        };
        scene.session.execute(Action::AddManeuver { spec });
    }
    if keys.just_pressed(KeyCode::KeyB) {
        let out = scene.session.execute(Action::ExecuteManeuver);
        scene.notices = format!("{out:?}");
    }
    if keys.just_pressed(KeyCode::KeyZ) {
        let out = scene.session.execute(Action::BeginManeuverWarp);
        scene.notices = format!("{out:?}");
    }
    if keys.just_pressed(KeyCode::KeyO) {
        let body = scene.session.sim().fleet.ephemeris.bodies()
            [scene.session.sim().observation_body()]
        .id
        .clone();
        let craft = scene.craft.clone();
        let Outcome::Spawned(vessel) = scene.session.execute(Action::LaunchOrbitAt {
            body,
            craft,
            offset: DVec3::ZERO,
        }) else {
            panic!("spawn orbit")
        };
        scene.session.execute(Action::Select { vessel });
        scene.session.execute(Action::View {
            command: ViewCommand::Focus { body: None },
        });
    }
    if keys.just_pressed(KeyCode::KeyL) || keys.just_pressed(KeyCode::KeyI) {
        let Some(body) = selene_terrain(scene.session.sim()) else {
            scene.notices = "Selene descent requires configured Selene terrain".into();
            return;
        };
        let body_id = scene.session.sim().fleet.ephemeris.bodies()[body]
            .id
            .clone();
        let craft = scene.craft.clone();
        let terrain = &scene.session.sim().terrains[&body];
        let site = illuminated_site(scene.session.sim(), body);
        let height = if keys.just_pressed(KeyCode::KeyI) {
            20_000.0
        } else {
            2_000.0
        };
        let local = void_landing::FrameState {
            position: site * (terrain.radius_meters + terrain.height(site) + height),
            velocity: -site * 20.0,
        };
        let Outcome::Spawned(vessel) = scene.session.execute(Action::LaunchFlightAt {
            body: body_id,
            craft,
            position: local.position,
            velocity: local.velocity,
        }) else {
            panic!("descent fixture")
        };
        scene.session.execute(Action::Select { vessel });
        scene.session.execute(Action::View {
            command: ViewCommand::Focus { body: None },
        });
    }
    for (key, setting) in [
        (KeyCode::F2, Toggle::Wire),
        (KeyCode::F3, Toggle::Bounds),
        (KeyCode::F4, Toggle::Colliders),
        (KeyCode::F5, Toggle::Terrain),
    ] {
        if keys.just_pressed(key) {
            scene.session.execute(Action::View {
                command: ViewCommand::Toggle { setting },
            });
        }
    }
    if buttons.pressed(MouseButton::Left) {
        scene.session.execute(Action::View {
            command: ViewCommand::Drag {
                x: f64::from(motion.delta.x),
                y: f64::from(motion.delta.y),
            },
        });
    }
    if scroll.delta.y != 0.0 {
        scene.session.execute(Action::View {
            command: ViewCommand::Zoom {
                pixels: f64::from(scroll.delta.y) * 40.0,
            },
        });
    }
    let axis = |p, n| f64::from(keys.pressed(p) as u8) - f64::from(keys.pressed(n) as u8);
    let c = scene
        .session
        .sim()
        .fleet
        .control(&scene.session.sim().selected);
    let throttle = (c.throttle
        + axis(KeyCode::ShiftLeft, KeyCode::ControlLeft) * time.delta_secs_f64() * 0.4)
        .clamp(0.0, 1.0);
    let turn = DVec3::new(
        axis(KeyCode::KeyS, KeyCode::KeyW),
        axis(KeyCode::KeyE, KeyCode::KeyQ),
        axis(KeyCode::KeyD, KeyCode::KeyA),
    );
    if throttle != c.throttle || turn != c.turn {
        scene.session.execute(Action::Control { throttle, turn });
    }
    if !scene.paused {
        let rate = crate::flight::TIME_RATES[scene.rate];
        let seconds = time.delta_secs_f64().min(0.05) * rate;
        if let Outcome::Refused(reason) = scene.session.execute(Action::Advance {
            seconds,
            rails: rate > 4.0,
        }) {
            scene.notices = reason;
            scene.rate = 0;
        }
    }
    let (paused, rate) = (scene.paused, scene.rate);
    scene.session.execute(Action::EndFrame { paused, rate });
    scene.frames += 1;
    if scene.recording && scene.frames.is_multiple_of(60) {
        scene.session.mark();
    }
}
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn draw(
    mut commands: Commands,
    mut scene: NonSendMut<Scene>,
    assets: Res<RenderAssets>,
    gpu: (
        ResMut<Assets<Mesh>>,
        ResMut<Assets<GroundMaterial>>,
        ResMut<Assets<Image>>,
        ResMut<Assets<StandardMaterial>>,
    ),
    mut camera: Single<
        (
            &mut Transform,
            &mut AirSettings,
            &Projection,
            &mut AirLayers,
        ),
        With<Camera>,
    >,
    mut tiles: Query<
        &mut Visibility,
        (
            With<Tile>,
            Without<Piece>,
            Without<Camera>,
            Without<FarBody>,
            Without<Sun>,
        ),
    >,
    mut tile_transform: Query<
        &mut Transform,
        (
            With<Tile>,
            Without<Piece>,
            Without<Camera>,
            Without<FarBody>,
            Without<Sun>,
        ),
    >,
    mut pieces: Query<
        (&Piece, &mut Transform, &mut Visibility),
        (
            Without<Tile>,
            Without<Camera>,
            Without<FarBody>,
            Without<Sun>,
        ),
    >,
    mut far: Query<
        (Entity, &FarBody, &mut Transform, &mut Visibility),
        (Without<Tile>, Without<Piece>, Without<Camera>, Without<Sun>),
    >,
    mut light: Single<
        &mut Transform,
        (
            With<Sun>,
            Without<Camera>,
            Without<Tile>,
            Without<Piece>,
            Without<FarBody>,
        ),
    >,
    mut sky: Single<
        &mut Transform,
        (
            With<StarSky>,
            Without<Camera>,
            Without<Tile>,
            Without<Piece>,
            Without<FarBody>,
            Without<Sun>,
        ),
    >,
    mut hud: Single<&mut Text, With<Hud>>,
    window: Single<&Window>,
    mut gizmos: Gizmos,
) {
    let (mut meshes, mut grounds, mut images, mut standard) = gpu;
    let world = serde_json::to_value(&scene.session.sim().world).unwrap();
    if world != scene.render_world {
        for (_, (_, entities)) in scene.parts.drain() {
            for entity in entities {
                commands.entity(entity).despawn();
            }
        }
        for body in scene.bodies.values_mut() {
            body.field.unload(&mut commands, &mut meshes);
            grounds.remove(body.material.id());
        }
        for id in scene.owned_images.drain(..) {
            images.remove(id);
        }
        for (entity, body, _, _) in &mut far {
            commands.entity(entity).despawn();
            meshes.remove(body.1);
            standard.remove(body.2);
        }
        let (bodies, atmospheres, images_owned) = build_scenes(
            &mut commands,
            scene.session.sim(),
            &mut grounds,
            &mut images,
        );
        spawn_far(
            &mut commands,
            scene.session.sim(),
            &mut meshes,
            &mut standard,
        );
        scene.bodies = bodies;
        scene.atmospheres = atmospheres;
        scene.owned_images = images_owned;
        scene.render_world = world;
        scene.refresh = true;
        scene.generation += 1;
        return;
    }
    if scene.session.sim().terrains.iter().any(|(id, t)| {
        !scene
            .terrain_identity
            .get(id)
            .is_some_and(|old| std::sync::Arc::ptr_eq(t, old))
    }) {
        for (_, (_, entities)) in scene.parts.drain() {
            for entity in entities {
                commands.entity(entity).despawn();
            }
        }
        scene.refresh = true;
        scene.generation += 1;
        scene.terrain_identity = scene.session.sim().terrains.clone();
    }
    let target = scene.session.sim().observation_body();
    assert!(
        scene
            .session
            .sim()
            .world
            .bodies
            .contains_key(&scene.session.sim().fleet.ephemeris.bodies()[target].id),
        "unconfigured observation body"
    );
    if target != scene.active || scene.refresh {
        for body in scene.bodies.values_mut() {
            body.field.unload(&mut commands, &mut meshes);
        }
        // Fresh fields own fresh pending Tasks. Old completions cannot be accepted by this generation.
        let sim = scene.session.sim();
        let terrains = sim.terrains.clone();
        for (&body, s) in &mut scene.bodies {
            let terrain = terrains[&body].clone();
            let demo = void_landing::demo_rocket(&terrain);
            s.field = TileField::new(
                void_landing::landing_lod_options(&terrain, &demo.options.contact),
                Some(std::sync::Arc::new(Appearance {
                    terrain,
                    color: s.color,
                })),
                s.material.clone(),
            );
            s.field.no_frustum_culling = true;
        }
        scene.active = target;
        scene.refresh = false;
    }
    let sim = scene.session.sim();
    let fleet = &sim.fleet;
    let p = sim.presentation.clone();
    let sample = p.sample(sim);
    let body = scene.active;
    let surface = fleet.body_frames(body).1;
    let q = fleet
        .frames()
        .transform(surface, fleet.origin_frame())
        .rotation();
    let eye = fleet
        .frames()
        .transform(sample.focus_frame, surface)
        .apply_point(sample.camera(fleet, q).translation);
    let focus = sample
        .to_camera(fleet, sample.focus_frame, q)
        .apply_point(sample.focus_local);
    let (camera_transform, air, projection, layers) = &mut *camera;
    **camera_transform = Transform::default()
        .looking_to(focus.as_vec3(), (q.conjugate() * sample.view.up).as_vec3());
    let root = fleet
        .ephemeris
        .bodies()
        .iter()
        .find(|b| b.parent_index.is_none())
        .expect("sun");
    let sun = fleet
        .frames()
        .transform(fleet.body_frames(root.index).0, surface)
        .apply_point(DVec3::ZERO)
        - eye;
    assert!(
        sun.is_finite() && sun.length_squared() > 0.0,
        "invalid sun geometry"
    );
    let sun = sun.normalize();
    **light = Transform::default().looking_to(
        (-sun).as_vec3(),
        if sun.z.abs() < 0.99 { Vec3::Z } else { Vec3::Y },
    );
    **sky = Transform::from_rotation(q.conjugate().as_quat());
    let snapshots = fleet
        .vessel_ids()
        .iter()
        .flat_map(|id| fleet.part_snapshots(id))
        .collect::<Vec<_>>();
    for (_, b, mut t, mut v) in &mut far {
        let into = sample.to_camera(fleet, fleet.body_frames(b.0).1, q);
        *t = Transform::from_translation(into.apply_point(DVec3::ZERO).as_vec3())
            .with_rotation(into.rotation().as_quat())
            .with_scale(Vec3::splat(
                fleet.ephemeris.bodies()[b.0].radius_meters as f32,
            ));
        *v = if b.0 == body
            && !b.3
            && scene
                .bodies
                .get(&body)
                .is_some_and(|s| s.field.drawn_count() > 0)
        {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
    }
    let ground = scene.bodies.get(&body);
    let mut setting = preview_air(sim, body);
    let Projection::Perspective(projection) = projection else {
        panic!("perspective")
    };
    setting.update(
        eye,
        camera_transform.rotation,
        projection,
        f64::from(window.physical_height()) / (2.0 * (f64::from(projection.fov) / 2.0).tan()),
        sun,
    );
    **air = setting;
    use void_scenery::atmosphere_scene::{AtmosphereVolume, ordered_volumes};
    let volumes = scene
        .atmospheres
        .iter()
        .map(|(&id, a)| AtmosphereVolume {
            body: id,
            center: sample
                .to_camera(fleet, fleet.body_frames(id).1, q)
                .apply_point(DVec3::ZERO),
            radius: a.top_radius,
        })
        .collect::<Vec<_>>();
    let order = ordered_volumes(&volumes);
    layers.0 = order
        .into_iter()
        .map(|i| {
            let id = volumes[i].body;
            let a = &scene.atmospheres[&id];
            let into = sample.to_camera(fleet, fleet.body_frames(id).1, q);
            let turn = into.rotation().conjugate();
            let local_eye = -(turn * volumes[i].center);
            let local_sun = fleet
                .frames()
                .transform(fleet.body_frames(root.index).0, fleet.body_frames(id).1)
                .apply_point(DVec3::ZERO)
                - local_eye;
            assert!(
                local_sun.is_finite() && local_sun.length_squared() > 0.0,
                "invalid body sun geometry"
            );
            let mut air = a.air;
            air.update(
                local_eye,
                (turn.as_quat() * camera_transform.rotation).normalize(),
                projection,
                f64::from(window.physical_height())
                    / (2.0 * (f64::from(projection.fov) / 2.0).tan()),
                local_sun.normalize(),
            );
            (air, a.textures.clone())
        })
        .collect();

    if let Some(ground) = ground {
        let mut uniforms = grounds.get(&ground.material).expect("material").ground;
        crate::scenery::update_ground(&mut uniforms, eye, sun, fleet.time());
        grounds.get_mut(&ground.material).expect("material").ground = uniforms;
    }
    let ship = fleet.snapshot(&sim.selected);
    let selected = sim.selected.clone();
    let launch = sim.planet.body_id.clone();
    let body_name = fleet.ephemeris.bodies()[body].id.clone();
    let navigation = sim.nearby_body(&selected);
    let local = fleet.body_fixed_state(&selected, navigation);
    let agl = local.position.length()
        - fleet.ephemeris.bodies()[navigation].radius_meters
        - sim.terrains[&navigation].height(local.position.normalize());
    let ground_name = fleet.ephemeris.bodies()[navigation].id.clone();
    let navigation_name = fleet.ephemeris.bodies()[sim.navigation_body(&selected)]
        .id
        .clone();
    let world_time = fleet.time();
    let collider_count = fleet.terrain_tiles().len();
    let scene_count = fleet.ground_count();
    let mut collider_edges = vec![];
    if p.colliders {
        for tile in fleet.terrain_tiles() {
            let (vertices, triangles) = fleet.terrain_geometry(tile.scene, &tile.tile);
            let into = sample.to_camera(fleet, tile.frame, q);
            for indices in crate::overlay::unique_edges(&triangles).as_chunks::<2>().0 {
                let at = |i: u32| {
                    into.apply_point(
                        tile.local_position + Vec3::from_array(vertices[i as usize]).as_dvec3(),
                    )
                    .as_vec3()
                };
                collider_edges.push((at(indices[0]), at(indices[1])));
            }
        }
        for collider in fleet.vessel_collider_meshes() {
            let into = sample.to_camera(fleet, collider.frame, q);
            for indices in crate::overlay::unique_edges(&collider.mesh.triangles)
                .as_chunks::<2>()
                .0
            {
                let at = |i: u32| {
                    into.apply_point(
                        Vec3::from_array(collider.mesh.vertices[i as usize]).as_dvec3(),
                    )
                    .as_vec3()
                };
                collider_edges.push((at(indices[0]), at(indices[1])));
            }
        }
    }
    let into_parts = snapshots
        .iter()
        .map(|part| (part.id.clone(), sample.to_camera(fleet, part.frame, q)))
        .collect::<HashMap<_, _>>();
    for (visual, mut transform, mut visibility) in &mut pieces {
        if let Some(part) = snapshots.iter().find(|part| part.id == visual.id) {
            let into = into_parts[&part.id];
            *transform =
                Transform::from_translation(into.apply_point(part.local_position).as_vec3())
                    .with_rotation((into.rotation() * part.local_rotation).as_quat())
                    .mul_transform(visual.local);
            *visibility = if visual.flame && !part.firing {
                Visibility::Hidden
            } else {
                Visibility::Inherited
            };
        }
    }
    let live = snapshots
        .iter()
        .map(|p| (p.id.clone(), p.definition.id.to_string()))
        .collect();
    prune_parts(&mut commands, &mut scene.parts, &live);
    for part in &snapshots {
        if !scene.parts.contains_key(&part.id) {
            let into = into_parts[&part.id];
            let root = Transform::from_translation(into.apply_point(part.local_position).as_vec3())
                .with_rotation((into.rotation() * part.local_rotation).as_quat());
            let entities = assets.parts[&part.definition.id]
                .iter()
                .map(|piece| {
                    commands
                        .spawn((
                            Piece {
                                id: part.id.clone(),
                                local: piece.local,
                                flame: piece.flame,
                            },
                            Mesh3d(piece.mesh.clone()),
                            MeshMaterial3d(piece.material.clone()),
                            root.mul_transform(piece.local),
                            if piece.flame && !part.firing {
                                Visibility::Hidden
                            } else {
                                Visibility::Inherited
                            },
                        ))
                        .id()
                })
                .collect();
            scene
                .parts
                .insert(part.id.clone(), (part.definition.id.to_string(), entities));
        }
    }
    for (a, b) in collider_edges {
        gizmos.line(a, b, Color::srgb(0.2, 1.0, 0.4));
    }
    let (owned, pending, cache) = if let Some(body_scene) = scene.bodies.get_mut(&body) {
        body_scene.field.finish_builds();
        body_scene.field.select(&void_lod::LodView {
            camera: Some(void_lod::LodCamera {
                position: eye,
                distance_scale: 1.0,
                max_level: body_scene.field.lod.options.max_level,
                focal_pixels: f64::from(window.physical_height())
                    / (2.0 * (f64::from(projection.fov) / 2.0).tan()),
                min_observer_cell_pixels: 3.0,
            }),
            observer_positions: vec![eye],
            distance_scale: 1.0,
            horizon_culling: true,
        });
        body_scene.field.set_wireframe(&mut commands, p.wire);
        body_scene
            .field
            .draw(&mut commands, &mut meshes, &mut tile_transform, eye);
        if p.bounds {
            for line in body_scene.field.boundaries(eye) {
                gizmos.linestrip(line, Color::srgb(1.0, 0.2, 0.2));
            }
        }
        for mut v in &mut tiles {
            *v = if p.terrain {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            };
        }
        let owned = body_scene.field.owned_mesh_count();
        let pending = body_scene.field.building_count();
        let cache = body_scene.field.lod.cached_mesh_bytes();
        (owned, pending, cache)
    } else {
        (0, 0, 0)
    };
    let radius = scene.session.sim().fleet.ephemeris.bodies()[body].radius_meters;
    let camera_height = eye.length() - radius;
    let descriptor = &scene.session.sim().world.bodies[&body_name];
    let recipe = match descriptor.visual.surface {
        void_scenery::solar::SurfaceRecipe::SolidSurface => "solid terrain",
        void_scenery::solar::SurfaceRecipe::GasEnvelope { .. } => "gas cloud top (no collider)",
        void_scenery::solar::SurfaceRecipe::EmissiveStar { .. } => "emissive star (no collider)",
    };
    let optical = descriptor.visual.atmosphere;
    let physical_air = descriptor.air_density_scale.is_some();
    let exposure = setting.exposure;
    hud.0 = format!(
        "SCENERY WORLD · T+{world_time:.2}s\n{recipe} · radius {radius:.0}m · camera height {camera_height:.0}m · exposure {exposure:.3} · optical air {optical} / physical air {physical_air}\nlaunch {launch} · observation {body_name} · navigation {navigation_name} · selected {selected} {:?}\nAGL {agl:.1}m on {ground_name} · mass {:.1}kg · {} ground owners · {collider_count} collider tiles\nscene generation {} · 1 active / {} configured bodies · {owned} terrain meshes · {pending} pending · {} MB cache · {} app meshes / {} materials / {} textures\n[ / ] solar body · 7/8/9 near/orbit/far · Tab switch ship · 1/2 focus planets · Home ship · O orbit at observed body · L Selene descent · I transfer approach · T SAS · Space stage\nM add +20m/s maneuver · B execute · Z approach warp\nShift/Ctrl throttle · WASDQE attitude · P pause · ,/. warp · drag/scroll camera\nF2 wire F3 boundaries F4 colliders F5 terrain F6 save F7 load F8 stop recording · R reset\n{}",
        ship.mode,
        ship.mass_kg,
        scene_count,
        scene.generation,
        scene.session.sim().world.bodies.len(),
        cache / 1_000_000,
        meshes.len(),
        grounds.len() + standard.len(),
        images.len(),
        scene.notices
    );
}

fn prune_parts(
    commands: &mut Commands,
    parts: &mut HashMap<String, (String, Vec<Entity>)>,
    live: &HashMap<String, String>,
) {
    parts.retain(|id, (definition, entities)| {
        if live.get(id) == Some(definition) {
            return true;
        }
        for entity in entities {
            commands.entity(*entity).despawn();
        }
        false
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::{IntoSystem, System};
    #[test]
    fn startup_builds_finite_tables_for_mixed_and_airless_worlds() {
        for airless in [false, true] {
            let mut config = initial_with_atmospheres(false);
            if airless {
                for body in config.world.bodies.values_mut() {
                    body.air_density_scale = None;
                    body.visual.scattering = None;
                    body.visual.cloud_profile = None;
                    body.visual.atmosphere = false;
                    body.visual.clouds = false;
                }
            }
            let session = FlightSession::new(config);
            let mut world = World::new();
            let mut queue = bevy::ecs::world::CommandQueue::default();
            let mut materials = Assets::<GroundMaterial>::default();
            let mut images = Assets::<Image>::default();
            let (scenes, atmospheres, _) = build_scenes(
                &mut Commands::new(&mut queue, &world),
                session.sim(),
                &mut materials,
                &mut images,
            );
            assert_eq!(scenes.len(), 2);
            assert_eq!(atmospheres.len(), if airless { 0 } else { 1 });
            queue.apply(&mut world);
            assert!(world.contains_resource::<AirTextures>());
        }
    }
    #[test]
    fn multiple_atmospheres_own_distinct_tables_even_without_surface_terrain() {
        for moon_terrain in [true, false] {
            let mut config = initial_with_atmospheres(true);
            if !moon_terrain {
                let moon = config.world.bodies.get_mut("selene").unwrap();
                moon.terrain = None;
                moon.visual.surface = void_scenery::solar::SurfaceRecipe::GasEnvelope {
                    low: [0.2; 3],
                    high: [0.4; 3],
                    bands: 8.0,
                    turbulence: 0.0,
                    storm: 0.0,
                };
            }
            let session = FlightSession::new(config);
            let mut world = World::new();
            let mut queue = bevy::ecs::world::CommandQueue::default();
            let mut materials = Assets::<GroundMaterial>::default();
            let mut images = Assets::<Image>::default();
            let (scenes, atmospheres, _) = build_scenes(
                &mut Commands::new(&mut queue, &world),
                session.sim(),
                &mut materials,
                &mut images,
            );
            assert_eq!(atmospheres.len(), 2);
            assert_eq!(scenes.len(), if moon_terrain { 2 } else { 1 });
            let mut a = atmospheres.values();
            let (first, second) = (a.next().unwrap(), a.next().unwrap());
            assert_ne!(first.textures.transmittance, second.textures.transmittance);
            assert_ne!(
                first.air.rayleigh_scattering,
                second.air.rayleigh_scattering
            );
            assert_eq!(
                first.textures.shape, second.textures.shape,
                "shared immutable noise"
            );
            assert!(
                atmospheres
                    .values()
                    .all(|a| a.air.exposure == 1.0 && a.air.tone_mapping == 3.0)
            );
            assert!(scenes.keys().all(|&id| {
                let air = preview_air(session.sim(), id);
                air.enabled == 0.0 && air.clouds_enabled == 0.0
            }));
            queue.apply(&mut world);
        }
    }
    #[test]
    fn loading_a_new_definition_under_the_same_id_removes_old_visuals() {
        let mut world = World::new();
        let old = world.spawn_empty().id();
        let kept = world.spawn_empty().id();
        let mut parts = HashMap::from([
            ("p1".into(), ("old".into(), vec![old])),
            ("p2".into(), ("same".into(), vec![kept])),
        ]);
        let live = HashMap::from([("p1".into(), "new".into()), ("p2".into(), "same".into())]);
        let mut queue = bevy::ecs::world::CommandQueue::default();
        prune_parts(&mut Commands::new(&mut queue, &world), &mut parts, &live);
        queue.apply(&mut world);
        assert!(world.get_entity(old).is_err());
        assert!(world.get_entity(kept).is_ok());
        assert!(!parts.contains_key("p1"));
        assert!(parts.contains_key("p2"));
    }
    #[test]
    fn scene_system_queries_are_disjoint() {
        let mut world = World::new();
        let mut system = IntoSystem::into_system(draw);
        system.initialize(&mut world);
    }
    #[test]
    fn moon_shortcuts_use_stable_identity_in_the_solar_world() {
        let mut config = initial_with_atmospheres(false);
        config.world = void_fleet_flight::world::solar_scenery(
            &crate::flight::game_planet_by_id("aurelia", Some("layered")).planet,
        );
        let session = FlightSession::new(config.clone());
        let moon = selene_terrain(session.sim()).expect("authored moon");
        assert_eq!(session.sim().fleet.ephemeris.bodies()[moon].id, "selene");
        assert_ne!(
            session
                .sim()
                .terrains
                .keys()
                .copied()
                .find(|body| *body != session.sim().home),
            Some(moon)
        );
        config.world.bodies.remove("selene");
        let session = FlightSession::new(config);
        assert_eq!(selene_terrain(session.sim()), None);
    }
    #[test]
    fn solar_visual_rebuild_releases_all_owned_assets() {
        let mut initial = initial_with_atmospheres(false);
        initial.world = void_fleet_flight::world::solar_scenery(
            &crate::flight::game_planet_by_id("aurelia", Some("layered")).planet,
        );
        let session = FlightSession::new(initial);
        let mut world = World::new();
        let mut meshes = Assets::<Mesh>::default();
        let mut standard = Assets::<StandardMaterial>::default();
        let mut grounds = Assets::<GroundMaterial>::default();
        let mut images = Assets::<Image>::default();
        let mut baseline = None;
        for _ in 0..3 {
            let mut queue = bevy::ecs::world::CommandQueue::default();
            let mut commands = Commands::new(&mut queue, &world);
            let (scenes, atmospheres, owned) =
                build_scenes(&mut commands, session.sim(), &mut grounds, &mut images);
            spawn_far(&mut commands, session.sim(), &mut meshes, &mut standard);
            queue.apply(&mut world);
            let counts = (meshes.len(), standard.len(), grounds.len(), images.len());
            if let Some(expected) = baseline {
                assert_eq!(counts, expected);
            } else {
                baseline = Some(counts);
            }
            assert_eq!(scenes.len(), 5);
            assert_eq!(atmospheres.len(), 3);
            let owned_far: Vec<_> = world
                .query::<(Entity, &FarBody)>()
                .iter(&world)
                .map(|(e, b)| (e, b.1, b.2))
                .collect();
            for (entity, mesh, material) in owned_far {
                world.despawn(entity);
                meshes.remove(mesh);
                standard.remove(material);
            }
            for scene in scenes.values() {
                grounds.remove(scene.material.id());
            }
            for image in owned {
                images.remove(image);
            }
            world.remove_resource::<AirTextures>();
            drop(scenes);
            drop(atmospheres);
            assert_eq!(
                (meshes.len(), standard.len(), grounds.len(), images.len()),
                (0, 0, 0, 0)
            );
        }
    }
}
