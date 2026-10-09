//! Shared Fleet flight scene used by the independent integration lab and the main game.
mod ui;
use crate::{
    flight::game_planet_by_id,
    overlay::unique_edges,
    tiles::{Tile, TileField},
};
use bevy::{
    input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll},
    pbr::wireframe::WireframePlugin,
    prelude::*,
    render::settings::{WgpuFeatures, WgpuSettings},
};
use glam::{DQuat, DVec3};
use std::collections::{HashMap, HashSet};
use void_assembly::{Craft, Module, import_craft};
use void_assembly_lab::parts::RenderAssets;
use void_fleet_flight::session::{
    Action, FlightSession, InitialWorld, Outcome, Playback, Recording,
};
use void_landing::{demo_rocket, landing_lod_options};
use void_lod::{LodCamera, LodView};
use void_vessels::nearby_site;

use void_fleet_flight::presentation::{Toggle, ViewCommand};

#[derive(Clone, Debug, PartialEq, Eq)]
struct PortAddress {
    vessel: String,
    part: String,
    module: String,
}

fn ports(lab: &Lab, own: bool) -> Vec<PortAddress> {
    let f = &lab.session.sim().fleet;
    let selected = &lab.session.sim().selected;
    f.vessel_ids()
        .iter()
        .filter(|id| (*id == selected) == own)
        .flat_map(|id| {
            f.part_snapshots(id).into_iter().flat_map(move |p| {
                p.definition.modules.iter().filter_map(move |m| match m {
                    Module::DockingPort { id: module, .. } => Some(PortAddress {
                        vessel: id.clone(),
                        part: p.id.clone(),
                        module: module.clone(),
                    }),
                    _ => None,
                })
            })
        })
        .collect()
}
fn port_pose(lab: &Lab, port: &PortAddress) -> (DVec3, DVec3) {
    let f = &lab.session.sim().fleet;
    let p = f.parts().part(&port.part);
    let Module::DockingPort { node_id, .. } = p
        .definition
        .modules
        .iter()
        .find(|m| m.id() == port.module)
        .expect("port module")
    else {
        panic!("not a port")
    };
    let n = p
        .definition
        .nodes
        .iter()
        .find(|n| n.id == *node_id)
        .expect("port node");
    (
        p.pose.position + p.pose.rotation * n.position - f.centre_of_mass_local(&port.vessel),
        p.pose.rotation * n.direction,
    )
}
fn neutral_pilot(lab: &mut Lab) {
    let id = &lab.session.sim().selected;
    let c = lab.session.sim().fleet.control(id);
    let rcs = lab.session.sim().fleet.rcs_control(id);
    if c.turn != DVec3::ZERO {
        lab.session.execute(Action::Control {
            throttle: c.throttle,
            turn: DVec3::ZERO,
        });
    }
    if rcs.force != DVec3::ZERO || rcs.torque != DVec3::ZERO {
        lab.session.execute(Action::Rcs {
            control: void_vessels::RcsControl {
                enabled: rcs.enabled,
                ..Default::default()
            },
        });
    }
}
fn select_pilot(lab: &mut Lab, id: &str) {
    lab.session.sim().fleet.snapshot(id);
    neutral_pilot(lab);
    // Dock already selects its surviving owner. Reselecting it resets the user's camera.
    if lab.session.sim().selected != id {
        lab.session.execute(Action::Select { vessel: id.into() });
    }
    neutral_pilot(lab);
    lab.own_port = ports(lab, true).first().cloned();
    lab.target_port = None;
    refresh_ports(lab);
}
fn stellar_fixture_initial(mut initial: InitialWorld) -> InitialWorld {
    if std::env::args().any(|a| a == "--stellar-fixture") {
        assert!(
            initial.world.stellar.is_some() && initial.launch_body == "Sol/aurelia",
            "stellar fixture requires the authored Sol launch world"
        );
        initial.launch_site = initial
            .world
            .daylight_terrain_site(&initial.launch_body)
            .expect("stellar fixture Sol daylight terrain site");
    }
    initial
}
fn scenery_preset(lab: &mut Lab, body: usize, view: &str) {
    let sim = lab.session.sim();
    let fleet = &sim.fleet;
    let radius = fleet.ephemeris.bodies()[body].radius_meters;
    let emissive = sim
        .world
        .bodies
        .get(&fleet.ephemeris.bodies()[body].id)
        .is_some_and(|d| {
            matches!(
                d.visual.surface,
                void_scenery::solar::SurfaceRecipe::EmissiveStar { .. }
            )
        });
    let root = fleet
        .ephemeris
        .bodies()
        .iter()
        .find(|b| {
            b.parent_index.is_none()
                && fleet.ephemeris.system_of(b.index) == fleet.ephemeris.system_of(body)
        })
        .expect("world system root")
        .index;
    let local = if body == root {
        DVec3::new(1.0, 0.2, 0.3).normalize()
    } else {
        let sun = fleet
            .frames()
            .transform(fleet.body_frames(root).0, fleet.body_frames(body).1)
            .apply_point(DVec3::ZERO)
            .normalize();
        let east = if sun.z.abs() < 0.99 {
            DVec3::Z.cross(sun).normalize()
        } else {
            DVec3::X.cross(sun).normalize()
        };
        (sun + east * 0.7 + DVec3::Z * 0.15).normalize()
    };
    let direction = fleet
        .frames()
        .transform(fleet.body_frames(body).1, fleet.origin_frame())
        .apply_direction(local);
    let ratio = match view {
        "near" => 1.025,
        "orbit" => {
            if fleet.ephemeris.bodies()[body].id.rsplit('/').next() == Some("halo") {
                6.0
            } else {
                3.5
            }
        }
        "far" => 12.0,
        _ => panic!("view must be near/orbit/far"),
    };
    lab.session.execute(Action::View {
        command: ViewCommand::BodyPreset {
            body,
            direction,
            distance: radius * ratio,
        },
    });
    lab.session.execute(Action::View {
        command: ViewCommand::Exposure {
            value: if emissive { 0.1 } else { 6.309_573 },
        },
    });
    lab.notice = format!(
        "{} {} scenery; O launches an orbital fixture, Home returns to ship",
        lab.session.sim().fleet.ephemeris.bodies()[body].name,
        view
    );
}

fn reentry_fixture(lab: &mut Lab) {
    let sim = lab.session.sim();
    let fleet = &sim.fleet;
    let transform = fleet
        .frames()
        .transform(fleet.body_frames(sim.home).1, fleet.origin_frame());
    let local_velocity = DVec3::new(-500.0, 7500.0, 0.0);
    let state = transform.apply_state(void_frames::State {
        position: DVec3::X * (fleet.ephemeris.bodies()[sim.home].radius_meters + 110000.0),
        velocity: local_velocity,
    });
    let rotation =
        transform.rotation() * DQuat::from_rotation_arc(-DVec3::Y, local_velocity.normalize());
    let Outcome::Spawned(vessel) = lab.session.execute(Action::LaunchState {
        craft: lab.craft.clone(),
        position: state.position,
        velocity: state.velocity,
        rotation,
        angular_velocity: DVec3::ZERO,
    }) else {
        unreachable!()
    };
    select_pilot(lab, &vessel);
    lab.session.execute(Action::Sas { enabled: true });
    lab.paused = true;
    lab.rate = 0;
    lab.session.execute(Action::EndFrame {
        paused: true,
        rate: 0,
    });
    lab.notice="Reentry: 110 km, 7.5 km/s surface flow, shield forward. P resumes; thermal HUD shows skin/core, ablator and failure.".into();
}

fn rendezvous_fixture(lab: &mut Lab) {
    let Outcome::Spawned(a) = lab.session.execute(Action::LaunchOrbit {
        craft: lab.craft.clone(),
        offset: DVec3::ZERO,
    }) else {
        unreachable!()
    };
    select_pilot(lab, &a);
    let own = lab
        .own_port
        .clone()
        .expect("--rendezvous craft requires a docking port");
    let ship = lab.session.sim().fleet.snapshot(&a);
    let (mount, normal) = port_pose(lab, &own);
    let rotation = ship.rotation * DQuat::from_rotation_z(std::f64::consts::PI);
    let Outcome::Spawned(b) = lab.session.execute(Action::LaunchState {
        craft: lab.craft.clone(),
        position: ship.position + ship.rotation * mount - rotation * mount
            + ship.rotation * normal * 0.15,
        velocity: ship.velocity,
        rotation,
        angular_velocity: DVec3::ZERO,
    }) else {
        unreachable!()
    };
    lab.target_port = ports(lab, false).into_iter().find(|p| p.vessel == b);
    for p in [Some(own), lab.target_port.clone()].into_iter().flatten() {
        lab.session.execute(Action::ArmDock {
            part: p.part,
            module: p.module,
            armed: false,
        });
    }
    // A quarter orbit around the local up axis reveals both nose-to-nose rockets.
    lab.session.execute(Action::View {
        command: ViewCommand::Drag {
            x: std::f64::consts::FRAC_PI_2 / 0.005,
            y: 0.0,
        },
    });
    lab.paused = true;
    lab.rate = 0;
    lab.session.execute(Action::EndFrame {
        paused: true,
        rate: 0,
    });
    lab.notice = "Rendezvous preset: nose ports within capture range; arm both with F12 then Enter. P resumes.".into();
}
fn cycle_port(selected: &mut Option<PortAddress>, candidates: Vec<PortAddress>) {
    *selected = if candidates.is_empty() {
        None
    } else {
        let next = selected
            .as_ref()
            .and_then(|p| candidates.iter().position(|c| c == p))
            .map_or(0, |i| (i + 1) % candidates.len());
        Some(candidates[next].clone())
    };
}
// Port highlights are derived UI state, not checkpoint/journal data. Revalidate ownership
// after keyboard actions, replay and accepted simulation steps before drawing.
fn refresh_ports(lab: &mut Lab) {
    let own = ports(lab, true);
    let mut target = ports(lab, false);
    let f = &lab.session.sim().fleet;
    target.sort_by(|a, b| {
        f.relative(&a.vessel, &lab.session.sim().selected)
            .position
            .length_squared()
            .total_cmp(
                &f.relative(&b.vessel, &lab.session.sim().selected)
                    .position
                    .length_squared(),
            )
    });
    if lab.own_port.as_ref().is_none_or(|p| !own.contains(p)) {
        lab.own_port = own.first().cloned();
    }
    if lab.target_port.as_ref().is_none_or(|p| !target.contains(p)) {
        lab.target_port = target.first().cloned();
    }
}
fn docking_controls(lab: &mut Lab, keys: &ButtonInput<KeyCode>) {
    refresh_ports(lab);
    let own = ports(lab, true);
    let target = ports(lab, false);
    if keys.just_pressed(KeyCode::F10) {
        cycle_port(&mut lab.own_port, own);
    }
    if keys.just_pressed(KeyCode::F11) {
        cycle_port(&mut lab.target_port, target);
    }
    if keys.just_pressed(KeyCode::F12) {
        for p in [lab.own_port.clone(), lab.target_port.clone()]
            .into_iter()
            .flatten()
        {
            lab.session.execute(Action::ArmDock {
                part: p.part,
                module: p.module,
                armed: true,
            });
        }
        lab.notice = "Selected ports armed".into();
    }
    let action = if keys.just_pressed(KeyCode::Enter) {
        match (&lab.own_port, &lab.target_port) {
            (Some(a), Some(b)) => Some(Action::Dock {
                part_a: a.part.clone(),
                module_a: a.module.clone(),
                part_b: b.part.clone(),
                module_b: b.module.clone(),
            }),
            _ => {
                lab.notice = "Dock refused: select own and target ports".into();
                None
            }
        }
    } else if keys.just_pressed(KeyCode::Backspace) {
        lab.own_port.as_ref().map(|p| Action::Undock {
            part: p.part.clone(),
            module: p.module.clone(),
        })
    } else {
        None
    };
    if let Some(action) = action {
        neutral_pilot(lab);
        match lab.session.execute(action) {
            Outcome::Spawned(_) => {
                let id = lab.session.sim().selected.clone();
                select_pilot(lab, &id);
                lab.dirty = true;
                lab.prediction = None;
                lab.notice = "Docking topology updated".into();
            }
            Outcome::Refused(reason) => lab.notice = format!("Docking refused: {reason}"),
            other => panic!("unexpected docking outcome {other:?}"),
        }
    }
}
/// Main-game acceptance starting sites, expressed through the ordinary InitialWorld contract.
/// No alternate camera or physics runtime is created.
fn cinder_fixture(initial: &mut InitialWorld, site: &str) {
    let description = initial
        .world
        .bodies
        .get("cinder")
        .expect("Cinder fixture needs solar scenery");
    let Some(void_terrain::TerrainConfig::Impact(options)) = &description.terrain else {
        panic!("Cinder fixture requires impact terrain");
    };
    let direction = match site {
        "basin" => DVec3::from_array(options.basins[0].direction),
        "rim" => {
            let basin = &options.basins[0];
            let center = DVec3::from_array(basin.direction);
            (center
                + center.cross(DVec3::Z).normalize()
                    * (basin.radius_meters / options.radius_meters * 1.05))
                .normalize()
        }
        "ejecta" => {
            let impact = &options.rayed_impacts[1];
            let center = DVec3::from_array(impact.direction);
            (center
                + center.cross(DVec3::Z).normalize()
                    * (impact.radius_meters / options.radius_meters * 1.7))
                .normalize()
        }
        _ => panic!("--cinder-site must be basin/rim/ejecta"),
    };
    initial.launch_body = "cinder".into();
    initial.launch_site = direction;
}

fn ares_fixture(initial: &mut InitialWorld, site: &str) {
    let Some(void_terrain::TerrainConfig::Ares(options)) = &initial.world.bodies["ares"].terrain
    else {
        panic!("Ares fixture needs Ares terrain");
    };
    initial.launch_body = "ares".into();
    initial.launch_site = match site {
        "plains" => DVec3::new(-0.3, 0.7, 0.65).normalize(),
        "canyon" => {
            let (center, _, across) = options.canyon_frame();
            (center - across * 21_000.0 / options.impact.radius_meters).normalize()
        }
        "volcano" => DVec3::from_array(options.volcanoes[0].direction),
        _ => panic!("--ares-site must be plains/canyon/volcano"),
    };
}

fn vesper_fixture(initial: &mut InitialWorld, site: &str) {
    let Some(void_terrain::TerrainConfig::Volcanic(options)) =
        &initial.world.bodies["vesper"].terrain
    else {
        panic!("Vesper fixture requires volcanic terrain");
    };
    let volcanic = void_terrain::Volcanic::new(options);
    initial.launch_body = "vesper".into();
    initial.launch_site = match site {
        "plains" => initial
            .world
            .daylight_terrain_site("vesper")
            .expect("Vesper daylight terrain site"),
        "shield" | "upland" => {
            let built = initial.world.build();
            let source = built.ephemeris.as_ref();
            let frames = void_orbit::SystemFrames::new(source);
            let sun = frames
                .tree
                .at(0.0, source)
                .transform(
                    frames.inertial[initial.world.body_index("sol")],
                    frames.surface[initial.world.body_index("vesper")],
                )
                .apply_point(DVec3::ZERO)
                .normalize();
            if site == "shield" {
                volcanic.sunlit_shield_rim(sun)
            } else {
                volcanic.sunlit_upland(sun)
            }
        }
        _ => panic!("--vesper-site must be plains/shield/upland"),
    };
}

fn scenery_description(sim: &void_fleet_flight::FleetFlight) -> String {
    let id = &sim.fleet.ephemeris.bodies()[sim.observation_body()].id;
    match sim.world.bodies.get(id) {
        Some(d) => format!(
            "scenery {} / {} | optical air {} | physical air {} | exposure {:.3}",
            id,
            match d.visual.surface {
                void_scenery::solar::SurfaceRecipe::SolidSurface => "solid LOD",
                void_scenery::solar::SurfaceRecipe::Regolith => "regolith LOD",
                void_scenery::solar::SurfaceRecipe::MartianRegolith => "Ares dry volcanic LOD",
                void_scenery::solar::SurfaceRecipe::GasEnvelope { .. } => "gas visual",
                void_scenery::solar::SurfaceRecipe::EmissiveStar { .. } => "emissive star",
            },
            d.visual.atmosphere,
            d.air_density_scale.is_some(),
            sim.presentation.exposure
        ),
        None => format!("scenery {} / map sphere; no authored terrain or optics", id),
    }
}

fn plotting_description(sim: &void_fleet_flight::FleetFlight) -> String {
    use void_orbit::FrameSpec;
    let bodies = sim.fleet.ephemeris.bodies();
    match sim.presentation.plotting_frame {
        FrameSpec::Barycentric => "plot: barycentric".into(),
        FrameSpec::BodyInertial { body } => format!("plot: body inertial / {}", bodies[body].name),
        FrameSpec::BodySurface { body } => format!("plot: body surface / {}", bodies[body].name),
        FrameSpec::TwoBodyRotating { primary, secondary } => format!(
            "plot: two-body rotating / {} + {}",
            bodies[primary].name, bodies[secondary].name
        ),
    }
}

fn pilot_description(sim: &void_fleet_flight::FleetFlight) -> String {
    use void_assembly::ControlProfile;
    match sim.fleet.control_profile(&sim.selected) {
        Some(ControlProfile::Rover) => "P pause | W/S drive | A/D steer | Space brake | X park | F exit seat".into(),
        Some(ControlProfile::Aircraft) => "P pause | Space ignite/stage | Shift/Ctrl throttle | W/S pitch | A/D roll | Q/E yaw/steer | B brake".into(),
        Some(ControlProfile::Eva) => {
            let crew = sim.fleet.eva_crew(&sim.selected).expect("EVA profile crew");
            let grounded = sim.fleet.part_snapshots(&sim.selected).iter().any(|p| p.modules.values().any(|m| matches!(m,void_assembly::ModuleState::Crew { grounded:true,.. })));
            let fuel: f64 = sim.fleet.part_snapshots(&sim.selected).iter().map(|p|p.resources.get(&void_assembly::ResourceId::Monopropellant).copied().unwrap_or(0.0)).sum();
            format!("{} | {} | pack {} {:.2}kg\nP pause | W/S walk | A/D strafe | Q/E turn | Space jump | H pack | F board\nPack ON: Alt+W/S forward/back, D/A right/left, E/Q up/down | WASD QE torque",crew.name,if grounded { "grounded" } else { "airborne" },if sim.fleet.rcs_control(&sim.selected).enabled { "ON" } else { "OFF" },fuel)
        },
        _ => "P pause | Space stage | Shift/Ctrl throttle | X cut | WASD QE turn | T SAS".into(),
    }
}

fn vehicle_description(sim: &void_fleet_flight::FleetFlight) -> String {
    let Some(control) = sim.fleet.vehicle_control(&sim.selected) else {
        return String::new();
    };
    let wheels: Vec<_> = sim
        .fleet
        .part_snapshots(&sim.selected)
        .into_iter()
        .flat_map(|p| p.modules.into_values())
        .filter_map(|m| match m {
            void_assembly::ModuleState::Wheel { state, .. } => Some(state),
            _ => None,
        })
        .collect();
    if sim.fleet.control_profile(&sim.selected) == Some(void_assembly::ControlProfile::Aircraft) {
        return format!(
            "\nLanding gear: {}/{} supported | brake {:.0}% | steer {:.0}%",
            wheels.iter().filter(|s| s.grounded).count(),
            wheels.len(),
            control.brake * 100.0,
            control.steer * 100.0
        );
    }
    format!(
        "\nDRIVE {:.0}% steer {:.0}% brake {:.0}% | tires {}/{} grounded\nW/S drive | A/D steer | Space brake (latched) | X parking toggle; W/S releases brake",
        control.drive * 100.0,
        control.steer * 100.0,
        control.brake * 100.0,
        wheels.iter().filter(|s| s.grounded).count(),
        wheels.len()
    )
}

fn thermal_description(lab: &Lab) -> String {
    let parts = lab
        .session
        .sim()
        .fleet
        .part_snapshots(&lab.session.sim().selected);
    let mut hottest = (0.0, String::new());
    let mut core = 0.0_f64;
    let mut failed = 0;
    let mut shields = String::new();
    for p in parts {
        for state in p.modules.values() {
            if let void_assembly::ModuleState::Thermal { state } = state {
                if state.skin_k > hottest.0 {
                    hottest = (state.skin_k, p.id.clone());
                }
                core = core.max(state.core_k);
                failed += usize::from(state.failed);
                if let Some(m) = p.resources.get(&void_assembly::ResourceId::Ablator) {
                    shields.push_str(&format!(
                        "\n{} skin {:.0} K core {:.0} K ablator {:.3} kg{}",
                        p.id,
                        state.skin_k,
                        state.core_k,
                        m,
                        if state.failed { " FAILED" } else { "" }
                    ));
                }
            }
        }
    }
    if hottest.0 == 0.0 {
        String::new()
    } else {
        format!(
            "Heat: {} skin {:.0} K | max core {:.0} K | failed {}{}",
            hottest.1, hottest.0, core, failed, shields
        )
    }
}

fn docking_description(lab: &Lab) -> String {
    let f = &lab.session.sim().fleet;
    let id = &lab.session.sim().selected;
    let rcs = f.rcs_control(id);
    let allocation = f.rcs_allocation(id);
    let mono: f64 = f
        .part_snapshots(id)
        .iter()
        .map(|p| {
            p.resources
                .get(&void_assembly::ResourceId::Monopropellant)
                .copied()
                .unwrap_or(0.0)
        })
        .sum();
    let label = |port: &Option<PortAddress>| {
        port.as_ref().map_or("none".into(), |p| {
            format!(
                "{}/{} {:?}",
                p.part,
                p.module,
                f.parts().part(&p.part).modules[&p.module]
            )
        })
    };
    let mut text = format!(
        "RCS {} mono {:.3}kg | delivered F {:.1}N τ {:.1}Nm | residual {:.1}N/{:.1}Nm\nH RCS | manual RCS torque disengages SAS reaction wheel | Alt+W/S ±Z D/A ±X E/Q ±Y translate | WASD QE RCS torque\nF10 own {} | F11 target {} | F12 arm both | Enter dock | Backspace undock",
        if rcs.enabled { "ON" } else { "OFF" },
        mono,
        allocation.force.length(),
        allocation.torque.length(),
        allocation.force_residual.length(),
        allocation.torque_residual.length(),
        label(&lab.own_port),
        label(&lab.target_port)
    );
    if let (Some(a), Some(b)) = (&lab.own_port, &lab.target_port) {
        let sa = f.snapshot(&a.vessel);
        let sb = f.snapshot(&b.vessel);
        let (pa, na) = port_pose(lab, a);
        let (pb, nb) = port_pose(lab, b);
        let relative = f.relative(&b.vessel, &a.vessel);
        let ar = sa.rotation * pa;
        let br = sb.rotation * pb;
        text.push_str(&format!(
            "\nPorts {:.3}m | speed {:.3}m/s | angle {:.2}° | spin {:.3}rad/s",
            (relative.position + br - ar).length(),
            (relative.velocity + sb.angular_velocity.cross(br) - sa.angular_velocity.cross(ar))
                .length(),
            (-(sa.rotation * na).dot(sb.rotation * nb))
                .clamp(-1.0, 1.0)
                .acos()
                .to_degrees(),
            (sb.angular_velocity - sa.angular_velocity).length()
        ));
    }
    text
}

const RATES: [f64; 9] = crate::flight::TIME_RATES;
struct Lab {
    main_game: bool,
    rendezvous: bool,
    reentry: bool,
    water_review: Option<[f64; 3]>,
    own_port: Option<PortAddress>,
    target_port: Option<PortAddress>,
    pointer_over_label: bool,
    view: Option<void_view::ViewState>,
    eye: DVec3,
    focus_position: DVec3,
    orbits: void_view::MapOrbits,
    path: void_view::MapPath,
    body_plots: void_view::plot::BodyPlots,
    plot_path: void_view::plot::PlotPath,
    plot_plan: void_view::plot::PlotPath,
    plan_vessel: String,
    prediction_at: f64,
    prediction_generation: u64,
    session: FlightSession,
    save_path: std::path::PathBuf,
    record_path: Option<std::path::PathBuf>,
    frames: usize,
    playback: Option<Playback>,
    profile: Option<(void_diagnostics::Profiler, std::path::PathBuf)>,
    craft: Craft,
    paused: bool,
    rate: usize,
    dirty: bool,
    notice: String,
    spawned: u32,
    parts: HashMap<String, Vec<Entity>>,
    collision: HashMap<(u64, String), Vec<Vec3>>,
    prediction: Option<void_landing::CoastPrediction>,
}
#[derive(Resource)]
enum Ground {
    Plain(Box<TileField>, Handle<StandardMaterial>),
    World(Box<crate::world_scenery::WorldScenery>),
}
impl Ground {
    fn reset(&mut self, planet: &void_landing::LandingPlanet) {
        if let Self::Plain(field, material) = self {
            let demo = demo_rocket(&planet.terrain);
            **field = TileField::new(
                landing_lod_options(&planet.terrain, &demo.options.contact),
                Some(planet.terrain.clone()),
                material.clone(),
            );
        }
        // World GPU/sampler caches are keyed by immutable world configuration, not Arc address.
    }
    fn finish_builds(&mut self) {
        match self {
            Self::Plain(f, _) => f.finish_builds(),
            Self::World(w) => w.finish_builds(),
        }
    }
    fn readiness(&self) -> (usize, usize, usize, usize) {
        match self {
            Self::Plain(f, _) => (
                f.building_count(),
                f.last_requests,
                f.drawn_count(),
                f.lod.cached_mesh_bytes(),
            ),
            Self::World(w) => w.readiness(),
        }
    }
    fn max_level(&self) -> u32 {
        match self {
            Self::Plain(f, _) => f.lod.options.max_level,
            Self::World(w) => w.max_level(),
        }
    }
    fn select(&mut self, view: &LodView) {
        match self {
            Self::Plain(f, _) => f.select(view),
            Self::World(w) => w.select(view),
        }
    }
    fn set_wireframe(&mut self, c: &mut Commands, on: bool) {
        match self {
            Self::Plain(f, _) => f.set_wireframe(c, on),
            Self::World(w) => w.set_wireframe(c, on),
        }
    }
    fn boundaries(&self, eye: DVec3) -> Vec<Vec<Vec3>> {
        match self {
            Self::Plain(f, _) => f.boundaries(eye),
            Self::World(w) => w.boundaries(),
        }
    }
    fn draw<F: bevy::ecs::query::QueryFilter>(
        &mut self,
        c: &mut Commands,
        m: &mut Assets<Mesh>,
        t: &mut Query<&mut Transform, F>,
        eye: DVec3,
    ) {
        match self {
            Self::Plain(f, _) => f.draw(c, m, t, eye),
            Self::World(w) => w.draw(c, m, t),
        }
    }
}
/// A part's placement in the render world, from its own parts frame.
fn part_transform(
    to_camera: &mut impl FnMut(void_frames::FrameId) -> void_frames::Transform,
    p: &void_vessels::PartSnapshot,
) -> Transform {
    let into = to_camera(p.frame);
    Transform::from_translation(into.apply_point(p.local_position).as_vec3())
        .with_rotation((into.rotation() * p.local_rotation).as_quat())
}
impl Drop for Lab {
    fn drop(&mut self) {
        if !std::thread::panicking()
            && let Some((profile, path)) = &self.profile
        {
            profile.write(path);
        }
        if !std::thread::panicking()
            && let Some(path) = self.record_path.take()
        {
            self.session.finish_stream();
            eprintln!("Fleet recording saved: {}", path.display());
        }
    }
}
#[derive(Component)]
struct LabCamera;
#[derive(Component)]
struct SceneSun;
#[derive(Component)]
struct Hud;
#[derive(Component)]
struct NavballHeading;
#[derive(Component)]
struct Visual {
    id: String,
    local: Transform,
    flame: bool,
    wheel: Option<String>,
}
fn argument(name: &str) -> Option<String> {
    let args: Vec<_> = std::env::args().collect();
    args.iter()
        .position(|s| s == name)
        .map(|i| args.get(i + 1).expect("argument needs a value").clone())
}
pub fn run(main_game: bool) {
    if std::env::args().any(|a| a == "--help") {
        println!(
            "VOID flight: --planet <id> --terrain <config> --craft <json> --vacuum\n--world <initial-world.json> | --body <id> --view near|orbit|far --exposure <0..100>\n--cinder-site basin|rim|ejecta; --ares-site plains|canyon|volcano: paused main-game surface fixture; --ares-overview: recorded main-camera overview\n--vesper-site plains|shield|upland: paused Vesper volcanic ground fixture\n--rover: four-wheel ground craft; W/S drive, A/D steer, Space brake, X parking brake\n--aircraft: modular jet on explicit near-flat atmospheric runway world\n--stellar-neighborhood: three fictional systems at real stellar separation\n--stellar-fixture: declared remote ground/orbit starting ships for acceptance\n--splashdown: paused ocean capsule; --water-speed m/s --water-tilt degrees --water-entry-angle degrees; R repeat, Shift+R next\n--reentry: paused shielded capsule at 110 km\n--rendezvous: paused opposed nose ports in orbit (requires port-equipped craft; incompatible with load/replay)\n--record <journal> --replay <journal> --verify <journal> --save <checkpoint> --load <checkpoint>\nH RCS | Alt+W/S ±Z, D/A ±X, E/Q ±Y translation | WASD QE torque | T SAS reaction wheel\nF10 own port | F11 target port | F12 arm both | Enter dock | Backspace undock\nP pause | Tab vessel | Space stage | F6 save | F7 load | F8 finish recording\n1–4/G plot frames | J primary / Shift+J secondary | F1 body views | Home ship\nO orbit around observed body | Alt+F10/F11 exposure"
        );
        return;
    }
    if let Some(path) = argument("--recover-recording") {
        let output = argument("--output").expect("--recover-recording requires --output");
        let recovery = void_fleet_flight::session::durable::Recovery::read(path);
        recovery.write(&output);
        println!(
            "Recovered Fleet recording: {} committed actions, pending command {}, {} EOF bytes discarded; report {}.recovery.json",
            recovery.recording.entries.len(),
            recovery.pending.is_some(),
            recovery.discarded_tail_bytes,
            output
        );
        return;
    }
    if let Some(path) = argument("--verify-save") {
        let session = FlightSession::load_checkpoint(&path);
        println!(
            "Verified Fleet world save: T+{:.6} s, {} vessels, selected {}",
            session.sim().fleet.time(),
            session.sim().fleet.vessel_ids().len(),
            session.sim().selected
        );
        return;
    }
    if let Some(path) = argument("--verify") {
        let mut profile = void_diagnostics::Profiler::new();
        let started = std::time::Instant::now();
        let session = FlightSession::load(&path);
        profile.span("headless_verify", started, std::time::Instant::now());
        if let Some(output) = argument("--profile") {
            profile.write(output);
        }
        println!(
            "Verified Fleet session: T+{:.6} s, {} vessels, selected {}",
            session.sim().fleet.time(),
            session.sim().fleet.vessel_ids().len(),
            session.sim().selected
        );
        return;
    }
    if main_game && std::env::args().any(|a| a == "--rover") {
        assert!(
            !std::env::args().any(|a| matches!(
                a.as_str(),
                "--reentry"
                    | "--rendezvous"
                    | "--aircraft"
                    | "--load"
                    | "--replay"
                    | "--world"
                    | "--craft"
            )),
            "--rover is a new craft fixture and cannot replace explicit craft/world/load/replay or another fixture"
        );
    }
    if main_game && std::env::args().any(|a| a == "--splashdown") {
        assert!(
            !std::env::args().any(|a| [
                "--rover",
                "--aircraft",
                "--reentry",
                "--rendezvous",
                "--load",
                "--replay"
            ]
            .contains(&a.as_str())),
            "--splashdown is an explicit ocean fixture incompatible with other fixtures/load/replay"
        );
    }
    let aircraft_mode = main_game && std::env::args().any(|a| a == "--aircraft");
    if aircraft_mode {
        assert!(
            argument("--world").is_none()
                && argument("--terrain").is_none()
                && !std::env::args()
                    .any(|a| ["--reentry", "--rendezvous", "--vacuum"].contains(&a.as_str())),
            "--aircraft is an explicit atmospheric runway world; incompatible with --world/--terrain/--reentry/--rendezvous/--vacuum"
        );
    }
    let id = argument("--planet").unwrap_or(if aircraft_mode { "terra" } else { "aurelia" }.into());
    let requested_terrain = argument("--terrain");
    let mut planet = game_planet_by_id(
        &id,
        if aircraft_mode {
            Some("hills")
        } else {
            requested_terrain.as_deref()
        },
    );
    if aircraft_mode {
        planet.planet = void_fleet_flight::aircraft_acceptance_planet(planet.planet);
        planet.terrain_id = crate::flight::GameTerrain::Hills;
        planet.ocean = false;
        planet.sea_level = 0.0;
        planet.rock_height = 10.0;
        planet.snow_height = 100.0;
        // This explicit acceptance fixture starts on the day side of the authored light.
        planet.launch_site = Some(DVec3::new(0.8, -0.55, 0.25).normalize());
    }
    if main_game && std::env::args().any(|a| a == "--rover") && id == "terra" {
        // Keep the actual Hills terrain; select a sunlit starting site for visual acceptance.
        planet.launch_site = Some(DVec3::new(0.8, -0.55, 0.25).normalize());
    }
    let craft = argument("--craft").map_or_else(
        || {
            if aircraft_mode {
                void_assembly::aircraft()
            } else if main_game && std::env::args().any(|a| a == "--rover") {
                void_assembly::crew_rover()
            } else if main_game && std::env::args().any(|a| a == "--reentry") {
                void_assembly::reentry_capsule()
            } else if main_game {
                void_assembly::rcs_flight_rocket()
            } else {
                void_assembly::flight_rocket()
            }
        },
        |path| {
            import_craft(&std::fs::read_to_string(path).expect("read craft"))
                .expect("invalid craft")
        },
    );
    let site = planet
        .launch_site
        .unwrap_or_else(|| demo_rocket(&planet.planet.terrain).launch_site.normalize());
    let air =
        planet.planet.air_density_scale.is_some() && !std::env::args().any(|a| a == "--vacuum");
    let replay_path = argument("--replay");
    let surface_fixture_count = ["--cinder-site", "--ares-site", "--vesper-site"]
        .iter()
        .filter(|name| argument(name).is_some())
        .count();
    if surface_fixture_count > 0 {
        assert!(
            surface_fixture_count == 1,
            "surface fixtures cannot be combined"
        );
        assert!(
            argument("--world").is_none()
                && argument("--load").is_none()
                && replay_path.is_none()
                && argument("--planet").is_none()
                && argument("--terrain").is_none()
                && !std::env::args().any(|a| matches!(
                    a.as_str(),
                    "--reentry"
                        | "--rendezvous"
                        | "--aircraft"
                        | "--rover"
                        | "--stellar-neighborhood"
                        | "--stellar-fixture"
                        | "--splashdown"
                )),
            "surface fixture cannot override world/load/replay/planet/terrain or other fixtures"
        );
    }
    assert!(
        argument("--world").is_none() || (argument("--load").is_none() && replay_path.is_none()),
        "--world cannot override a checkpoint or replay world"
    );
    assert!(
        replay_path.is_none() || (argument("--load").is_none() && argument("--record").is_none()),
        "--replay cannot be combined with --load or --record"
    );
    let session = argument("--load").map_or_else(
        || {
            if let Some(path) = argument("--world") {
                assert!(
                    argument("--planet").is_none()
                        && argument("--terrain").is_none()
                        && argument("--craft").is_none()
                        && !std::env::args().any(|a| a == "--vacuum"),
                    "--world cannot be mixed with planet/terrain/craft/vacuum overrides"
                );
                let initial: InitialWorld =
                    serde_json::from_str(&std::fs::read_to_string(path).expect("read world"))
                        .expect("invalid initial world");
                return FlightSession::new(stellar_fixture_initial(initial));
            }
            let mut initial = InitialWorld::new(&planet.planet, &craft, site, air);
            if main_game && !aircraft_mode && planet.planet.body_id == "aurelia" {
                initial.world = void_fleet_flight::world::expanded_solar_scenery(&planet.planet);
                if !air {
                    for body in initial.world.bodies.values_mut() {
                        body.air_density_scale = None;
                        body.visual.atmosphere = false;
                        body.visual.scattering = None;
                        body.visual.clouds = false;
                        body.visual.cloud_profile = None;
                    }
                }
            }
            if std::env::args().any(|a| a == "--stellar-neighborhood") {
                assert!(
                    main_game && planet.planet.body_id == "aurelia",
                    "--stellar-neighborhood requires the main Aurelia game"
                );
                assert!(
                    air,
                    "stellar neighborhood currently uses explicit authored air profiles"
                );
                initial.world = void_fleet_flight::world::stellar_neighborhood(&planet.planet);
                initial.launch_body = "Sol/aurelia".into();
            }
            if main_game
                && std::env::args().any(|a| a == "--rover")
                && planet.planet.body_id == "aurelia"
            {
                initial.launch_site = initial
                    .world
                    .daylight_terrain_site(&initial.launch_body)
                    .expect("rover fixture daylight terrain site");
            }
            if let Some(site) = argument("--cinder-site") {
                assert!(main_game, "Cinder fixture is a main-game entry");
                cinder_fixture(&mut initial, &site);
            }
            if let Some(site) = argument("--ares-site") {
                assert!(main_game, "Ares fixture is a main-game entry");
                ares_fixture(&mut initial, &site);
            }
            if let Some(site) = argument("--vesper-site") {
                assert!(main_game, "Vesper fixture is a main-game entry");
                vesper_fixture(&mut initial, &site);
            }
            FlightSession::new(stellar_fixture_initial(if main_game {
                initial.with_air_dynamics(void_vessels::AirDynamics::ForceAndTorque)
            } else {
                initial
            }))
        },
        FlightSession::load_checkpoint,
    );
    let craft = session.recording_initial().craft.clone();
    let mut lab = new_lab(session, craft);
    if aircraft_mode {
        lab.notice = "Aircraft runway fixture: Space ignite | Shift throttle | B hold brakes | W/S elevator | A/D roll | Q/E yaw and nose steering".into();
    }
    lab.main_game = main_game;
    lab.rendezvous = main_game && std::env::args().any(|a| a == "--rendezvous");
    lab.reentry = main_game && std::env::args().any(|a| a == "--reentry");
    assert!(
        !(lab.rendezvous && lab.reentry),
        "choose either --reentry or --rendezvous"
    );
    assert!(
        !lab.reentry || (argument("--load").is_none() && replay_path.is_none()),
        "--reentry cannot be combined with --load or --replay"
    );
    assert!(
        !lab.rendezvous || (argument("--load").is_none() && replay_path.is_none()),
        "--rendezvous cannot be combined with --load or --replay"
    );
    lab.paused = !main_game
        || argument("--load").is_some()
        || argument("--cinder-site").is_some()
        || argument("--ares-site").is_some()
        || argument("--vesper-site").is_some();
    if argument("--load").is_none() && replay_path.is_none() {
        lab.session.execute(Action::View {
            command: ViewCommand::Configure {
                main_camera: main_game,
            },
        });
        lab.session.execute(Action::EndFrame {
            paused: lab.paused,
            rate: lab.rate,
        });
    }
    if let Some(path) = replay_path {
        let (playback, session) = Playback::new(Recording::read(path));
        lab.session = session;
        lab.craft = lab.session.recording_initial().craft.clone();
        lab.playback = Some(playback);
        lab.paused = false;
    }
    lab.save_path = argument("--save")
        .unwrap_or("lab-log/fleet-save.json".into())
        .into();
    lab.record_path = argument("--record").map(Into::into);
    if let Some(path) = &lab.record_path {
        lab.session.begin_stream(path);
    }
    if lab.rendezvous && argument("--load").is_none() && lab.playback.is_none() {
        rendezvous_fixture(&mut lab);
    }
    if lab.reentry {
        reentry_fixture(&mut lab);
    }
    if std::env::args().any(|a| a == "--stellar-fixture") {
        assert!(
            main_game && lab.session.sim().world.stellar.is_some(),
            "--stellar-fixture needs a stellar world"
        );
        assert!(
            argument("--load").is_none() && lab.playback.is_none(),
            "stellar fixture cannot override load/replay"
        );
        let site = lab
            .session
            .sim()
            .world
            .daylight_terrain_site("Beryl/aurelia")
            .expect("stellar fixture Beryl daylight terrain site");
        lab.session.execute(Action::LaunchGroundAt {
            body: "Beryl/aurelia".into(),
            craft: lab.craft.clone(),
            site,
        });
        lab.session.execute(Action::LaunchOrbitAt {
            body: "Cygnus/aurelia".into(),
            craft: lab.craft.clone(),
            offset: DVec3::ZERO,
        });
        lab.paused = true;
        lab.notice = "STELLAR ACCEPTANCE FIXTURE: Sol/Beryl ground ships on real daylight terrain; Cygnus orbit is a declared starting state, not a completed interstellar trip. Tab selects ship.".into();
        lab.session.execute(Action::EndFrame {
            paused: true,
            rate: lab.rate,
        });
    }
    if main_game && std::env::args().any(|a| a == "--splashdown") {
        assert!(
            argument("--load").is_none() && lab.playback.is_none(),
            "splashdown cannot replace load/replay"
        );
        let number = |name: &str, default: f64| {
            argument(name).map_or(default, |s| {
                s.parse::<f64>()
                    .unwrap_or_else(|_| panic!("invalid {name}: {s}"))
            })
        };
        let speed = number("--water-speed", 2.);
        let tilt = number("--water-tilt", 0.);
        let entry = number("--water-entry-angle", 0.);
        assert!(
            speed.is_finite()
                && speed >= 0.
                && tilt.is_finite()
                && tilt.abs() <= 180.
                && entry.is_finite()
                && (0. ..=90.).contains(&entry),
            "invalid splashdown parameters"
        );
        lab.water_review = Some([speed, tilt, entry]);
        water_fixture(&mut lab);
    }
    if let Some(body) = argument("--body") {
        assert!(
            argument("--replay").is_none(),
            "--body cannot override replay camera"
        );
        let index = lab.session.sim().world.body_index(&body);
        scenery_preset(
            &mut lab,
            index,
            &argument("--view").unwrap_or("orbit".into()),
        );
    } else {
        assert!(argument("--view").is_none(), "--view requires --body");
    }
    if std::env::args().any(|a| a == "--ares-overview") {
        let site = argument("--ares-site").expect("--ares-overview requires --ares-site");
        assert!(
            argument("--body").is_none(),
            "Ares overview cannot override a body view"
        );
        let distance: f64 = match site.as_str() {
            "canyon" => 650_000.0,
            "volcano" => 700_000.0,
            "plains" => 200_000.0,
            _ => unreachable!(),
        };
        for command in [
            ViewCommand::Focus { body: None },
            ViewCommand::Drag { x: 0.0, y: 350.0 },
            ViewCommand::Zoom {
                pixels: -(distance / 40.0).ln() / 0.002,
            },
        ] {
            lab.session.execute(Action::View { command });
        }
        lab.notice = format!("Ares {site} overview; Home returns to the ground craft");
    }
    if matches!(
        argument("--vesper-site").as_deref(),
        Some("shield" | "upland")
    ) {
        // Same main camera and journalled view commands, viewing actual ground through normal haze.
        lab.session.execute(Action::View {
            command: ViewCommand::Zoom { pixels: -2500.0 },
        });
        lab.session.execute(Action::View {
            command: ViewCommand::Drag { x: 0.0, y: 45.0 },
        });
    }
    if let Some(value) = argument("--exposure") {
        assert!(
            argument("--replay").is_none(),
            "exposure cannot override replay"
        );
        lab.session.execute(Action::View {
            command: ViewCommand::Exposure {
                value: value.parse().expect("exposure number"),
            },
        });
    }
    lab.profile =
        argument("--profile").map(|path| (void_diagnostics::Profiler::new(), path.into()));
    let benchmark_path = argument("--render-benchmark");
    let render_path = argument("--render-profile");
    assert!(
        benchmark_path.is_none() || render_path.is_none(),
        "choose --render-benchmark or --render-profile"
    );
    assert!(
        benchmark_path.is_none() || (lab.playback.is_none() && lab.record_path.is_none()),
        "benchmark cannot record or replay pilot input"
    );
    let benchmark = benchmark_path
        .as_ref()
        .map(|_| RenderBenchmark::from_arguments());
    if let Some(config) = &benchmark {
        lab.paused = true;
        if argument("--load").is_some() {
            assert!(
                argument("--benchmark-scenario").is_none(),
                "benchmark: saved scene cannot also apply a preset"
            );
        } else if config.scenario == "orbit" {
            let Outcome::Spawned(vessel) = lab.session.execute(Action::LaunchOrbit {
                craft: lab.craft.clone(),
                offset: DVec3::ZERO,
            }) else {
                panic!("benchmark orbit spawn")
            };
            lab.session.execute(Action::Select { vessel });
        } else if config.scenario == "map" {
            lab.session.execute(Action::View {
                command: ViewCommand::Focus {
                    body: Some(lab.session.sim().home),
                },
            });
        }
        lab.session.execute(Action::EndFrame {
            paused: true,
            rate: 0,
        });
        let checkpoint_path =
            std::path::PathBuf::from(benchmark_path.as_ref().unwrap()).with_extension("world.json");
        lab.session.save_checkpoint(&checkpoint_path);
        if lab.profile.is_none() {
            let path = std::path::PathBuf::from(benchmark_path.as_ref().unwrap())
                .with_extension("cpu.json");
            lab.profile = Some((void_diagnostics::Profiler::new(), path));
        }
    }
    let mut app = App::new();
    if let Some(path) = benchmark_path.as_ref().or(render_path.as_ref()) {
        app.insert_resource(crate::render_metrics::RenderMetrics::new(
            path.into(),
            benchmark.as_ref().map(|b| b.frames),
        ));
    }
    let mut plugins = DefaultPlugins
        .set(bevy::log::LogPlugin {
            custom_layer: crate::render_metrics::error_layer,
            fmt_layer: crate::render_metrics::quiet_draw_formatter,
            filter: if benchmark_path.is_some() || render_path.is_some() {
                format!(
                    "{},bevy_render::render_phase::draw_state=trace,void_draw_submission=trace",
                    bevy::log::DEFAULT_FILTER
                )
            } else {
                bevy::log::DEFAULT_FILTER.into()
            },
            ..default()
        })
        .set(WindowPlugin {
            primary_window: if benchmark.is_some() {
                None
            } else {
                Some(Window {
                    title: if main_game {
                        "VOID"
                    } else {
                        "VOID Fleet flight integration"
                    }
                    .into(),
                    ..default()
                })
            },
            exit_condition: if benchmark.is_some() {
                bevy::window::ExitCondition::DontExit
            } else {
                bevy::window::ExitCondition::OnAllClosed
            },
            ..default()
        })
        .set(bevy::render::RenderPlugin {
            render_creation: WgpuSettings {
                features: WgpuFeatures::POLYGON_MODE_LINE,
                ..default()
            }
            .into(),
            ..default()
        });
    if benchmark.is_some() {
        plugins = plugins
            .disable::<bevy::winit::WinitPlugin>()
            .disable::<bevy::render::pipelined_rendering::PipelinedRenderingPlugin>();
    }
    app.add_plugins((plugins, WireframePlugin::default()))
        .insert_resource(ClearColor(if main_game {
            Color::BLACK
        } else {
            Color::srgb(0.02, 0.025, 0.04)
        }))
        .insert_resource(GlobalAmbientLight {
            brightness: if main_game { 40.0 } else { 100.0 },
            color: Color::srgb_u8(0xcb, 0xe7, 0xff),
            ..default()
        })
        .insert_non_send(lab)
        .add_systems(Startup, (setup, setup_scenery).chain())
        .add_systems(
            Update,
            (
                begin_profile_frame,
                ui::interactions,
                ui::scroll_panels,
                controls,
                simulate,
                refresh_scenery,
                draw,
                draw_map,
                instruments,
                ui::refresh,
                ui::stages,
                ui::indicators,
                ui::apply_font,
                update_scenery,
                capture_frame,
            )
                .chain(),
        );
    if main_game {
        app.add_plugins(crate::scenery::SceneryPlugin);
    }
    if benchmark_path.is_some() || render_path.is_some() {
        app.add_plugins(crate::render_metrics::RenderMetricsPlugin);
        if benchmark.is_none() {
            app.world_mut()
                .resource_mut::<crate::render_metrics::RenderFrameTag>()
                .measure = true;
        }
    }
    if let Some(config) = benchmark {
        app.world_mut().spawn(Window {
            resolution: bevy::window::WindowResolution::new(config.width, config.height),
            ..default()
        });
        app.insert_resource(config)
            .add_plugins(bevy::app::ScheduleRunnerPlugin::run_loop(
                std::time::Duration::from_millis(1),
            ))
            .add_systems(
                PostUpdate,
                benchmark_tick.after(crate::render_metrics::collect),
            );
    }
    app.run();
}
// Capture the GPU window image independently of the desktop/VNC presentation path.
fn capture_frame(mut commands: Commands, keys: Res<ButtonInput<KeyCode>>) {
    if keys.just_pressed(KeyCode::PrintScreen) {
        use bevy::render::view::screenshot::{Screenshot, save_to_disk};
        std::fs::create_dir_all("lab-log/screenshots").expect("create screenshot directory");
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("screenshot clock predates Unix epoch")
            .as_nanos();
        let path = format!(
            "lab-log/screenshots/frame-{}-{stamp}.png",
            std::process::id()
        );
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(path));
    }
}
#[derive(Resource)]
struct RenderBenchmark {
    width: u32,
    height: u32,
    frames: usize,
    settle: usize,
    scenario: String,
    updates: usize,
    stable: usize,
    phase: u8,
    drain: usize,
    run_updates: usize,
    timeout: f64,
}
impl RenderBenchmark {
    fn from_arguments() -> Self {
        let number = |key: &str, default: &str| {
            argument(key)
                .unwrap_or(default.into())
                .parse::<usize>()
                .expect("benchmark: expected positive integer")
        };
        let scenario = argument("--benchmark-scenario").unwrap_or("surface".into());
        assert!(
            ["surface", "orbit", "map"].contains(&scenario.as_str()),
            "benchmark: unknown scenario"
        );
        let width = u32::try_from(number("--width", "640")).unwrap();
        let height = u32::try_from(number("--height", "360")).unwrap();
        let frames = number("--benchmark-frames", "120");
        let settle = number("--benchmark-settle", "60");
        assert!(
            width > 0 && height > 0 && frames > 0 && settle > 0,
            "benchmark: zero extent or phase duration"
        );
        Self {
            width,
            height,
            frames,
            settle,
            scenario,
            updates: 0,
            stable: 0,
            phase: 0,
            drain: 0,
            run_updates: 0,
            timeout: 180.0,
        }
    }
}
fn benchmark_tick(
    mut config: ResMut<RenderBenchmark>,
    mut metrics: ResMut<crate::render_metrics::RenderMetrics>,
    mut tag: ResMut<crate::render_metrics::RenderFrameTag>,
    ground: Res<Ground>,
    mut lab: NonSendMut<Lab>,
    mut exit: MessageWriter<bevy::app::AppExit>,
) {
    assert!(
        metrics.started.elapsed().as_secs_f64() < config.timeout,
        "benchmark: renderer did not settle/complete before timeout"
    );
    assert!(
        metrics.errors.lock().unwrap().is_empty(),
        "benchmark: render errors; inspect the log"
    );
    config.updates += 1;
    if config.phase == 0 {
        let (building, requests, drawn, bytes) = ground.readiness();
        let air_ready = !lab.main_game
            || metrics
                .last_paths
                .iter()
                .any(|p| p.ends_with("void_air/elapsed_cpu"));
        let ready = building == 0
            && requests == 0
            && drawn > 0
            && metrics.last_pending_pipelines == Some(0)
            && air_ready
            && metrics.last_paths.contains("render/ui/elapsed_cpu");
        config.stable = if ready { config.stable + 1 } else { 0 };
        if config.updates >= config.settle && config.stable >= 5 {
            config.phase = 1;
            tag.measure = true;
            if let Some((profile, _)) = &mut lab.profile {
                *profile = void_diagnostics::Profiler::new();
            }
            metrics.benchmark = Some(
                serde_json::json!({"scenario":config.scenario,"renderer":"offscreen Image target; Winit disabled",
                "main_game_shaders":lab.main_game,"width":config.width,"height":config.height,
                "settle_updates":config.updates,"stable_ready_updates":config.stable,
                "drawn_tiles_at_run":drawn,"pending_tiles_at_run":building,"cached_mesh_bytes_at_run":bytes,
                "requested_delivered_frames":config.frames,
                "world_checkpoint":std::path::PathBuf::from(argument("--render-benchmark").unwrap()).with_extension("world.json"),
                "scene_source":if argument("--load").is_some() { "saved checkpoint" } else { "preset" },
                "model_version":void_fleet_flight::session::MODEL_VERSION}),
            );
        }
    } else if config.phase == 1 {
        config.run_updates += 1;
        if metrics.capture.frames() >= config.frames {
            config.phase = 2;
            tag.measure = false;
            if let Some((profile, path)) = lab.profile.take() {
                profile.write(path);
            }
        }
    } else {
        config.drain += 1;
        if config.drain >= 20 {
            let report = metrics.benchmark.as_mut().unwrap().as_object_mut().unwrap();
            report.insert("run_main_updates".into(), config.run_updates.into());
            report.insert("drain_updates".into(), config.drain.into());
            metrics.write();
            println!(
                "Rendered benchmark: {} delivered frames, {}x{}, {} scenario",
                metrics.capture.frames(),
                config.width,
                config.height,
                config.scenario
            );
            exit.write(bevy::app::AppExit::Success);
        }
    }
}
const WATER_REVIEW_CASES: [[f64; 3]; 6] = [
    [2., 0., 0.],
    [20., 45., 0.],
    [80., 90., 0.],
    [200., 0., 0.],
    [80., 45., 45.],
    [200., 90., 60.],
];
fn water_fixture(lab: &mut Lab) {
    let [speed, tilt, entry] = lab.water_review.expect("water review parameters");
    let id = void_fleet_flight::water::splashdown_at(
        &mut lab.session,
        &void_assembly::reentry_capsule(),
        speed,
        tilt.to_radians(),
        entry.to_radians(),
    );
    select_pilot(lab, &id);
    lab.paused = true;
    lab.session.execute(Action::View {
        command: ViewCommand::Zoom { pixels: 320. },
    });
    lab.session.execute(Action::EndFrame {
        paused: true,
        rate: lab.rate,
    });
    lab.notice = format!(
        "Splashdown: {speed} m/s, body tilt {tilt}°, entry from vertical {entry}°. P start/pause; R repeat; Shift+R next case."
    );
}
fn new_lab(session: FlightSession, craft: Craft) -> Lab {
    let f = &session.sim().fleet;
    let orbits = void_view::MapOrbits::new(f.ephemeris.bodies());
    Lab {
        main_game: false,
        rendezvous: false,
        reentry: false,
        water_review: None,
        own_port: None,
        target_port: None,
        pointer_over_label: false,
        view: None,
        eye: DVec3::ZERO,
        focus_position: DVec3::ZERO,
        orbits,
        path: void_view::MapPath::new(),
        body_plots: Default::default(),
        plot_path: Default::default(),
        plot_plan: Default::default(),
        plan_vessel: String::new(),
        prediction_at: f64::NEG_INFINITY,
        prediction_generation: 0,
        session,
        save_path: "lab-log/fleet-save.json".into(),
        record_path: None,
        frames: 0,
        playback: None,
        profile: None,
        craft,
        paused: true,
        rate: 0,
        dirty: false,
        notice: String::new(),
        spawned: 0,
        parts: HashMap::new(),
        collision: HashMap::new(),
        prediction: None,
    }
}
#[allow(clippy::too_many_arguments)]
fn setup(
    mut commands: Commands,
    lab: NonSend<Lab>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut fonts: Option<ResMut<Assets<Font>>>,
    window: Single<&Window>,
    benchmark: Option<Res<RenderBenchmark>>,
) {
    let assets = RenderAssets::new(&mut meshes, &mut materials);
    commands.insert_resource(assets);
    let demo = demo_rocket(&lab.session.sim().planet.terrain);
    let material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.32, 0.42, 0.28),
        perceptual_roughness: 1.0,
        ..default()
    });
    commands.insert_resource(Ground::Plain(
        Box::new(TileField::new(
            landing_lod_options(&lab.session.sim().planet.terrain, &demo.options.contact),
            Some(lab.session.sim().planet.terrain.clone()),
            material.clone(),
        )),
        material,
    ));
    let target = benchmark
        .as_ref()
        .map(|b| {
            let image = Image::new_target_texture(
                b.width,
                b.height,
                bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
                None,
            );
            bevy::camera::RenderTarget::Image(images.add(image).into())
        })
        .unwrap_or_default();
    commands.spawn((
        Camera3d::default(),
        target,
        Transform::default(),
        bevy::ui::IsDefaultUiCamera,
        LabCamera,
    ));
    commands.spawn((
        SceneSun,
        DirectionalLight {
            illuminance: if lab.main_game { 1000.0 } else { 8000.0 },
            ..default()
        },
        Transform::default().looking_to(Vec3::new(-1.0, -0.4, -0.7), Vec3::Z),
    ));
    if lab.main_game {
        crate::map::spawn_map_labels(&mut commands, lab.session.sim().fleet.ephemeris.bodies());
        ui::spawn(
            &mut commands,
            &mut images,
            fonts.as_mut().expect("main game font assets"),
            f64::from(window.scale_factor()),
        );
    } else {
        commands.insert_resource(ui::UiState::default());
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
}

#[allow(clippy::type_complexity)]
fn instruments(
    lab: NonSend<Lab>,
    mut balls: Query<&mut crate::navball::Navball>,
    mut images: ResMut<Assets<Image>>,
    mut labels: Query<
        (&mut Text, &mut Node, &mut TextColor, &mut Visibility),
        With<crate::navball::NavballLabel>,
    >,
    mut heading: Query<&mut Text, (With<NavballHeading>, Without<crate::navball::NavballLabel>)>,
) {
    if balls.is_empty() {
        return;
    }
    let sim = lab.session.sim();
    let fleet = &sim.fleet;
    let ship = fleet.snapshot(&sim.selected);
    let reference = sim.navigation_body(&sim.selected);
    let local = fleet
        .frames()
        .transform(
            fleet.vessel_frame(&sim.selected),
            fleet.body_frames(reference).1,
        )
        .apply_state(void_frames::State {
            position: fleet.centre_of_mass_local(&sim.selected),
            velocity: DVec3::ZERO,
        });
    let inertial = fleet
        .frames()
        .transform(
            fleet.vessel_frame(&sim.selected),
            fleet.body_frames(reference).0,
        )
        .apply_state(void_frames::State {
            position: fleet.centre_of_mass_local(&sim.selected),
            velocity: DVec3::ZERO,
        });
    let inertial_axes = fleet
        .frames()
        .transform(fleet.body_frames(reference).0, fleet.origin_frame())
        .rotation();
    let q = surface_axes(fleet, reference);
    let (nose, top) = vehicle_navball_axes(fleet.control_profile(&sim.selected));
    let input = void_navball::NavballInput {
        nose: (ship.rotation * nose).normalize(),
        top: (ship.rotation * top).normalize(),
        up: q * local.position.normalize(),
        pole: fleet.ephemeris.bodies()[reference].rotation.axis(),
        prime_meridian: q * DVec3::X,
        velocity: if lab.session.sim().presentation.speed_surface {
            q * local.velocity
        } else {
            inertial_axes * inertial.velocity
        },
    };
    for mut ball in &mut balls {
        let reading = crate::navball::draw_navball(&mut ball, &input, &mut images, &mut labels);
        for mut text in &mut heading {
            text.0 = format!(
                "HDG {:03}° · {:+.0}°",
                reading.heading.round() as i64 % 360,
                reading.pitch
            );
        }
    }
}
fn vehicle_navball_axes(profile: Option<void_assembly::ControlProfile>) -> (DVec3, DVec3) {
    use void_assembly::ControlProfile;
    match profile {
        None | Some(ControlProfile::Flight) => (DVec3::Y, DVec3::Z),
        Some(ControlProfile::Aircraft | ControlProfile::Rover | ControlProfile::Eva) => {
            (DVec3::Z, DVec3::Y)
        }
    }
}
/// A body's surface axes in origin-frame coordinates; the render world uses the observed body's.
fn surface_axes(fleet: &void_vessels::Fleet, body: usize) -> glam::DQuat {
    fleet
        .frames()
        .transform(fleet.body_frames(body).1, fleet.origin_frame())
        .rotation()
}
fn crew_transfer(lab: &mut Lab) {
    let fleet = &lab.session.sim().fleet;
    let selected = lab.session.sim().selected.clone();
    let action = if fleet.eva_crew(&selected).is_some() {
        let frames = fleet.frames();
        let mut candidates = Vec::new();
        for carrier in fleet.vessel_ids().into_iter().filter(|id| id != &selected) {
            for seat in fleet
                .crew_seats(&carrier)
                .into_iter()
                .filter(|s| s.occupant.is_none())
            {
                let hatch = frames
                    .transform(fleet.part_frame(&seat.part), fleet.vessel_frame(&selected))
                    .apply_point(seat.parameters.hatch_position);
                candidates.push((hatch.length_squared(), seat.part, seat.module));
            }
        }
        candidates.sort_by(|a, b| a.0.total_cmp(&b.0));
        let Some((_, part, module)) = candidates.into_iter().next() else {
            lab.notice = "Board refused: no empty seat".into();
            return;
        };
        Action::EvaBoard { part, module }
    } else {
        let Some(seat) = fleet
            .crew_seats(&selected)
            .into_iter()
            .find(|s| s.occupant.is_some())
        else {
            lab.notice = "Exit refused: no crew in selected vehicle".into();
            return;
        };
        Action::EvaExit {
            part: seat.part,
            module: seat.module,
        }
    };
    match lab.session.execute(action) {
        Outcome::Refused(reason) => lab.notice = reason,
        Outcome::Spawned(_) => {
            lab.notice.clear();
            lab.prediction = None;
            lab.own_port = None;
            lab.target_port = None;
        }
        other => panic!("unexpected crew transfer outcome {other:?}"),
    }
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
    markers: Query<(&Interaction, &crate::map::MapMarker)>,
    ui_state: Option<Res<ui::UiState>>,
    mut lab: NonSendMut<Lab>,
) {
    let lab = &mut *lab;
    if ui_state.as_ref().is_some_and(|s| s.editing) {
        if lab.playback.is_none() {
            neutral_pilot(lab);
        }
        return;
    }
    let empty_buttons = ButtonInput::default();
    let empty_motion = AccumulatedMouseMotion::default();
    let empty_scroll = AccumulatedMouseScroll::default();
    let over_ui = ui_state.as_ref().is_some_and(|s| s.pointer);
    let buttons = if over_ui { &empty_buttons } else { &*buttons };
    let motion = if over_ui { &empty_motion } else { &*motion };
    let scroll = if over_ui { &empty_scroll } else { &*scroll };
    let (over_label, clicked) =
        crate::map::label_click(&markers, buttons, lab.view.map_or(0.0, |s| s.map_weight));
    if lab.playback.is_none()
        && let Some(kind) = clicked
    {
        let bodies = lab.session.sim().fleet.ephemeris.bodies();
        let body = match kind {
            void_view::LabelKind::Body(i) => Some(i),
            void_view::LabelKind::Star => Some(
                bodies
                    .iter()
                    .find(|b| b.parent_index.is_none())
                    .expect("root body")
                    .index,
            ),
            void_view::LabelKind::Vessel | void_view::LabelKind::Apsis => None,
        };
        lab.session.execute(Action::View {
            command: ViewCommand::Focus { body },
        });
    }
    lab.pointer_over_label = over_label || over_ui;
    if !window.focused {
        if lab.playback.is_none() {
            neutral_pilot(lab);
        }
        return;
    }
    if lab.playback.is_some() {
        if keys.just_pressed(KeyCode::KeyP) {
            lab.paused = !lab.paused;
        }
        return;
    }
    if keys.just_pressed(KeyCode::F6) {
        lab.session.save_checkpoint(&lab.save_path);
        lab.notice = format!("Saved {}", lab.save_path.display());
    }
    if keys.just_pressed(KeyCode::F7) {
        let checkpoint = void_fleet_flight::checkpoint::FlightCheckpoint::read(&lab.save_path);
        lab.session.execute(Action::LoadWorld {
            checkpoint: Box::new(checkpoint),
        });
        lab.craft = lab.session.recording_initial().craft.clone();
        lab.dirty = true;
        lab.prediction = None;
        lab.paused = true;
        lab.rate = 0;
        lab.own_port = None;
        lab.target_port = None;
        neutral_pilot(lab);
        lab.notice = format!("Loaded {}", lab.save_path.display());
    }
    if keys.just_pressed(KeyCode::F8) {
        if let Some(path) = lab.record_path.take() {
            lab.session.finish_stream();
            lab.notice = format!("Recording finished: {}", path.display());
        } else {
            lab.notice = "No recording active; start with --record <file>".into();
        }
    }
    if keys.just_pressed(KeyCode::F9) {
        if let Some((profile, path)) = lab.profile.take() {
            profile.write(&path);
            lab.notice = format!("CPU profile finished: {}", path.display());
        } else {
            lab.notice = "No CPU profile active; start with --profile <file>".into();
        }
    }
    if keys.just_pressed(KeyCode::KeyP) {
        lab.paused = !lab.paused;
        if lab.paused {
            neutral_pilot(lab);
        }
    }
    if keys.just_pressed(KeyCode::KeyR) {
        if keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight])
            && let Some(current) = lab.water_review
        {
            let next = WATER_REVIEW_CASES
                .iter()
                .position(|c| *c == current)
                .map_or(0, |i| (i + 1) % WATER_REVIEW_CASES.len());
            lab.water_review = Some(WATER_REVIEW_CASES[next]);
        }
        let initial = lab.session.recording_initial().clone();
        lab.session.execute(Action::ResetWorld {
            initial: Box::new(initial),
        });
        lab.dirty = true;
        lab.prediction = None;
        lab.paused = true;
        lab.rate = 0;
        lab.spawned = 0;
        lab.own_port = None;
        lab.target_port = None;
        lab.notice.clear();
        if lab.rendezvous {
            rendezvous_fixture(lab);
        }
        if lab.reentry {
            reentry_fixture(lab);
        }
        if lab.water_review.is_some() {
            water_fixture(lab);
        }
    }
    if keys.just_pressed(KeyCode::Tab)
        && lab.main_game
        && keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight])
    {
        let bodies = lab.session.sim().fleet.ephemeris.bodies();
        let body = match lab.session.sim().presentation.focus_body {
            None => Some(0),
            Some(i) if i + 1 < bodies.len() => Some(i + 1),
            Some(_) => None,
        };
        lab.session.execute(Action::View {
            command: ViewCommand::Focus { body },
        });
    } else if keys.just_pressed(KeyCode::Tab) {
        let old = lab.session.sim().selected.clone();
        let ids = lab.session.sim().fleet.vessel_ids();
        let i = ids
            .iter()
            .position(|id| *id == old)
            .expect("selected vessel");
        select_pilot(lab, &ids[(i + 1) % ids.len()]);
        lab.prediction = None;
    }
    if keys.just_pressed(KeyCode::KeyO) {
        let body = lab.session.sim().fleet.ephemeris.bodies()[lab.session.sim().observation_body()]
            .id
            .clone();
        let Outcome::Spawned(id) = lab.session.execute(Action::LaunchOrbitAt {
            body,
            craft: lab.craft.clone(),
            offset: DVec3::ZERO,
        }) else {
            unreachable!()
        };
        select_pilot(lab, &id);
    }
    if keys.just_pressed(KeyCode::KeyN) {
        lab.spawned += 1;
        let site = nearby_site(
            lab.session.sim().launch_site,
            30.0 * lab.spawned as f64,
            lab.session.sim().planet.terrain.radius_meters,
        );
        lab.session.execute(Action::LaunchGround {
            craft: lab.craft.clone(),
            site,
        });
    }
    if lab.main_game && keys.just_pressed(KeyCode::KeyF) {
        crew_transfer(lab);
        return;
    }
    let warp_was_active = lab.session.sim().maneuver_warp.active();
    let id = lab.session.sim().selected.clone();
    let commanded = lab.session.sim().fleet.has_command(&id);
    if !commanded
        && keys.any_pressed([
            KeyCode::KeyW,
            KeyCode::KeyA,
            KeyCode::KeyS,
            KeyCode::KeyD,
            KeyCode::KeyQ,
            KeyCode::KeyE,
            KeyCode::ShiftLeft,
            KeyCode::ShiftRight,
        ])
    {
        lab.notice = if lab.session.sim().fleet.requires_crew(&id) {
            "Control refused: healthy pilot must occupy a healthy seat".into()
        } else {
            "Control refused: command capability unavailable or thermally failed".into()
        };
    }
    let vehicle =
        lab.session.sim().fleet.control_profile(&id) == Some(void_assembly::ControlProfile::Rover);
    let eva =
        lab.session.sim().fleet.control_profile(&id) == Some(void_assembly::ControlProfile::Eva);
    if vehicle {
        let previous = lab
            .session
            .sim()
            .fleet
            .vehicle_control(&id)
            .expect("wheel controls");
        let drive = if commanded && !lab.paused {
            axis(&keys, KeyCode::KeyW, KeyCode::KeyS)
        } else {
            0.0
        };
        let steer = if commanded && !lab.paused {
            axis(&keys, KeyCode::KeyD, KeyCode::KeyA)
        } else {
            0.0
        };
        let brake = if keys.pressed(KeyCode::Space) || !commanded {
            1.0
        } else if keys.just_pressed(KeyCode::KeyX) {
            if previous.brake > 0.0 { 0.0 } else { 1.0 }
        } else if drive != 0.0 {
            0.0
        } else {
            previous.brake
        };
        let control = void_assembly::VehicleControl {
            drive,
            steer,
            brake,
        };
        if control != previous {
            lab.session.execute(Action::Vehicle { control });
        }
    }
    if eva {
        let pack = lab.session.sim().fleet.rcs_control(&id).enabled;
        let control = if commanded && !lab.paused && !pack {
            void_assembly::EvaControl {
                forward: axis(&keys, KeyCode::KeyW, KeyCode::KeyS),
                strafe: axis(&keys, KeyCode::KeyD, KeyCode::KeyA),
                yaw: axis(&keys, KeyCode::KeyE, KeyCode::KeyQ),
            }
        } else {
            void_assembly::EvaControl::default()
        };
        if lab.session.sim().fleet.eva_control(&id) != Some(control)
            && let Outcome::Refused(reason) = lab.session.execute(Action::Eva { control })
        {
            lab.notice = reason;
        }
        if keys.just_pressed(KeyCode::Space)
            && let Outcome::Refused(reason) = lab.session.execute(Action::EvaJump)
        {
            lab.notice = reason;
        }
    }
    if keys.just_pressed(KeyCode::KeyT) && commanded && !eva {
        let enabled = lab.session.sim().fleet.sas_phase(&id) == void_vessels::SasPhase::Off;
        if enabled && !lab.session.sim().fleet.has_reaction_wheel(&id) {
            lab.notice =
                "SAS unavailable: aircraft has aerodynamic controls, no reaction wheel".into();
        } else {
            lab.session.execute(Action::Sas { enabled });
        }
    }
    let mut c = lab.session.sim().fleet.control(&id);
    let dt = time.delta_secs_f64().min(0.05);
    let throttle_axis = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]) as i32
        - keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]) as i32;
    if !lab.session.sim().fleet.command_failed(&id)
        && !(lab.water_review.is_some() && lab.paused)
        && !keys.just_pressed(KeyCode::Tab)
        && !keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight])
    {
        c.throttle = (c.throttle + f64::from(throttle_axis) * dt * 0.5).clamp(0.0, 1.0);
    }
    if keys.just_pressed(KeyCode::KeyX) {
        c.throttle = 0.0;
    }
    let turn = if commanded
        && !lab.paused
        && !vehicle
        && (!eva || lab.session.sim().fleet.rcs_control(&id).enabled)
    {
        DVec3::new(
            axis(&keys, KeyCode::KeyS, KeyCode::KeyW),
            if lab.session.sim().fleet.control_profile(&id)
                == Some(void_assembly::ControlProfile::Aircraft)
            {
                // +Z nose / +Y top: player right is -X, hence negative yaw about +Y.
                axis(&keys, KeyCode::KeyQ, KeyCode::KeyE)
            } else {
                axis(&keys, KeyCode::KeyE, KeyCode::KeyQ)
            },
            axis(&keys, KeyCode::KeyD, KeyCode::KeyA),
        )
    } else {
        DVec3::ZERO
    };
    if lab.main_game {
        let mut rcs = lab.session.sim().fleet.rcs_control(&id);
        if keys.just_pressed(KeyCode::KeyH) && commanded {
            let available = lab.session.sim().fleet.part_snapshots(&id).iter().any(|p| {
                p.definition
                    .modules
                    .iter()
                    .any(|m| matches!(m, Module::Rcs { .. }))
            });
            if available {
                rcs.enabled = !rcs.enabled;
            } else {
                lab.notice = "RCS unavailable: selected vessel has no nozzles".into();
            }
        }
        let translate = keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]);
        rcs.force = if commanded && !lab.paused && rcs.enabled && translate {
            DVec3::new(
                axis(&keys, KeyCode::KeyD, KeyCode::KeyA),
                axis(&keys, KeyCode::KeyE, KeyCode::KeyQ),
                axis(&keys, KeyCode::KeyW, KeyCode::KeyS),
            ) * 80.0
        } else {
            DVec3::ZERO
        };
        if eva {
            rcs.force.x = -rcs.force.x;
        }
        let pack_turn = if eva {
            DVec3::new(turn.x, -turn.y, turn.z)
        } else {
            turn
        };
        rcs.torque = if rcs.enabled && !translate {
            pack_turn * 30.0
        } else {
            DVec3::ZERO
        };
        if rcs.torque != DVec3::ZERO
            && lab.session.sim().fleet.sas_phase(&id) != void_vessels::SasPhase::Off
        {
            lab.session.execute(Action::Sas { enabled: false });
            lab.notice = "Manual RCS torque disengaged SAS reaction wheel".into();
        }
        c.turn = if rcs.enabled || translate {
            DVec3::ZERO
        } else {
            turn
        };
        if rcs != lab.session.sim().fleet.rcs_control(&id) {
            lab.session.execute(Action::Rcs { control: rcs });
        }
    } else {
        c.turn = turn;
    }
    if lab.session.sim().fleet.control_profile(&id) == Some(void_assembly::ControlProfile::Aircraft)
        && lab.session.sim().fleet.has_wheels(&id)
    {
        let wheel_control = void_assembly::VehicleControl {
            drive: 0.0,
            steer: if lab.paused { 0.0 } else { -turn.y },
            brake: if lab.paused || keys.pressed(KeyCode::KeyB) {
                1.0
            } else {
                0.0
            },
        };
        if lab.session.sim().fleet.vehicle_control(&id) != Some(wheel_control) {
            lab.session.execute(Action::Vehicle {
                control: wheel_control,
            });
        }
    }
    let previous = lab.session.sim().fleet.control(&id);
    if previous.throttle != c.throttle || previous.turn != c.turn {
        lab.session.execute(Action::Control {
            throttle: c.throttle,
            turn: c.turn,
        });
    }
    if lab.main_game
        && lab.session.sim().fleet.control_profile(&id)
            == Some(void_assembly::ControlProfile::Flight)
        && !keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight])
    {
        docking_controls(lab, &keys);
    }
    if keys.just_pressed(KeyCode::Space) && !vehicle && !eva {
        lab.session.execute(Action::Stage);
        lab.own_port = None;
        lab.target_port = None;
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
        lab.prediction = Some(lab.session.predict(600.0));
    }
    if lab.session.sim().fleet.control_profile(&id) == Some(void_assembly::ControlProfile::Flight) {
        plan_controls(lab, &keys);
    }
    if warp_was_active && !lab.session.sim().maneuver_warp.active() {
        lab.rate = 0;
    }
    if lab.main_game {
        plot_controls(lab, &keys);
        if keys.just_pressed(KeyCode::F1) {
            let sim = lab.session.sim();
            let body = sim.observation_body();
            let radius = sim.fleet.ephemeris.bodies()[body].radius_meters;
            let ratio = sim.presentation.distance / radius;
            let view = if ratio < 1.1 {
                "orbit"
            } else if ratio < 10.0 {
                "far"
            } else {
                "near"
            };
            scenery_preset(lab, body, view);
        }
        if keys.just_pressed(KeyCode::Home)
            && keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight])
        {
            if lab.session.sim().world.stellar.is_some() {
                lab.session.execute(Action::View {
                    command: ViewCommand::BodyPreset {
                        body: 0,
                        direction: DVec3::new(0.1, 0.2, 1.0).normalize(),
                        distance: 16.0 * void_multiscale::LIGHT_YEAR,
                    },
                });
                lab.notice =
                    "Stellar neighborhood · real distances; select star labels then zoom in".into();
            } else {
                lab.notice = "Stellar overview requires --stellar-neighborhood".into();
            }
        }
        if keys.just_pressed(KeyCode::Home)
            && !keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight])
            && !keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight])
        {
            lab.session.execute(Action::View {
                command: ViewCommand::Focus { body: None },
            });
        }
        if keys.just_pressed(KeyCode::F10)
            && keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight])
        {
            let value = (lab.session.sim().presentation.exposure / 2.0).max(0.001);
            lab.session.execute(Action::View {
                command: ViewCommand::Exposure { value },
            });
        }
        if keys.just_pressed(KeyCode::F11)
            && keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight])
        {
            let value = (lab.session.sim().presentation.exposure * 2.0).min(100.0);
            lab.session.execute(Action::View {
                command: ViewCommand::Exposure { value },
            });
        }
    }
    view_controls(lab, &keys, buttons, motion, scroll);
}
fn plot_controls(lab: &mut Lab, keys: &ButtonInput<KeyCode>) {
    use void_orbit::FrameSpec;
    let sim = lab.session.sim();
    let bodies = sim.fleet.ephemeris.bodies();
    let current = sim.presentation.plotting_frame;
    let (mut primary, mut secondary, mut mode) = match current {
        FrameSpec::Barycentric => (sim.home, None, 0),
        FrameSpec::BodyInertial { body } => (body, None, 1),
        FrameSpec::BodySurface { body } => (body, None, 2),
        FrameSpec::TwoBodyRotating { primary, secondary } => (primary, Some(secondary), 3),
    };
    let mut changed = false;
    for (key, value) in [
        (KeyCode::Digit1, 0),
        (KeyCode::Digit2, 1),
        (KeyCode::Digit3, 2),
        (KeyCode::Digit4, 3),
    ] {
        if keys.just_pressed(key) {
            mode = value;
            changed = true;
        }
    }
    if keys.just_pressed(KeyCode::KeyG) {
        mode = (mode + 1) % 4;
        changed = true;
    }
    if keys.just_pressed(KeyCode::KeyJ) {
        if keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]) {
            let candidates: Vec<_> = bodies
                .iter()
                .filter(|b| {
                    b.index != primary
                        && sim.fleet.ephemeris.system_of(b.index)
                            == sim.fleet.ephemeris.system_of(primary)
                })
                .map(|b| b.index)
                .collect();
            if candidates.is_empty() {
                lab.notice = "Two-body plot requires two bodies".into();
                return;
            }
            let next = secondary
                .and_then(|s| candidates.iter().position(|&i| i == s))
                .map_or(0, |i| (i + 1) % candidates.len());
            secondary = Some(candidates[next]);
            mode = 3;
        } else {
            primary = (primary + 1) % bodies.len();
            if secondary.is_some_and(|s| {
                s == primary
                    || sim.fleet.ephemeris.system_of(s) != sim.fleet.ephemeris.system_of(primary)
            }) {
                secondary = None;
            }
        }
        changed = true;
    }
    if !changed {
        return;
    }
    let frame = match mode {
        0 => FrameSpec::Barycentric,
        1 => FrameSpec::BodyInertial { body: primary },
        2 => FrameSpec::BodySurface { body: primary },
        3 => {
            let secondary = secondary
                .or_else(|| {
                    bodies
                        .iter()
                        .find(|b| b.parent_index == Some(primary))
                        .map(|b| b.index)
                })
                .or(bodies[primary].parent_index)
                .or_else(|| {
                    bodies
                        .iter()
                        .find(|b| {
                            b.index != primary
                                && sim.fleet.ephemeris.system_of(b.index)
                                    == sim.fleet.ephemeris.system_of(primary)
                        })
                        .map(|b| b.index)
                });
            let Some(secondary) = secondary else {
                lab.notice = "Two-body plot requires two bodies".into();
                return;
            };
            FrameSpec::TwoBodyRotating { primary, secondary }
        }
        _ => unreachable!(),
    };
    lab.session.execute(Action::View {
        command: ViewCommand::PlotFrame { frame },
    });
    lab.notice = "Plotting frame changed; physics frame unchanged".into();
}

fn plan_controls(lab: &mut Lab, keys: &ButtonInput<KeyCode>) {
    use void_orbit::{ManeuverSpec, ReferenceMode};
    let id = lab.session.sim().selected.clone();
    let run = |lab: &mut Lab, action: Action| match lab.session.execute(action) {
        Outcome::Applied => lab.notice.clear(),
        Outcome::Refused(reason) => lab.notice = reason,
        other => panic!("unexpected maneuver outcome {other:?}"),
    };
    if keys.just_pressed(KeyCode::KeyM) {
        let sim = lab.session.sim();
        let fleet = &sim.fleet;
        let ship = fleet.snapshot(&id);
        let mut positions = vec![DVec3::ZERO; fleet.ephemeris.bodies().len()];
        fleet.ephemeris.positions_at(fleet.time(), &mut positions);
        let reference = void_orbit::DominanceTree::new(fleet.ephemeris.bodies())
            .dominant(&positions, ship.position);
        let start_time = sim
            .plans
            .get(&id)
            .and_then(|p| p.plan.burns().last())
            .map_or(fleet.time() + 60.0, |b| b.end_time + 60.0);
        run(
            lab,
            Action::AddManeuver {
                spec: ManeuverSpec {
                    start_time,
                    reference_body: reference,
                    reference_mode: ReferenceMode::Auto,
                    prograde: 100.0,
                    normal: 0.0,
                    radial: 0.0,
                },
            },
        );
    }
    let Some(p) = lab.session.sim().plans.get(&id) else {
        return;
    };
    if p.plan.count() == 0 {
        return;
    }
    let selected = p.selected;
    let count = p.plan.count();
    let mut spec = p.plan.maneuver(selected);
    if keys.just_pressed(KeyCode::BracketLeft) {
        run(
            lab,
            Action::SelectManeuver {
                index: selected.saturating_sub(1),
            },
        );
    }
    if keys.just_pressed(KeyCode::BracketRight) {
        run(
            lab,
            Action::SelectManeuver {
                index: (selected + 1).min(count - 1),
            },
        );
    }
    let step = if keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]) {
        10.0
    } else {
        1.0
    };
    let prograde = axis(keys, KeyCode::ArrowUp, KeyCode::ArrowDown) * step;
    let normal = axis(keys, KeyCode::ArrowRight, KeyCode::ArrowLeft) * step;
    let radial = axis(keys, KeyCode::PageUp, KeyCode::PageDown) * step;
    let seconds = (f64::from(keys.pressed(KeyCode::End))
        - f64::from(
            keys.pressed(KeyCode::Home) && keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]),
        ))
        * step;
    if prograde != 0.0 || normal != 0.0 || radial != 0.0 || seconds != 0.0 {
        spec.prograde += prograde;
        spec.normal += normal;
        spec.radial += radial;
        spec.start_time += seconds;
        run(
            lab,
            Action::EditManeuver {
                index: selected,
                spec,
            },
        );
    }
    if keys.just_pressed(KeyCode::KeyV) {
        if spec.reference_mode == ReferenceMode::Auto {
            spec.reference_mode = ReferenceMode::Fixed;
        } else if spec.reference_body + 1 < lab.session.sim().fleet.ephemeris.bodies().len() {
            spec.reference_body += 1;
        } else {
            spec.reference_mode = ReferenceMode::Auto;
            spec.reference_body = lab.session.sim().home;
        }
        run(
            lab,
            Action::EditManeuver {
                index: selected,
                spec,
            },
        );
    }
    if keys.just_pressed(KeyCode::Delete) {
        run(lab, Action::RemoveManeuver { index: selected });
    }
    if keys.just_pressed(KeyCode::KeyY) {
        run(
            lab,
            Action::PlaceManeuverAtApsis {
                index: selected,
                apsis: void_orbit::ApsisKind::Periapsis,
            },
        );
    }
    if keys.just_pressed(KeyCode::KeyU) {
        run(
            lab,
            Action::PlaceManeuverAtApsis {
                index: selected,
                apsis: void_orbit::ApsisKind::Apoapsis,
            },
        );
    }
    if keys.just_pressed(KeyCode::KeyZ) {
        if lab.session.sim().maneuver_warp.active() {
            run(lab, Action::CancelManeuverWarp);
            lab.rate = 0;
        } else {
            run(lab, Action::BeginManeuverWarp);
            if lab.session.sim().maneuver_warp.active() {
                lab.rate = RATES.len() - 1;
                lab.paused = false;
            }
        }
    }
    if keys.just_pressed(KeyCode::KeyB) {
        run(lab, Action::ExecuteManeuver);
    }
    if keys.just_pressed(KeyCode::Escape) {
        run(lab, Action::AbortManeuver);
    }
}
fn plan_description(lab: &Lab) -> String {
    let sim = lab.session.sim();
    if sim.fleet.control_profile(&sim.selected) != Some(void_assembly::ControlProfile::Flight) {
        return String::new();
    }
    let Some(p) = sim.plans.get(&sim.selected) else {
        return "M add maneuver | Z warp before burn | B execute first | Esc abort".into();
    };
    let mut text = format!(
        "Plan: {} maneuvers, {} completed | {}",
        p.plan.count(),
        p.plan.completed_count,
        p.message
    );
    if p.plan.count() > 0 {
        let spec = p.plan.maneuver(p.selected);
        let status = match p.plan.status(p.selected) {
            Ok(burn) => format!("burn {:.2}s", burn.end_time - burn.start_time),
            Err(reason) => format!("unavailable: {reason}"),
        };
        text.push_str(&format!(
            "\n[{}] T+{:.2}s Δv {:+.1}/{:+.1}/{:+.1}m/s {:?} {} | {}",
            p.selected + 1,
            spec.start_time,
            spec.prograde,
            spec.normal,
            spec.radial,
            spec.reference_mode,
            sim.fleet.ephemeris.bodies()[spec.reference_body].name,
            status
        ));
    }
    text.push_str("\nM add | [] select | arrows prograde/normal | PgUp/Dn radial | Alt+Home/End time | Y/U apsis | V reference | Del remove | Z warp | B execute first | Esc abort");
    text
}

fn view_controls(
    lab: &mut Lab,
    keys: &ButtonInput<KeyCode>,
    buttons: &ButtonInput<MouseButton>,
    motion: &AccumulatedMouseMotion,
    scroll: &AccumulatedMouseScroll,
) {
    for (key, setting) in [
        (KeyCode::F2, Toggle::Wire),
        (KeyCode::F3, Toggle::Bounds),
        (KeyCode::F4, Toggle::Colliders),
        (KeyCode::F5, Toggle::Terrain),
        (KeyCode::KeyK, Toggle::AltitudeAgl),
        (KeyCode::KeyL, Toggle::SpeedSurface),
    ] {
        if keys.just_pressed(key) {
            lab.session.execute(Action::View {
                command: ViewCommand::Toggle { setting },
            });
        }
    }
    if buttons.pressed(MouseButton::Left) && !lab.pointer_over_label && motion.delta != Vec2::ZERO {
        lab.session.execute(Action::View {
            command: ViewCommand::Drag {
                x: f64::from(motion.delta.x),
                y: f64::from(motion.delta.y),
            },
        });
    }
    let pixels = f64::from(scroll.delta.y)
        * match scroll.unit {
            bevy::input::mouse::MouseScrollUnit::Line => 40.0,
            bevy::input::mouse::MouseScrollUnit::Pixel => 1.0,
        };
    if pixels != 0.0 {
        lab.session.execute(Action::View {
            command: ViewCommand::Zoom { pixels },
        });
    }
}

fn begin_profile_frame(time: Res<Time>, mut lab: NonSendMut<Lab>) {
    if let Some((profile, _)) = &mut lab.profile {
        profile.sample("frame_interval", time.delta_secs_f64() * 1000.0);
    }
}
fn simulate(time: Res<Time>, window: Single<&Window>, mut lab: NonSendMut<Lab>) {
    let started = std::time::Instant::now();
    simulate_inner(&time, &window, &mut lab);
    if lab.main_game {
        let sim = lab.session.sim();
        let t = sim.fleet.time();
        let clear = sim
            .fleet
            .clearance(&sim.selected, sim.nearby_body(&sim.selected));
        if clear > 20.0
            && (lab.prediction.is_none() || t < lab.prediction_at || t - lab.prediction_at >= 2.0)
        {
            lab.prediction = Some(lab.session.predict(6000.0));
            lab.prediction_at = t;
            lab.prediction_generation += 1;
        } else if clear <= 20.0 {
            lab.prediction = None;
        }
    }
    if let Some((profile, _)) = &mut lab.profile {
        profile.span("simulation", started, std::time::Instant::now());
    }
}
fn simulate_inner(time: &Time, window: &Window, lab: &mut Lab) {
    if lab.paused || !window.focused {
        if lab.playback.is_none() {
            lab.session.execute(Action::EndFrame {
                paused: lab.paused,
                rate: lab.rate,
            });
        }
        return;
    }
    if let Some(mut playback) = lab.playback.take() {
        let terrain_before = lab.session.sim().planet.terrain.clone();
        if playback.next_frame(&mut lab.session) {
            lab.playback = Some(playback);
        } else {
            lab.paused = true;
            lab.notice = "Replay complete: all world marks verified".into();
        }
        if !std::sync::Arc::ptr_eq(&terrain_before, &lab.session.sim().planet.terrain) {
            lab.dirty = true;
            lab.prediction = None;
            lab.craft = lab.session.recording_initial().craft.clone();
        }
        return;
    }
    let maneuver_warp = lab.session.sim().maneuver_warp.active();
    if maneuver_warp {
        lab.rate = RATES.len() - 1;
    }
    if lab.rate > 2 {
        let f = &lab.session.sim().fleet;
        let sim = lab.session.sim();
        while lab.rate > 2
            && f.vessel_ids().iter().any(|id| {
                f.snapshot(id).mode == void_vessels::VesselMode::Orbit
                    && f.clearance(id, sim.nearby_body(id))
                        < sim.terrains[&sim.nearby_body(id)].radius_meters
                            * crate::flight::rails_min_clearance_radii(RATES[lab.rate])
            })
        {
            lab.rate -= 1;
            lab.notice = "Warp limited by orbital vessel clearance".into();
        }
    }
    let rate = RATES[lab.rate];
    lab.frames += 1;
    let outcome = lab.session.execute(Action::Advance {
        seconds: time.delta_secs_f64().min(0.05) * rate,
        rails: rate > 4.0,
    });
    if maneuver_warp && !lab.session.sim().maneuver_warp.active() {
        lab.rate = 0;
        if let void_fleet_flight::warp::ManeuverWarp::Stopped { message } =
            &lab.session.sim().maneuver_warp
        {
            lab.notice = message.clone();
        }
    }
    match outcome {
        Outcome::Advanced(true) => {}
        Outcome::Advanced(false) => {
            lab.rate = 0;
            lab.notice = "Rails stopped at an encounter or ground band".into();
        }
        Outcome::Refused(reason) => {
            lab.rate = 0;
            lab.notice = format!("Warp refused: {reason}");
        }
        other => panic!("unexpected advance outcome: {other:?}"),
    }
    lab.session.execute(Action::EndFrame {
        paused: lab.paused,
        rate: lab.rate,
    });
    if lab.frames.is_multiple_of(60) {
        lab.session.mark();
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
    tile_entities: Query<Entity, With<Tile>>,
    mut camera: Single<&mut Transform, With<LabCamera>>,
    projection: Single<&Projection, With<LabCamera>>,
    mut hud: Single<&mut Text, With<Hud>>,
    window: Single<&Window>,
    mut gizmos: Gizmos,
) {
    let started = std::time::Instant::now();
    let lab = &mut *lab;
    refresh_ports(lab);
    if lab.dirty {
        for (_, entities) in lab.parts.drain() {
            for e in entities {
                commands.entity(e).despawn();
            }
        }
        lab.collision.clear();
        lab.orbits = void_view::MapOrbits::new(lab.session.sim().fleet.ephemeris.bodies());
        lab.path = void_view::MapPath::new();
        if matches!(*ground, Ground::Plain(..)) {
            for entity in &tile_entities {
                commands.entity(entity).despawn();
            }
            ground.reset(&lab.session.sim().planet);
        }
        lab.dirty = false;
    }
    let sim = lab.session.sim();
    let f = &sim.fleet;
    let render_body = sim.observation_body();
    let surface = f.body_frames(render_body).1;
    // All world meshes/shaders use the observed body's axes, camera-relative.
    let q = surface_axes(f, render_body);
    let selected = f.snapshot(&sim.selected);
    let up = f
        .frames()
        .transform(f.vessel_frame(&sim.selected), surface)
        .apply_point(f.centre_of_mass_local(&sim.selected))
        .normalize();
    let sample = sim.presentation.sample(sim);
    if let Ground::World(world) = &mut *ground {
        world.prepare(sim, &sample, q, &mut commands, &mut meshes);
    }
    let mut to_camera = HashMap::new();
    let mut to_camera = |from: void_frames::FrameId| {
        *to_camera
            .entry(from)
            .or_insert_with(|| sample.to_camera(f, from, q))
    };
    let focus = to_camera(sample.focus_frame).apply_point(sample.focus_local);
    let eye = f
        .frames()
        .transform(sample.focus_frame, surface)
        .apply_point(sample.camera(f, q).translation);
    let camera_up = if sim.presentation.main_camera {
        q.conjugate() * sample.view.up
    } else {
        up
    };
    **camera = Transform::default().looking_to(focus.as_vec3(), camera_up.as_vec3());
    lab.focus_position = sample.focus;
    lab.view = Some(sample.view);
    lab.eye = eye;
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
            let root = part_transform(&mut to_camera, p);
            let mut entities: Vec<Entity> = assets.parts[&p.definition.id]
                .iter()
                .map(|piece| {
                    commands
                        .spawn((
                            Visual {
                                id: p.id.clone(),
                                local: piece.local,
                                flame: piece.flame,
                                wheel: None,
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
            for module in &p.definition.modules {
                let Module::Wheel {
                    id: mid,
                    parameters: d,
                } = module
                else {
                    continue;
                };
                let axle = (-d.suspension_direction).cross(d.forward);
                let tire = meshes.add(Cylinder::new(d.radius_meters as f32, 0.2));
                let stripe = meshes.add(Cuboid::new(0.03, 0.21, (d.radius_meters * 1.8) as f32));
                let local =
                    Transform::from_rotation(DQuat::from_rotation_arc(DVec3::Y, axle).as_quat());
                for (mesh, material, shape) in [
                    (
                        tire,
                        assets.parts[&p.definition.id][0].material.clone(),
                        local,
                    ),
                    (stripe, assets.center.clone(), Transform::IDENTITY),
                ] {
                    entities.push(
                        commands
                            .spawn((
                                Visual {
                                    id: p.id.clone(),
                                    local: shape,
                                    flame: false,
                                    wheel: Some(mid.clone()),
                                },
                                Mesh3d(mesh),
                                MeshMaterial3d(material),
                                root,
                                Visibility::Inherited,
                            ))
                            .id(),
                    );
                }
            }
            lab.parts.insert(p.id.clone(), entities);
        }
    }
    for (visual, mut transform, mut visibility) in &mut parts {
        if let Some(p) = snapshots.iter().find(|p| p.id == visual.id) {
            let mut root = part_transform(&mut to_camera, p);
            if let Some(mid) = &visual.wheel {
                let d = p
                    .definition
                    .modules
                    .iter()
                    .find_map(|m| match m {
                        Module::Wheel { id, parameters } if id == mid => Some(parameters),
                        _ => None,
                    })
                    .expect("wheel definition");
                let void_assembly::ModuleState::Wheel { state, .. } = p.modules[mid] else {
                    panic!("wheel visual state")
                };
                let up = -d.suspension_direction;
                let axle = up.cross(d.forward);
                let rotation = DQuat::from_axis_angle(up, -state.steer_radians)
                    * DQuat::from_axis_angle(axle, state.spin_radians);
                root = root.mul_transform(
                    Transform::from_translation(
                        (d.suspension_origin
                            + d.suspension_direction * state.suspension_length_meters)
                            .as_vec3(),
                    )
                    .with_rotation(rotation.as_quat()),
                );
            }
            *transform = root.mul_transform(visual.local);
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
            f.frames()
                .transform(p.frame, surface)
                .apply_point(p.local_position)
        })
        .collect();
    ground.finish_builds();
    let max_level = ground.max_level();
    ground.select(&LodView {
        camera: Some(LodCamera {
            position: eye,
            distance_scale: 1.0,
            max_level,
            focal_pixels: match *projection {
                Projection::Perspective(p) => {
                    f64::from(window.physical_height().max(1))
                        / (2.0 * (f64::from(p.fov) / 2.0).tan())
                }
                _ => panic!("Fleet scene requires a perspective camera"),
            },
            min_observer_cell_pixels: 3.0,
        }),
        observer_positions: observers,
        distance_scale: 1.0,
        horizon_culling: true,
    });
    ground.set_wireframe(&mut commands, lab.session.sim().presentation.wire);
    ground.draw(&mut commands, &mut meshes, &mut tiles, eye);
    for mut v in &mut tile_visibility {
        *v = if lab.session.sim().presentation.terrain {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
    if lab.session.sim().presentation.bounds {
        for line in ground.boundaries(eye) {
            gizmos.linestrip(line, Color::srgb(1.0, 0.2, 0.2));
        }
    }
    let terrain_tiles = f.terrain_tiles();
    let live = terrain_tiles
        .iter()
        .map(|t| (t.scene, t.tile.clone()))
        .collect::<HashSet<_>>();
    lab.collision.retain(|key, _| live.contains(key));
    if lab.session.sim().presentation.colliders {
        for collider in f.vessel_collider_meshes() {
            let into = to_camera(collider.frame);
            let vertices = &collider.mesh.vertices;
            let indices = unique_edges(&collider.mesh.triangles);
            for edge in indices.as_chunks::<2>().0 {
                let point = |i: u32| {
                    into.apply_point(Vec3::from_array(vertices[i as usize]).as_dvec3())
                        .as_vec3()
                };
                gizmos.line(point(edge[0]), point(edge[1]), Color::srgb(0.2, 1.0, 0.4));
            }
        }
        for tile in terrain_tiles {
            let key = (tile.scene, tile.tile.clone());
            let vertices = lab.collision.entry(key).or_insert_with(|| {
                let (vertices, triangles) = f.terrain_geometry(tile.scene, &tile.tile);
                unique_edges(&triangles)
                    .iter()
                    .map(|i| Vec3::from_array(vertices[*i as usize]))
                    .collect()
            });
            let into = to_camera(tile.frame);
            let point = |v: Vec3| {
                into.apply_point(tile.local_position + v.as_dvec3())
                    .as_vec3()
            };
            for edge in vertices.as_chunks::<2>().0 {
                gizmos.line(point(edge[0]), point(edge[1]), Color::srgb(0.2, 1.0, 0.4));
            }
        }
    }
    if let Some(prediction) = &lab.prediction
        && !lab.main_game
    {
        gizmos.linestrip(
            prediction.points.iter().map(|(_, p)| (*p - eye).as_vec3()),
            Color::srgb(0.2, 0.9, 1.0),
        );
    }
    if !lab.main_game
        && let Some(plan) = lab.session.sim().plans.get(&lab.session.sim().selected)
    {
        let trajectory = &plan.plan.trajectory;
        let into = to_camera(f.origin_frame());
        gizmos.linestrip(
            (0..trajectory.count()).map(|i| into.apply_point(trajectory.position(i)).as_vec3()),
            Color::srgb(1.0, 0.6, 0.15),
        );
    }
    if lab.main_game {
        let into = to_camera(f.vessel_frame(&sim.selected));
        let allocation = f.rcs_allocation(&sim.selected);
        for nozzle in allocation.nozzles.iter().filter(|n| n.throttle > 0.001) {
            let start = into.apply_point(nozzle.point);
            let end = into
                .apply_point(nozzle.point - nozzle.full_force.normalize() * nozzle.throttle * 1.5);
            gizmos.line(start.as_vec3(), end.as_vec3(), Color::srgb(0.4, 0.8, 1.0));
        }
        for (port, color) in [
            (&lab.own_port, Color::srgb(0.1, 1.0, 0.3)),
            (&lab.target_port, Color::srgb(1.0, 0.5, 0.1)),
        ] {
            if let Some(port) = port {
                let (mount, normal) = port_pose(lab, port);
                let local = mount + f.centre_of_mass_local(&port.vessel);
                let into = to_camera(f.vessel_frame(&port.vessel));
                gizmos.line(
                    into.apply_point(local).as_vec3(),
                    into.apply_point(local + normal).as_vec3(),
                    color,
                );
            }
        }
    }
    let p = f.thrust(&lab.session.sim().selected);
    let navigation = sim.navigation_body(&sim.selected);
    let body = &f.ephemeris.bodies()[navigation];
    let relative = f
        .frames()
        .transform(f.vessel_frame(&sim.selected), f.body_frames(navigation).0)
        .apply_state(void_frames::State {
            position: f.centre_of_mass_local(&sim.selected),
            velocity: DVec3::ZERO,
        });
    let axes = f
        .frames()
        .transform(f.body_frames(navigation).0, f.origin_frame())
        .rotation();
    let r = axes * relative.position;
    let v = axes * relative.velocity;
    let orbital = void_orbit::osculating_orbit(r, v, body.gm);
    let surface = f
        .frames()
        .transform(f.vessel_frame(&sim.selected), f.body_frames(navigation).1)
        .apply_state(void_frames::State {
            position: f.centre_of_mass_local(&sim.selected),
            velocity: DVec3::ZERO,
        });
    let altitude =
        if lab.session.sim().presentation.altitude_agl && sim.terrains.contains_key(&navigation) {
            f.clearance(&lab.session.sim().selected, navigation)
        } else {
            r.length() - body.radius_meters
        };
    let speed = if lab.session.sim().presentation.speed_surface {
        surface.velocity.length()
    } else {
        v.length()
    };
    let fuel: f64 = f
        .part_snapshots(&lab.session.sim().selected)
        .iter()
        .map(|p| p.fuel_kg)
        .sum();
    let aircraft_data = if f.control_profile(&sim.selected)
        == Some(void_assembly::ControlProfile::Aircraft)
    {
        let data = f.air_data(&sim.selected);
        let control = f.control(&sim.selected).turn;
        let airflow = if data.dynamic_pressure_pa < 1.0 {
            "low airflow; AoA/stall unavailable".to_owned()
        } else {
            format!(
                "max section AoA {:.1}° stall {:.0}%",
                data.maximum_angle_radians.to_degrees(),
                data.maximum_stall * 100.0
            )
        };
        format!(
            "\nAIR: {:.1}m/s q {:.1}kPa {} | pitch {:.0}% yaw {:.0}% roll {:.0}%\nB hold brakes | Q/E nose steering | aerodynamic control, no reaction wheel",
            data.airspeed_mps,
            data.dynamic_pressure_pa / 1000.0,
            airflow,
            control.x * 100.0,
            control.y * 100.0,
            control.z * 100.0
        )
    } else {
        String::new()
    };
    let stellar_status = sim
        .world
        .stellar
        .as_ref()
        .map(|stellar| {
            let system = f.vessel_system(&sim.selected).0;
            let name = if system == 0 {
                stellar.home.id.as_str()
            } else {
                stellar.neighbors[system - 1].placement.id.as_str()
            };
            format!(" · {name} system · stellar distances in light years")
        })
        .unwrap_or_default();
    **hud = Text::new(format!(
        "{}{}\n{} ({}) | {:?} | {} | {}x\nT+{:.2}s {} {:.1}m {} {:.1}m/s | {}\nmass {:.1}kg fuel {:.1}kg throttle {:.0}% force {:.1}kN SAS {:?}\nPe {:.1}km Ap {:.1}km | {} vessels\n{}\nTab vessel | Shift+Tab body focus | click map labels | 1–4/G plot frame | J body | Shift+J pair\nN home-site craft | O orbital craft | R reset | , . warp | K altitude | L speed\nF1 near/orbit/far | Home ship | Ctrl+Home stellar overview | Alt+F10/F11 exposure\nF2 wire | F3 boundaries | F4 actual colliders | F5 terrain\nF6 save | F7 load (paused) | F8 finish recording | F9 finish CPU profile\n{}",
        if lab.main_game {
            "VOID"
        } else {
            "FLEET FLIGHT INTEGRATION"
        },
        stellar_status,
        selected.name,
        lab.session.sim().selected,
        selected.mode,
        if if lab.playback.is_some() {
            lab.session.sim().presentation.paused
        } else {
            lab.paused
        } {
            "paused"
        } else {
            "running"
        },
        RATES[if lab.playback.is_some() {
            lab.session.sim().presentation.rate
        } else {
            lab.rate
        }],
        f.time(),
        if lab.session.sim().presentation.altitude_agl && sim.terrains.contains_key(&navigation) {
            "AGL"
        } else {
            "ALT"
        },
        altitude,
        if lab.session.sim().presentation.speed_surface {
            "surface"
        } else {
            "orbit"
        },
        speed,
        body.name,
        selected.mass_kg,
        fuel,
        f.control(&lab.session.sim().selected).throttle * 100.0,
        p.force.length() / 1000.0,
        f.sas_phase(&lab.session.sim().selected),
        (orbital.periapsis_radius_meters - body.radius_meters) / 1000.0,
        (orbital.apoapsis_radius_meters - body.radius_meters) / 1000.0,
        f.vessel_ids().len(),
        pilot_description(sim),
        format_args!(
            "{}{}\n{}\n{}\n{}\n{}\n{}",
            lab.notice,
            aircraft_data,
            scenery_description(sim),
            plotting_description(sim),
            format!(
                "{}{} | Water {:.0} N",
                thermal_description(lab),
                vehicle_description(sim),
                sim.fleet.water_wrench(&sim.selected).force.length()
            ),
            if lab.main_game
                && f.control_profile(&sim.selected) == Some(void_assembly::ControlProfile::Flight)
            {
                docking_description(lab)
            } else {
                String::new()
            },
            plan_description(lab)
        ),
    ));
    if let Some((profile, _)) = &mut lab.profile {
        profile.span("draw_lod_overlays", started, std::time::Instant::now());
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn main_reentry_heat_survives_checkpoint_and_journal() {
        let planet = void_landing::earth_size();
        let craft = void_assembly::reentry_capsule();
        let initial = InitialWorld::new(&planet, &craft, void_vessels::flat_site(&planet), true)
            .with_air_dynamics(void_vessels::AirDynamics::ForceAndTorque);
        let mut lab = new_lab(FlightSession::new(initial).with_recording(), craft);
        lab.main_game = true;
        reentry_fixture(&mut lab);
        lab.session.execute(Action::Advance {
            seconds: 120.0,
            rails: false,
        });
        lab.session.execute(Action::EndFrame {
            paused: true,
            rate: 0,
        });
        lab.session.mark();
        let id = format!("{}/shield", lab.session.sim().selected);
        let shield = lab.session.sim().fleet.parts().part(&id);
        let remaining = shield.resource(void_assembly::ResourceId::Ablator);
        println!(
            "reentry at 120 seconds: {remaining} kg ablator, {}",
            thermal_description(&lab)
        );
        assert!(
            remaining < 30.0,
            "the playable fixture must actually enter the atmosphere and spend ablator"
        );
        assert!(thermal_description(&lab).contains("ablator"));
        let checkpoint = void_fleet_flight::checkpoint::FlightCheckpoint::capture(
            lab.session.sim(),
            lab.session.recording_initial().clone(),
        );
        let mut loaded = FlightSession::from_checkpoint(checkpoint);
        assert_eq!(
            void_fleet_flight::session::world_mark(loaded.sim()),
            void_fleet_flight::session::world_mark(lab.session.sim())
        );
        for s in [&mut loaded, &mut lab.session] {
            s.execute(Action::Advance {
                seconds: 0.25,
                rails: false,
            });
            s.execute(Action::EndFrame {
                paused: true,
                rate: 0,
            });
        }
        assert_eq!(
            void_fleet_flight::session::world_mark(loaded.sim()),
            void_fleet_flight::session::world_mark(lab.session.sim())
        );
        lab.session.mark();
        let replayed = FlightSession::from_recording(lab.session.recording());
        assert_eq!(
            void_fleet_flight::session::world_mark(replayed.sim()),
            void_fleet_flight::session::world_mark(lab.session.sim())
        );
    }
    use super::*;
    use void_assembly::demo_craft;
    #[test]
    fn water_review_repeat_and_next_reset_without_accumulating_vessels() {
        let mut planet = void_landing::earth_size();
        planet.sea_level = Some(1800.);
        let craft = void_assembly::reentry_capsule();
        let initial = InitialWorld::new(&planet, &craft, DVec3::X, true);
        let mut lab = new_lab(FlightSession::new(initial).with_recording(), craft);
        lab.main_game = true;
        lab.water_review = Some(WATER_REVIEW_CASES[0]);
        water_fixture(&mut lab);
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_non_send(lab)
            .insert_resource(ButtonInput::<KeyCode>::default())
            .insert_resource(ButtonInput::<MouseButton>::default())
            .insert_resource(AccumulatedMouseMotion::default())
            .insert_resource(AccumulatedMouseScroll::default())
            .add_systems(Update, controls);
        app.world_mut().spawn(Window::default());
        for next in [false, true, false] {
            {
                let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
                keys.reset_all();
                keys.press(KeyCode::KeyR);
                if next {
                    keys.press(KeyCode::ShiftLeft);
                }
            }
            app.update();
            let lab = app.world().non_send::<Lab>();
            let sim = lab.session.sim();
            assert!(lab.paused);
            assert_eq!(sim.fleet.time(), 0.);
            assert_eq!(sim.fleet.vessel_ids().len(), 2);
            assert_eq!(sim.fleet.control(&sim.selected).throttle, 0.);
            let speed = sim
                .fleet
                .body_fixed_state(&sim.selected, sim.home)
                .velocity
                .length();
            assert!((speed - lab.water_review.unwrap()[0]).abs() < 1e-6);
        }
        let mut lab = app.world_mut().non_send_mut::<Lab>();
        assert_eq!(lab.water_review, Some(WATER_REVIEW_CASES[1]));
        let replayed = FlightSession::from_recording(lab.session.recording());
        assert_eq!(
            void_fleet_flight::session::world_mark(replayed.sim()),
            void_fleet_flight::session::world_mark(lab.session.sim())
        );
    }
    fn rendezvous_lab() -> Lab {
        let planet = game_planet_by_id("aurelia", None);
        let craft = void_assembly::rcs_flight_rocket();
        let site = demo_rocket(&planet.planet.terrain).launch_site.normalize();
        let initial = InitialWorld::new(&planet.planet, &craft, site, true)
            .with_air_dynamics(void_vessels::AirDynamics::ForceAndTorque);
        let mut lab = new_lab(FlightSession::new(initial).with_recording(), craft);
        lab.main_game = true;
        lab.rendezvous = true;
        lab.session.execute(Action::View {
            command: ViewCommand::Configure { main_camera: true },
        });
        rendezvous_fixture(&mut lab);
        lab
    }
    #[test]
    fn main_rendezvous_actions_capture_undock_and_replay() {
        let mut lab = rendezvous_lab();
        assert_eq!(
            lab.session.sim().fleet.options.air_dynamics,
            void_vessels::AirDynamics::ForceAndTorque
        );
        assert!(lab.paused);
        let a = lab.own_port.clone().unwrap();
        let b = lab.target_port.clone().unwrap();
        let mut keys = ButtonInput::default();
        keys.press(KeyCode::Enter);
        docking_controls(&mut lab, &keys);
        assert!(lab.notice.contains("disarmed"));
        keys.release_all();
        keys.clear();
        keys.press(KeyCode::F12);
        docking_controls(&mut lab, &keys);
        keys.release_all();
        keys.clear();
        keys.press(KeyCode::Enter);
        let direction = lab.session.sim().presentation.direction;
        let distance = lab.session.sim().presentation.distance;
        docking_controls(&mut lab, &keys);
        assert_eq!(lab.session.sim().presentation.direction, direction);
        assert_eq!(lab.session.sim().presentation.distance, distance);
        assert_eq!(
            lab.session.sim().fleet.vessel_ids().len(),
            2,
            "{}",
            lab.notice
        );
        assert_eq!(
            lab.session.sim().fleet.vessel_of_part(&a.part),
            lab.session.sim().fleet.vessel_of_part(&b.part)
        );
        assert!(
            lab.session
                .sim()
                .fleet
                .parts()
                .part(&a.part)
                .definition
                .modules
                .iter()
                .any(|m| matches!(m, Module::Command { .. }))
        );
        keys.release_all();
        keys.clear();
        keys.press(KeyCode::Backspace);
        docking_controls(&mut lab, &keys);
        assert_eq!(lab.session.sim().presentation.direction, direction);
        assert_eq!(lab.session.sim().presentation.distance, distance);
        assert_eq!(lab.session.sim().fleet.vessel_ids().len(), 3);
        assert_ne!(
            lab.session.sim().fleet.vessel_of_part(&a.part),
            lab.session.sim().fleet.vessel_of_part(&b.part)
        );
        assert_eq!(
            lab.session.sim().fleet.parts().part(&a.part).modules[&a.module],
            void_assembly::ModuleState::DockingPort { armed: false }
        );
        let mark = void_fleet_flight::session::world_mark(lab.session.sim());
        let replayed = FlightSession::from_recording(lab.session.recording());
        assert_eq!(mark, void_fleet_flight::session::world_mark(replayed.sim()));
    }
    #[test]
    fn main_pilot_handoff_clears_transient_controls_and_preserves_rcs_enable() {
        let mut lab = rendezvous_lab();
        let old = lab.session.sim().selected.clone();
        let target = lab.target_port.as_ref().unwrap().vessel.clone();
        lab.session.execute(Action::Rcs {
            control: void_vessels::RcsControl {
                enabled: true,
                force: DVec3::X * 80.0,
                torque: DVec3::Y * 30.0,
            },
        });
        lab.session.execute(Action::Control {
            throttle: 0.2,
            turn: DVec3::X,
        });
        select_pilot(&mut lab, &target);
        let rcs = lab.session.sim().fleet.rcs_control(&old);
        assert!(rcs.enabled);
        assert_eq!(rcs.force, DVec3::ZERO);
        assert_eq!(rcs.torque, DVec3::ZERO);
        assert_eq!(lab.session.sim().fleet.control(&old).turn, DVec3::ZERO);
        assert_eq!(lab.session.sim().fleet.control(&old).throttle, 0.2);
        assert!(!lab.session.sim().fleet.rcs_control(&target).enabled);
    }
    #[test]
    fn main_rendezvous_mounts_and_checkpoint_continuation() {
        let mut lab = rendezvous_lab();
        let a = lab.own_port.clone().unwrap();
        let b = lab.target_port.clone().unwrap();
        let f = &lab.session.sim().fleet;
        let sa = f.snapshot(&a.vessel);
        let sb = f.snapshot(&b.vessel);
        let (pa, na) = port_pose(&lab, &a);
        let (pb, nb) = port_pose(&lab, &b);
        assert!(
            (f.relative(&b.vessel, &a.vessel).position + sb.rotation * pb - sa.rotation * pa)
                .length()
                < 0.151
        );
        assert!((sa.rotation * na).dot(sb.rotation * nb) < -0.999);
        // Independent frame-tree mount lookup checks the rendered attachment geometry.
        let frames = f.frames();
        let mount = |port: &PortAddress| {
            let part = f.parts().part(&port.part);
            let Module::DockingPort { node_id, .. } = part
                .definition
                .modules
                .iter()
                .find(|m| m.id() == port.module)
                .unwrap()
            else {
                unreachable!()
            };
            let node = part
                .definition
                .nodes
                .iter()
                .find(|n| n.id == *node_id)
                .unwrap();
            let into = frames.transform(f.part_frame(&port.part), f.part_frame(&a.part));
            (
                into.apply_point(node.position),
                into.rotation() * node.direction,
            )
        };
        let (am, an) = mount(&a);
        let (bm, bn) = mount(&b);
        // LaunchState adds the setup displacement at solar-origin f64 precision (about 15 µm).
        assert!(
            ((bm - am).length() - 0.15).abs() < 2e-5,
            "frame mounts {:?} {:?}, distance {}",
            am,
            bm,
            (bm - am).length()
        );
        assert!(an.dot(bn) < -0.999999);
        assert!(
            lab.session
                .sim()
                .presentation
                .direction
                .dot(sa.rotation * na)
                .abs()
                < 0.1
        );
        assert_eq!(
            lab.craft
                .parts
                .iter()
                .find(|p| p.attachment.is_none())
                .unwrap()
                .id,
            "p1"
        );
        lab.session.execute(Action::Rcs {
            control: void_vessels::RcsControl {
                enabled: true,
                force: DVec3::X * 80.0,
                ..Default::default()
            },
        });
        lab.session.execute(Action::Advance {
            seconds: 0.05,
            rails: false,
        });
        let checkpoint = void_fleet_flight::checkpoint::FlightCheckpoint::capture(
            lab.session.sim(),
            lab.session.recording_initial().clone(),
        );
        let serialized = serde_json::to_string(&checkpoint).unwrap();
        let mut loaded = FlightSession::from_checkpoint(serde_json::from_str(&serialized).unwrap());
        let advance = Action::Advance {
            seconds: 0.05,
            rails: false,
        };
        lab.session.execute(advance.clone());
        loaded.execute(advance);
        assert_eq!(
            void_fleet_flight::session::world_mark(lab.session.sim()),
            void_fleet_flight::session::world_mark(loaded.sim())
        );
    }
    #[test]
    fn main_keyboard_focus_pause_and_held_handoff_neutralize_requests() {
        let mut lab = rendezvous_lab();
        lab.paused = false;
        let old = lab.session.sim().selected.clone();
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_non_send(lab)
            .insert_resource(ButtonInput::<KeyCode>::default())
            .insert_resource(ButtonInput::<MouseButton>::default())
            .insert_resource(AccumulatedMouseMotion::default())
            .insert_resource(AccumulatedMouseScroll::default())
            .add_systems(Update, controls);
        app.world_mut().spawn(Window::default());
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.press(KeyCode::KeyH);
            keys.press(KeyCode::AltLeft);
            keys.press(KeyCode::KeyW);
        }
        app.update();
        assert_eq!(
            app.world()
                .non_send::<Lab>()
                .session
                .sim()
                .fleet
                .rcs_control(&old)
                .force,
            DVec3::Z * 80.0
        );
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.clear();
            keys.press(KeyCode::Tab);
        }
        app.update();
        let lab = app.world().non_send::<Lab>();
        assert_ne!(lab.session.sim().selected, old);
        assert_eq!(lab.session.sim().fleet.rcs_control(&old).force, DVec3::ZERO);
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.release_all();
            keys.clear();
            keys.press(KeyCode::KeyH);
            keys.press(KeyCode::AltLeft);
            keys.press(KeyCode::KeyW);
        }
        app.update();
        let selected = app.world().non_send::<Lab>().session.sim().selected.clone();
        assert_eq!(
            app.world()
                .non_send::<Lab>()
                .session
                .sim()
                .fleet
                .rcs_control(&selected)
                .force,
            DVec3::Z * 80.0
        );
        app.world_mut()
            .non_send_mut::<Lab>()
            .session
            .execute(Action::Sas { enabled: true });
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.clear();
            keys.release(KeyCode::AltLeft);
        }
        app.update();
        assert_eq!(
            app.world()
                .non_send::<Lab>()
                .session
                .sim()
                .fleet
                .sas_phase(&selected),
            void_vessels::SasPhase::Off
        );
        assert!(
            app.world()
                .non_send::<Lab>()
                .notice
                .contains("disengaged SAS")
        );
        app.world_mut()
            .query::<&mut Window>()
            .single_mut(app.world_mut())
            .unwrap()
            .focused = false;
        app.update();
        assert_eq!(
            app.world()
                .non_send::<Lab>()
                .session
                .sim()
                .fleet
                .rcs_control(&selected)
                .force,
            DVec3::ZERO
        );
        app.world_mut()
            .query::<&mut Window>()
            .single_mut(app.world_mut())
            .unwrap()
            .focused = true;
        {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.clear();
            keys.press(KeyCode::KeyP);
        }
        app.update();
        let lab = app.world().non_send::<Lab>();
        assert!(lab.paused);
        assert_eq!(
            lab.session.sim().fleet.rcs_control(&selected).force,
            DVec3::ZERO
        );
    }
    fn aircraft_input_app(airborne: bool) -> App {
        let planet = void_fleet_flight::aircraft_acceptance_planet(void_landing::earth_size());
        let craft = void_assembly::aircraft();
        let site = DVec3::X;
        let initial = InitialWorld::new(&planet, &craft, site, true)
            .with_air_dynamics(void_vessels::AirDynamics::ForceAndTorque);
        let mut lab = new_lab(FlightSession::new(initial), craft.clone());
        lab.main_game = true;
        lab.paused = false;
        if airborne {
            let f = &lab.session.sim().fleet;
            let body = lab.session.sim().home;
            let transform = f
                .frames()
                .transform(f.body_frames(body).1, f.origin_frame());
            let upright = void_landing::upright_at(site);
            let state = transform.apply_state(void_frames::State {
                position: site * (planet.terrain.radius_meters + 1000.0),
                velocity: upright * DVec3::Z * 100.0,
            });
            let Outcome::Spawned(id) = lab.session.execute(Action::LaunchState {
                craft,
                position: state.position,
                velocity: state.velocity,
                rotation: transform.rotation() * upright,
                angular_velocity: DVec3::ZERO,
            }) else {
                panic!("aircraft flight fixture")
            };
            select_pilot(&mut lab, &id);
        }
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_non_send(lab)
            .insert_resource(ButtonInput::<KeyCode>::default())
            .insert_resource(ButtonInput::<MouseButton>::default())
            .insert_resource(AccumulatedMouseMotion::default())
            .insert_resource(AccumulatedMouseScroll::default())
            .add_systems(Update, controls);
        app.world_mut().spawn(Window::default());
        app
    }
    fn aircraft_world_torque(app: &App) -> DVec3 {
        let sim = app.world().non_send::<Lab>().session.sim();
        let load = sim.fleet.aerodynamic_wrench(&sim.selected);
        sim.fleet
            .frames()
            .transform(load.frame, sim.fleet.origin_frame())
            .apply_direction(load.torque)
    }
    #[test]
    fn aircraft_keyboard_produces_pitch_up_and_player_right_bank_and_yaw() {
        // Prove geometric directions from actual aerodynamic loads, independently of catalog
        // part names and the mirrored navball. Player right is nose cross top.
        for key in [KeyCode::KeyW, KeyCode::KeyD, KeyCode::KeyE] {
            let mut app = aircraft_input_app(true);
            let q = {
                let sim = app.world().non_send::<Lab>().session.sim();
                sim.fleet.snapshot(&sim.selected).rotation
            };
            let nose = q * DVec3::Z;
            let top = q * DVec3::Y;
            let right = nose.cross(top);
            let neutral = aircraft_world_torque(&app);
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(key);
            app.update();
            let change = aircraft_world_torque(&app) - neutral;
            let response = match key {
                KeyCode::KeyW => change.cross(nose).dot(top),
                KeyCode::KeyD => change.cross(top).dot(right),
                KeyCode::KeyE => change.cross(nose).dot(right),
                _ => unreachable!(),
            };
            assert!(
                response > 100.0,
                "{key:?}: world torque {change:?}, response {response}"
            );
        }
    }
    #[test]
    fn aircraft_keyboard_taxi_turns_toward_player_right() {
        let mut app = aircraft_input_app(false);
        let (id, start, up) = {
            let mut lab = app.world_mut().non_send_mut::<Lab>();
            lab.session.execute(Action::Advance {
                seconds: 5.0,
                rails: false,
            });
            lab.session.execute(Action::Stage);
            lab.session.execute(Action::Vehicle {
                control: void_assembly::VehicleControl::default(),
            });
            lab.session.execute(Action::Control {
                throttle: 0.3,
                turn: DVec3::ZERO,
            });
            lab.session.execute(Action::Advance {
                seconds: 5.0,
                rails: false,
            });
            let sim = lab.session.sim();
            let q = sim.fleet.snapshot(&sim.selected).rotation;
            (sim.selected.clone(), q * DVec3::Z, q * DVec3::Y)
        };
        let right = start.cross(up);
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyE);
        for _ in 0..40 {
            app.update();
            app.world_mut()
                .non_send_mut::<Lab>()
                .session
                .execute(Action::Advance {
                    seconds: 0.05,
                    rails: false,
                });
        }
        let lab = app.world().non_send::<Lab>();
        let end = lab.session.sim().fleet.snapshot(&id).rotation * DVec3::Z;
        assert!(
            end.dot(right) > 0.02,
            "E taxi nose {start:?} -> {end:?}, right {right:?}"
        );
    }
    pub(super) fn initialized_scene(main_game: bool) -> App {
        let planet = game_planet_by_id(if main_game { "aurelia" } else { "pebble" }, None);
        let craft = void_assembly::flight_rocket();
        let site = demo_rocket(&planet.planet.terrain).launch_site.normalize();
        let mut initial = InitialWorld::new(&planet.planet, &craft, site, main_game);
        if main_game {
            initial.world = void_fleet_flight::world::expanded_solar_scenery(&planet.planet);
        }
        let sim = FlightSession::new(initial).with_recording();
        let mut lab = new_lab(sim, craft);
        lab.main_game = main_game;
        lab.session.execute(Action::View {
            command: ViewCommand::Configure {
                main_camera: main_game,
            },
        });
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::asset::AssetPlugin::default()))
            .insert_resource(Assets::<Mesh>::default())
            .insert_resource(Assets::<bevy::mesh::skinning::SkinnedMeshInverseBindposes>::default())
            .insert_resource(Assets::<StandardMaterial>::default())
            .insert_resource(Assets::<Image>::default())
            .insert_resource(Assets::<Font>::default())
            .insert_resource(Assets::<crate::scenery::GroundMaterial>::default())
            .insert_resource(Assets::<crate::scenery::StarMaterial>::default())
            .insert_non_send(lab)
            .add_plugins(bevy::gizmos::GizmoPlugin)
            .add_systems(Startup, (setup, setup_scenery).chain())
            .add_systems(
                Update,
                (refresh_scenery, draw, draw_map, instruments, update_scenery).chain(),
            );
        // A Window component supplies dimensions; no WindowPlugin or OS window is created.
        app.world_mut().spawn(Window::default());
        app.update();
        {
            let mut lab = app.world_mut().non_send_mut::<Lab>();
            lab.session.execute(Action::View {
                command: ViewCommand::Toggle {
                    setting: Toggle::Colliders,
                },
            });
            lab.session.execute(Action::View {
                command: ViewCommand::Toggle {
                    setting: Toggle::Bounds,
                },
            });
            lab.session.execute(Action::View {
                command: ViewCommand::Toggle {
                    setting: Toggle::Wire,
                },
            });
            lab.session.execute(Action::View {
                command: ViewCommand::Toggle {
                    setting: Toggle::Terrain,
                },
            });
        }
        app.update();
        let lab = app.world().non_send::<Lab>();
        assert!(!lab.parts.is_empty());
        assert!(!lab.collision.is_empty());
        app
    }
    #[test]
    fn main_orbital_predictions_replay_and_resume_without_observation_side_effects() {
        let mut app = initialized_scene(true);
        app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            std::time::Duration::from_millis(31),
        ))
        .add_systems(Update, simulate.before(draw));
        {
            let mut lab = app.world_mut().non_send_mut::<Lab>();
            let craft = lab.craft.clone();
            let Outcome::Spawned(id) = lab.session.execute(Action::LaunchOrbit {
                craft,
                offset: DVec3::ZERO,
            }) else {
                panic!("orbital fixture spawn");
            };
            lab.session.execute(Action::Select { vessel: id });
            lab.paused = false;
        }
        // Exercise the actual main-game automatic forecast, not a renderer-only pad fixture.
        for _ in 0..5 {
            app.world_mut().non_send_mut::<Lab>().prediction_at = f64::NEG_INFINITY;
            app.update();
        }
        let mut lab = app.world_mut().non_send_mut::<Lab>();
        assert_eq!(lab.prediction_generation, 5);
        assert!(lab.prediction.as_ref().unwrap().points.len() > 3);
        let mut replay = FlightSession::from_recording(lab.session.recording()).with_recording();
        let saved = void_fleet_flight::checkpoint::FlightCheckpoint::capture(
            lab.session.sim(),
            lab.session.recording_initial().clone(),
        );
        let mut loaded = FlightSession::from_checkpoint(saved).with_recording();
        let expected = void_fleet_flight::session::world_mark(lab.session.sim());
        assert_eq!(
            void_fleet_flight::session::world_mark(replay.sim()),
            expected
        );
        assert_eq!(
            void_fleet_flight::session::world_mark(loaded.sim()),
            expected
        );
        for session in [&mut lab.session, &mut replay, &mut loaded] {
            session.execute(Action::Advance {
                seconds: 0.219,
                rails: false,
            });
            session.execute(Action::EndFrame {
                paused: false,
                rate: 0,
            });
        }
        let expected = void_fleet_flight::session::world_mark(lab.session.sim());
        assert_eq!(
            void_fleet_flight::session::world_mark(replay.sim()),
            expected
        );
        assert_eq!(
            void_fleet_flight::session::world_mark(loaded.sim()),
            expected
        );
    }
    #[test]
    fn paused_window_inputs_replay_camera_and_rendering_does_not_change_marks() {
        let mut app = initialized_scene(true);
        let initial_direction = app
            .world()
            .non_send::<Lab>()
            .session
            .sim()
            .presentation
            .direction;
        app.insert_resource(ButtonInput::<KeyCode>::default())
            .insert_resource(ButtonInput::<MouseButton>::default())
            .insert_resource(AccumulatedMouseMotion {
                delta: Vec2::new(14.0, -8.0),
            })
            .insert_resource(AccumulatedMouseScroll {
                unit: bevy::input::mouse::MouseScrollUnit::Line,
                delta: Vec2::new(0.0, -2.0),
            })
            .add_systems(Update, (controls, simulate).chain().before(draw));
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .press(KeyCode::KeyL);
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        app.update();
        let expected = {
            let mut lab = app.world_mut().non_send_mut::<Lab>();
            assert_ne!(lab.session.sim().presentation.direction, initial_direction);
            assert!(!lab.session.sim().presentation.speed_surface);
            assert_eq!(lab.session.sim().fleet.time(), 0.0);
            let recording = lab.session.recording();
            assert!(matches!(
                recording.entries.last().unwrap().action,
                Action::EndFrame { paused: true, .. }
            ));
            let replay = FlightSession::from_recording(recording).with_recording();
            let expected = void_fleet_flight::session::world_mark(lab.session.sim());
            assert_eq!(
                void_fleet_flight::session::world_mark(replay.sim()),
                expected
            );
            expected
        };
        // Disable input/physics systems for repeated presentation-only updates by loading the
        // completed recording into playback and pausing its host. No OS window is involved.
        {
            let mut lab = app.world_mut().non_send_mut::<Lab>();
            let recording = lab.session.recording();
            let (playback, _) = Playback::new(recording);
            lab.playback = Some(playback);
            lab.paused = true;
        }
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .reset_all();
        app.world_mut()
            .resource_mut::<AccumulatedMouseMotion>()
            .delta = Vec2::ZERO;
        app.world_mut()
            .resource_mut::<AccumulatedMouseScroll>()
            .delta = Vec2::ZERO;
        for _ in 0..3 {
            app.update();
        }
        assert_eq!(
            void_fleet_flight::session::world_mark(app.world().non_send::<Lab>().session.sim()),
            expected
        );
    }

    #[test]
    fn integration_scene_initializes_and_draws_without_a_window_or_renderer() {
        let _ = initialized_scene(false);
    }
    #[test]
    fn main_scene_draws_fleet_scenery_and_navball_without_a_window_or_renderer() {
        let mut app = initialized_scene(true);
        assert!(matches!(
            app.world_mut()
                .query::<&Msaa>()
                .single(app.world())
                .unwrap(),
            Msaa::Off
        ));
        let camera = app
            .world_mut()
            .query::<&Camera3d>()
            .single(app.world())
            .unwrap();
        assert!(
            bevy::render::render_resource::TextureUsages::from(camera.depth_texture_usages)
                .contains(bevy::render::render_resource::TextureUsages::TEXTURE_BINDING)
        );
        assert_eq!(
            app.world_mut()
                .query::<&bevy::camera::Hdr>()
                .iter(app.world())
                .count(),
            1
        );
        let home = app.world().non_send::<Lab>().session.sim().home;
        let Ground::World(world) = app.world().resource::<Ground>() else {
            panic!("main world renderer");
        };
        let material = world.bodies[&home].material.clone();
        let sea = game_planet_by_id("aurelia", None)
            .planet
            .terrain
            .radius_meters
            + void_terrain::SEA_LEVEL;
        assert_eq!(
            app.world()
                .resource::<Assets<crate::scenery::GroundMaterial>>()
                .get(&material)
                .unwrap()
                .ground
                .bottom_radius,
            sea as f32
        );
        let layers = app
            .world_mut()
            .query::<&crate::air::AirLayers>()
            .single(app.world())
            .unwrap();
        assert_eq!(layers.0.len(), 3);
        let earth = layers
            .0
            .iter()
            .find(|(a, _)| a.bottom_radius == sea as f32)
            .expect("Earth optical layer");
        assert_eq!(earth.0.sea_level, 0.0);
        assert_eq!(
            app.world_mut()
                .query::<&crate::navball::Navball>()
                .iter(app.world())
                .count(),
            1
        );
        let planet = game_planet_by_id("luna", None);
        let craft = demo_craft();
        let site = demo_rocket(&planet.planet.terrain).launch_site.normalize();
        {
            let mut lab = app.world_mut().non_send_mut::<Lab>();
            lab.session =
                FlightSession::new(InitialWorld::new(&planet.planet, &craft, site, false))
                    .with_recording();
            lab.dirty = true;
        }
        app.update();
        let Ground::World(world) = app.world().resource::<Ground>() else {
            unreachable!()
        };
        let material = world.bodies[&0].material.clone();
        assert_eq!(
            app.world()
                .resource::<Assets<crate::scenery::GroundMaterial>>()
                .get(&material)
                .unwrap()
                .ground
                .bottom_radius,
            planet.planet.terrain.radius_meters as f32
        );
    }

    #[test]
    fn solar_renderer_switches_all_bodies_and_reuses_assets_after_checkpoint_restore() {
        let mut app = initialized_scene(true);
        let images = app.world().resource::<Assets<Image>>().len();
        let materials = app
            .world()
            .resource::<Assets<crate::scenery::GroundMaterial>>()
            .len();
        let ids: Vec<_> = app
            .world()
            .non_send::<Lab>()
            .session
            .sim()
            .world
            .bodies
            .keys()
            .cloned()
            .collect();
        let before = app
            .world()
            .non_send::<Lab>()
            .session
            .sim()
            .fleet
            .snapshot(&app.world().non_send::<Lab>().session.sim().selected);
        for id in ids {
            {
                let mut lab = app.world_mut().non_send_mut::<Lab>();
                let body = lab.session.sim().world.body_index(&id);
                let radius = lab.session.sim().fleet.ephemeris.bodies()[body].radius_meters;
                lab.session.execute(Action::View {
                    command: ViewCommand::BodyPreset {
                        body,
                        direction: DVec3::new(1.0, 0.2, 0.3).normalize(),
                        distance: radius * 3.5,
                    },
                });
            }
            app.update();
            let lab = app.world().non_send::<Lab>();
            let after = lab
                .session
                .sim()
                .fleet
                .snapshot(&lab.session.sim().selected);
            assert_eq!(before.position, after.position);
            assert_eq!(before.velocity, after.velocity);
            let Ground::World(world) = app.world().resource::<Ground>() else {
                unreachable!()
            };
            assert_eq!(world.active, lab.session.sim().observation_body());
            assert_eq!(world.bodies.len(), 5);
            assert_eq!(world.atmospheres.len(), 3);
        }
        for _ in 0..3 {
            {
                let mut lab = app.world_mut().non_send_mut::<Lab>();
                let checkpoint = void_fleet_flight::checkpoint::FlightCheckpoint::capture(
                    lab.session.sim(),
                    lab.session.recording_initial().clone(),
                );
                lab.session = FlightSession::from_checkpoint(checkpoint).with_recording();
                lab.dirty = true;
            }
            app.update();
            assert_eq!(app.world().resource::<Assets<Image>>().len(), images);
            assert_eq!(
                app.world()
                    .resource::<Assets<crate::scenery::GroundMaterial>>()
                    .len(),
                materials
            );
            assert!(!app.world().non_send::<Lab>().parts.is_empty());
        }
    }

    #[test]
    fn warp_key_and_window_step_resume_one_x_before_ignition() {
        let planet = void_landing::earth_size();
        let pod = void_vessels::pod_tank("Resting pod");
        let craft = demo_craft();
        let mut session = FlightSession::new(InitialWorld::new(
            &planet,
            &pod,
            void_vessels::flat_site(&planet),
            false,
        ))
        .with_recording();
        session.execute(Action::Advance {
            seconds: 20.0,
            rails: false,
        });
        let Outcome::Spawned(id) = session.execute(Action::LaunchOrbit {
            craft: craft.clone(),
            offset: DVec3::ZERO,
        }) else {
            panic!("launch")
        };
        session.execute(Action::Select { vessel: id });
        session.execute(Action::Stage);
        let start = session.sim().fleet.time() + 200.037;
        session.execute(Action::AddManeuver {
            spec: void_orbit::ManeuverSpec {
                start_time: start,
                reference_body: session.sim().home,
                reference_mode: void_orbit::ReferenceMode::Fixed,
                prograde: 10.0,
                normal: 0.0,
                radial: 0.0,
            },
        });
        let mut lab = new_lab(session, craft);
        let mut keys = ButtonInput::<KeyCode>::default();
        keys.press(KeyCode::KeyZ);
        plan_controls(&mut lab, &keys);
        assert!(lab.session.sim().maneuver_warp.active());
        assert!(!lab.paused);
        let mut time = Time::<()>::default();
        time.advance_by(std::time::Duration::from_secs(1));
        simulate_inner(&time, &Window::default(), &mut lab);
        assert_eq!(lab.rate, 0);
        assert_eq!(lab.session.sim().fleet.time(), start - 30.0);
        assert!(lab.notice.contains("30 seconds"));
    }

    #[test]
    fn maneuver_keys_arm_a_plan_and_switch_ship_without_aborting_it() {
        let mut app = initialized_scene(true);
        let id = {
            let mut lab = app.world_mut().non_send_mut::<Lab>();
            let craft = lab.craft.clone();
            let Outcome::Spawned(id) = lab.session.execute(Action::LaunchOrbit {
                craft,
                offset: DVec3::ZERO,
            }) else {
                panic!("launch")
            };
            lab.session.execute(Action::Select { vessel: id.clone() });
            lab.session.execute(Action::Stage);
            id
        };
        app.insert_resource(ButtonInput::<KeyCode>::default())
            .insert_resource(ButtonInput::<MouseButton>::default())
            .insert_resource(AccumulatedMouseMotion::default())
            .insert_resource(AccumulatedMouseScroll::default())
            .add_systems(Update, controls.before(draw));
        for key in [KeyCode::KeyM, KeyCode::KeyB, KeyCode::Tab] {
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.reset_all();
            keys.press(key);
            app.update();
        }
        let lab = app.world().non_send::<Lab>();
        assert_ne!(lab.session.sim().selected, id);
        assert!(lab.session.sim().plans[&id].executing);
        assert_eq!(
            lab.session.sim().fleet.guidance(&id).unwrap().status,
            void_vessels::GuidanceStatus::Armed
        );
    }

    #[test]
    fn unfocused_window_does_not_inject_control_changes_into_replay() {
        let mut app = initialized_scene(false);
        let initial = app
            .world()
            .non_send::<Lab>()
            .session
            .recording_initial()
            .clone();
        let mut flown = FlightSession::new(initial).with_recording();
        flown.execute(Action::Control {
            throttle: 0.0,
            turn: DVec3::X * 0.25,
        });
        flown.execute(Action::Advance {
            seconds: 0.113,
            rails: false,
        });
        flown.mark();
        flown.execute(Action::Advance {
            seconds: 0.113,
            rails: false,
        });
        let (mut replay, mut session) = Playback::new(flown.recording());
        assert!(replay.next_frame(&mut session));
        let before = void_fleet_flight::session::world_mark(session.sim());
        {
            let mut lab = app.world_mut().non_send_mut::<Lab>();
            lab.session = session;
            lab.playback = Some(replay);
        }
        app.world_mut()
            .query::<&mut Window>()
            .single_mut(app.world_mut())
            .unwrap()
            .focused = false;
        app.insert_resource(ButtonInput::<KeyCode>::default())
            .insert_resource(ButtonInput::<MouseButton>::default())
            .insert_resource(AccumulatedMouseMotion::default())
            .insert_resource(AccumulatedMouseScroll::default())
            .add_systems(Update, controls.before(draw));
        app.update();
        assert_eq!(
            before,
            void_fleet_flight::session::world_mark(app.world().non_send::<Lab>().session.sim())
        );
    }
    #[test]
    fn native_hud_initializes_and_visual_toggles_restore_authored_layers() {
        let mut app = initialized_scene(true);
        app.insert_resource(ButtonInput::<KeyCode>::default())
            .insert_resource(ButtonInput::<MouseButton>::default())
            .insert_resource(AccumulatedMouseMotion::default())
            .insert_resource(AccumulatedMouseScroll::default())
            .add_message::<bevy::input::keyboard::KeyboardInput>()
            .add_systems(
                Update,
                (
                    ui::interactions,
                    ui::refresh,
                    ui::stages,
                    ui::indicators,
                    ui::apply_font,
                )
                    .chain()
                    .after(draw),
            );
        app.init_resource::<bevy::text::FontCx>();
        app.update();
        for off in [true, false] {
            {
                let mut lab = app.world_mut().non_send_mut::<Lab>();
                for setting in [
                    Toggle::VisualAir,
                    Toggle::VisualClouds,
                    Toggle::VisualOcean,
                    Toggle::VisualStars,
                ] {
                    lab.session.execute(Action::View {
                        command: ViewCommand::Toggle { setting },
                    });
                }
                let vesper = lab
                    .session
                    .sim()
                    .fleet
                    .ephemeris
                    .bodies()
                    .iter()
                    .find(|b| b.id.rsplit('/').next() == Some("vesper"))
                    .expect("Vesper authored")
                    .index;
                lab.session.execute(Action::View {
                    command: ViewCommand::Focus { body: Some(vesper) },
                });
            }
            app.update();
            let layers = app
                .world_mut()
                .query::<&crate::air::AirLayers>()
                .single(app.world())
                .unwrap();
            if off {
                assert!(
                    layers
                        .0
                        .iter()
                        .all(|(a, _)| a.enabled == 0. && a.clouds_enabled == 0.)
                );
            } else {
                assert!(
                    layers
                        .0
                        .iter()
                        .any(|(a, _)| a.enabled > 0. && a.clouds_enabled > 0.)
                );
            }
            let sky = app
                .world_mut()
                .query_filtered::<&Visibility, With<Sky>>()
                .single(app.world())
                .unwrap();
            assert_eq!(
                *sky,
                if off {
                    Visibility::Hidden
                } else {
                    Visibility::Inherited
                }
            );
            let Ground::World(world) = app.world().resource::<Ground>() else {
                panic!("world scenery");
            };
            let sim = app.world().non_send::<Lab>().session.sim();
            let vesper = sim
                .fleet
                .ephemeris
                .bodies()
                .iter()
                .find(|b| b.id.rsplit('/').next() == Some("vesper"))
                .unwrap()
                .index;
            let material = world.bodies[&vesper].material.clone();
            let g = &app
                .world()
                .resource::<Assets<crate::scenery::GroundMaterial>>()
                .get(&material)
                .unwrap()
                .ground;
            if off {
                assert_eq!(g.atmosphere_enabled, 0.);
                assert_eq!(g.ocean_enabled, 0.);
                assert_eq!(g.continuous_cloud, Vec4::ZERO);
            } else {
                assert!(g.atmosphere_enabled > 0.);
                assert!(g.continuous_cloud.x > 0.);
            }
        }
    }
}

#[derive(Component)]
struct Sky;
#[allow(clippy::too_many_arguments)]
fn setup_scenery(
    mut commands: Commands,
    lab: NonSend<Lab>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut standard: ResMut<Assets<StandardMaterial>>,
    grounds: Option<ResMut<Assets<crate::scenery::GroundMaterial>>>,
    stars: Option<ResMut<Assets<crate::scenery::StarMaterial>>>,
    camera: Single<Entity, With<LabCamera>>,
) {
    if !lab.main_game {
        return;
    }
    let world = crate::world_scenery::WorldScenery::new(
        &mut commands,
        lab.session.sim(),
        grounds.expect("world ground assets").into_inner(),
        &mut images,
        &mut meshes,
        &mut standard,
    );
    commands.insert_resource(Ground::World(Box::new(world)));
    let (positions, colors) = void_scenery::generate_stars(&void_scenery::DEFAULT_STARS);
    commands.spawn((
        Sky,
        Visibility::Inherited,
        Mesh3d(meshes.add(crate::scenery::star_mesh(positions, &colors))),
        MeshMaterial3d(
            stars
                .expect("star assets")
                .into_inner()
                .add(crate::scenery::StarMaterial { brightness: 0.08 }),
        ),
        Transform::default(),
        bevy::camera::visibility::NoFrustumCulling,
    ));
    commands.entity(*camera).insert((
        Camera3d {
            depth_texture_usages: (bevy::render::render_resource::TextureUsages::RENDER_ATTACHMENT
                | bevy::render::render_resource::TextureUsages::TEXTURE_BINDING)
                .into(),
            ..default()
        },
        bevy::camera::Hdr,
        bevy::core_pipeline::tonemapping::DebandDither::Disabled,
        Projection::Perspective(PerspectiveProjection {
            fov: 58.0_f32.to_radians(),
            far: 1e20,
            ..default()
        }),
        Msaa::Off,
        bevy::core_pipeline::tonemapping::Tonemapping::None,
        crate::air::AirLayers(vec![]),
        crate::air::AirSettings::new(&void_scenery::earth_like_atmosphere(
            lab.session.sim().planet.terrain.radius_meters,
        )),
    ));
}
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn refresh_scenery(
    mut commands: Commands,
    lab: NonSend<Lab>,
    mut ground: ResMut<Ground>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut standard: ResMut<Assets<StandardMaterial>>,
    grounds: Option<ResMut<Assets<crate::scenery::GroundMaterial>>>,
    far: Query<(Entity, &crate::world_scenery::FarBody)>,
    markers: Query<Entity, With<crate::map::MapMarker>>,
) {
    let Ground::World(world) = &mut *ground else {
        return;
    };
    if !lab.dirty
        || serde_json::to_value(&world.world).unwrap()
            == serde_json::to_value(&lab.session.sim().world).unwrap()
    {
        return;
    }
    let grounds = grounds.expect("world ground assets").into_inner();
    world.unload(&mut commands, &mut meshes, grounds, &mut images);
    for (entity, body) in &far {
        commands.entity(entity).despawn();
        meshes.remove(body.1);
        standard.remove(body.2);
    }
    for e in &markers {
        commands.entity(e).despawn();
    }
    crate::map::spawn_map_labels(&mut commands, lab.session.sim().fleet.ephemeris.bodies());
    **world = crate::world_scenery::WorldScenery::new(
        &mut commands,
        lab.session.sim(),
        grounds,
        &mut images,
        &mut meshes,
        &mut standard,
    );
}
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn update_scenery(
    lab: NonSend<Lab>,
    ground: Res<Ground>,
    window: Single<&Window>,
    grounds: Option<ResMut<Assets<crate::scenery::GroundMaterial>>>,
    mut camera: Query<
        (
            &Transform,
            &mut crate::air::AirSettings,
            &Projection,
            &mut crate::air::AirLayers,
        ),
        With<LabCamera>,
    >,
    mut sky: Query<(&mut Transform, &mut Visibility), (With<Sky>, Without<LabCamera>)>,
    mut light: Query<&mut Transform, (With<SceneSun>, Without<Sky>, Without<LabCamera>)>,
    mut far: Query<
        (
            &crate::world_scenery::FarBody,
            &mut Transform,
            &mut Visibility,
        ),
        (Without<Sky>, Without<SceneSun>, Without<LabCamera>),
    >,
) {
    let Ground::World(world) = &*ground else {
        return;
    };
    let sim = lab.session.sim();
    let fleet = &sim.fleet;
    let q = surface_axes(fleet, sim.observation_body());
    let sample = sim.presentation.sample(sim);
    let grounds = grounds.expect("world ground assets").into_inner();
    for (transform, mut air, projection, mut layers) in &mut camera {
        let Projection::Perspective(p) = projection else {
            panic!("world camera requires perspective");
        };
        let focal =
            f64::from(window.physical_height().max(1)) / (2.0 * (f64::from(p.fov) / 2.0).tan());
        let (settings, volumes, sun) = world.update_air(
            sim,
            &sample,
            q,
            crate::world_scenery::AirView {
                camera: transform,
                projection: p,
                focal,
            },
            grounds,
        );
        *air = settings;
        *layers = volumes;
        for mut t in &mut light {
            *t = Transform::default().looking_to(
                (-sun).as_vec3(),
                if sun.z.abs() < 0.99 { Vec3::Z } else { Vec3::Y },
            );
        }
    }
    for (mut t, mut visibility) in &mut sky {
        *t = Transform::from_rotation(q.conjugate().as_quat());
        *visibility = if sim.presentation.visual_stars {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
    for (body, mut t, mut visibility) in &mut far {
        let into = sample.to_camera(fleet, fleet.body_frames(body.0).1, q);
        *t = Transform::from_translation(into.apply_point(DVec3::ZERO).as_vec3())
            .with_rotation(into.rotation().as_quat())
            .with_scale(Vec3::splat(
                fleet.ephemeris.bodies()[body.0].radius_meters as f32,
            ));
        let solid = world.bodies.get(&body.0);
        *visibility = if solid.is_some()
            && (!sim.presentation.terrain
                || (body.0 == world.active
                    && !body.3
                    && solid.is_some_and(|b| b.field.drawn_count() > 0)))
        {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn draw_map(
    mut lab: NonSendMut<Lab>,
    camera: Single<(&Camera, &GlobalTransform), With<LabCamera>>,
    mut markers: Query<(
        &crate::map::MapMarker,
        &mut Node,
        &mut Visibility,
        &ComputedNode,
    )>,
    mut texts: Query<(&mut Text, &mut Visibility), Without<crate::map::MapMarker>>,
    mut gizmos: Gizmos,
) {
    let lab = &mut *lab;
    if !lab.main_game {
        return;
    }
    let view = lab.view.expect("main camera state");
    let fleet = &lab.session.sim().fleet;
    let home = lab.session.sim().observation_body();
    let bodies = fleet.ephemeris.bodies();
    let mut positions = vec![DVec3::ZERO; bodies.len()];
    let mut velocities = positions.clone();
    fleet
        .ephemeris
        .states_at(fleet.time(), &mut positions, Some(&mut velocities));
    let ship = fleet.snapshot(&lab.session.sim().selected);
    let reference = void_orbit::DominanceTree::new(bodies).dominant(&positions, ship.position);
    let q = surface_axes(fleet, home);
    let frame = void_view::MapFrame {
        time: fleet.time(),
        positions: &positions,
        velocities: &velocities,
        origin: lab.focus_position,
        vessel: ship.position,
        vessel_velocity: ship.velocity,
        plotting: void_view::PlottingFrame {
            kind: lab.session.sim().presentation.path_frame(),
            reference,
        },
        // Simulation time makes refresh cadence independent of replay rendering speed.
        wall_ms: fleet.time() * 1000.0,
    };
    let spec = lab.session.sim().presentation.plotting_frame;
    let apsis_reference = match spec {
        void_orbit::FrameSpec::BodyInertial { body }
        | void_orbit::FrameSpec::BodySurface { body } => body,
        void_orbit::FrameSpec::TwoBodyRotating { primary, .. } => primary,
        void_orbit::FrameSpec::Barycentric => reference,
    };
    if let Some(prediction) = &lab.prediction {
        lab.plot_path.update(
            &fleet.ephemeris,
            &prediction.trajectory,
            spec,
            lab.prediction_generation,
            fleet.time(),
            frame.origin,
            apsis_reference,
        );
    } else {
        lab.plot_path = Default::default();
    }
    if lab.plan_vessel != lab.session.sim().selected {
        lab.plot_plan = Default::default();
        lab.plan_vessel = lab.session.sim().selected.clone();
    }
    if let Some(p) = lab.session.sim().plans.get(&lab.plan_vessel) {
        lab.plot_plan.update(
            &fleet.ephemeris,
            &p.plan.trajectory,
            spec,
            p.plan.generation,
            fleet.time(),
            frame.origin,
            apsis_reference,
        );
    } else {
        lab.plot_plan = Default::default();
    }
    let eye_inertial = fleet
        .frames()
        .transform(fleet.body_frames(home).1, fleet.origin_frame())
        .apply_point(lab.eye);
    let render = |v: DVec3| (q.conjugate() * (v + frame.origin - eye_inertial)).as_vec3();
    if view.map_weight > 0.0 {
        for (body, points) in bodies.iter().zip(lab.body_plots.update(
            &fleet.ephemeris,
            spec,
            fleet.time(),
            frame.origin,
        )) {
            gizmos.linestrip(
                points.into_iter().map(&render),
                crate::map::color(&body.color).with_alpha(view.map_weight as f32),
            );
        }
        for (path, color) in [
            (&lab.plot_path, crate::map::color(crate::map::PATH_COLOR)),
            (&lab.plot_plan, Color::srgb(1.0, 0.6, 0.15)),
        ] {
            gizmos.linestrip(
                path.points.iter().copied().map(&render),
                color.with_alpha(view.map_weight as f32),
            );
        }
    }
    let wanted = void_view::map_labels(
        bodies,
        &frame,
        lab.session.sim().presentation.focus_body,
        &lab.plot_path.apsides,
    );
    let (camera, transform) = *camera;
    crate::map::place_map_labels(
        camera,
        transform,
        &mut markers,
        &mut texts,
        &wanted,
        view.map_weight,
        &render,
    );
}

#[cfg(test)]
mod mercury_fixture_tests {
    use super::*;
    #[test]
    fn vesper_ground_fixture_uses_real_world_and_terrain() {
        let planet = void_landing::aurelia();
        let craft = void_vessels::pod_tank("Vesper fixture witness");
        for site in ["plains", "shield", "upland"] {
            let mut initial = InitialWorld::new(&planet, &craft, DVec3::X, true);
            initial.world = void_fleet_flight::world::solar_scenery(&planet);
            vesper_fixture(&mut initial, site);
            let sim = initial.build();
            assert_eq!(sim.fleet.ephemeris.bodies()[sim.home].id, "vesper");
            assert!(matches!(
                sim.planet.terrain.config(),
                void_terrain::TerrainConfig::Volcanic(_)
            ));
            assert_eq!(
                sim.planet.terrain.config(),
                sim.terrains[&sim.home].config()
            );
        }
    }
    #[test]
    fn surface_entries_are_real_initial_worlds_with_shared_terrain() {
        let planet = void_landing::aurelia();
        let craft = void_vessels::pod_tank("Mercury fixture witness");
        let mut sites = Vec::new();
        for site in ["basin", "rim", "ejecta"] {
            let mut initial = InitialWorld::new(&planet, &craft, DVec3::X, true);
            initial.world = void_fleet_flight::world::solar_scenery(&planet);
            cinder_fixture(&mut initial, site);
            assert_eq!(initial.launch_body, "cinder");
            assert!((initial.launch_site.length() - 1.0).abs() < 1e-12);
            sites.push(initial.launch_site);
            let sim = initial.build();
            assert_eq!(sim.fleet.ephemeris.bodies()[sim.home].id, "cinder");
            assert_eq!(sim.fleet.vessel_ids().len(), 1);
            assert_eq!(
                sim.planet.terrain.config(),
                sim.terrains[&sim.home].config()
            );
        }
        assert!((sites[0] - sites[1]).length() > 0.1);
        assert!((sites[1] - sites[2]).length() > 0.1);
    }
}

#[cfg(test)]
mod ares_fixture_tests {
    use super::*;
    #[test]
    fn ground_sites_use_ordinary_world_and_owner() {
        let planet = void_landing::aurelia();
        let craft = void_vessels::pod_tank("Ares fixture witness");
        for site in ["plains", "canyon", "volcano"] {
            let mut initial = InitialWorld::new(&planet, &craft, DVec3::X, true);
            initial.world = void_fleet_flight::world::solar_scenery(&planet);
            ares_fixture(&mut initial, site);
            let sim = initial.build();
            assert_eq!(sim.fleet.ephemeris.bodies()[sim.home].id, "ares");
            assert_eq!(
                sim.planet.terrain.config(),
                sim.terrains[&sim.home].config()
            );
            assert!(sim.planet.terrain.height(initial.launch_site) > 0.0);
            let sun = sim
                .fleet
                .frames()
                .transform(
                    sim.fleet.body_frames(initial.world.body_index("sol")).0,
                    sim.fleet.body_frames(sim.home).1,
                )
                .apply_point(DVec3::ZERO)
                .normalize();
            eprintln!(
                "Ares {site}: body-fixed sun {sun}, site cosine {}",
                sun.dot(initial.launch_site)
            );
            assert!(
                sun.dot(initial.launch_site) > 0.15,
                "{site} must begin in useful daylight: {sun}"
            );
        }
    }
}
