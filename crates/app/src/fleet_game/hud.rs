//! Text for the HUD and the DEV diagnostics.
use super::*;

pub(super) fn scenery_description(sim: &void_fleet_flight::FleetFlight) -> String {
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

pub(super) fn plotting_description(sim: &void_fleet_flight::FleetFlight) -> String {
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

pub(super) fn pilot_description(sim: &void_fleet_flight::FleetFlight) -> String {
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

pub(super) fn vehicle_description(sim: &void_fleet_flight::FleetFlight) -> String {
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

pub(super) fn thermal_description(session: &FlightSession) -> String {
    let parts = session.sim().fleet.part_snapshots(&session.sim().selected);
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

pub(super) fn plan_description(session: &FlightSession) -> String {
    let sim = session.sim();
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
