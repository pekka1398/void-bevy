//! The LOD quadtree with the LOD lab's landing preset and camera controls, on scenery's layered
//! terrain (`--terrain layered`, the default) or a smooth sphere coloured by tile level
//! (`--terrain sphere`). Every tile is its own anchor: its f64 body-fixed origin minus the f64
//! camera position becomes the f32 translation, so vertices stay small however far the tile is
//! from the planet centre. Tiles are built in the background. No sea or atmosphere is drawn yet:
//! the layered planet's sea floor shows as dark basins.

use std::f64::consts::{FRAC_PI_2, FRAC_PI_3};
use std::sync::Arc;

use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit};
use bevy::pbr::wireframe::{WireframeConfig, WireframePlugin};
use bevy::prelude::*;
use bevy::render::RenderPlugin;
use bevy::render::render_resource::WgpuFeatures;
use bevy::render::settings::WgpuSettings;
use glam::DVec3;
use serde_json::Value;
use void_app::tiles::{Tile, TileField, anchor};
use void_lod::{LodCamera, LodView, PlanetLodOptions};
use void_terrain::{DEFAULT_LAYERED, LayeredOptions, Terrain, TerrainConfig};

const PRESETS: &str = include_str!("../../lod/presets/planets.json");
/// The lab's 60° vertical field of view.
const FOV_Y: f64 = FRAC_PI_3;

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins
                .set(RenderPlugin {
                    render_creation: WgpuSettings {
                        features: WgpuFeatures::POLYGON_MODE_LINE,
                        ..default()
                    }
                    .into(),
                    ..default()
                })
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "void · lod".into(),
                        ..default()
                    }),
                    ..default()
                }),
            WireframePlugin::default(),
        ))
        .insert_resource(ClearColor(Color::srgb(0.01, 0.01, 0.02)))
        .insert_resource(WireframeConfig {
            global: false,
            default_color: Color::BLACK,
            ..default()
        })
        .insert_resource(GlobalAmbientLight {
            brightness: 150.0,
            ..default()
        })
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (controls, finish_builds, select_and_build, draw).chain(),
        )
        .run();
}

#[derive(Resource)]
struct Planet {
    field: TileField,
    /// The preset's camera split scale, level cap and minimum observer cell pixels.
    camera_settings: (f64, u32, f64),
}

#[derive(Resource)]
struct View {
    camera: OrbitCamera,
    /// Body-fixed probe position: the observer the LOD refines around.
    probe: DVec3,
    camera_lod: bool,
    horizon_culling: bool,
}

/// The LOD lab's orbit camera (`lab/lod/src/app/OrbitCamera.ts`): a unit sub-camera direction `p`
/// with a tangent frame (`north` is screen-up looking straight down, so the poles are no
/// singularity), a pan offset, a tilt, and zoom on the distance from the planet centre.
/// Body-fixed axes with y up, as the lab.
struct OrbitCamera {
    p: DVec3,
    north: DVec3,
    offset: DVec3,
    distance: f64,
    /// 0 looks straight down at the centre; towards π/2 looks at the horizon.
    tilt: f64,
    max_distance: f64,
}

fn rotate(value: DVec3, axis: DVec3, radians: f64) -> DVec3 {
    let (s, c) = radians.sin_cos();
    value * c + axis.cross(value) * s + axis * (axis.dot(value) * (1.0 - c))
}

impl OrbitCamera {
    fn new(start: DVec3, distance: f64, max_distance: f64) -> Self {
        let p = start.normalize();
        let north = (DVec3::Y - p * DVec3::Y.dot(p)).normalize();
        Self {
            p,
            north,
            offset: DVec3::ZERO,
            distance,
            tilt: 0.0,
            max_distance,
        }
    }

    /// Position, forward and up, body-fixed.
    fn pose(&self) -> (DVec3, DVec3, DVec3) {
        let forward = -self.p * self.tilt.cos() + self.north * self.tilt.sin();
        let up = self.p * self.tilt.sin() + self.north * self.tilt.cos();
        (
            self.p * self.distance + self.offset,
            forward.normalize(),
            up.normalize(),
        )
    }

    /// Translate in the view plane; neither forward nor up rotates.
    fn pan_screen(&mut self, dx: f64, dy: f64, height: f64) {
        let (_, forward, up) = self.pose();
        let right = forward.cross(up).normalize();
        let meters = self.distance * 2.0 * (FOV_Y / 2.0).tan() / height;
        self.offset += right * (-dx * meters) + up * (dy * meters);
    }

    /// Rotate position and orientation around the planet's centre.
    fn orbit_around_center(&mut self, horizontal: f64, vertical: f64) {
        self.p = rotate(self.p, DVec3::Y, horizontal);
        self.north = rotate(self.north, DVec3::Y, horizontal);
        self.offset = rotate(self.offset, DVec3::Y, horizontal);
        let right = self.north.cross(self.p).normalize();
        self.p = rotate(self.p, right, vertical).normalize();
        self.north = rotate(self.north, right, vertical).normalize();
        self.offset = rotate(self.offset, right, vertical);
    }

    /// Turn the heading about the local vertical and change the tilt.
    fn turn(&mut self, heading: f64, tilt: f64) {
        let east = self.north.cross(self.p);
        self.north = (self.north * heading.cos() + east * heading.sin()).normalize();
        self.tilt = (self.tilt + tilt).clamp(0.0, FRAC_PI_2 - 0.01);
    }

    /// Multiplicative zoom on the distance from the centre; the pan offset scales with it, so the
    /// centre stays where it is on screen.
    fn zoom(&mut self, factor: f64) {
        let next = (self.distance * factor).min(self.max_distance);
        self.offset *= next / self.distance;
        self.distance = next;
    }
}

#[derive(Component)]
struct Hud;

#[derive(Component)]
struct Probe;

/// Tile transforms, kept apart from the camera's and the probe's.
type TileOnly = (With<Tile>, Without<Camera>, Without<Probe>);

/// The probe's name on screen, so it can be found from any distance.
#[derive(Component)]
struct ProbeLabel;

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let presets: Value = serde_json::from_str(PRESETS).expect("presets/planets.json");
    let p = &presets["landing"];
    let f = |v: &Value| v.as_f64().unwrap_or_else(|| panic!("preset value {v}"));
    let terrain = match terrain_argument().as_str() {
        "layered" => Some(Arc::new(Terrain::from_config(&TerrainConfig::Layered(
            LayeredOptions {
                radius_meters: f(&p["radiusMeters"]),
                ..DEFAULT_LAYERED
            },
        )))),
        "sphere" => None,
        other => panic!("--terrain {other}: layered or sphere"),
    };
    // The declared height range is the terrain's; the sphere keeps the preset's, so culling and
    // LOD match the lab. The LOD band covers the whole range, as the landing preset's does.
    let max_height = terrain
        .as_ref()
        .map_or(f(&p["maxSurfaceHeightMeters"]), |t| t.max_height_meters);
    let options = PlanetLodOptions {
        radius_meters: f(&p["radiusMeters"]),
        min_surface_height_meters: f(&p["minSurfaceHeightMeters"]),
        max_surface_height_meters: max_height,
        occluder_radius_meters: f(&p["occluderRadiusMeters"]),
        lod_surface_band_meters: max_height,
        resolution: f(&p["tileResolution"]) as usize,
        max_level: f(&p["maxLevel"]) as u32,
        // The lab writes Infinity, which JSON stores as null.
        split_distance_ratios: p["splitDistanceRatios"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r.as_f64().unwrap_or(f64::INFINITY))
            .collect(),
        retain_frames: 90,
        max_cached_tiles: f(&p["maxCachedTiles"]) as usize,
    };
    let c = &p["lodCamera"];
    let camera_settings = (
        f(&c["distanceScale"]),
        f(&c["maxLevel"]) as u32,
        f(&c["minObserverCellPixels"]),
    );
    let radius = options.radius_meters;
    let material = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.9,
        ..default()
    });
    commands.insert_resource(Planet {
        field: TileField::new(options, terrain, material),
        camera_settings,
    });

    // The lab's start: camera along the preset's direction, the probe off its sightline.
    let cam = &p["camera"];
    let start = &cam["initialDirection"];
    let direction = DVec3::new(f(&start[0]), f(&start[1]), f(&start[2])).normalize();
    let probe_r = radius * f(&p["probe"]["initialRadiusRadii"]);
    let theta = direction.y.acos();
    let phi = direction.z.atan2(direction.x) + f(&p["probe"]["initialPhiOffsetRadians"]);
    commands.insert_resource(View {
        camera: OrbitCamera::new(
            direction,
            radius * f(&cam["initialDistanceRadii"]),
            radius * f(&cam["maxDistanceRadii"]),
        ),
        probe: DVec3::new(
            theta.sin() * phi.cos(),
            theta.cos(),
            theta.sin() * phi.sin(),
        ) * probe_r,
        camera_lod: true,
        horizon_culling: true,
    });

    commands.spawn((
        Probe,
        Mesh3d(meshes.add(Sphere::new(1.0).mesh().uv(24, 12))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.4, 0.1),
            unlit: true,
            ..default()
        })),
        Transform::default(),
    ));
    commands.spawn((
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection {
            fov: FOV_Y as f32,
            far: 1e10,
            ..default()
        }),
        Transform::default(),
    ));
    commands.spawn((
        DirectionalLight {
            illuminance: light_consts::lux::AMBIENT_DAYLIGHT,
            ..default()
        },
        Transform::default().looking_to(Vec3::new(-0.7, -0.85, -1.0), Vec3::Y),
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
            top: px(10),
            left: px(10),
            ..default()
        },
    ));
    commands.spawn((
        ProbeLabel,
        Text::new("probe"),
        TextFont {
            font_size: FontSize::Px(14.0),
            ..default()
        },
        TextColor(Color::srgb(1.0, 0.55, 0.2)),
        Node {
            position_type: PositionType::Absolute,
            ..default()
        },
        Visibility::Hidden,
    ));
}

#[allow(clippy::too_many_arguments)]
fn controls(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    time: Res<Time>,
    window: Single<&Window>,
    mut planet: ResMut<Planet>,
    mut view: ResMut<View>,
    mut wireframe: ResMut<WireframeConfig>,
) {
    if keys.just_pressed(KeyCode::KeyV) {
        view.camera_lod = !view.camera_lod;
    }
    if keys.just_pressed(KeyCode::KeyH) {
        view.horizon_culling = !view.horizon_culling;
    }
    if keys.just_pressed(KeyCode::KeyB) {
        wireframe.global = !wireframe.global;
    }
    // As the lab's panel: , and . step the probe's minimum cell size by 0.5 px; 0 turns it off.
    for (key, step) in [(KeyCode::Comma, -0.5), (KeyCode::Period, 0.5)] {
        if keys.just_pressed(key) {
            planet.camera_settings.2 = (planet.camera_settings.2 + step).max(0.0);
        }
    }
    if keys.just_pressed(KeyCode::KeyP) {
        // Straight above the probe, 30 km higher, looking down.
        let max = view.camera.max_distance;
        view.camera = OrbitCamera::new(view.probe, view.probe.length() + 30_000.0, max);
    }

    // As the lab: right drag orbits the centre, Shift + left drag turns, left drag pans.
    let (dx, dy) = (f64::from(motion.delta.x), f64::from(motion.delta.y));
    if buttons.pressed(MouseButton::Right) {
        view.camera.orbit_around_center(-dx * 0.005, -dy * 0.005);
    } else if buttons.pressed(MouseButton::Left) && keys.pressed(KeyCode::ShiftLeft) {
        view.camera.turn(-dx * 0.005, dy * 0.005);
    } else if buttons.pressed(MouseButton::Left) {
        view.camera.pan_screen(dx, dy, f64::from(window.height()));
    }
    // The lab's wheel: exp(pixels × 1e-4), a line counting as a browser's 100 px notch.
    let pixels = match scroll.unit {
        MouseScrollUnit::Line => -f64::from(scroll.delta.y) * 100.0,
        MouseScrollUnit::Pixel => -f64::from(scroll.delta.y),
    };
    if pixels != 0.0 {
        view.camera.zoom((pixels.clamp(-500.0, 500.0) * 1e-4).exp());
    }

    // Probe: arrows move it over the ground at a speed set by its height, PageUp/PageDown climb.
    let radius = planet.field.lod.options.radius_meters;
    let dt = f64::from(time.delta_secs());
    // Heights are above the ground, so the probe follows the terrain as it moves.
    let mut altitude = view.probe.length() - radius - planet.ground(view.probe);
    let speed = (altitude.max(200.0) * 0.5).min(2e6);
    let up = view.probe.normalize();
    let east = DVec3::Y.cross(up).normalize();
    let north = up.cross(east);
    let mut step = DVec3::ZERO;
    for (key, direction) in [
        (KeyCode::ArrowUp, north),
        (KeyCode::ArrowDown, -north),
        (KeyCode::ArrowRight, east),
        (KeyCode::ArrowLeft, -east),
    ] {
        if keys.pressed(key) {
            step += direction;
        }
    }
    if keys.pressed(KeyCode::PageUp) {
        altitude += speed * dt;
    }
    if keys.pressed(KeyCode::PageDown) {
        altitude = (altitude - speed * dt).max(2.0);
    }
    let direction = (up + step * (speed * dt / radius)).normalize();
    view.probe = direction * (radius + planet.ground(direction) + altitude);
}

fn finish_builds(mut planet: ResMut<Planet>) {
    planet.field.finish_builds();
}

fn select_and_build(mut planet: ResMut<Planet>, view: Res<View>, window: Single<&Window>) {
    let (distance_scale, max_level, min_pixels) = planet.camera_settings;
    let focal_pixels = f64::from(window.height()) / (2.0 * (FOV_Y / 2.0).tan());
    let camera = view.camera_lod.then(|| LodCamera {
        position: view.camera.pose().0,
        distance_scale,
        max_level,
        focal_pixels,
        min_observer_cell_pixels: min_pixels,
    });
    planet.field.select(&LodView {
        observer_positions: vec![view.probe],
        camera,
        distance_scale: 1.0,
        horizon_culling: view.horizon_culling,
    });
}

#[allow(clippy::too_many_arguments)]
fn draw(
    mut commands: Commands,
    mut planet: ResMut<Planet>,
    view: Res<View>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut tiles: Query<&mut Transform, TileOnly>,
    mut camera: Single<(&Camera, &mut Transform, &mut Projection)>,
    mut label: Single<(&mut Node, &mut Visibility), With<ProbeLabel>>,
    mut probe: Single<&mut Transform, (With<Probe>, Without<Camera>)>,
    mut hud: Single<&mut Text, With<Hud>>,
) {
    let planet = &mut *planet;
    let radius = planet.field.lod.options.radius_meters;
    let (eye, forward, up) = view.camera.pose();
    planet
        .field
        .draw(&mut commands, &mut meshes, &mut tiles, eye);

    // The probe: sized by its distance from the camera, so it stays a few pixels across.
    let probe_altitude = view.probe.length() - radius - planet.ground(view.probe);
    let probe_size = ((view.probe - eye).length() * 0.006) as f32;
    **probe = anchor(view.probe, eye).with_scale(Vec3::splat(probe_size));

    let (camera_component, camera_transform, projection) = &mut *camera;
    **camera_transform = Transform::default().looking_to(forward.as_vec3(), up.as_vec3());
    let (label_node, label_visibility) = &mut *label;
    match camera_component.world_to_viewport(
        &GlobalTransform::from(**camera_transform),
        (view.probe - eye).as_vec3(),
    ) {
        Ok(at) => {
            label_node.left = px(at.x + 10.0);
            label_node.top = px(at.y - 8.0);
            **label_visibility = Visibility::Visible;
        }
        Err(_) => **label_visibility = Visibility::Hidden,
    }
    let camera_altitude = eye.length() - radius - planet.ground(eye);
    if let Projection::Perspective(p) = &mut **projection {
        p.near = (camera_altitude * 0.3).clamp(0.5, 1e6) as f32;
    }

    hud.0 = format!(
        "tiles {} drawn (L{}-L{}), {} requested, {} building, {} cached | select {:.2} ms\n\
         camera altitude {}   probe altitude {}\n\
         camera LOD {} (V)   horizon culling {} (H)   wireframe (B)   probe min cell {} (, .)\n\
         left drag pan | right drag orbit | Shift+left drag turn | wheel zoom | P camera over the probe\n\
         arrows move the probe | PageUp/PageDown probe height",
        planet.field.drawn_count(),
        planet.field.levels.0,
        planet.field.levels.1,
        planet.field.last_requests,
        planet.field.building_count(),
        planet.field.lod.cached_tile_count(),
        planet.field.last_select_ms,
        meters(camera_altitude),
        meters(probe_altitude),
        on(view.camera_lod),
        on(view.horizon_culling),
        if planet.camera_settings.2 > 0.0 {
            format!("{} px", planet.camera_settings.2)
        } else {
            "off".into()
        },
    );
}

fn on(flag: bool) -> &'static str {
    if flag { "on" } else { "off" }
}

fn meters(m: f64) -> String {
    if m.abs() >= 1e4 {
        format!("{:.1} km", m / 1e3)
    } else {
        format!("{m:.0} m")
    }
}

/// `--terrain NAME`, layered by default.
fn terrain_argument() -> String {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [] => "layered".into(),
        [flag, name] if flag == "--terrain" => name.clone(),
        other => panic!("unknown arguments {other:?}; use --terrain layered|sphere"),
    }
}

impl Planet {
    /// Ground height under a body-fixed point, 0 on the sphere.
    fn ground(&self, point: DVec3) -> f64 {
        self.field
            .terrain
            .as_ref()
            .map_or(0.0, |t| t.height(point.normalize()))
    }
}
