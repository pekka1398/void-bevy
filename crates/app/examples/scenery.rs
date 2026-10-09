//! The scenery lab's page in Bevy: lab/lod's tiles in the lab's ground and sea shader, the star
//! field, the air and volumetric clouds integrated together over the scene, the sun's disc, the
//! orbit view from the ground to 200,000 km, and the lab's exposure and tone mappings (three.js's
//! ACES filmic, AgX and Neutral).
//!
//! `--terrain layered|lod|hills` (the lab's `?terrain=`), `--at LAT,LON` in degrees (`?at=`), and
//! `--preset ground|sunset|night|cloud|plane|orbit|space` to start from a preset, `--tone
//! aces|agx|neutral`.
//!
//! Mouse as the lab: left drag pans, right drag orbits the planet centre, Shift + left drag turns,
//! the wheel zooms. Keys stand in for the panel: 1–7 presets (ground, sunset, night, cloud layer,
//! 10 km, 400 km, 20,000 km) · `,` `.` local time · R time rate · `[` `]` sun declination ·
//! `-` `=` sea level (hold) · Z X exposure · T tone mapping · K L cloud coverage · A atmosphere · M multi-scatter ·
//! C clouds · W weather only · O ocean · S stars.

use std::f64::consts::{FRAC_PI_2, PI};
use std::sync::Arc;

use bevy::camera::Hdr;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::core_pipeline::tonemapping::{DebandDither, Tonemapping};
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit};
use bevy::prelude::*;
use bevy::render::render_resource::TextureUsages;
use bevy::render::view::Msaa;
use bevy::window::PrimaryWindow;
use glam::DVec3;
use void_app::air::{AirSettings, AirTextures, ToneMapping, noise_volume_image, weather_image};
use void_app::scenery::{
    GroundMaterial, GroundUniforms, SceneryPlugin, StarMaterial, star_mesh, table_image,
    update_ground,
};
use void_app::tiles::{Tile, TileField};
use void_lod::{
    DemoTerrain, HOLMAN_SPLIT_DISTANCE_RATIOS, LodCamera, LodView, PlanetLodOptions, SurfaceSampler,
};
use void_scenery::atmosphere::{
    TRANSMITTANCE_HEIGHT, TRANSMITTANCE_WIDTH, build_transmittance_table,
};
use void_scenery::clouds::{
    DETAIL_SIZE, SHAPE_SIZE, WEATHER_HEIGHT, WEATHER_WIDTH, build_cloud_noise, build_cloud_weather,
};
use void_scenery::tables::{
    IRRADIANCE_HEIGHT, IRRADIANCE_WIDTH, MULTIPLE_SCATTERING_SIZE, build_irradiance_table,
    build_multiple_scattering_table,
};
use void_scenery::{DEFAULT_STARS, OrbitView, earth_like_atmosphere, generate_stars};
use void_terrain::{DEFAULT_LAYERED, MAX_HEIGHT, SEA_LEVEL, Terrain, TerrainConfig};

const DEG: f64 = PI / 180.0;
const FOV_DEGREES: f64 = 60.0;
/// The lab's time-rate choices, hours per second: stopped, 1 min/s, 15 min/s, 2 h/s.
const RATES: [(f64, &str); 4] = [
    (0.0, "stopped"),
    (0.02, "1 min/s"),
    (0.25, "15 min/s"),
    (2.0, "2 h/s"),
];

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins.set(WindowPlugin {
                primary_window: Some(Window {
                    title: "void · scenery".into(),
                    ..default()
                }),
                ..default()
            }),
            SceneryPlugin,
        ))
        .insert_resource(ClearColor(Color::BLACK))
        .add_systems(Startup, setup)
        .add_systems(Update, (controls, frame).chain())
        .run();
}

/// A terrain the lab can show (Terrains.ts): its sampler, bounds, sea level and rock and snow heights.
struct SceneryTerrain {
    label: String,
    sampler: Arc<dyn SurfaceSampler + Send + Sync>,
    radius_meters: f64,
    max_height_meters: f64,
    default_sea_level: f64,
    rock_height: f64,
    snow_height: f64,
}

impl SceneryTerrain {
    /// Full-detail height under the camera: a 1 m cell is the layered planet's full detail, and the
    /// hills and lab/lod's continents ignore the cell.
    fn height(&self, direction: DVec3) -> f64 {
        self.sampler.sample(direction, 1.0).height_meters
    }
}

fn scenery_terrain(id: &str) -> SceneryTerrain {
    match id {
        // This lab's layered planet: continents, mountain belts, eroded hills down to metres.
        "layered" => SceneryTerrain {
            label: "scenery | layered".into(),
            sampler: Arc::new(Terrain::from_config(&TerrainConfig::Layered(
                DEFAULT_LAYERED,
            ))),
            radius_meters: DEFAULT_LAYERED.radius_meters,
            max_height_meters: MAX_HEIGHT,
            default_sea_level: SEA_LEVEL,
            rock_height: SEA_LEVEL + 2600.0,
            snow_height: SEA_LEVEL + 4800.0,
        },
        // lab/lod's kilometre-scale planet: warped continents, flat ocean floor, ridged mountains.
        "lod" => {
            let t = DemoTerrain::preset("normal");
            SceneryTerrain {
                label: format!("lab/lod | {}", t.name),
                radius_meters: t.radius_meters,
                max_height_meters: t.max_height_meters,
                default_sea_level: 300.0,
                rock_height: t.params.rock_height_meters,
                snow_height: t.params.snow_height_meters,
                sampler: Arc::new(t),
            }
        }
        // lab/landing's Aurelia hills, the ground lab/flight flies over.
        "hills" => {
            let t = void_landing::aurelia().terrain;
            SceneryTerrain {
                label: "lab/landing | Aurelia hills".into(),
                radius_meters: t.radius_meters,
                max_height_meters: t.max_height_meters,
                default_sea_level: 1800.0,
                rock_height: 4500.0,
                snow_height: 6000.0,
                sampler: t,
            }
        }
        other => panic!("unknown terrain {other:?}; valid: layered, lod, hills"),
    }
}

struct Args {
    terrain: String,
    at: (f64, f64),
    preset: Option<String>,
    tone: ToneMapping,
}

fn args() -> Args {
    let mut out = Args {
        terrain: "layered".into(),
        at: (0.3, 0.5),
        preset: None,
        tone: ToneMapping::AcesFilmic,
    };
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let value = it.next().unwrap_or_else(|| panic!("{flag} needs a value"));
        match flag.as_str() {
            "--terrain" => out.terrain = value,
            "--preset" => out.preset = Some(value),
            "--tone" => {
                out.tone = match value.as_str() {
                    "aces" => ToneMapping::AcesFilmic,
                    "agx" => ToneMapping::AgX,
                    "neutral" => ToneMapping::Neutral,
                    other => panic!("--tone {other}: aces, agx or neutral"),
                }
            }
            "--at" => {
                let parts: Vec<f64> = value
                    .split(',')
                    .map(|p| p.trim().parse().expect("--at LAT,LON in degrees"))
                    .collect();
                assert!(
                    parts.len() == 2 && parts[0].abs() <= 90.0,
                    "--at {value}: expected latitude,longitude in degrees"
                );
                out.at = (parts[0] * DEG, parts[1] * DEG);
            }
            other => panic!(
                "unknown argument {other}; use --terrain layered|lod|hills, --at LAT,LON, --preset NAME, --tone aces|agx|neutral"
            ),
        }
    }
    out
}

#[derive(Resource)]
struct Scenery {
    view: OrbitView,
    terrain: SceneryTerrain,
    radius: f64,
    /// Longitude where the sun is overhead; the sky turns with it.
    sub_solar_longitude: f64,
    seconds: f64,
    rate: usize,
    declination_degrees: f64,
    sea_level: f64,
    /// Exposure as the lab's slider: ×10^value.
    exposure: f64,
    tone_mapping: ToneMapping,

    atmosphere: bool,
    multiple: bool,
    ocean: bool,
    stars: bool,
    clouds: bool,
    weather_only: bool,
    coverage: f64,

    last_frame_height: f64,
    ground: Handle<GroundMaterial>,
    uniforms: GroundUniforms,
    star_material: Handle<StarMaterial>,
    tables_ms: f64,
    clouds_ms: f64,
    fps: f64,
}

#[derive(Resource)]
struct Ground(TileField<GroundMaterial>);

#[derive(Component)]
struct Sky;

#[derive(Component)]
struct Hud;

/// Disjoint transform queries: tiles, the camera, the star sky.
type TileOnly = (With<Tile>, Without<Camera>, Without<Sky>);
type CameraOnly = (With<Camera>, Without<Tile>, Without<Sky>);
type SkyOnly = (With<Sky>, Without<Tile>, Without<Camera>);

/// Where the camera is: body-fixed position, its unit up, the ground or sea height under it and the
/// camera's height above that.
struct Where {
    position: DVec3,
    up: DVec3,
    surface: f64,
    height: f64,
    latitude: f64,
    longitude: f64,
}

impl Scenery {
    fn surface_height(&self, up: DVec3) -> f64 {
        let land = self.terrain.height(up);
        if self.ocean {
            land.max(self.sea_level)
        } else {
            land
        }
    }

    fn here(&self) -> Where {
        let position = self.view.pose().position;
        let r = position.length();
        let up = position / r;
        let surface = self.surface_height(up);
        Where {
            position,
            up,
            surface,
            height: r - self.radius - surface,
            latitude: f64::asin(up.z),
            longitude: up.y.atan2(up.x),
        }
    }

    /// Lifts the camera back to MIN_HEIGHT when a pan, orbit or zoom took it under the ground or sea.
    fn keep_above_surface(&mut self) {
        let here = self.here();
        if here.height < OrbitView::MIN_HEIGHT {
            self.view
                .set_radius(self.radius + here.surface + OrbitView::MIN_HEIGHT);
        }
    }

    /// Local solar time at the camera, hours.
    fn local_time(&self) -> f64 {
        let hours = 12.0 + (self.here().longitude - self.sub_solar_longitude) / (15.0 * DEG);
        hours.rem_euclid(24.0)
    }

    fn set_local_time(&mut self, hours: f64) {
        self.sub_solar_longitude = self.here().longitude + (12.0 - hours) * 15.0 * DEG;
    }

    fn sun(&self) -> DVec3 {
        let d = self.declination_degrees * DEG;
        DVec3::new(
            d.cos() * self.sub_solar_longitude.cos(),
            d.cos() * self.sub_solar_longitude.sin(),
            d.sin(),
        )
    }
}

fn setup(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut grounds: ResMut<Assets<GroundMaterial>>,
    mut star_materials: ResMut<Assets<StarMaterial>>,
) {
    let Args {
        terrain: terrain_id,
        at,
        preset,
        tone,
    } = args();
    let terrain = scenery_terrain(&terrain_id);
    let radius = terrain.radius_meters;
    let max_height = terrain.max_height_meters;

    let started = std::time::Instant::now();
    let params = earth_like_atmosphere(radius);
    let transmittance = build_transmittance_table(&params);
    let multiple = build_multiple_scattering_table(&params, &transmittance, 64, 20);
    let irradiance = build_irradiance_table(&params, &transmittance, &multiple, 128, 24);
    let tables_ms = started.elapsed().as_secs_f64() * 1e3;
    let started = std::time::Instant::now();
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    let weather = build_cloud_weather(threads);
    let shape = build_cloud_noise(SHAPE_SIZE, false);
    let detail = build_cloud_noise(DETAIL_SIZE, true);
    let clouds_ms = started.elapsed().as_secs_f64() * 1e3;

    let uniforms = GroundUniforms::new(
        &params,
        terrain.default_sea_level,
        terrain.rock_height,
        terrain.snow_height,
    );
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
    commands.insert_resource(AirTextures {
        transmittance,
        multiple: images.add(table_image(
            &multiple,
            MULTIPLE_SCATTERING_SIZE,
            MULTIPLE_SCATTERING_SIZE,
        )),
        irradiance,
        weather: images.add(weather_image(weather, WEATHER_WIDTH, WEATHER_HEIGHT)),
        shape: images.add(noise_volume_image(shape, SHAPE_SIZE)),
        detail: images.add(noise_volume_image(detail, DETAIL_SIZE)),
    });
    let mut field = TileField::new(
        PlanetLodOptions {
            radius_meters: radius,
            min_surface_height_meters: 0.0,
            max_surface_height_meters: max_height,
            occluder_radius_meters: radius,
            lod_surface_band_meters: max_height,
            resolution: 33,
            max_level: HOLMAN_SPLIT_DISTANCE_RATIOS.len() as u32,
            split_distance_ratios: HOLMAN_SPLIT_DISTANCE_RATIOS.to_vec(),
            retain_frames: 90,
            max_cached_tiles: 2400,
        },
        Some(terrain.sampler.clone()),
        ground.clone(),
    );
    field.no_frustum_culling = true;
    commands.insert_resource(Ground(field));

    let (positions, colors) = generate_stars(&DEFAULT_STARS);
    let star_material = star_materials.add(StarMaterial { brightness: 0.08 });
    commands.spawn((
        Sky,
        Mesh3d(meshes.add(star_mesh(positions, &colors))),
        MeshMaterial3d(star_material.clone()),
        Transform::default(),
        NoFrustumCulling,
    ));

    let (lat, lon) = at;
    let start = DVec3::new(lat.cos() * lon.cos(), lat.cos() * lon.sin(), lat.sin());
    let mut scenery = Scenery {
        view: OrbitView::new(start, radius + max_height + 2e6, 0.0, 200e6),
        sea_level: terrain.default_sea_level,
        terrain,
        radius,
        sub_solar_longitude: lon + 2.0 * 15.0 * DEG,
        seconds: 0.0,
        rate: 0,
        declination_degrees: 10.0,
        exposure: 0.8,
        tone_mapping: tone,

        atmosphere: true,
        multiple: true,
        ocean: true,
        stars: true,
        clouds: true,
        weather_only: false,
        coverage: void_scenery::clouds::DEFAULT_CLOUD_COVERAGE,
        last_frame_height: f64::INFINITY,
        ground,
        uniforms,
        star_material,
        tables_ms,
        clouds_ms,
        fps: 30.0,
    };
    scenery.keep_above_surface();
    if let Some(name) = preset {
        let found = PRESETS
            .iter()
            .find(|p| p.0 == name)
            .unwrap_or_else(|| panic!("unknown preset {name}"));
        apply_preset(&mut scenery, *found);
    }
    commands.insert_resource(scenery);

    commands.spawn((
        Camera3d {
            // The air pass reads the scene's depth.
            depth_texture_usages: (TextureUsages::RENDER_ATTACHMENT
                | TextureUsages::TEXTURE_BINDING)
                .into(),
            ..default()
        },
        AirSettings::new(&params),
        Hdr,
        Msaa::Off,
        // three.js's tone mapping runs at the end of the air pass instead.
        Tonemapping::None,
        DebandDither::Disabled,
        Projection::Perspective(PerspectiveProjection {
            fov: (FOV_DEGREES * DEG) as f32,
            near: 0.1,
            far: 1e14,
            ..default()
        }),
        Transform::default(),
    ));
    commands.spawn((
        Hud,
        Text::new(""),
        TextFont {
            font_size: FontSize::Px(13.0),
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

fn smoothstep(a: f64, b: f64, x: f64) -> f64 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Height above the surface, pitch above the horizon, local time and heading from north (Presets).
const PRESETS: [(&str, f64, f64, f64, f64); 7] = [
    ("ground", 2.0, 0.05, 10.0, 0.0),
    ("sunset", 300.0, 0.03, 18.1, 270.0 * DEG),
    ("night", 2.0, 0.4, 23.0, 0.0),
    ("cloud", 0.0, 0.0, 12.0, 0.0),
    ("plane", 10e3, -0.08, 15.0, 0.0),
    ("orbit", 400e3, -0.3, 9.0, 0.0),
    ("space", 20e6, -FRAC_PI_2, 15.0, 0.0),
];

/// Over the current spot, jump to a preset's height, pitch, time and heading.
fn apply_preset(
    s: &mut Scenery,
    (name, height, pitch, hours, heading): (&str, f64, f64, f64, f64),
) {
    let here = s.here();
    let preset_radius = if name == "cloud" {
        s.radius + (s.sea_level + 3000.0).max(here.surface + OrbitView::MIN_HEIGHT)
    } else {
        s.radius + here.surface + height
    };
    s.view
        .place(here.up, preset_radius, heading, FRAC_PI_2 + pitch);
    s.set_local_time(hours);
}

#[allow(clippy::too_many_arguments)]
fn controls(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    time: Res<Time>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut s: ResMut<Scenery>,
) {
    let s = &mut *s;
    let dt = f64::from(time.delta_secs());
    let digits = [
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
        KeyCode::Digit6,
        KeyCode::Digit7,
    ];
    for (key, (name, height, pitch, hours, heading)) in digits.into_iter().zip(PRESETS) {
        if keys.just_pressed(key) {
            apply_preset(s, (name, height, pitch, hours, heading));
        }
    }
    if keys.just_pressed(KeyCode::Comma) {
        let t = s.local_time();
        s.set_local_time((t - 0.25).rem_euclid(24.0));
    }
    if keys.just_pressed(KeyCode::Period) {
        let t = s.local_time();
        s.set_local_time((t + 0.25).rem_euclid(24.0));
    }
    if keys.just_pressed(KeyCode::KeyR) {
        s.rate = (s.rate + 1) % RATES.len();
    }
    if keys.just_pressed(KeyCode::BracketLeft) {
        s.declination_degrees = (s.declination_degrees - 0.5).max(-30.0);
    }
    if keys.just_pressed(KeyCode::BracketRight) {
        s.declination_degrees = (s.declination_degrees + 0.5).min(30.0);
    }
    let sea = f64::from(u8::from(keys.pressed(KeyCode::Equal)))
        - f64::from(u8::from(keys.pressed(KeyCode::Minus)));
    s.sea_level = (s.sea_level + sea * 500.0 * dt).clamp(0.0, s.terrain.max_height_meters);
    if keys.just_pressed(KeyCode::KeyZ) {
        s.exposure = (s.exposure - 0.05).max(-1.0);
    }
    if keys.just_pressed(KeyCode::KeyX) {
        s.exposure = (s.exposure + 0.05).min(3.0);
    }
    if keys.just_pressed(KeyCode::KeyT) {
        s.tone_mapping = match s.tone_mapping {
            ToneMapping::AcesFilmic => ToneMapping::AgX,
            ToneMapping::AgX => ToneMapping::Neutral,
            _ => ToneMapping::AcesFilmic,
        };
    }
    if keys.just_pressed(KeyCode::KeyK) {
        s.coverage = (s.coverage - 0.05).max(0.0);
    }
    if keys.just_pressed(KeyCode::KeyL) {
        s.coverage = (s.coverage + 0.05).min(1.0);
    }
    for (key, flag) in [
        (KeyCode::KeyA, &mut s.atmosphere),
        (KeyCode::KeyM, &mut s.multiple),
        (KeyCode::KeyO, &mut s.ocean),
        (KeyCode::KeyC, &mut s.clouds),
        (KeyCode::KeyW, &mut s.weather_only),
        (KeyCode::KeyS, &mut s.stars),
    ] {
        if keys.just_pressed(key) {
            *flag = !*flag;
        }
    }

    // Mouse, as lab/lod: left drag pans, right drag orbits the planet centre, Shift + left drag turns.
    let (dx, dy) = (f64::from(motion.delta.x), f64::from(motion.delta.y));
    if dx != 0.0 || dy != 0.0 {
        let height = s.here().height;
        // lab/lod's 0.005 rad per pixel, slowed near the ground so a pixel stays about a pixel of ground.
        let orbit_rate = 0.005 * (height / s.radius).min(1.0);
        let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
        if buttons.pressed(MouseButton::Right) {
            s.view
                .orbit_around_center(-dx * orbit_rate, -dy * orbit_rate);
        } else if buttons.pressed(MouseButton::Left) && shift {
            s.view.turn(-dx * 0.005, dy * 0.005);
        } else if buttons.pressed(MouseButton::Left) {
            s.view.pan_screen(
                dx,
                dy,
                FOV_DEGREES * DEG,
                f64::from(window.height()),
                height,
            );
        }
        s.keep_above_surface();
    }
    // The wheel scales the height above the surface: about ×1.2 per 100 px notch (a line is a notch).
    let pixels = match scroll.unit {
        MouseScrollUnit::Line => -f64::from(scroll.delta.y) * 100.0,
        MouseScrollUnit::Pixel => -f64::from(scroll.delta.y),
    };
    if pixels != 0.0 {
        let here = s.here();
        s.view.set_radius(
            s.radius + here.surface + here.height * (pixels.clamp(-500.0, 500.0) * 0.0018).exp(),
        );
        s.keep_above_surface();
    }
}

#[allow(clippy::too_many_arguments)]
fn frame(
    mut commands: Commands,
    time: Res<Time>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut s: ResMut<Scenery>,
    mut field: ResMut<Ground>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut grounds: ResMut<Assets<GroundMaterial>>,
    mut star_materials: ResMut<Assets<StarMaterial>>,
    mut tiles: Query<&mut Transform, TileOnly>,
    mut camera: Single<(&mut Transform, &mut AirSettings, &Projection), CameraOnly>,
    mut sky: Single<(&mut Transform, &mut Visibility), SkyOnly>,
    mut hud: Single<&mut Text, With<Hud>>,
) {
    let s = &mut *s;
    let dt = f64::from(time.delta_secs()).min(0.1);
    let raw = f64::from(time.delta_secs()).max(1e-3);
    s.fps += (1.0 / raw - s.fps) * 0.05;
    // High-orbit redraws do not advance the ocean clock.
    if s.ocean && s.last_frame_height < 20000.0 {
        s.seconds += dt;
    }
    // The sun moves west as the planet turns east.
    s.sub_solar_longitude -= RATES[s.rate].0 * 15.0 * DEG * dt;
    let sun = s.sun();

    // The sea level may have put the surface above the camera.
    s.keep_above_surface();
    let here = s.here();
    let (right, up, back) = s.view.basis();
    let (camera_transform, air, projection) = &mut *camera;
    camera_transform.rotation = Quat::from_mat3(&Mat3::from_cols(
        right.as_vec3(),
        up.as_vec3(),
        back.as_vec3(),
    ));

    let focal_pixels =
        f64::from(window.physical_height()) / (2.0 * (FOV_DEGREES * DEG / 2.0).tan());
    field.0.finish_builds();
    field.0.select(&LodView {
        observer_positions: vec![here.position],
        camera: Some(LodCamera {
            position: here.position,
            distance_scale: 1.0,
            max_level: HOLMAN_SPLIT_DISTANCE_RATIOS.len() as u32,
            focal_pixels,
            min_observer_cell_pixels: 2.0,
        }),
        distance_scale: 1.0,
        horizon_culling: true,
    });
    field
        .0
        .draw(&mut commands, &mut meshes, &mut tiles, here.position);
    s.last_frame_height = here.height;

    if let Projection::Perspective(perspective) = &**projection {
        air.update(
            here.position,
            camera_transform.rotation,
            perspective,
            focal_pixels,
            sun,
        );
    }
    air.enabled = f32::from(u8::from(s.atmosphere));
    air.multiple_enabled = f32::from(u8::from(s.multiple));
    air.clouds_enabled = f32::from(u8::from(s.clouds));
    air.weather_only = f32::from(u8::from(s.weather_only));
    air.coverage = s.coverage as f32;
    air.sea_level = s.sea_level as f32;
    air.exposure = 10f64.powf(s.exposure) as f32;
    air.tone_mapping = s.tone_mapping as u8 as f32;

    update_ground(&mut s.uniforms, here.position, sun, s.seconds);
    s.uniforms.sea_level = s.sea_level as f32;
    s.uniforms.ocean_enabled = f32::from(u8::from(s.ocean));
    s.uniforms.atmosphere_enabled = f32::from(u8::from(s.atmosphere));
    if let Some(mut material) = grounds.get_mut(&s.ground) {
        material.ground = s.uniforms;
    }

    // The sky turns with the sun. Stars fade where the camera is in sunlit air.
    let (sky_transform, sky_visibility) = &mut *sky;
    sky_transform.rotation = Quat::from_rotation_z(s.sub_solar_longitude as f32);
    **sky_visibility = if s.stars {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
    let altitude = here.surface + here.height;
    let sun_mu = here.up.dot(sun);
    let daylight = smoothstep(-0.18, 0.02, sun_mu)
        * (1.0 - smoothstep(0.0, 60e3, altitude))
        * f64::from(u8::from(s.atmosphere));
    if let Some(mut stars) = star_materials.get_mut(&s.star_material) {
        stars.brightness = (0.08 * (1.0 - daylight)) as f32;
    }

    let on = |b: bool| if b { "on" } else { "off" };
    hud.0 = format!(
        "SCENERY   {}\n\
         height {} AGL | {} ASL\n\
         lat {:.3} lon {:.3} pitch {:.0} deg\n\
         sun {:.1} deg above horizon   local time {}   rate {}   declination {:.1} deg\n\
         sea level {:.0} m   exposure x{:.2}   tone mapping {}   atmosphere {}   multi-scatter {}   ocean {}   stars {}\n\
         clouds {}   weather only {}   cloud coverage {:.2}\n\
                  sky tables {:.0} ms   cloud noise {:.0} ms   tiles {} drawn (L{}-L{}) | {} building   {:.0} fps\n\
         left drag pan | right drag orbit | Shift+left turn | wheel zoom\n\
         1-7 ground, sunset, night, cloud layer, 10 km, 400 km, 20,000 km | , . time | R rate | [ ] declination | - = sea | Z X exposure | T tone | K L coverage | A M C W O S toggles",
        s.terrain.label,
        meters(here.height),
        meters(altitude - s.sea_level),
        here.latitude / DEG,
        here.longitude / DEG,
        (s.view.tilt_radians() - FRAC_PI_2) / DEG,
        f64::asin(sun_mu.clamp(-1.0, 1.0)) / DEG,
        hours(s.local_time()),
        RATES[s.rate].1,
        s.declination_degrees,
        s.sea_level,
        10f64.powf(s.exposure),
        s.tone_mapping.label(),
        on(s.atmosphere),
        on(s.multiple),
        on(s.ocean),
        on(s.stars),
        on(s.clouds),
        on(s.weather_only),
        s.coverage,
        s.tables_ms,
        s.clouds_ms,
        field.0.drawn_count(),
        field.0.levels.0,
        field.0.levels.1,
        field.0.building_count(),
        s.fps,
    );
}

fn meters(m: f64) -> String {
    if m >= 1e5 {
        format!("{:.0} km", m / 1e3)
    } else if m >= 1e3 {
        format!("{:.2} km", m / 1e3)
    } else {
        format!("{m:.1} m")
    }
}

fn hours(h: f64) -> String {
    let whole = h.floor();
    format!(
        "{:02}:{:02}",
        whole as u32,
        ((h - whole) * 60.0).floor() as u32
    )
}
