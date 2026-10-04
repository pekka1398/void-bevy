//! The six owning TS lab scenarios, usable by both headless checks and the Bevy lab.
use crate::{Fleet, FleetOptions, GroundSpec, VesselMode};
use glam::{DQuat, DVec3};
use std::sync::Arc;
use void_assembly::{Craft, add_part, demo_craft, fresh_craft};
use void_environment::{BodyEnvironment, Environment};
use void_frames::{BodyId, BodyStates};
use void_landing::{
    ContactWorldOptions, LandingPlanet, aurelia, level_for_tile_size, pebble, planet_ephemeris,
};
use void_orbit::{PropagationRun, VesselState};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scenario {
    Launch,
    Encounter,
    Coast,
    Separate,
    Join,
    Sas,
}
impl Scenario {
    pub const ALL: [Self; 6] = [
        Self::Launch,
        Self::Encounter,
        Self::Coast,
        Self::Separate,
        Self::Join,
        Self::Sas,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::Launch => "Ground launch",
            Self::Encounter => "Orbital encounter",
            Self::Coast => "Bubble coast",
            Self::Separate => "Spinning separation",
            Self::Join => "Collision / join",
            Self::Sas => "SAS tumble",
        }
    }
}
pub struct LabScene {
    pub fleet: Fleet,
    pub planet: LandingPlanet,
    pub body_index: usize,
    pub references: Vec<(String, PropagationRun)>,
    pub scenario: Scenario,
}
pub fn pod_tank(name: &str) -> Craft {
    let mut c = add_part(&fresh_craft(), "tank-small", "p1", "bottom", "top").unwrap();
    c.name = name.into();
    c
}
pub fn nearby_site(d: DVec3, metres: f64, radius: f64) -> DVec3 {
    let east = if d.x.hypot(d.y) > 1e-9 {
        DVec3::new(-d.y, d.x, 0.0).normalize()
    } else {
        DVec3::X
    };
    d * (metres / radius).cos() + east * (metres / radius).sin()
}
pub fn flat_site(planet: &LandingPlanet) -> DVec3 {
    let t = &planet.terrain;
    let base = DVec3::new(0.8, 0.55, 0.25).normalize();
    let east = DVec3::new(-base.y, base.x, 0.0).normalize();
    let north = base.cross(east);
    let mut best = base;
    let mut slope = f64::INFINITY;
    for i in -20..20 {
        for j in -20..20 {
            let d = (base
                + east * (i as f64 * 50.0 / t.radius_meters)
                + north * (j as f64 * 50.0 / t.radius_meters))
                .normalize();
            let h = |e: f64, n: f64| {
                t.height(
                    (d + east * (e / t.radius_meters) + north * (n / t.radius_meters)).normalize(),
                )
            };
            let value = (h(3.0, 0.0) - h(-3.0, 0.0))
                .abs()
                .max((h(0.0, 3.0) - h(0.0, -3.0)).abs())
                / 6.0;
            if value < slope {
                slope = value;
                best = d;
            }
        }
    }
    best
}
pub fn create_lab_scene(scenario: Scenario) -> LabScene {
    let planet = if scenario == Scenario::Launch {
        pebble()
    } else {
        aurelia()
    };
    let (ephemeris, body_index) = planet_ephemeris(&planet);
    let r = planet.terrain.radius_meters + 400_000.0;
    let (centre, velocity) = ephemeris.body_state(BodyId(body_index), 0.0);
    let gm = ephemeris.bodies()[body_index].gm;
    let initial = |offset: DVec3, dv: DVec3| void_landing::FrameState {
        position: centre + DVec3::X * r + offset,
        velocity: velocity + DVec3::Y * (gm / r).sqrt() + dv,
    };
    let environment = Arc::new(
        Environment::new(&ephemeris)
            .with(body_index, BodyEnvironment::airless(planet.terrain.clone())),
    );
    let ground = GroundSpec {
        body_index,
        band_enter_meters: 200.0,
        band_exit_meters: 400.0,
        tiles: ContactWorldOptions {
            step_seconds: 1.0 / 60.0,
            tile_level: level_for_tile_size(planet.terrain.radius_meters, 300.0),
            tile_resolution: 33,
            tile_reach_meters: 300.0,
            tile_keep_meters: 600.0,
            recenter_meters: 5000.0,
            sleeping: true,
        },
    };
    let mut fleet = Fleet::new(
        ephemeris,
        environment,
        0.0,
        vec![ground],
        FleetOptions::default(),
    );
    let mut references = vec![];
    match scenario {
        Scenario::Launch => {
            let site = flat_site(&planet);
            fleet.launch_landed(&demo_craft(), body_index, site);
            fleet.launch_landed(
                &pod_tank("Near vessel / 50 m"),
                body_index,
                nearby_site(site, 50.0, planet.terrain.radius_meters),
            );
            fleet.launch_landed(
                &pod_tank("Far vessel / 5 km"),
                body_index,
                nearby_site(site, 5000.0, planet.terrain.radius_meters),
            );
        }
        Scenario::Encounter | Scenario::Coast => {
            let b = if scenario == Scenario::Encounter {
                initial(DVec3::new(30.0, 0.0, -3850.0), DVec3::Z * 9.0)
            } else {
                initial(DVec3::Y * 300.0, DVec3::ZERO)
            };
            for (name, s) in [
                ("Vessel A", initial(DVec3::ZERO, DVec3::ZERO)),
                ("Vessel B", b),
            ] {
                let id = fleet.launch(&pod_tank(name), s, DQuat::IDENTITY, DVec3::ZERO);
                references.push((
                    id.clone(),
                    PropagationRun::new(VesselState {
                        time: 0.0,
                        position: s.position,
                        velocity: s.velocity,
                        mass_kg: fleet.snapshot(&id).mass_kg,
                    }),
                ));
            }
        }
        Scenario::Separate => {
            fleet.launch(
                &demo_craft(),
                initial(DVec3::ZERO, DVec3::ZERO),
                DQuat::from_rotation_x(0.6),
                DVec3::Y * 0.05,
            );
        }
        Scenario::Join => {
            fleet.launch(
                &pod_tank("Join A"),
                initial(DVec3::ZERO, DVec3::ZERO),
                DQuat::IDENTITY,
                DVec3::ZERO,
            );
            fleet.launch(
                &pod_tank("Join B"),
                initial(DVec3::new(0.05, -4.6, 0.0), DVec3::Y * 0.1),
                DQuat::from_xyzw(1.0, 0.0, 0.0, 0.0),
                DVec3::ZERO,
            );
        }
        Scenario::Sas => {
            fleet.launch(
                &pod_tank("Tumbling vessel"),
                initial(DVec3::ZERO, DVec3::ZERO),
                DQuat::IDENTITY,
                DVec3::new(0.2, 0.1, -0.2),
            );
            fleet.launch(
                &pod_tank("Companion"),
                initial(DVec3::Y * 200.0, DVec3::ZERO),
                DQuat::IDENTITY,
                DVec3::ZERO,
            );
        }
    }
    fleet.advance(0.0);
    assert!(fleet.vessel_ids().iter().all(|id| fleet.snapshot(id).mode
        == if scenario == Scenario::Launch {
            VesselMode::Ground
        } else if matches!(scenario, Scenario::Encounter | Scenario::Separate) {
            VesselMode::Orbit
        } else {
            VesselMode::Bubble
        }));
    LabScene {
        fleet,
        planet,
        body_index,
        references,
        scenario,
    }
}
