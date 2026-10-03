//! VOID: the main game, as the TS game's `src/main.ts` (lab/flight). Aurelia inside the Sol
//! system, the two-stage demo rocket on scenery's layered terrain, Rapier contact physics near the
//! ground and orbit propagation in flight, staging, time warp with on-rails coasting, and lab/view's
//! single view from the pad out to the map.
//!
//! `--planet aurelia|aurelia-fast|terra|luna|pebble`, `--terrain layered|hills`.
//!
//! Space: ignite the booster, then separate and ignite the upper stage | Shift / Ctrl: throttle,
//! X: cut | W/S pitch, A/D yaw, Q/E roll | `,` `.`: time rate | P: pause | R: reset | drag: orbit
//! the camera, wheel: zoom out into the map | Tab or a label: focus | G: path frame | K: AGL / ALT
//! | L: SURFACE / ORBIT | F1: keys.
//!
//! Drawn in the planet's body-fixed axes with the camera at the origin: terrain tiles and the
//! rocket's parts (body-fixed in the physics) need no turning; bodies and the map (f64 in the
//! ecliptic) are turned by the planet's orientation each frame.

use bevy::camera::Hdr;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::core_pipeline::tonemapping::{DebandDither, Tonemapping};
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit};
use bevy::pbr::wireframe::{Wireframe, WireframeColor, WireframePlugin};
use bevy::prelude::*;
use bevy::render::RenderPlugin;
use bevy::render::render_resource::TextureUsages;
use bevy::render::render_resource::WgpuFeatures;
use bevy::render::settings::WgpuSettings;
use bevy::render::view::Msaa;
use glam::{DMat3, DQuat, DVec3};
use serde_json::json;
use std::time::Instant;
use void_app::aero_field::RocketAir;
use void_app::air::{AirSettings, AirTextures, noise_volume_image, weather_image};
use void_app::flight::{
    GamePlanet, PARTS, PHYSICS_MAX_RATE, TIME_RATES, distance_text, game_planet_by_id,
    mission_time, rate_text, vessel_axes, warp_limit,
};
use void_app::input::{FocusTarget, Input, Key};
use void_app::lab_log::LabLog;
use void_app::map::{
    MapMarker, PATH_COLOR, PLAN_COLOR, color, draw_map_lines, label_click, place_map_labels,
    spawn_map_labels,
};
use void_app::navball::{Navball, NavballLabel, draw_navball, spawn_navball};
use void_app::overlay::{ColliderLine, ColliderLines, DebugView};
use void_app::parts::{ColliderShape, spawn_shape};
use void_app::scenery::{
    GroundMaterial, GroundUniforms, SceneryPlugin, StarMaterial, star_mesh, table_image,
    update_ground,
};
use void_app::session::{Header, Mark, Recorder, Session};
use void_app::tiles::{Tile, TileField};
use void_landing::{
    AttitudeSample, CoastPrediction, DemoRocket, FrameState, LanderControl, PartJointRocket,
    PhysicsMode, RocketPart, STEERING_TORQUE, demo_rocket, landing_lod_options, planet_ephemeris,
    predict_coast,
};
use void_lod::{LodCamera, LodView};
use void_navball::NavballInput;
use void_orbit::{
    ApsisKind, AttitudeLaw, CelestialBody, Control, DominanceTree, Ephemeris, FlightPlan,
    ManeuverSpec, PlanEngine, PropagationRun, ReferenceMode, STANDARD_GRAVITY, VesselState,
    body_orientation, osculating_orbit,
};
use void_sas::{SAS_TUNING, StabilityAssist};
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
use void_scenery::{DEFAULT_STARS, earth_like_atmosphere, generate_stars};
use void_view::{
    FocusGeometry, FocusKind, LabelKind, MapFrame, MapOrbits, MapPath, OrbitCamera, PathFrameKind,
    PlottingFrame, ViewMode, ViewState, camera_spin, map_labels, view_state,
};

/// The coast forecast is recomputed this often in simulated time, and at every staging.
const PREDICTION_INTERVAL_SECONDS: f64 = 2.0;
/// Long enough for one low orbit; a suborbital coast ends at the ground first.
const PREDICTION_HORIZON_SECONDS: f64 = 6000.0;
const THROTTLE_RATE_PERCENT_PER_SECOND: f64 = 50.0;
const VESSEL_DISTANCE: f64 = 45.0;
const FOV_DEGREES: f32 = 58.0;
/// scenery's exposure (10^0.8), applied in the air pass to everything drawn.
const EXPOSURE: f32 = 6.309_573;

/// An overlay colour that comes out of the air pass's exposure and tone mapping about as given.
fn overlay(color: Color) -> Color {
    let c = color.to_linear();
    Color::linear_rgba(
        c.red / EXPOSURE,
        c.green / EXPOSURE,
        c.blue / EXPOSURE,
        c.alpha,
    )
}

/// The plan is integrated this many steps a frame.
const PLAN_STEPS_PER_FRAME: u64 = 1500;

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "VOID".into(),
                        ..default()
                    }),
                    ..default()
                })
                .set(RenderPlugin {
                    render_creation: WgpuSettings {
                        features: WgpuFeatures::POLYGON_MODE_LINE,
                        ..default()
                    }
                    .into(),
                    ..default()
                }),
            SceneryPlugin,
            WireframePlugin::default(),
        ))
        .insert_resource(ClearColor(Color::BLACK))
        .insert_resource(GlobalAmbientLight {
            color: Color::srgb_u8(0xcb, 0xe7, 0xff),
            brightness: 40.0,
            ..default()
        })
        .insert_resource(pilot_from_arguments())
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                read_input,
                simulate,
                terrain,
                draw,
                overlays,
                scenery,
                labels,
                instruments,
                hud,
                log_sample,
            )
                .chain(),
        )
        .run();
}

/// This frame's pilot input, filled by the window or by a recording being replayed, plus the two
/// session jobs: writing one, and flying one that was written.
#[derive(Resource, Default)]
struct Pilot {
    input: Input,
    /// Where to record, before `setup` knows which planet the header should name.
    record_to: Option<String>,
    recorder: Option<Recorder>,
    replay: Option<Replay>,
    /// Frames flown, counted whether recording or not, so a mark can name one.
    frame: usize,
}

/// A recorded session being flown back, and how far through it we are.
struct Replay {
    session: Session,
    next: usize,
    /// The marks compared so far, and the worst gap found, in metres and metres per second.
    compared: usize,
    worst: (f64, f64),
}

/// `--record <file>` writes this flight as a session; `--replay <file>` flies one back and leaves
/// when it runs out of frames. Both at once would mean recording a replay, which is a copy of the
/// file with the window's timing noise added, so it is refused.
fn pilot_from_arguments() -> Pilot {
    let (record, replay) = (argument("--record"), argument("--replay"));
    assert!(
        record.is_none() || replay.is_none(),
        "a flight is either recorded or replayed, not both"
    );
    let mut pilot = Pilot::default();
    if let Some(path) = replay {
        let session = Session::read(&path);
        assert!(
            !session.frames.is_empty(),
            "replay: {path} has no frames to fly"
        );
        // The planet is part of the recording because the same inputs on another world are another
        // flight; the window's own --planet would otherwise silently win.
        println!(
            "replay: {path}: {} frames, {:.1} simulated seconds, {} marks, on {} with {} terrain",
            session.frames.len(),
            session.seconds(),
            session.marks.len(),
            session.header.planet,
            session.header.terrain
        );
        pilot.replay = Some(Replay {
            session,
            next: 0,
            compared: 0,
            worst: (0.0, 0.0),
        });
    }
    if let Some(path) = record {
        pilot.record_to = Some(path);
    }
    pilot
}

fn argument(name: &str) -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1).cloned())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Focus {
    Vessel,
    Body(usize),
}

#[derive(Resource)]
struct Game {
    planet: GamePlanet,
    ephemeris: Ephemeris,
    home: usize,
    bodies: Vec<CelestialBody>,
    dominance: DominanceTree,
    demo: DemoRocket,
    rocket: PartJointRocket,
    /// 0 on the pad, 1 booster burning, 2 upper stage on its own.
    stage: u8,
    engine_armed: bool,
    throttle_percent: f64,
    paused: bool,
    prediction: Option<CoastPrediction>,
    prediction_at: f64,
    prediction_generation: u64,
    time_rate: f64,
    warp_note: Option<(String, std::time::Instant)>,
    altitude_agl: bool,
    speed_surface: bool,
    path_frame: PathFrameKind,
    focus: Focus,
    camera: OrbitCamera,
    dragging: bool,
    state: Option<ViewState>,
    positions: Vec<DVec3>,
    velocities: Vec<DVec3>,
    orbits: MapOrbits,
    path: MapPath,
    started: std::time::Instant,
    help: bool,
    /// The lab's dev panel switches: mesh edges, tile boundaries, colliders, terrain drawing.
    debug: DebugView,
    /// The labs' session log (debug builds).
    log: Option<LabLog>,
    timings: Timings,
    /// lab/sas's stability assist on the steering torque; T toggles it. A maneuver burn steers
    /// itself, and SAS locks afresh after it.
    sas: StabilityAssist,
    sas_suspended: bool,
    /// The orbit crate's multi-burn plan, made after upper-stage separation in free flight.
    plan: Option<FlightPlan>,
    plan_path: MapPath,
    selected: usize,
    plan_message: String,
    executing: bool,
    warp_to_maneuver: bool,
    // This frame's geometry: the camera (ecliptic, barycentric), the render axes (the planet's
    // body-fixed axes), the upper stage, the focus, its reference body, the dominant body.
    eye: DVec3,
    axes: [DVec3; 3],
    upper: FrameState,
    origin: DVec3,
    reference: usize,
    navigation: usize,
    spin: (usize, f64),
}

impl Game {
    fn launch(&mut self) -> PartJointRocket {
        let mut rocket = PartJointRocket::landed(
            &mut self.ephemeris,
            self.home,
            self.planet.planet.terrain.clone(),
            self.demo.full.clone(),
            self.demo.upper.clone(),
            self.demo.booster.clone(),
            self.demo.options,
            self.demo.launch_site,
        );
        rocket.set_air_field(RocketAir::for_planet(&self.planet.planet, &self.demo));
        rocket
    }

    /// An ecliptic vector in render axes (the planet's body-fixed axes).
    fn render(&self, v: DVec3) -> Vec3 {
        DVec3::new(
            v.dot(self.axes[0]),
            v.dot(self.axes[1]),
            v.dot(self.axes[2]),
        )
        .as_vec3()
    }

    /// A barycentric point in the planet's body-fixed frame.
    fn body_fixed(&self, p: DVec3) -> DVec3 {
        let d = p - self.positions[self.home];
        DVec3::new(
            d.dot(self.axes[0]),
            d.dot(self.axes[1]),
            d.dot(self.axes[2]),
        )
    }

    /// The Sun's direction in the planet's body-fixed frame; a fixed inertial light for a lone
    /// planet.
    fn sun_body_fixed(&self) -> DVec3 {
        let star = self.bodies.iter().position(|b| b.parent_index.is_none());
        let ecliptic = match star {
            Some(s) if s != self.home => {
                (self.positions[s] - self.positions[self.home]).normalize()
            }
            _ => DVec3::X,
        };
        DVec3::new(
            ecliptic.dot(self.axes[0]),
            ecliptic.dot(self.axes[1]),
            ecliptic.dot(self.axes[2]),
        )
    }

    fn part_inertial(&self, which: RocketPart) -> FrameState {
        let t = self.rocket.time();
        self.rocket.frame.to_inertial(
            &self.ephemeris,
            t,
            self.rocket.part_state(&self.ephemeris, which),
        )
    }

    fn live_parts(&self) -> Vec<RocketPart> {
        PARTS
            .into_iter()
            .filter(|&p| self.rocket.part_mode(p) != PhysicsMode::Destroyed)
            .collect()
    }

    fn engine_throttle(&self) -> f64 {
        if self.engine_armed {
            self.throttle_percent / 100.0
        } else {
            0.0
        }
    }

    fn warp_limit(&self) -> (f64, Option<String>) {
        warp_limit(
            &self.rocket,
            &self.ephemeris,
            if self.executing {
                1.0
            } else {
                self.engine_throttle()
            },
            self.bodies[self.home].radius_meters,
        )
    }

    fn note(&mut self, text: String) {
        self.warp_note = Some((text, std::time::Instant::now()));
    }

    /// Set a rate no higher than the current limit; asking for more says why it is held lower. An
    /// on-rails rate that is not allowed leaves a physics rate as it is rather than raising it.
    fn set_time_rate(&mut self, requested: f64) {
        let (limit, reason) = self.warp_limit();
        let rate = if requested <= limit {
            requested
        } else if limit > PHYSICS_MAX_RATE {
            limit
        } else {
            self.time_rate.min(limit)
        };
        if rate < requested
            && let Some(reason) = reason
        {
            self.note(format!("{}x needs {reason}", requested));
        }
        self.time_rate = rate;
    }

    fn step_time_rate(&mut self, step: i32) {
        let index = TIME_RATES
            .iter()
            .position(|&r| r == self.time_rate)
            .expect("time rate in the row") as i32;
        let next = (index + step).clamp(0, TIME_RATES.len() as i32 - 1) as usize;
        self.set_time_rate(TIME_RATES[next]);
    }

    fn log(&mut self, event: serde_json::Value) {
        if let Some(log) = self.log.as_mut() {
            log.write(event);
        }
    }

    fn focus_name(&self) -> String {
        match self.focus {
            Focus::Vessel => "vessel".into(),
            Focus::Body(i) => self.bodies[i].name.clone(),
        }
    }

    fn set_focus(&mut self, next: Focus) {
        self.focus = next;
        self.camera.distance = match next {
            Focus::Vessel => VESSEL_DISTANCE,
            Focus::Body(i) => self.bodies[i].radius_meters * 4.0,
        };
        let (focus, distance, t) = (self.focus_name(), self.camera.distance, self.rocket.time());
        self.log(json!({ "event": "focus", "focus": focus, "distance": distance, "simTime": t }));
    }

    fn stage(&mut self) {
        if self.executing {
            return;
        }
        match self.stage {
            0 => {
                self.stage = 1;
                self.engine_armed = true;
            }
            1 => {
                self.rocket.separate(&self.ephemeris);
                self.stage = 2;
                self.engine_armed = true;
                self.prediction_at = f64::NEG_INFINITY;
            }
            _ => {}
        }
    }

    fn reset(&mut self) {
        self.rocket = self.launch();
        self.stage = 0;
        self.engine_armed = false;
        self.throttle_percent = 0.0;
        self.paused = false;
        self.time_rate = 1.0;
        self.prediction = None;
        self.prediction_at = f64::NEG_INFINITY;
        self.prediction_generation += 1;
        self.focus = Focus::Vessel;
        self.camera.distance = VESSEL_DISTANCE;
        self.sas.set_enabled(false);
        self.sas_suspended = false;
        self.plan = None;
        self.selected = 0;
        self.plan_message.clear();
        self.executing = false;
        self.warp_to_maneuver = false;
        self.log(json!({ "event": "reset" }));
    }

    // --- The maneuver plan, as the TS game's panel ------------------------------------------

    fn plan_ready(&self) -> bool {
        self.rocket.separated()
            && self.rocket.mode() == PhysicsMode::Flight
            && self.rocket.part_mode(RocketPart::Upper) == PhysicsMode::Flight
            && self.stage == 2
            && self.engine_armed
            && self.rocket.fuel_kg() > 0.0
    }

    fn plan_state(&self) -> PropagationRun {
        let upper = self.part_inertial(RocketPart::Upper);
        PropagationRun::new(VesselState {
            time: self.rocket.time(),
            position: upper.position,
            velocity: upper.velocity,
            mass_kg: self.rocket.mass_kg(),
        })
    }

    /// Put every auto-reference burn on the body whose sphere of influence holds the plan at
    /// its ignition.
    fn resolve_plan_references(&mut self) {
        let Some(plan) = self.plan.as_mut() else {
            return;
        };
        let mut positions = vec![DVec3::ZERO; self.bodies.len()];
        for i in 0..plan.count() {
            let spec = plan.maneuver(i);
            if spec.reference_mode != ReferenceMode::Auto {
                continue;
            }
            let Some(at) = plan.position_at(&mut self.ephemeris, spec.start_time) else {
                continue;
            };
            self.ephemeris.positions_at(spec.start_time, &mut positions);
            let body = self.dominance.dominant(&positions, at);
            if body != spec.reference_body {
                plan.replace(
                    i,
                    ManeuverSpec {
                        reference_body: body,
                        ..spec
                    },
                );
            }
        }
    }

    /// Run a panel action; its error becomes the panel's message.
    fn plan_action(&mut self, action: impl FnOnce(&mut Self) -> Result<(), String>) {
        match action(self) {
            Ok(()) => self.plan_message.clear(),
            Err(message) => self.plan_message = message,
        }
    }

    fn add_maneuver(&mut self) -> Result<(), String> {
        if self.plan.is_none() {
            if !self.plan_ready() {
                return Err(
                    "Separate the upper stage and reach free flight before planning a maneuver"
                        .into(),
                );
            }
            let mut plan = FlightPlan::new(
                &self.ephemeris,
                self.demo.options.tolerances,
                PlanEngine {
                    thrust_newtons: self.demo.upper.thrust_newtons,
                    exhaust_velocity: self.demo.upper.specific_impulse_seconds * STANDARD_GRAVITY,
                    dry_mass_kg: self.demo.upper.dry_mass_kg,
                },
                PREDICTION_HORIZON_SECONDS,
            );
            plan.rebase(&self.plan_state());
            self.plan = Some(plan);
        }
        let t = self.rocket.time();
        let reference = self
            .dominance
            .dominant(&self.positions, self.upper.position);
        let plan = self.plan.as_mut().unwrap();
        let after = plan.burns().last().map_or(t, |b| b.end_time);
        self.selected = plan.add(ManeuverSpec {
            start_time: t.max(after) + 600.0,
            reference_body: reference,
            reference_mode: ReferenceMode::Auto,
            prograde: 0.0,
            normal: 0.0,
            radial: 0.0,
        });
        self.resolve_plan_references();
        Ok(())
    }

    fn edit_maneuver(
        &mut self,
        change: impl FnOnce(ManeuverSpec) -> ManeuverSpec,
    ) -> Result<(), String> {
        if self.executing && self.selected == 0 {
            return Err("Cannot edit a burn in progress".into());
        }
        let plan = self.plan.as_mut().ok_or("No maneuver plan")?;
        let spec = plan.maneuver(self.selected);
        plan.replace(self.selected, change(spec));
        self.resolve_plan_references();
        Ok(())
    }

    fn remove_maneuver(&mut self) -> Result<(), String> {
        if self.executing && self.selected == 0 {
            return Err("Cannot remove a burn in progress".into());
        }
        let plan = self.plan.as_mut().ok_or("No maneuver plan")?;
        plan.remove(self.selected);
        self.selected = self.selected.min(plan.count().saturating_sub(1));
        if plan.count() == 0 {
            self.plan = None;
            self.warp_to_maneuver = false;
        }
        Ok(())
    }

    fn place_at_apsis(&mut self, kind: ApsisKind) -> Result<(), String> {
        let t = self.rocket.time();
        let plan = self.plan.as_mut().ok_or("No maneuver plan")?;
        let start = plan.start_at_apsis(&mut self.ephemeris, self.selected, kind, t)?;
        self.edit_maneuver(|spec| ManeuverSpec {
            start_time: start,
            ..spec
        })
    }

    fn warp_to_burn(&mut self) -> Result<(), String> {
        let plan = self.plan.as_ref().ok_or("No maneuver plan")?;
        match plan.burns().first() {
            Some(burn) if burn.start_time > self.rocket.time() + 30.0 => {
                self.warp_to_maneuver = true;
                self.paused = false;
                self.throttle_percent = 0.0;
                Ok(())
            }
            _ => Err("No future executable burn at least 30 s away".into()),
        }
    }

    /// The burn flying now steers the rocket: full thrust along its Frenet direction, the upper
    /// stage turned from +Y onto it.
    fn maneuver_control(&self) -> LanderControl {
        let burn = self.plan.as_ref().and_then(|p| p.burns().first().copied());
        let Some(Control::Thrust(control)) = burn.and_then(|b| b.control) else {
            panic!("active maneuver has no orbital burn");
        };
        let AttitudeLaw::Frenet {
            reference_body,
            tangent: a,
            normal: b,
            radial: c,
        } = control.attitude
        else {
            panic!("active maneuver has no Frenet attitude");
        };
        let (cp, cv) = (
            self.positions[reference_body],
            self.velocities[reference_body],
        );
        let tangent = (self.upper.velocity - cv).normalize();
        let normal = (self.upper.position - cp).cross(tangent).normalize();
        let radial = tangent.cross(normal);
        let direction = (tangent * a + normal * b + radial * c).normalize();
        let local = DVec3::new(
            direction.dot(self.axes[0]),
            direction.dot(self.axes[1]),
            direction.dot(self.axes[2]),
        );
        let w = 1.0 + local.y;
        let rotation = if w < 1e-12 {
            DQuat::from_xyzw(1.0, 0.0, 0.0, 0.0)
        } else {
            DQuat::from_xyzw(local.z, 0.0, -local.x, w).normalize()
        };
        LanderControl {
            throttle: 1.0,
            up: 0.0,
            prograde: 0.0,
            orbital_attitude: Some(control.attitude),
            rotation: Some(rotation),
            ..Default::default()
        }
    }

    /// The plan panel as text.
    fn plan_text(&self) -> String {
        if !self.plan_ready() && self.plan.is_none() {
            return String::new();
        }
        let mut out = String::from("\nMANEUVER | upper stage in flight\n");
        match &self.plan {
            None => {
                out += "  N: add a maneuver\n";
            }
            Some(plan) => {
                for i in 0..plan.count() {
                    let spec = plan.maneuver(i);
                    let dv = DVec3::new(spec.prograde, spec.normal, spec.radial).length();
                    out += &format!(
                        "  {}{}. T+{:.0}  dv {dv:.0} m/s  ({:+.0} / {:+.0} / {:+.0})  {}{}\n",
                        if i == self.selected { ">" } else { " " },
                        i + 1,
                        spec.start_time,
                        spec.prograde,
                        spec.normal,
                        spec.radial,
                        match spec.reference_mode {
                            ReferenceMode::Auto =>
                                format!("auto: {}", self.bodies[spec.reference_body].name),
                            ReferenceMode::Fixed => self.bodies[spec.reference_body].name.clone(),
                        },
                        if plan.status(i).is_err() {
                            "  BLOCKED"
                        } else {
                            ""
                        },
                    );
                }
                if plan.count() > 0 {
                    out += &match plan.status(self.selected) {
                        Ok(burn) => format!(
                            "  burn {:.0}-{:.0} s | {:.1} kg fuel{}\n",
                            burn.start_time,
                            burn.end_time,
                            burn.mass_before_kg - burn.mass_after_kg,
                            if self.executing { " | FIRING" } else { "" }
                        ),
                        Err(reason) => format!("  {reason}\n"),
                    };
                }
            }
        }
        if !self.plan_message.is_empty() {
            out += &format!("  ! {}\n", self.plan_message);
        }
        out += "  N add | Del remove | [ ] select | Up/Down prograde | Left/Right normal | PgUp/PgDn radial\n  Home/End start -/+60 s | Alt: x10 | Y at Pe | U at Ap | V reference | B warp to burn\n";
        out
    }

    /// The focus's geometry, position and reference body.
    fn focus_geometry(&self, vessel: DVec3) -> (FocusGeometry, DVec3, usize) {
        match self.focus {
            Focus::Vessel => {
                let reference = self.dominance.dominant(&self.positions, vessel);
                let body = &self.bodies[reference];
                let from_centre = vessel - self.positions[reference];
                let r = from_centre.length();
                (
                    FocusGeometry {
                        kind: FocusKind::Vessel,
                        radial: Some(from_centre / r),
                        north: body.rotation.axis(),
                        reference_radius: body.radius_meters,
                        altitude: r - body.radius_meters,
                        focus_radius: 0.0,
                    },
                    vessel,
                    reference,
                )
            }
            Focus::Body(i) => {
                let body = &self.bodies[i];
                (
                    FocusGeometry {
                        kind: FocusKind::Body,
                        radial: None,
                        north: body.rotation.axis(),
                        reference_radius: body.radius_meters,
                        altitude: 0.0,
                        focus_radius: body.radius_meters,
                    },
                    self.positions[i],
                    i,
                )
            }
        }
    }

    fn map_frame(&self) -> MapFrame<'_> {
        MapFrame {
            time: self.rocket.time(),
            positions: &self.positions,
            velocities: &self.velocities,
            origin: self.eye,
            vessel: self.upper.position,
            vessel_velocity: self.upper.velocity,
            plotting: PlottingFrame {
                kind: self.path_frame,
                reference: self.navigation,
            },
            wall_ms: self.started.elapsed().as_secs_f64() * 1e3,
        }
    }
}

#[derive(Resource)]
struct Ground(TileField<GroundMaterial>);

/// Main-thread timings since the last log sample, ms: (sum, max).
#[derive(Default)]
struct Timings {
    frames: u32,
    frame: (f64, f64),
    physics: (f64, f64),
    lod: (f64, f64),
    select: (f64, f64),
    draw: (f64, f64),
    collider: (f64, f64),
    last_sample: Option<Instant>,
}

impl Timings {
    fn add(phase: &mut (f64, f64), ms: f64) {
        phase.0 += ms;
        phase.1 = phase.1.max(ms);
    }
}

/// Green edges of the terrain triangles each loaded Rapier collider holds, by tile origin.
/// scenery's shading: the ground material's uniforms and the star field's material.
#[derive(Resource)]
struct Scenery {
    ground: Handle<GroundMaterial>,
    uniforms: GroundUniforms,
    stars: Handle<StarMaterial>,
}

#[derive(Component)]
struct Sky;

#[derive(Component)]
struct BodySphere(usize);

#[derive(Component)]
struct Part(RocketPart);

#[derive(Component)]
struct Flame(RocketPart);

#[derive(Component)]
struct Sun;

#[derive(Component)]
struct Hud;

#[derive(Component)]
struct NavballHeading;

/// The simulated game on its pad, with no window, no assets and no Bevy: everything `Game` holds is
/// plain data, so the window's `setup` and a headless replay build the same thing the same way.
fn new_game(planet_id: &str, terrain: Option<&str>) -> Game {
    let planet = game_planet_by_id(planet_id, terrain);
    let (mut ephemeris, home) = planet_ephemeris(&planet.planet);
    let bodies = ephemeris.bodies().to_vec();
    let mut demo = demo_rocket(&planet.planet.terrain);
    if let Some(site) = planet.launch_site {
        demo.launch_site = site;
    }
    let mut rocket = PartJointRocket::landed(
        &mut ephemeris,
        home,
        planet.planet.terrain.clone(),
        demo.full.clone(),
        demo.upper.clone(),
        demo.booster.clone(),
        demo.options,
        demo.launch_site,
    );
    rocket.set_air_field(RocketAir::for_planet(&planet.planet, &demo));
    // Start looking at the rocket from the side, a little above the horizon.
    let start = rocket.frame.to_inertial(
        &ephemeris,
        0.0,
        rocket.part_state(&ephemeris, RocketPart::Upper),
    );
    let radial = (start.position - ephemeris.body_position(home, 0.0)).normalize();
    let side = DVec3::new(-radial.y, radial.x, 0.0).normalize();
    let camera = OrbitCamera::new((side + 0.3 * radial).normalize(), VESSEL_DISTANCE);
    let n = bodies.len();
    let dominance = DominanceTree::new(&bodies);
    Game {
        orbits: MapOrbits::new(&bodies),
        path: MapPath::new(),
        planet,
        ephemeris,
        home,
        dominance,
        demo,
        rocket,
        stage: 0,
        engine_armed: false,
        throttle_percent: 0.0,
        paused: false,
        prediction: None,
        prediction_at: f64::NEG_INFINITY,
        prediction_generation: 0,
        time_rate: 1.0,
        warp_note: None,
        altitude_agl: true,
        speed_surface: true,
        path_frame: PathFrameKind::Inertial,
        focus: Focus::Vessel,
        camera,
        dragging: false,
        state: None,
        positions: vec![DVec3::ZERO; n],
        velocities: vec![DVec3::ZERO; n],
        started: std::time::Instant::now(),
        help: false,
        debug: DebugView::default(),
        log: LabLog::open("flight"),
        timings: Timings::default(),
        sas: StabilityAssist::new(STEERING_TORQUE, SAS_TUNING),
        sas_suspended: false,
        plan: None,
        plan_path: MapPath::new(),
        selected: 0,
        plan_message: String::new(),
        executing: false,
        warp_to_maneuver: false,
        eye: DVec3::ZERO,
        axes: [DVec3::X, DVec3::Y, DVec3::Z],
        upper: start,
        origin: start.position,
        reference: home,
        navigation: home,
        spin: (home, 0.0),
        bodies,
    }
}

#[allow(clippy::too_many_arguments)]
fn setup(
    mut pilot: ResMut<Pilot>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut grounds: ResMut<Assets<GroundMaterial>>,
    mut star_materials: ResMut<Assets<StarMaterial>>,
    window: Single<&Window>,
) {
    // A replay names its own world: flying the recorded inputs on a different planet would be a
    // different flight, so the session's header wins over --planet rather than being checked later.
    let (planet_id, terrain) = match &pilot.replay {
        Some(replay) => (
            replay.session.header.planet.clone(),
            Some(replay.session.header.terrain.clone()),
        ),
        None => (
            argument("--planet").unwrap_or_else(|| "aurelia".into()),
            argument("--terrain"),
        ),
    };
    let mut game = new_game(&planet_id, terrain.as_deref());
    if let Some(path) = pilot.record_to.take() {
        let header = Header {
            planet: planet_id.clone(),
            terrain: format!("{:?}", game.planet.terrain_id).to_lowercase(),
            version: void_app::session::VERSION,
        };
        let recorder = Recorder::create(&path, &header);
        println!("recording to {}", recorder.path().display());
        pilot.recorder = Some(recorder);
    }
    let (planet, demo) = (game.planet.clone(), game.demo.clone());
    let (bodies, home) = (game.bodies.clone(), game.home);

    // scenery's atmosphere tables and cloud noise, and the ground and sea shader on the tiles.
    let scenery_started = Instant::now();
    let params = earth_like_atmosphere(planet.planet.terrain.radius_meters);
    let transmittance = build_transmittance_table(&params);
    let multiple = build_multiple_scattering_table(&params, &transmittance, 64, 20);
    let irradiance = build_irradiance_table(&params, &transmittance, &multiple, 128, 24);
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    let mut uniforms = GroundUniforms::new(
        &params,
        planet.sea_level,
        planet.rock_height,
        planet.snow_height,
    );
    uniforms.ocean_enabled = f32::from(u8::from(planet.ocean));
    uniforms.atmosphere_enabled = f32::from(u8::from(planet.atmosphere));
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
        weather: images.add(weather_image(
            build_cloud_weather(threads),
            WEATHER_WIDTH,
            WEATHER_HEIGHT,
        )),
        shape: images.add(noise_volume_image(
            build_cloud_noise(SHAPE_SIZE, false),
            SHAPE_SIZE,
        )),
        detail: images.add(noise_volume_image(
            build_cloud_noise(DETAIL_SIZE, true),
            DETAIL_SIZE,
        )),
    });
    let mut field = TileField::new(
        landing_lod_options(&planet.planet.terrain, &demo.options.contact),
        Some(planet.planet.terrain.clone()),
        ground.clone(),
    );
    // The sea is raised in the vertex shader, beyond the tiles' bounds.
    field.no_frustum_culling = true;
    field.wireframe_color = overlay(Color::WHITE);
    let scenery_build_ms = scenery_started.elapsed().as_secs_f64() * 1e3;
    let lod_options = field.lod.options.clone();
    commands.insert_resource(Ground(field));
    commands.insert_resource(ColliderLines::new(materials.add(StandardMaterial {
        base_color: overlay(Color::srgb_u8(0x3d, 0xff, 0x6e)),
        unlit: true,
        ..default()
    })));
    // The star catalogue is inertial (ecliptic axes), even while the planet spins.
    let (star_positions, star_colors) = generate_stars(&DEFAULT_STARS);
    let stars = star_materials.add(StarMaterial { brightness: 0.08 });
    commands.spawn((
        Sky,
        Mesh3d(meshes.add(star_mesh(star_positions, &star_colors))),
        MeshMaterial3d(stars.clone()),
        Transform::default(),
        NoFrustumCulling,
    ));
    commands.insert_resource(Scenery {
        ground,
        uniforms,
        stars,
    });

    spawn_map_labels(&mut commands, &bodies);

    // Bodies other than the home planet are plain spheres; the star is unlit.
    let sphere = meshes.add(Sphere::new(1.0).mesh().uv(64, 32));
    for body in &bodies {
        if body.index == home {
            continue;
        }
        let material = materials.add(StandardMaterial {
            base_color: color(&body.color),
            unlit: body.parent_index.is_none(),
            perceptual_roughness: 0.9,
            ..default()
        });
        commands.spawn((
            BodySphere(body.index),
            Mesh3d(sphere.clone()),
            MeshMaterial3d(material),
            Transform::default(),
        ));
    }

    // The rocket's parts, drawn from their colliders, with a flame under each nozzle.
    let hull = materials.add(StandardMaterial {
        base_color: Color::srgb(0.92, 0.93, 0.95),
        perceptual_roughness: 0.5,
        ..default()
    });
    let booster_hull = materials.add(StandardMaterial {
        base_color: Color::srgb(0.75, 0.77, 0.8),
        perceptual_roughness: 0.6,
        ..default()
    });
    let flame = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.6, 0.2),
        emissive: LinearRgba::rgb(8.0, 3.0, 0.6),
        unlit: true,
        ..default()
    });
    let flame_mesh = meshes.add(Cone {
        radius: 0.4,
        height: 1.0,
    });
    for (which, shape, material, nozzle) in [
        (RocketPart::Upper, &demo.upper_shape, &hull, -1.05),
        (
            RocketPart::Booster,
            &demo.booster_shape,
            &booster_hull,
            -1.4,
        ),
    ] {
        commands
            .spawn((Part(which), Transform::default(), Visibility::default()))
            .with_children(|parent| {
                spawn_shape(parent, shape, &mut meshes, material);
                parent.spawn((
                    Flame(which),
                    Mesh3d(flame_mesh.clone()),
                    MeshMaterial3d(flame.clone()),
                    Transform::from_xyz(0.0, nozzle as f32, 0.0)
                        .with_rotation(Quat::from_rotation_x(std::f32::consts::PI)),
                    Visibility::Hidden,
                ));
            });
    }

    let mut air = AirSettings::new(&params);
    air.enabled = f32::from(u8::from(planet.atmosphere));
    air.clouds_enabled = f32::from(u8::from(planet.atmosphere));
    air.sea_level = planet.sea_level as f32;
    // scenery's initial exposure and ACES.
    air.exposure = EXPOSURE;
    air.tone_mapping = 0.0;
    // In the solar system the real Sun is drawn as a body; a lone planet uses the sky's disc.
    let star = bodies.iter().position(|b| b.parent_index.is_none());
    air.sun_disc_enabled = f32::from(u8::from(star == Some(home)));
    commands.spawn((
        Camera3d {
            // The air pass reads the scene's depth.
            depth_texture_usages: (TextureUsages::RENDER_ATTACHMENT
                | TextureUsages::TEXTURE_BINDING)
                .into(),
            ..default()
        },
        air,
        Hdr,
        Msaa::Off,
        // three.js's tone mapping runs at the end of the air pass instead.
        Tonemapping::None,
        DebandDither::Disabled,
        Projection::Perspective(PerspectiveProjection {
            fov: FOV_DEGREES.to_radians(),
            far: 1e14,
            ..default()
        }),
    ));
    commands.spawn((
        Sun,
        DirectionalLight {
            // scenery's sun has irradiance 1 before exposure; this matches it under Bevy's default
            // camera exposure (EV100 9.7).
            illuminance: 1000.0,
            ..default()
        },
        Transform::default(),
    ));
    // lab/navball's ball, bottom centre, with the nose's heading and pitch under it.
    let ball = spawn_navball(
        &mut commands,
        &mut images,
        150.0,
        window.scale_factor() as f64,
    );
    let heading = commands
        .spawn((
            NavballHeading,
            Text::new(""),
            TextFont {
                font_size: FontSize::Px(13.0),
                ..default()
            },
        ))
        .id();
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            bottom: px(10),
            width: percent(100),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: px(4),
            ..default()
        })
        .add_children(&[ball, heading]);
    commands.spawn((
        Hud,
        Text::new(""),
        TextFont {
            font_size: FontSize::Px(13.0),
            ..default()
        },
        Node {
            position_type: PositionType::Absolute,
            top: px(12),
            left: px(12),
            ..default()
        },
    ));

    let session = json!({
        "event": "session",
        "planet": planet_id,
        "terrain": format!("{:?}", game.planet.terrain_id).to_lowercase(),
        "lod": {
            "radiusMeters": lod_options.radius_meters,
            "maxLevel": lod_options.max_level,
            "resolution": lod_options.resolution,
            "maxCachedTiles": lod_options.max_cached_tiles,
        },
        "scenery": {
            "seaLevel": game.planet.sea_level,
            "atmosphere": game.planet.atmosphere,
            "tablesBuildMs": scenery_build_ms,
        },
    });
    game.log(session);
    commands.insert_resource(game);
}

/// Every key the game reads, paired with the window's code for it. Modifiers collapse to one key
/// each: the game has never cared which shift is down, and a recording that said would not replay
/// on a keyboard laid out differently.
const KEYS: [(Key, &[KeyCode]); 42] = [
    (Key::Space, &[KeyCode::Space]),
    (Key::Tab, &[KeyCode::Tab]),
    (Key::Comma, &[KeyCode::Comma]),
    (Key::Period, &[KeyCode::Period]),
    (Key::Delete, &[KeyCode::Delete]),
    (Key::Backspace, &[KeyCode::Backspace]),
    (Key::BracketLeft, &[KeyCode::BracketLeft]),
    (Key::BracketRight, &[KeyCode::BracketRight]),
    (Key::ArrowUp, &[KeyCode::ArrowUp]),
    (Key::ArrowDown, &[KeyCode::ArrowDown]),
    (Key::ArrowLeft, &[KeyCode::ArrowLeft]),
    (Key::ArrowRight, &[KeyCode::ArrowRight]),
    (Key::PageUp, &[KeyCode::PageUp]),
    (Key::PageDown, &[KeyCode::PageDown]),
    (Key::Home, &[KeyCode::Home]),
    (Key::End, &[KeyCode::End]),
    (Key::F1, &[KeyCode::F1]),
    (Key::F2, &[KeyCode::F2]),
    (Key::F3, &[KeyCode::F3]),
    (Key::F4, &[KeyCode::F4]),
    (Key::F5, &[KeyCode::F5]),
    (Key::A, &[KeyCode::KeyA]),
    (Key::B, &[KeyCode::KeyB]),
    (Key::D, &[KeyCode::KeyD]),
    (Key::E, &[KeyCode::KeyE]),
    (Key::G, &[KeyCode::KeyG]),
    (Key::K, &[KeyCode::KeyK]),
    (Key::L, &[KeyCode::KeyL]),
    (Key::N, &[KeyCode::KeyN]),
    (Key::P, &[KeyCode::KeyP]),
    (Key::Q, &[KeyCode::KeyQ]),
    (Key::R, &[KeyCode::KeyR]),
    (Key::S, &[KeyCode::KeyS]),
    (Key::T, &[KeyCode::KeyT]),
    (Key::U, &[KeyCode::KeyU]),
    (Key::V, &[KeyCode::KeyV]),
    (Key::W, &[KeyCode::KeyW]),
    (Key::X, &[KeyCode::KeyX]),
    (Key::Y, &[KeyCode::KeyY]),
    (
        Key::Shift,
        &[KeyCode::ShiftLeft, KeyCode::ShiftRight] as &[KeyCode],
    ),
    (
        Key::Control,
        &[KeyCode::ControlLeft, KeyCode::ControlRight] as &[KeyCode],
    ),
    (
        Key::Alt,
        &[KeyCode::AltLeft, KeyCode::AltRight] as &[KeyCode],
    ),
];

/// This frame's pilot input, read once from the window so that every system downstream of it reads
/// data instead of the keyboard — which is what lets a recording stand in for the keyboard.
#[allow(clippy::too_many_arguments)]
fn read_input(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    markers: Query<(&Interaction, &MapMarker)>,
    game: Res<Game>,
    mut pilot: ResMut<Pilot>,
    mut exit: MessageWriter<AppExit>,
) {
    let pilot = &mut *pilot;
    // A session being replayed drives the game instead of the keyboard, and the window becomes a
    // way to watch it: nothing on the keyboard reaches the game, or the replay would not be one.
    if let Some(replay) = &mut pilot.replay {
        match replay.session.frames.get(replay.next) {
            Some(frame) => {
                pilot.input = *frame;
                replay.next += 1;
                return;
            }
            None => {
                assert_eq!(
                    replay.compared,
                    replay.session.marks.len(),
                    "replay: not every mark was checked"
                );
                println!(
                    "replay: {} frames, {:.1} simulated seconds, {} marks compared, worst {:.3e} m and {:.3e} m/s",
                    replay.session.frames.len(),
                    replay.session.seconds(),
                    replay.compared,
                    replay.worst.0,
                    replay.worst.1
                );
                exit.write(AppExit::Success);
                pilot.input = Input::default();
                return;
            }
        }
    }
    // At most 50 ms of wall time per frame: a stalled frame does not become a physics leap.
    let mut input = Input::new(time.delta_secs_f64().clamp(0.0, 0.05));
    for (key, codes) in KEYS {
        if codes.iter().any(|&c| keys.just_pressed(c)) {
            input.press(key);
        } else if codes.iter().any(|&c| keys.pressed(c)) {
            input.hold(key);
        }
    }
    let any = [MouseButton::Left, MouseButton::Right, MouseButton::Middle];
    input.mouse_held = buttons.any_pressed(any);
    let (over_label, clicked) =
        label_click(&markers, &buttons, game.state.map_or(0.0, |s| s.map_weight));
    input.mouse_pressed = buttons.any_just_pressed(any) && !over_label;
    input.focus = match clicked {
        Some(LabelKind::Vessel) => Some(FocusTarget::Vessel),
        Some(LabelKind::Body(i)) => Some(FocusTarget::Body(i)),
        Some(LabelKind::Star) => Some(FocusTarget::Body(
            game.bodies
                .iter()
                .position(|b| b.parent_index.is_none())
                .expect("a star"),
        )),
        Some(LabelKind::Apsis) | None => None,
    };
    input.drag = (motion.delta.x as f64, motion.delta.y as f64);
    input.scroll_pixels = match scroll.unit {
        MouseScrollUnit::Line => -f64::from(scroll.delta.y) * 100.0,
        MouseScrollUnit::Pixel => -f64::from(scroll.delta.y),
    };
    pilot.input = input;
    if let Some(recorder) = &mut pilot.recorder {
        recorder.frame(&input);
    }
}

/// A mark is written every second or so of frames: often enough to find where a replay diverged,
/// rarely enough that a long session stays a file a person can read.
const FRAMES_PER_MARK: usize = 60;

/// The state digest a session is checked against: where the upper stage is in the planet's own
/// frame, how fast, how heavy, and which stage is flying.
fn mark(game: &Game, frame: usize) -> Mark {
    let state = game.rocket.body_fixed_state(&game.ephemeris);
    Mark {
        frame,
        sim_time: game.rocket.time(),
        position: state.position.to_array(),
        velocity: state.velocity.to_array(),
        mass_kg: game.rocket.mass_kg(),
        stage: game.stage,
    }
}

/// Keyboard actions, applied after the pointer actions of the same frame.
fn apply_keys(game: &mut Game, keys: &Input) {
    let shift = keys.held(Key::Shift);
    if keys.just_pressed(Key::Space) {
        game.stage();
    }
    if keys.just_pressed(Key::T) {
        game.sas.toggle();
    }
    if keys.just_pressed(Key::X) {
        game.throttle_percent = 0.0;
    }
    if keys.just_pressed(Key::P) {
        game.paused = !game.paused;
    }
    if keys.just_pressed(Key::G) {
        game.path_frame = match game.path_frame {
            PathFrameKind::Inertial => PathFrameKind::Surface,
            PathFrameKind::Surface => PathFrameKind::Inertial,
        };
        let (frame, t) = (path_frame_name(game.path_frame), game.rocket.time());
        game.log(json!({ "event": "path-frame", "pathFrame": frame, "simTime": t }));
    }
    // The lab's dev panel switches.
    if keys.just_pressed(Key::F2) {
        game.debug.wire = !game.debug.wire;
    }
    if keys.just_pressed(Key::F3) {
        game.debug.bounds = !game.debug.bounds;
    }
    if keys.just_pressed(Key::F4) {
        game.debug.colliders = !game.debug.colliders;
    }
    if keys.just_pressed(Key::F5) {
        game.debug.terrain = !game.debug.terrain;
        let (visible, t) = (game.debug.terrain, game.rocket.time());
        game.log(json!({ "event": "terrain-visibility", "visible": visible, "simTime": t }));
    }
    if keys.just_pressed(Key::K) {
        game.altitude_agl = !game.altitude_agl;
    }
    if keys.just_pressed(Key::L) {
        game.speed_surface = !game.speed_surface;
    }
    if keys.just_pressed(Key::R) {
        game.reset();
    }
    if keys.just_pressed(Key::Comma) {
        game.step_time_rate(-1);
    }
    if keys.just_pressed(Key::Period) {
        game.step_time_rate(1);
    }
    if keys.just_pressed(Key::F1) {
        game.help = !game.help;
    }
    maneuver_keys(keys, game);
    if keys.just_pressed(Key::Tab) {
        let mut order = vec![Focus::Vessel];
        order.extend((0..game.bodies.len()).map(Focus::Body));
        let current = order.iter().position(|&f| f == game.focus).unwrap_or(0);
        let step = if shift { order.len() - 1 } else { 1 };
        game.set_focus(order[(current + step) % order.len()]);
    }
}
fn apply_pointer(game: &mut Game, keys: &Input) {
    if let Some(target) = keys.focus {
        game.set_focus(match target {
            FocusTarget::Vessel => Focus::Vessel,
            FocusTarget::Body(i) => {
                assert!(i < game.bodies.len(), "input: unknown body {i}");
                Focus::Body(i)
            }
        });
    }

    if keys.mouse_pressed {
        game.dragging = true;
    }
    if !keys.mouse_held {
        game.dragging = false;
    }
    if let Some(state) = game.state {
        if game.dragging && keys.drag != (0.0, 0.0) {
            game.camera.drag(keys.drag.0, keys.drag.1, state.up);
        }
        if keys.scroll_pixels != 0.0 {
            game.camera.zoom(
                (keys.scroll_pixels * 0.0012).exp(),
                state.min_distance,
                state.max_distance,
            );
        }
    }
}

/// The maneuver panel's actions on keys (the lab's panel buttons and fields).
fn maneuver_keys(keys: &Input, game: &mut Game) {
    let alt = keys.held(Key::Alt);
    let step = if alt { 10.0 } else { 1.0 };
    if keys.just_pressed(Key::N) {
        game.plan_action(Game::add_maneuver);
    }
    if game.plan.is_none() {
        return;
    }
    if keys.any_just_pressed([Key::Delete, Key::Backspace]) {
        game.plan_action(Game::remove_maneuver);
    }
    let count = game.plan.as_ref().map_or(0, |p| p.count());
    if keys.just_pressed(Key::BracketLeft) {
        game.selected = game.selected.saturating_sub(1);
    }
    if keys.just_pressed(Key::BracketRight) {
        game.selected = (game.selected + 1).min(count.saturating_sub(1));
    }
    type Edit = (Key, fn(&mut ManeuverSpec, f64), f64);
    let edits: [Edit; 8] = [
        (Key::ArrowUp, |s, d| s.prograde += d, step),
        (Key::ArrowDown, |s, d| s.prograde -= d, step),
        (Key::ArrowRight, |s, d| s.normal += d, step),
        (Key::ArrowLeft, |s, d| s.normal -= d, step),
        (Key::PageUp, |s, d| s.radial += d, step),
        (Key::PageDown, |s, d| s.radial -= d, step),
        (Key::End, |s, d| s.start_time += d, 60.0 * step),
        (Key::Home, |s, d| s.start_time -= d, 60.0 * step),
    ];
    for (key, edit, amount) in edits {
        if keys.just_pressed(key) {
            game.plan_action(|g| {
                g.edit_maneuver(|mut spec| {
                    edit(&mut spec, amount);
                    spec
                })
            });
        }
    }
    if keys.just_pressed(Key::V) {
        // Auto, then each body fixed, then auto again.
        let n = game.bodies.len();
        game.plan_action(|g| {
            g.edit_maneuver(|spec| match spec.reference_mode {
                ReferenceMode::Auto => ManeuverSpec {
                    reference_mode: ReferenceMode::Fixed,
                    reference_body: 0,
                    ..spec
                },
                ReferenceMode::Fixed if spec.reference_body + 1 < n => ManeuverSpec {
                    reference_body: spec.reference_body + 1,
                    ..spec
                },
                ReferenceMode::Fixed => ManeuverSpec {
                    reference_mode: ReferenceMode::Auto,
                    ..spec
                },
            })
        });
    }
    if keys.just_pressed(Key::Y) {
        game.plan_action(|g| g.place_at_apsis(ApsisKind::Periapsis));
    }
    if keys.just_pressed(Key::U) {
        game.plan_action(|g| g.place_at_apsis(ApsisKind::Apoapsis));
    }
    if keys.just_pressed(Key::B) {
        game.plan_action(Game::warp_to_burn);
    }
}

fn simulate(mut pilot: ResMut<Pilot>, mut game: ResMut<Game>) {
    let pilot = &mut *pilot;
    if pilot
        .replay
        .as_ref()
        .is_some_and(|r| pilot.frame >= r.session.frames.len())
    {
        return;
    }
    step(&mut game, &pilot.input);
    pilot.frame += 1;
    let here = mark(&game, pilot.frame);
    if let Some(recorder) = &mut pilot.recorder
        && pilot.frame.is_multiple_of(FRAMES_PER_MARK)
    {
        recorder.mark(&here);
    }
    // On replay, every mark the recording left at this frame must still be where it was. A gap is
    // reported with the frame it appeared at rather than only at the end, because the first one is
    // the one worth looking at — the rest are its consequences.
    if let Some(replay) = &mut pilot.replay {
        for was in replay
            .session
            .marks
            .iter()
            .filter(|m| m.frame == here.frame)
        {
            let (dp, dv) = was.distance(&here);
            replay.compared += 1;
            replay.worst = (replay.worst.0.max(dp), replay.worst.1.max(dv));
            assert!(
                was.stage == here.stage
                    && (was.mass_kg - here.mass_kg).abs() < 1e-9
                    && (was.sim_time - here.sim_time).abs() < 1e-9,
                "replay: frame {} flew stage {} at {:.3} kg, the recording had stage {} at {:.3} kg",
                here.frame,
                here.stage,
                here.mass_kg,
                was.stage,
                was.mass_kg
            );
            assert!(
                dp < REPLAY_POSITION_METERS && dv < REPLAY_VELOCITY_METERS_PER_SECOND,
                "replay: frame {} (T+{:.3} s) is {dp:.3e} m and {dv:.3e} m/s from the recording",
                here.frame,
                here.sim_time
            );
        }
    }
}

/// How far a replayed frame may be from the recorded one. The game is deterministic given its state
/// and the input, so this is the arithmetic's own room and not a tolerance for differences in
/// behaviour: a millimetre is already thousands of times the 33 µm that the frame conversions cost
/// at an astronomical unit, and anything approaching it means something other than rounding moved.
const REPLAY_POSITION_METERS: f64 = 1e-3;
const REPLAY_VELOCITY_METERS_PER_SECOND: f64 = 1e-6;

/// One frame of the game: a function of the state it is given and the pilot's input, and of nothing
/// else. The window calls it with what the keyboard said; a recording calls it with what the
/// keyboard said when the session was flown, which is why those two agree.
fn step(game: &mut Game, keys: &Input) {
    apply_pointer(game, keys);
    apply_keys(game, keys);
    let wall = keys.seconds;
    let delta = keys.axis(Key::Shift, Key::Control);
    if delta != 0.0 && !keys.just_pressed(Key::Tab) {
        game.throttle_percent = (game.throttle_percent
            + delta * wall * THROTTLE_RATE_PERCENT_PER_SECOND)
            .clamp(0.0, 100.0);
    }
    let before = game.rocket.time();
    let next_burn = game.plan.as_ref().and_then(|p| p.burns().first().copied());
    if game.warp_to_maneuver
        && let Some(burn) = next_burn
    {
        if burn.start_time - before - 30.0 <= 0.0 {
            game.warp_to_maneuver = false;
            game.set_time_rate(1.0);
        } else {
            game.set_time_rate(TIME_RATES[TIME_RATES.len() - 1]);
        }
    }
    // Burning, waking on the ground or coming down lowers the rate at once, as KSP does. Falling out
    // of on-rails goes straight to 1x, so there is time to react.
    let (limit, reason) = game.warp_limit();
    if game.time_rate > limit {
        if let Some(reason) = reason {
            game.note(format!("{}x dropped: needs {reason}", game.time_rate));
        }
        game.time_rate = if limit > PHYSICS_MAX_RATE { limit } else { 1.0 };
    }
    Timings::add(&mut game.timings.frame, keys.seconds * 1e3);
    game.timings.frames += 1;
    let physics_started = Instant::now();
    if !game.paused {
        let mut dt = wall * game.time_rate;
        if let Some(burn) = next_burn {
            if game.warp_to_maneuver {
                dt = dt.min((burn.start_time - before - 30.0).max(0.0));
            }
            if !game.executing && burn.start_time > before {
                dt = dt.min(burn.start_time - before);
            }
            if game.executing {
                dt = dt.min((burn.end_time - before).max(0.0));
            }
        }
        if game.time_rate > PHYSICS_MAX_RATE {
            game.rocket.advance_on_rails(&mut game.ephemeris, dt);
        } else {
            let pilot = DVec3::new(
                keys.axis(Key::S, Key::W),
                keys.axis(Key::E, Key::Q),
                keys.axis(Key::D, Key::A),
            );
            let mut control = LanderControl {
                throttle: game.engine_throttle(),
                up: 1.0,
                ..Default::default()
            };
            if game.executing {
                game.sas_suspended = true;
            } else if game.sas_suspended {
                // SAS locks afresh after a burn.
                game.sas_suspended = false;
                if game.sas.enabled() {
                    game.sas.set_enabled(true);
                }
            }
            if game.executing {
                let control = game.maneuver_control();
                game.rocket.advance(&mut game.ephemeris, dt, &control, None);
            } else if game.sas.enabled() {
                // lab/sas's stability assist, every physics step on the attitude at its start.
                let sas = &mut game.sas;
                let mut steer = |s: AttitudeSample, dt: f64| {
                    sas.command(s.rotation, s.angular_velocity, &s.inertia_local, pilot, dt)
                };
                game.rocket
                    .advance(&mut game.ephemeris, dt, &control, Some(&mut steer));
            } else {
                control.turn = Some(pilot);
                game.rocket.advance(&mut game.ephemeris, dt, &control, None);
            }
        }
    }
    Timings::add(
        &mut game.timings.physics,
        physics_started.elapsed().as_secs_f64() * 1e3,
    );
    // A burn starts when its time comes (or is dropped when it has no Δv); it ends at its end
    // time, and the plan continues from the state it left.
    let t = game.rocket.time();
    let due = game.plan.as_ref().and_then(|p| p.burns().first().copied());
    if !game.executing
        && let Some(burn) = due
        && t >= burn.start_time - 1e-7
    {
        let state = game.plan_state();
        let plan = game.plan.as_mut().unwrap();
        plan.rebase(&state);
        match plan.burns().first() {
            Some(b) if b.control.is_some() => {
                assert!(
                    game.plan_ready(),
                    "maneuver ignition requires a separated upper stage in free flight"
                );
                game.executing = true;
                game.set_time_rate(1.0);
                game.throttle_percent = 0.0;
            }
            Some(_) => {
                plan.complete_first(&state);
                game.selected = game.selected.saturating_sub(1);
            }
            None => {}
        }
    }
    if game.executing
        && let Some(burn) = game.plan.as_ref().and_then(|p| p.burns().first().copied())
        && t >= burn.end_time - 1e-7
    {
        game.executing = false;
        game.throttle_percent = 0.0;
        let state = game.plan_state();
        game.plan.as_mut().unwrap().complete_first(&state);
        game.selected = game.selected.saturating_sub(1);
    }
    // A booster lost while attached leaves the upper stage flying on its own.
    if game.stage == 1 && game.rocket.separated() {
        game.stage = 2;
    }
    let t = game.rocket.time();
    game.ephemeris
        .states_at(t, &mut game.positions, Some(&mut game.velocities));
    update_prediction(game);
    if let Some(plan) = game.plan.as_mut()
        && plan.count() > 0
    {
        plan.extend(&mut game.ephemeris, PLAN_STEPS_PER_FRAME);
    }

    game.axes = body_orientation(&game.bodies[game.home].rotation, t);
    game.upper = game.part_inertial(RocketPart::Upper);
    let (geometry, origin, reference) = game.focus_geometry(game.upper.position);
    let state = view_state(ViewMode::Single, false, &geometry, game.camera.distance);
    game.camera.distance =
        game.camera
            .clamp_distance(game.camera.distance, state.min_distance, state.max_distance);
    // The camera turns with the ground near it, or with the path frame on the map.
    let navigation = game
        .dominance
        .dominant(&game.positions, game.upper.position);
    let spin = camera_spin(&state, game.path_frame, reference, navigation);
    if spin.1 > 0.0 && t > before {
        let rotation = game.bodies[spin.0].rotation;
        game.camera
            .corotate(rotation.axis(), rotation.rate() * (t - before) * spin.1);
    }
    game.camera.clamp_to_up(state.up);
    game.eye = origin + game.camera.direction * game.camera.distance;
    game.state = Some(state);
    game.origin = origin;
    game.reference = reference;
    game.navigation = navigation;
    game.spin = spin;
}

fn update_prediction(game: &mut Game) {
    let clearance = game.rocket.clearance(&game.ephemeris);
    if clearance < game.rocket.spec().half_extents.y {
        game.prediction = None;
        return;
    }
    let t = game.rocket.time();
    if t - game.prediction_at < PREDICTION_INTERVAL_SECONDS {
        return;
    }
    game.prediction_at = t;
    let state = game.rocket.body_fixed_state(&game.ephemeris);
    game.prediction = Some(predict_coast(
        &mut game.ephemeris,
        &game.rocket.frame,
        &game.planet.planet.terrain,
        game.rocket.options.tolerances,
        t,
        state,
        game.rocket.mass_kg(),
        PREDICTION_HORIZON_SECONDS,
    ));
    game.prediction_generation += 1;
    let coasting = game.plan.as_ref().is_some_and(|p| {
        p.count() > 0 && !game.executing && p.burns().first().is_some_and(|b| t < b.start_time)
    });
    if coasting {
        let state = game.plan_state();
        game.plan.as_mut().unwrap().rebase(&state);
        game.resolve_plan_references();
    }
}

/// lab/lod's observers: every live part, plus the camera, which also alone decides horizon
/// culling. The camera splits with the same table, stopping one level above the collision level;
/// the rocket's own detail stops where its cells would be under 2 px on screen.
fn terrain(mut game: ResMut<Game>, mut ground: ResMut<Ground>, window: Single<&Window>) {
    let started = Instant::now();
    let observers = game
        .live_parts()
        .into_iter()
        .map(|p| game.rocket.part_state(&game.ephemeris, p).position)
        .collect();
    let height = window.physical_height().max(1) as f64;
    let focal_pixels = height / 2.0 / (f64::from(FOV_DEGREES).to_radians() / 2.0).tan();
    let max_level = ground.0.lod.options.max_level.saturating_sub(1);
    ground.0.finish_builds();
    ground.0.select(&LodView {
        observer_positions: observers,
        camera: Some(LodCamera {
            position: game.body_fixed(game.eye),
            distance_scale: 1.0,
            max_level,
            focal_pixels,
            min_observer_cell_pixels: 2.0,
        }),
        distance_scale: 1.0,
        horizon_culling: true,
    });
    Timings::add(&mut game.timings.lod, started.elapsed().as_secs_f64() * 1e3);
    Timings::add(&mut game.timings.select, ground.0.last_select_ms);
}

type CameraOnly = (
    With<Camera3d>,
    Without<Tile>,
    Without<BodySphere>,
    Without<Part>,
    Without<Flame>,
    Without<Sun>,
);
type SphereOnly = (Without<Tile>, Without<Part>, Without<Flame>, Without<Sun>);
type PartOnly = (Without<Tile>, Without<Flame>, Without<Sun>);
type FlameOnly = (Without<Tile>, Without<Sun>);

#[allow(clippy::too_many_arguments)]
fn draw(
    mut commands: Commands,
    mut game: ResMut<Game>,
    mut ground: ResMut<Ground>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut tiles: Query<&mut Transform, With<Tile>>,
    mut camera: Single<(&mut Transform, &mut Projection), CameraOnly>,
    mut spheres: Query<(&BodySphere, &mut Transform), SphereOnly>,
    mut parts: Query<(&Part, &mut Transform, &mut Visibility), PartOnly>,
    mut flames: Query<(&Flame, &mut Transform, &mut Visibility), FlameOnly>,
    mut sun: Single<&mut Transform, (With<Sun>, Without<Tile>)>,
    mut gizmos: Gizmos,
) {
    let game = &mut *game;
    let Some(state) = game.state else { return };
    let started = Instant::now();

    // Camera at the origin, looking at the focus, up as the view state says.
    let (camera_transform, projection) = &mut *camera;
    **camera_transform =
        Transform::default().looking_to(game.render(-game.camera.direction), game.render(state.up));
    if let Projection::Perspective(p) = &mut **projection {
        p.near = (game.camera.distance * 1e-3).max(0.05) as f32;
    }

    // Tiles and parts are body-fixed: relative to the camera in body-fixed axes, already render
    // axes.
    let eye = game.body_fixed(game.eye);
    ground.0.draw(&mut commands, &mut meshes, &mut tiles, eye);
    for (part, mut transform, mut visibility) in &mut parts {
        let alive = game.rocket.part_mode(part.0) != PhysicsMode::Destroyed;
        *visibility = if alive {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        let state = game.rocket.part_state(&game.ephemeris, part.0);
        *transform = Transform::from_translation((state.position - eye).as_vec3())
            .with_rotation(game.rocket.part_orientation(part.0).as_quat());
    }
    let firing = if game.paused || !game.engine_armed || game.rocket.fuel_kg() <= 0.0 {
        0.0
    } else if game.executing {
        1.0
    } else {
        game.throttle_percent / 100.0
    };
    for (flame, mut transform, mut visibility) in &mut flames {
        let burning = firing > 0.0
            && match flame.0 {
                RocketPart::Booster => game.stage == 1,
                RocketPart::Upper => game.stage == 2,
            };
        *visibility = if burning {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        let length = (2.0 + 6.0 * firing) as f32;
        let nozzle = if flame.0 == RocketPart::Upper {
            -1.05
        } else {
            -1.4
        };
        transform.scale = Vec3::new(1.0, length, 1.0);
        transform.translation.y = nozzle - length / 2.0;
    }

    for (body, mut transform) in &mut spheres {
        let radius = game.bodies[body.0].radius_meters as f32;
        *transform = Transform::from_translation(game.render(game.positions[body.0] - game.eye))
            .with_scale(Vec3::splat(radius));
    }
    // Lone-body planets have a fixed inertial light; solar-system flights use the real Sun.
    let sunward = game.sun_body_fixed().as_vec3();
    **sun = Transform::default().looking_to(-sunward, Vec3::Y);

    // The map: bodies' orbits and the coast forecast, at the map weight's opacity.
    let bodies = game.bodies.clone();
    // Taken out while the frame borrows the game.
    let mut orbits = std::mem::replace(&mut game.orbits, MapOrbits::new(&[]));
    let mut path = std::mem::take(&mut game.path);
    let mut plan_path = std::mem::take(&mut game.plan_path);
    let frame = game.map_frame();
    orbits.update(&bodies, &frame);
    match &game.prediction {
        Some(prediction) => path.update(
            &game.ephemeris,
            &prediction.trajectory,
            game.prediction_generation,
            &frame,
            true,
        ),
        None => path.hide(),
    }
    match &game.plan {
        Some(plan) if plan.count() > 0 => plan_path.update(
            &game.ephemeris,
            &plan.trajectory,
            plan.generation,
            &frame,
            false,
        ),
        _ => plan_path.hide(),
    }
    let render = |v: DVec3| game.render(v);
    draw_map_lines(
        &mut gizmos,
        &bodies,
        &orbits,
        &[(&path, color(PATH_COLOR)), (&plan_path, color(PLAN_COLOR))],
        &frame,
        state.map_weight as f32,
        &render,
    );
    game.orbits = orbits;
    game.path = path;
    game.plan_path = plan_path;
    Timings::add(
        &mut game.timings.draw,
        started.elapsed().as_secs_f64() * 1e3,
    );
}

fn path_frame_name(kind: PathFrameKind) -> &'static str {
    match kind {
        PathFrameKind::Inertial => "inertial",
        PathFrameKind::Surface => "surface",
    }
}

/// The lab's debug overlays: white mesh edges, red tile boundaries, the terrain hidden for
/// profiling, and in green the rocket's collider shapes and each Rapier terrain collider's
/// triangle edges (read back from the colliders themselves).
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn overlays(
    mut commands: Commands,
    mut game: ResMut<Game>,
    mut ground: ResMut<Ground>,
    mut lines: ResMut<ColliderLines>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut tiles: Query<&mut Visibility, (With<Tile>, Without<ColliderLine>)>,
    shapes: Query<(Entity, Has<Wireframe>), With<ColliderShape>>,
    mut collider_lines: Query<
        (&mut Transform, &mut Visibility),
        (With<ColliderLine>, Without<Tile>),
    >,
    mut gizmos: Gizmos,
) {
    let debug = game.debug;
    if ground.0.wireframe() != debug.wire {
        ground.0.set_wireframe(&mut commands, debug.wire);
    }
    let visible = if debug.terrain {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    for mut v in &mut tiles {
        v.set_if_neq(visible);
    }
    let eye = game.body_fixed(game.eye);
    if debug.bounds {
        let red = overlay(Color::srgb(1.0, 0.2, 0.2));
        for line in ground.0.boundaries(eye) {
            gizmos.linestrip(line, red);
        }
    }
    let green = overlay(Color::srgb_u8(0x3d, 0xff, 0x6e));
    for (entity, wired) in &shapes {
        if wired != debug.colliders {
            if debug.colliders {
                commands
                    .entity(entity)
                    .insert((Wireframe, WireframeColor { color: green }));
            } else {
                commands
                    .entity(entity)
                    .remove::<(Wireframe, WireframeColor)>();
            }
        }
    }

    let started = Instant::now();
    lines.sync(
        &mut commands,
        &mut meshes,
        &mut collider_lines,
        &game.rocket.contact_worlds(),
        eye,
        debug.colliders,
    );
    Timings::add(
        &mut game.timings.collider,
        started.elapsed().as_secs_f64() * 1e3,
    );
}

/// The lab's once-a-second `flight-sample`: time, mode, camera, terrain and main-thread timings.
fn log_sample(mut game: ResMut<Game>, ground: Res<Ground>, window: Single<&Window>) {
    let game = &mut *game;
    if game.log.is_none() {
        return;
    }
    let now = Instant::now();
    let due = game
        .timings
        .last_sample
        .is_none_or(|last| now.duration_since(last).as_secs_f64() >= 1.0);
    let Some(state) = game.state else { return };
    if !due {
        return;
    }
    let t = &game.timings;
    let frames = f64::from(t.frames.max(1));
    let phase = |p: (f64, f64)| json!({ "mean": p.0 / frames, "max": p.1 });
    let altitude = (game.focus == Focus::Vessel).then(|| {
        (game.upper.position - game.positions[game.reference]).length()
            - game.bodies[game.reference].radius_meters
    });
    let sample = json!({
        "event": "flight-sample",
        "simTime": game.rocket.time(),
        "timeRate": game.time_rate,
        "mode": format!("{:?}", game.rocket.mode()).to_lowercase(),
        "stage": game.stage,
        "focus": game.focus_name(),
        "distance": game.camera.distance,
        "mapWeight": state.map_weight,
        "corotation": state.corotation,
        "cameraSpin": { "body": game.spin.0, "weight": game.spin.1 },
        "pathFrame": path_frame_name(game.path_frame),
        "altitude": altitude,
        "terrainVisible": game.debug.terrain,
        "scenery": {
            "atmosphere": game.planet.atmosphere,
            "clouds": game.planet.atmosphere,
            "ocean": game.planet.ocean,
            "width": window.physical_width(),
            "height": window.physical_height(),
        },
        "tiles": ground.0.drawn_count(),
        "requests": ground.0.last_requests,
        "queued": ground.0.building_count(),
        "tileStats": {
            "cached": ground.0.lod.cached_tile_count(),
            "cacheBytes": ground.0.lod.cached_mesh_bytes(),
        },
        "perf": {
            "frames": t.frames,
            "frameMs": phase(t.frame),
            "physicsMs": phase(t.physics),
            "lodMs": phase(t.lod),
            "selectMs": phase(t.select),
            "colliderMs": phase(t.collider),
            "drawMs": phase(t.draw),
        },
    });
    game.timings = Timings {
        last_sample: Some(now),
        ..Timings::default()
    };
    if let Some(log) = game.log.as_mut() {
        log.write(sample);
        log.flush();
    }
}

/// scenery's shaders in the body-fixed frame: the camera and the Sun for the air pass and the
/// ground, and the inertial stars turned into the planet's axes, fading in sunlit air.
#[allow(clippy::type_complexity)]
fn scenery(
    game: Res<Game>,
    mut scenery: ResMut<Scenery>,
    window: Single<&Window>,
    mut grounds: ResMut<Assets<GroundMaterial>>,
    mut star_materials: ResMut<Assets<StarMaterial>>,
    mut camera: Single<(&Transform, &mut AirSettings, &Projection), With<Camera3d>>,
    mut sky: Single<&mut Transform, (With<Sky>, Without<Camera3d>)>,
) {
    let scenery = &mut *scenery;
    let eye = game.body_fixed(game.eye);
    let sun = game.sun_body_fixed();
    let (transform, air, projection) = &mut *camera;
    let focal_pixels = f64::from(window.physical_height().max(1))
        / (2.0 * (f64::from(FOV_DEGREES).to_radians() / 2.0).tan());
    if let Projection::Perspective(perspective) = projection {
        air.update(eye, transform.rotation, perspective, focal_pixels, sun);
    }
    update_ground(&mut scenery.uniforms, eye, sun, game.rocket.time());
    if let Some(mut material) = grounds.get_mut(&scenery.ground) {
        material.ground = scenery.uniforms;
    }
    // Ecliptic to body-fixed: the planet's axes as rows.
    let [x, y, z] = game.axes;
    sky.rotation = DQuat::from_mat3(&DMat3::from_cols(x, y, z).transpose()).as_quat();
    let up = eye.normalize();
    let altitude = eye.length() - game.bodies[game.home].radius_meters;
    let smooth = |a: f64, b: f64, v: f64| {
        let t = ((v - a) / (b - a)).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    };
    let daylight = smooth(-0.18, 0.02, up.dot(sun))
        * (1.0 - smooth(0.0, 60e3, altitude))
        * f64::from(u8::from(game.planet.atmosphere));
    if let Some(mut stars) = star_materials.get_mut(&scenery.stars) {
        stars.brightness = (0.08 * (1.0 - daylight)) as f32;
    }
}

#[allow(clippy::type_complexity)]
fn labels(
    game: Res<Game>,
    camera: Single<(&Camera, &GlobalTransform)>,
    mut markers: Query<(&MapMarker, &mut Node, &mut Visibility, &ComputedNode)>,
    mut texts: Query<(&mut Text, &mut Visibility), Without<MapMarker>>,
) {
    let Some(state) = game.state else { return };
    let (camera, camera_transform) = *camera;
    let frame = game.map_frame();
    let focus = match game.focus {
        Focus::Vessel => None,
        Focus::Body(i) => Some(i),
    };
    let apsides = game.path.apsis_positions(&frame);
    let wanted = map_labels(&game.bodies, &frame, focus, &apsides);
    let render = |v: DVec3| game.render(v);
    place_map_labels(
        camera,
        camera_transform,
        &mut markers,
        &mut texts,
        &wanted,
        state.map_weight,
        &render,
    );
}

/// The navball, drawn in the ecliptic around the dominant body's local vertical and north; its
/// markers follow SURFACE / ORBIT.
#[allow(clippy::type_complexity)]
fn instruments(
    game: Res<Game>,
    mut balls: Query<&mut Navball>,
    mut images: ResMut<Assets<Image>>,
    mut labels: Query<(&mut Text, &mut Node, &mut TextColor, &mut Visibility), With<NavballLabel>>,
    mut heading: Single<&mut Text, (With<NavballHeading>, Without<NavballLabel>)>,
) {
    let t = game.rocket.time();
    let axes = game.axes;
    let to_ecliptic = |v: DVec3| axes[0] * v.x + axes[1] * v.y + axes[2] * v.z;
    let (nose, top) = vessel_axes(game.rocket.part_orientation(RocketPart::Upper));
    let body = &game.bodies[game.navigation];
    let velocity = if game.speed_surface {
        to_ecliptic(game.rocket.body_fixed_state(&game.ephemeris).velocity)
    } else {
        game.upper.velocity - game.velocities[game.navigation]
    };
    let input = NavballInput {
        // Rapier's f32 attitude: renormalise so the ball's unit checks hold.
        nose: to_ecliptic(nose).normalize(),
        top: to_ecliptic(top).normalize(),
        up: (game.upper.position - game.positions[game.navigation]).normalize(),
        pole: body.rotation.axis(),
        prime_meridian: body_orientation(&body.rotation, t)[0],
        velocity,
    };
    for mut ball in &mut balls {
        let reading = draw_navball(&mut ball, &input, &mut images, &mut labels);
        heading.0 = format!(
            "HDG {:03} | {}{:.0} deg",
            reading.heading.round() as i64 % 360,
            if reading.pitch >= 0.0 { "+" } else { "" },
            reading.pitch
        );
    }
}

fn hud(game: Res<Game>, ground: Res<Ground>, mut text: Single<&mut Text, With<Hud>>) {
    let Some(state) = game.state else { return };
    let rocket = &game.rocket;
    let body = &game.bodies[game.navigation];
    let home = &game.bodies[game.home];
    let r = game.upper.position - game.positions[game.navigation];
    let v = game.upper.velocity - game.velocities[game.navigation];
    let osc = osculating_orbit(r, v, body.gm);
    let (limit, reason) = game.warp_limit();

    let rates: String = TIME_RATES
        .iter()
        .map(|&rate| {
            let label = rate_text(rate);
            if rate == game.time_rate {
                format!("[{label}]")
            } else if rate > limit {
                format!(" ({label})")
            } else {
                format!(" {label} ")
            }
        })
        .collect();
    let note = match &game.warp_note {
        Some((note, at)) if at.elapsed().as_secs_f64() < 3.0 => note.clone(),
        _ => String::new(),
    };
    let blocked = reason
        .map(|r| format!("above {}: needs {r}", rate_text(limit)))
        .unwrap_or_default();

    let altitude = if game.altitude_agl {
        format!(
            "AGL  {}",
            distance_text(
                (rocket.clearance(&game.ephemeris) - rocket.spec().half_extents.y).max(0.0)
            )
        )
    } else {
        format!("ALT  {}", distance_text(r.length() - body.radius_meters))
    };
    let speed = if game.speed_surface {
        let ground_speed = rocket.body_fixed_state(&game.ephemeris).velocity.length();
        format!("SURFACE {ground_speed:.1} m/s over {}", home.name)
    } else {
        format!("ORBIT   {:.1} m/s about {}", v.length(), body.name)
    };
    let engine = if !game.engine_armed {
        "unlit"
    } else if game.throttle_percent > 0.0 && rocket.fuel_kg() > 0.0 {
        "firing"
    } else {
        "staged"
    };
    let mut stages = String::new();
    for which in PARTS {
        let (name, order, capacity) = match which {
            RocketPart::Booster => ("booster", 1, game.demo.booster.fuel_mass_kg),
            RocketPart::Upper => ("upper stage", 2, game.demo.upper.fuel_mass_kg),
        };
        let status = if rocket.part_mode(which) == PhysicsMode::Destroyed {
            "lost"
        } else if game.stage > order {
            "separated"
        } else if game.stage == order {
            "ACTIVE"
        } else if game.stage + 1 == order {
            "next"
        } else {
            "waiting"
        };
        let left = rocket.part_fuel_kg(which);
        stages += &format!(
            "  {name:<12} {status:<9} fuel {left:>6.0} kg ({:>3.0}%)  dv {:>5.0} m/s\n",
            100.0 * left / capacity,
            rocket.part_delta_v(which)
        );
    }
    let hint = match game.stage {
        0 => "Space: ignite booster",
        1 => "Space: separate, ignite upper stage",
        _ => "",
    };
    let orbit = if state.map_weight > 0.0 {
        let impact = game
            .prediction
            .as_ref()
            .and_then(|p| p.impact)
            .map(|(at, _)| format!("in {:.0} s", (at - rocket.time()).max(0.0)))
            .unwrap_or_else(|| "-".into());
        let apoapsis = if osc.apoapsis_radius_meters.is_finite() {
            distance_text(osc.apoapsis_radius_meters - body.radius_meters)
        } else {
            "escape".into()
        };
        format!(
            "\nORBIT | {}  ({:.0}% map)\n  Ap {apoapsis}\n  Pe {}\n  impact {impact}\n  PATH {}\n",
            body.name,
            state.map_weight * 100.0,
            distance_text(osc.periapsis_radius_meters - body.radius_meters),
            if game.path_frame == PathFrameKind::Surface {
                format!("with {}'s surface (G)", body.name)
            } else {
                "inertial (G)".into()
            },
        )
    } else {
        String::new()
    };
    let help = if game.help {
        "\nSpace: ignite booster, then separate and ignite the upper stage\n\
         Shift/Ctrl: throttle | X: cut | W/S pitch | A/D yaw | Q/E roll | T: SAS\n\
         , . time rate | P pause | R reset\n\
         drag: orbit camera | wheel: zoom out into the map\n\
         Tab or a label: focus | G: path frame | K: AGL/ALT | L: SURFACE/ORBIT | F1: keys\n\
         After upper-stage separation in flight, N adds a maneuver (keys in its panel)\n"
    } else {
        "\nF1: keys\n"
    };
    let plan = game.plan_text();
    let on = |b: bool| if b { "on" } else { "off" };
    let focus = match game.focus {
        Focus::Vessel => format!("vessel (reference {})", game.bodies[game.reference].name),
        Focus::Body(i) => game.bodies[i].name.clone(),
    };
    text.0 = format!(
        "{}  {}{}\n{rates}\n{note}{blocked}\n\n\
         STAGES | {:?}\n{stages}  {hint}\n\n\
         THR {:>3.0}%  {engine}   SAS {}\n\
         {altitude}\n\
         {speed}\n{orbit}{plan}\n\
         focus   {focus}\n\
         camera  {} | map {:.0}% | up {:.0}% | co-rotate {:.0}% {}\n\
         tiles   {} drawn, L{}-L{}, {} building\n\
         debug   F2 mesh edges {} | F3 tile boundaries {} | F4 colliders {} | F5 terrain {}{}\n\
         planet  {}{help}",
        mission_time(rocket.time()),
        rate_text(game.time_rate),
        if game.paused { "  PAUSED" } else { "" },
        rocket.mode(),
        game.throttle_percent,
        if game.sas.enabled() {
            format!("ON ({})", game.sas.phase().label())
        } else {
            "off (T)".into()
        },
        distance_text(game.camera.distance),
        state.map_weight * 100.0,
        state.up_weight * 100.0,
        game.spin.1 * 100.0,
        game.bodies[game.spin.0].name,
        ground.0.drawn_count(),
        ground.0.levels.0,
        ground.0.levels.1,
        ground.0.building_count(),
        on(game.debug.wire),
        on(game.debug.bounds),
        on(game.debug.colliders),
        on(game.debug.terrain),
        if game.log.is_some() {
            " | log lab-log/flight.jsonl"
        } else {
            ""
        },
        // The default font has no middle dot or superscripts.
        game.planet.planet.label.replace('·', "|").replace('²', "2"),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A launch written as frames, the way a recording holds one: throttle up, stage, fly a while
    /// steering, then separate. It is short enough to replay in a test and long enough to go
    /// through the parts of the frame that matter — the engine, the staging, the steering, and the
    /// hand-off from contact physics to free flight.
    fn scripted_launch() -> Vec<Input> {
        let mut frames = Vec::new();
        let frame = |seconds: f64| Input::new(seconds);
        // Stability assist on first. Without it a steering input is a torque with nothing to stop
        // it, so the stack keeps turning and comes back down on its side — which is what the rocket
        // really does, and not what this script is for.
        let mut assist = frame(1.0 / 60.0);
        assist.press(Key::T);
        frames.push(assist);
        // Three seconds of holding shift brings the throttle to full.
        for _ in 0..180 {
            let mut f = frame(1.0 / 60.0);
            f.hold(Key::Shift);
            frames.push(f);
        }
        // Ignition.
        let mut ignite = frame(1.0 / 60.0);
        ignite.press(Key::Space);
        frames.push(ignite);
        // Ninety seconds of climb, with a short pitch-over a third of the way up: long enough to
        // put the steering, the air and the hand-off into the flight, short enough that the stack
        // is still going up at the end of it rather than coming back down on its side.
        for i in 0..90 * 60 {
            let mut f = frame(1.0 / 60.0);
            if (70 * 60..70 * 60 + 18).contains(&i) {
                f.hold(Key::W);
            }
            frames.push(f);
        }
        // Separation, then half a minute of coasting with a roll input to move the attitude.
        let mut separate = frame(1.0 / 60.0);
        separate.press(Key::Space);
        frames.push(separate);
        for i in 0..30 * 60 {
            let mut f = frame(1.0 / 60.0);
            if i % 120 < 20 {
                f.hold(Key::D);
            }
            frames.push(f);
        }
        frames
    }

    /// Fly frames through the game, writing a mark on the same frames the window would.
    fn fly(game: &mut Game, frames: &[Input], mut each_mark: impl FnMut(Mark)) {
        for (i, input) in frames.iter().enumerate() {
            step(game, input);
            let frame = i + 1;
            if frame.is_multiple_of(FRAMES_PER_MARK) {
                each_mark(mark(game, frame));
            }
        }
    }

    /// Where the stack is, as an orbit about the home planet: this is what "reached orbit" means.
    fn orbit_of(game: &Game) -> void_orbit::OsculatingOrbit {
        let state = game.part_inertial(RocketPart::Upper);
        let n = game.bodies.len();
        let (mut positions, mut velocities) = (vec![DVec3::ZERO; n], vec![DVec3::ZERO; n]);
        game.ephemeris
            .states_at(game.rocket.time(), &mut positions, Some(&mut velocities));
        void_orbit::osculating_orbit(
            state.position - positions[game.home],
            state.velocity - velocities[game.home],
            game.bodies[game.home].gm,
        )
    }

    /// A gravity turn written as frames: climb straight up for `vertical` seconds, then tip the nose
    /// over with a pitch pulse every `interval` seconds until `pitches` of them have gone in, and
    /// hold the throttle open through staging to orbit.
    fn gravity_turn(
        vertical: f64,
        interval: f64,
        pulse_frames: usize,
        pitches: usize,
    ) -> Vec<Input> {
        let mut frames = Vec::new();
        let mut assist = Input::new(1.0 / 60.0);
        assist.press(Key::T);
        frames.push(assist);
        for _ in 0..180 {
            let mut f = Input::new(1.0 / 60.0);
            f.hold(Key::Shift);
            frames.push(f);
        }
        let mut ignite = Input::new(1.0 / 60.0);
        ignite.press(Key::Space);
        frames.push(ignite);
        let total = 700 * 60;
        let start = (vertical * 60.0) as usize;
        let every = (interval * 60.0) as usize;
        for i in 0..total {
            let mut f = Input::new(1.0 / 60.0);
            if i >= start {
                let since = i - start;
                if since / every < pitches && since % every < pulse_frames {
                    f.hold(Key::W);
                }
            }
            frames.push(f);
        }
        frames
    }

    /// Fly a gravity turn and report the best orbit it ever held, which is at upper-stage burnout
    /// rather than at the end of a fixed window: after the fuel is gone a suborbital stack is on its
    /// way back down, and measuring there says nothing about how close it came.
    fn fly_to_burnout(frames: &[Input], trace: bool) -> (Game, void_orbit::OsculatingOrbit) {
        let mut game = new_game("aurelia", Some("layered"));
        let mut staged = false;
        let mut best = orbit_of(&game);
        let radius = game.planet.planet.terrain.radius_meters;
        for (i, input) in frames.iter().enumerate() {
            let mut input = *input;
            // Stage the moment the booster runs dry, which is what a pilot does.
            if !staged && game.stage == 1 && game.rocket.part_fuel_kg(RocketPart::Booster) <= 0.0 {
                input.press(Key::Space);
                staged = true;
            }
            step(&mut game, &input);
            let o = orbit_of(&game);
            if o.periapsis_radius_meters > best.periapsis_radius_meters {
                best = o;
            }
            if trace && i.is_multiple_of(30 * 60) {
                let state = game.rocket.body_fixed_state(&game.ephemeris);
                let up = state.position.normalize();
                let nose = game.rocket.part_orientation(RocketPart::Upper) * DVec3::Y;
                println!(
                    "  T+{:6.1} s stage {} alt {:8.0} m speed {:7.0} m/s pitch {:5.1} deg fuel {:6.0}+{:6.0} kg periapsis {:9.0} km apoapsis {:9.0} km",
                    game.rocket.time(),
                    game.stage,
                    state.position.length() - radius,
                    state.velocity.length(),
                    nose.dot(up).clamp(-1.0, 1.0).acos().to_degrees(),
                    game.rocket.part_fuel_kg(RocketPart::Upper),
                    game.rocket.part_fuel_kg(RocketPart::Booster),
                    (o.periapsis_radius_meters - radius) / 1e3,
                    (o.apoapsis_radius_meters - radius) / 1e3,
                );
            }
        }
        (game, best)
    }

    #[test]
    #[ignore = "a diagnostic, run by hand"]
    fn trace_one_gravity_turn() {
        let frames = gravity_turn(40.0, 8.0, 20, 16);
        let (game, best) = fly_to_burnout(&frames, true);
        let radius = game.planet.planet.terrain.radius_meters;
        println!(
            "best orbit held: periapsis {:.0} km, apoapsis {:.0} km, e {:.3}",
            (best.periapsis_radius_meters - radius) / 1e3,
            (best.apoapsis_radius_meters - radius) / 1e3,
            best.eccentricity
        );
    }

    #[test]
    #[ignore = "a parameter search, run by hand when the rocket or the air changes"]
    fn tune_the_gravity_turn() {
        let mut best = (f64::NEG_INFINITY, (0.0, 0.0, 0, 0));
        for vertical in [45.0, 60.0, 75.0] {
            for interval in [14.0, 20.0, 28.0] {
                for pulse in [30, 45, 60] {
                    for pitches in [5, 7, 9] {
                        let frames = gravity_turn(vertical, interval, pulse, pitches);
                        let (game, o) = fly_to_burnout(&frames, false);
                        let radius = game.planet.planet.terrain.radius_meters;
                        let score = (o.periapsis_radius_meters - radius).min(300e3);
                        if score > best.0 {
                            best = (score, (vertical, interval, pulse, pitches));
                            println!(
                                "vertical {vertical:4.0} interval {interval:3.0} pulse {pulse:3} pitches {pitches:3}: periapsis {:9.1} km apoapsis {:9.1} km e {:.3}",
                                (o.periapsis_radius_meters - radius) / 1e3,
                                (o.apoapsis_radius_meters - radius) / 1e3,
                                o.eccentricity
                            );
                        }
                    }
                }
            }
        }
        println!("best: {:?} with periapsis {:.1} km", best.1, best.0 / 1e3);
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let directory = std::env::temp_dir().join("void-replay-test");
        std::fs::create_dir_all(&directory).expect("a scratch directory");
        directory.join(name)
    }

    #[test]
    fn a_recorded_session_replays_to_the_same_flight() {
        let path = scratch("launch.jsonl");
        let frames = scripted_launch();
        let header = Header {
            planet: "aurelia".into(),
            terrain: "layered".into(),
            version: void_app::session::VERSION,
        };
        // Fly it once, recording as the window would.
        let mut game = new_game(&header.planet, Some(&header.terrain));
        {
            let mut recorder = Recorder::create(&path, &header);
            for frame in &frames {
                recorder.frame(frame);
            }
            let mut marks = 0;
            let radius = game.planet.planet.terrain.radius_meters;
            fly(&mut game, &frames, |m| {
                recorder.mark(&m);
                marks += 1;
                if m.frame.is_multiple_of(1800) {
                    println!(
                        "  T+{:6.1} s: stage {}, {:8.1} m up at {:7.1} m/s, {:7.0} kg",
                        m.sim_time,
                        m.stage,
                        DVec3::from_array(m.position).length() - radius,
                        DVec3::from_array(m.velocity).length(),
                        m.mass_kg
                    );
                }
            });
            assert!(marks > 100, "only {marks} marks over the flight");
        }
        let flown = mark(&game, frames.len());
        // The flight has to have been a flight, or the replay below proves nothing: off the ground,
        // through both stagings, and moving.
        let altitude =
            DVec3::from_array(flown.position).length() - game.planet.planet.terrain.radius_meters;
        println!(
            "recorded launch: {} frames, {:.0} s, stage {}, {:.1} km up at {:.0} m/s, {:.0} kg",
            frames.len(),
            flown.sim_time,
            flown.stage,
            altitude / 1e3,
            DVec3::from_array(flown.velocity).length(),
            flown.mass_kg
        );
        assert!(flown.stage == 2 && altitude > 20e3);

        // Now fly the file, from a game built the same way and nothing carried over.
        let session = Session::read(&path);
        assert_eq!(session.header, header);
        assert_eq!(session.frames, frames, "the file must hold what was flown");
        let mut replayed = new_game(&session.header.planet, Some(&session.header.terrain));
        let mut marks = session.marks.iter();
        let mut worst = (0.0_f64, 0.0_f64);
        fly(&mut replayed, &session.frames, |m| {
            let was = marks
                .next()
                .expect("a recorded mark for every replayed one");
            assert_eq!(was.frame, m.frame);
            assert_eq!((was.stage, was.mass_kg), (m.stage, m.mass_kg));
            let (dp, dv) = was.distance(&m);
            worst = (worst.0.max(dp), worst.1.max(dv));
        });
        assert!(
            marks.next().is_none(),
            "every recorded mark must be reached"
        );
        println!(
            "replay: worst {:.2e} m and {:.2e} m/s over {} marks",
            worst.0,
            worst.1,
            session.marks.len()
        );
        assert!(worst.0 == 0.0 && worst.1 == 0.0, "the replay must be exact");
        std::fs::remove_file(&path).expect("remove the scratch recording");
    }

    #[test]
    fn replay_system_checks_nonperiodic_marks_and_stops_at_the_last_frame() {
        let input = Input::new(0.01);
        let mut expected = new_game("aurelia", Some("layered"));
        step(&mut expected, &input);
        let session = Session {
            header: Header {
                planet: "aurelia".into(),
                terrain: "layered".into(),
                version: void_app::session::VERSION,
            },
            frames: vec![input],
            marks: vec![mark(&expected, 1)],
        };
        let mut app = App::new();
        app.insert_resource(new_game("aurelia", Some("layered")))
            .insert_resource(Pilot {
                input,
                replay: Some(Replay {
                    session,
                    next: 1,
                    compared: 0,
                    worst: (0.0, 0.0),
                }),
                ..default()
            })
            .add_systems(Update, simulate);
        app.update();
        assert_eq!(
            app.world()
                .resource::<Pilot>()
                .replay
                .as_ref()
                .unwrap()
                .compared,
            1
        );
        let before = mark(app.world().resource::<Game>(), 1);
        app.update();
        assert_eq!(app.world().resource::<Pilot>().frame, 1);
        assert_eq!(mark(app.world().resource::<Game>(), 1), before);
    }

    #[test]
    fn replayed_pointer_inputs_preserve_camera_and_label_drag_blocking() {
        let mut frames = vec![Input::new(0.01)];
        let mut pointer = Input::new(0.01);
        pointer.mouse_held = true;
        pointer.mouse_pressed = true;
        pointer.drag = (30.0, -12.0);
        pointer.scroll_pixels = -100.0;
        frames.push(pointer);
        pointer.mouse_pressed = false;
        pointer.scroll_pixels = 0.0;
        frames.push(pointer);
        frames.push(Input::new(0.01));
        let mut label = pointer;
        label.focus = Some(FocusTarget::Body(0));
        frames.push(label);
        let mut first = new_game("aurelia", Some("layered"));
        let mut second = new_game("aurelia", Some("layered"));
        step(&mut first, &frames[0]);
        let direction = first.camera.direction;
        step(&mut first, &frames[1]);
        assert_ne!(first.camera.direction, direction);
        for input in &frames[2..] {
            step(&mut first, input);
        }
        for input in &frames {
            step(&mut second, input);
        }
        assert_eq!(first.camera.direction, second.camera.direction);
        assert_eq!(first.camera.distance, second.camera.distance);
        assert_eq!(first.focus, second.focus);
        assert!(!first.dragging, "a label click cannot start a drag");
    }

    #[test]
    fn recorded_focus_clicks_reach_the_game_without_window_input() {
        let mut game = new_game("aurelia", Some("layered"));
        let mut input = Input::new(0.0);
        input.focus = Some(FocusTarget::Body(game.home));
        step(&mut game, &input);
        assert_eq!(game.focus, Focus::Body(game.home));
        input.focus = Some(FocusTarget::Vessel);
        step(&mut game, &input);
        assert_eq!(game.focus, Focus::Vessel);
    }

    #[test]
    fn a_changed_flight_fails_its_marks() {
        // The point of the marks, and the only reason a recording is more than a way to watch the
        // bug again: if the flight comes out different, the session says so by itself. Here the
        // difference is one steering input removed, which is about as small as a change gets.
        let frames = scripted_launch();
        let mut recorded = Vec::new();
        let mut game = new_game("aurelia", Some("layered"));
        fly(&mut game, &frames, |m| recorded.push(m));
        let mut changed = frames.clone();
        let altered = changed
            .iter_mut()
            .rfind(|f| f.held(Key::W))
            .expect("the script holds W");
        *altered = Input::new(altered.seconds);
        let mut other = new_game("aurelia", Some("layered"));
        let mut marks = recorded.iter();
        let mut worst = 0.0_f64;
        fly(&mut other, &changed, |m| {
            let was = marks.next().expect("a mark for every frame");
            worst = worst.max(was.distance(&m).0);
        });
        println!(
            "one steering frame removed: the flight ends up {worst:.2e} m away, against a {REPLAY_POSITION_METERS:.0e} m tolerance"
        );
        assert!(
            worst > REPLAY_POSITION_METERS,
            "removing a steering input moved the flight only {worst:.2e} m, so the marks would not \
             have caught it: either the tolerance is too wide or the input does nothing"
        );
    }
}
