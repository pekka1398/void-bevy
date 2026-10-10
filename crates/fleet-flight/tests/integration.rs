use glam::{DQuat, DVec3};
use void_assembly::{Craft, demo_craft, export_craft, import_craft};
use void_fleet_flight::FleetFlight;
use void_frames::State;
use void_landing::PlanetFrame;
use void_testkit::earth_size;
use void_testkit::{flat_site, pod_tank};
use void_vessels::{SasPhase, VesselControl, VesselMode};

fn make(craft: &Craft, air: bool) -> FleetFlight {
    let planet = earth_size();
    let site = flat_site(&planet);
    FleetFlight::new(planet, craft, site, air)
}
fn airborne(sim: &mut FleetFlight, craft: &Craft, height: f64, speed: f64, offset: f64) -> String {
    let frame = PlanetFrame::new(&sim.fleet.ephemeris, sim.home);
    let direction = DVec3::X;
    let r = frame.body.radius_meters + sim.planet.terrain.height(direction) + height;
    let state = frame.to_inertial(
        &sim.fleet.ephemeris,
        sim.fleet.time(),
        State {
            position: DVec3::new(r, offset, 0.0),
            velocity: DVec3::Y * speed,
        },
    );
    let a = void_orbit::body_orientation(&frame.body.rotation, sim.fleet.time());
    let axes = DQuat::from_mat3(&glam::DMat3::from_cols(a[0], a[1], a[2])).normalize();
    let id = sim.fleet.launch(craft, state, axes, DVec3::ZERO);
    sim.fleet.advance(0.0);
    id
}
#[test]
fn exported_custom_craft_stages_and_keeps_each_vessels_controls() {
    let mut craft = demo_craft();
    craft.name = "Acceptance craft".into();
    craft
        .parts
        .iter_mut()
        .find(|p| p.id == "p2")
        .unwrap()
        .resources
        .insert(void_assembly::ResourceId::LiquidPropellant, 200.0);
    let loaded = import_craft(&export_craft(&craft).unwrap()).unwrap();
    let mut sim = make(&loaded, false);
    assert_eq!(sim.mode(), VesselMode::Ground);
    assert_eq!(sim.fleet.fuel("v1/p2"), 200.0);
    sim.sas(true);
    sim.control(VesselControl {
        throttle: 0.7,
        turn: DVec3::ZERO,
    });
    assert!(sim.stage().is_empty());
    let mass = sim.fleet.snapshot("v1").mass_kg;
    let children = sim.stage();
    assert_eq!(children.len(), 1);
    let child = &children[0];
    assert!(
        (sim.fleet.snapshot("v1").mass_kg + sim.fleet.snapshot(child).mass_kg - mass).abs() < 1e-9
    );
    assert_eq!(sim.fleet.control(child).throttle, 0.0);
    sim.select(child);
    sim.control(VesselControl {
        throttle: 0.2,
        turn: DVec3::ZERO,
    });
    sim.select("v1");
    assert_eq!(sim.fleet.control("v1").throttle, 0.7);
    assert_ne!(sim.fleet.sas_phase("v1"), SasPhase::Off);
}
#[test]
fn pressure_reduces_thrust_without_double_charging_fuel() {
    let craft = demo_craft();
    let (mut air, mut vacuum) = (make(&craft, true), make(&craft, false));
    for sim in [&mut air, &mut vacuum] {
        sim.control(VesselControl {
            throttle: 1.0,
            turn: DVec3::ZERO,
        });
        sim.stage();
    }
    let (a, v) = (air.fleet.thrust("v1"), vacuum.fleet.thrust("v1"));
    assert!(a.force.length() < v.force.length() && a.force.length() > 0.8 * v.force.length());
    assert_eq!(a.flow_kg_per_second, v.flow_kg_per_second);
    air.advance(1.0, false).unwrap();
    vacuum.advance(1.0, false).unwrap();
    assert!((air.fleet.fuel("v1/p5") - vacuum.fleet.fuel("v1/p5")).abs() < 1e-9);
    assert!(
        air.advance(1.0, true)
            .unwrap_err()
            .contains("engine firing")
    );
}
#[test]
fn air_slows_ground_orbit_and_bubble_owners() {
    let craft = pod_tank("Coasting air check");
    for (height, bubble, expected) in [
        (100.0, false, VesselMode::Ground),
        (10_000.0, false, VesselMode::Orbit),
        (10_000.0, true, VesselMode::Bubble),
    ] {
        let (mut air, mut vacuum) = (make(&craft, true), make(&craft, false));
        let (a, v) = (
            airborne(&mut air, &craft, height, 100.0, 0.0),
            airborne(&mut vacuum, &craft, height, 100.0, 0.0),
        );
        if bubble {
            airborne(&mut air, &craft, height, 100.0, 30.0);
            airborne(&mut vacuum, &craft, height, 100.0, 30.0);
        }
        assert_eq!(air.fleet.snapshot(&a).mode, expected);
        air.fleet.advance(1.0);
        vacuum.fleet.advance(1.0);
        let ay = air.fleet.body_fixed_state(&a, air.home).velocity.y;
        let vy = vacuum.fleet.body_fixed_state(&v, vacuum.home).velocity.y;
        println!("{expected:?}: with air {ay} vs vacuum {vy} m/s");
        assert!(ay < vy - 0.01, "{expected:?} did not receive drag");
    }
}
#[test]
fn high_orbit_is_vacuum_and_coast_prediction_does_not_advance_fleet() {
    let craft = pod_tank("Orbit check");
    let (mut air, mut vacuum) = (make(&craft, true), make(&craft, false));
    let a = air.launch_orbital(&craft, DVec3::ZERO);
    let v = vacuum.launch_orbital(&craft, DVec3::ZERO);
    air.select(&a);
    vacuum.select(&v);
    let time = air.fleet.time();
    let before = air.fleet.snapshot(&a);
    let predicted = air.predict(30.0);
    assert!(predicted.points.len() >= 3);
    assert_eq!(air.fleet.time(), time);
    assert_eq!(air.fleet.snapshot(&a).position, before.position);
    air.advance(30.0, false).unwrap();
    vacuum.advance(30.0, false).unwrap();
    assert!((air.fleet.snapshot(&a).position - vacuum.fleet.snapshot(&v).position).length() < 1e-5);
}

fn air_coast_differential(step_seconds: f64) -> (f64, f64) {
    let craft = pod_tank("Owner differential");
    let (mut orbit, mut bubble, mut rails) =
        (make(&craft, true), make(&craft, true), make(&craft, true));
    for sim in [&mut orbit, &mut bubble, &mut rails] {
        // This differential concerns airborne owners only; omit the unrelated launch-pad ship.
        let (ephemeris, _) = void_testkit::planet_ephemeris(&sim.planet);
        // The same world: Aurelia's air at density scale 1.
        let environment = sim.fleet.environment().clone();
        sim.fleet = void_vessels::Fleet::new(
            ephemeris,
            environment,
            0.0,
            vec![],
            void_vessels::FleetOptions {
                step_seconds,
                ..Default::default()
            },
        );
    }
    let o = airborne(&mut orbit, &craft, 10_000.0, 100.0, 0.0);
    let b = airborne(&mut bubble, &craft, 10_000.0, 100.0, 0.0);
    airborne(&mut bubble, &craft, 10_000.0, 100.0, 30.0);
    let r = airborne(&mut rails, &craft, 10_000.0, 100.0, 0.0);
    // All three coast for the same elapsed time; the extra bubble neighbour must not touch the
    // measured vessel.
    for sim in [&mut orbit, &mut bubble, &mut rails] {
        sim.fleet.advance(6.0);
    }
    assert_eq!(bubble.fleet.snapshot(&b).mode, VesselMode::Bubble);
    assert!(rails.fleet.rails_blocker().is_none());
    orbit.fleet.advance(10.0);
    bubble.fleet.advance(10.0);
    assert!(rails.fleet.advance_on_rails(10.0));
    let frame = PlanetFrame::new(&orbit.fleet.ephemeris, orbit.home);
    let body_fixed = |sim: &FleetFlight, id: &str| {
        let snapshot = sim.fleet.snapshot(id);
        frame.to_body_fixed(
            &sim.fleet.ephemeris,
            sim.fleet.time(),
            State {
                position: snapshot.position,
                velocity: snapshot.velocity,
            },
        )
    };
    let reference = body_fixed(&orbit, &o);
    let mut bubble_error = (0.0, 0.0);
    for (name, sim, id, budget) in [
        ("bubble", &bubble, &b, f64::INFINITY),
        ("rails", &rails, &r, 1e-3),
    ] {
        let state = body_fixed(sim, id);
        let dp = (state.position - reference.position).length();
        let dv = (state.velocity - reference.velocity).length();
        println!("air coast {name} vs orbit: {dp:e} m, {dv:e} m/s");
        if name == "bubble" {
            bubble_error = (dp, dv);
        }
        // Rails and orbit share the adaptive propagator; their outputs must match tightly.
        assert!(
            dp < budget && dv < budget / 10.0,
            "{name}: {dp} m, {dv} m/s"
        );
    }
    bubble_error
}

#[test]
fn atmospheric_coast_agrees_across_orbit_bubble_and_rails() {
    let coarse = air_coast_differential(1.0 / 60.0);
    let fine = air_coast_differential(1.0 / 240.0);
    // Velocity-dependent air is sampled once per fixed scene step. Its first-order error must
    // shrink with the step, rather than conceal an owner mismatch behind a larger tolerance.
    assert!(fine.0 < 0.1 && fine.1 < 0.01, "refined coast: {fine:?}");
    assert!(
        fine.0 < coarse.0 * 0.3 && fine.1 < coarse.1 * 0.3,
        "four-times finer steps did not converge: coarse {coarse:?}, fine {fine:?}"
    );
}

#[test]
fn fleet_plan_uses_live_staged_engine_and_trait_ephemeris_without_spending_fuel() {
    let craft = demo_craft();
    let mut sim = make(&craft, false);
    assert!(sim.new_plan(&sim.selected, 60.0).is_err()); // Ground ship is not ready.
    let vessel = sim.launch_orbital(&craft, DVec3::ZERO);
    sim.select(&vessel);
    assert!(sim.new_plan(&vessel, 60.0).is_err()); // Unstaged engines are not invented.
    sim.stage();
    sim.stage();
    let before = sim.fleet.snapshot(&vessel);
    let before_time = sim.fleet.time();
    let engine = sim.plan_engine(&vessel).unwrap();
    assert!((engine.exhaust_velocity - 340.0 * void_assembly::G0).abs() < 1e-9);
    // Only the upper connected tank's 700 kg is reachable after separation, not the detached stage.
    assert!((before.mass_kg - engine.dry_mass_kg - 700.0).abs() < 1e-9);
    assert_eq!(sim.fleet.control(&vessel).throttle, 0.0);
    let mut plan = sim.new_plan(&vessel, 60.0).unwrap();
    plan.add(void_orbit::ManeuverSpec {
        start_time: sim.fleet.time() + 20.0,
        reference_body: sim.home,
        reference_mode: void_orbit::ReferenceMode::Fixed,
        prograde: 100.0,
        normal: 0.0,
        radial: 0.0,
    });
    let burn = *plan.status(0).as_ref().unwrap();
    let expected = before.mass_kg * (-100.0 / engine.exhaust_velocity).exp();
    assert!((burn.mass_after_kg - expected).abs() < 1e-9);
    plan.extend(&mut sim.fleet.ephemeris, 100_000);
    assert!(plan.complete());
    assert!(
        plan.position_at(&mut sim.fleet.ephemeris, burn.end_time)
            .is_some()
    );
    assert_eq!(sim.fleet.time(), before_time);
    assert_eq!(sim.fleet.snapshot(&vessel).mass_kg, before.mass_kg);
    assert_eq!(sim.fleet.snapshot(&vessel).position, before.position);
}

#[test]
fn main_rocket_stage_masses_engines_and_delta_v() {
    use void_assembly::{Module, compile, flight_rocket};
    let craft = flight_rocket();
    assert_eq!(import_craft(&export_craft(&craft).unwrap()).unwrap(), craft);
    let compiled = compile(&craft).unwrap();
    let stage = |ids: &[&str]| {
        let parts: Vec<_> = compiled
            .parts
            .iter()
            .filter(|p| ids.contains(&p.instance.id.as_str()))
            .collect();
        let dry: f64 = parts.iter().map(|p| p.definition.dry_mass_kg).sum();
        let fuel: f64 = parts.iter().map(|p| p.instance.resource_mass()).sum();
        let (thrust, isp) = parts
            .iter()
            .flat_map(|p| &p.definition.modules)
            .find_map(|m| {
                if let Module::Engine {
                    thrust_newtons,
                    isp_seconds,
                    ..
                } = m
                {
                    Some((*thrust_newtons, *isp_seconds))
                } else {
                    None
                }
            })
            .unwrap();
        (dry, fuel, thrust, isp)
    };
    let (ud, uf, ut, ui) = stage(&["p1", "p2", "p3"]);
    let (bd, bf, bt, bi) = stage(&[
        "p4", "p5", "p6", "leg0", "leg1", "leg2", "leg3", "foot0", "foot1", "foot2", "foot3",
    ]);
    assert_eq!((ud, uf, ut, ui), (300.0, 1470.0, 20_000.0, 340.0));
    assert_eq!((bd, bf, bt, bi), (500.0, 5350.0, 120_000.0, 310.0));
    assert_eq!(compiled.summary(None).mass_kg, 7620.0);
    let dv = void_assembly::G0
        * (bi * ((ud + uf + bd + bf) / (ud + uf + bd)).ln() + ui * ((ud + uf) / ud).ln());
    assert!((dv - 9600.0).abs() < 1.0, "ideal delta-v: {dv}");
    let mut sim = make(&craft, false);
    sim.control(VesselControl {
        throttle: 1.0,
        turn: DVec3::ZERO,
    });
    assert!(sim.stage().is_empty());
    assert_eq!(sim.fleet.thrust("v1").force.length(), 120_000.0);
    assert_eq!(sim.stage().len(), 1);
    assert_eq!(sim.fleet.snapshot("v1").mass_kg, 1770.0);
    assert_eq!(sim.fleet.thrust("v1").force.length(), 20_000.0);
    // Pressure/nozzle ratings must be available for every new engine definition.
    let mut air = make(&craft, true);
    air.control(VesselControl {
        throttle: 1.0,
        turn: DVec3::ZERO,
    });
    air.stage();
    assert!(air.fleet.thrust("v1").force.length() < 120_000.0);
    air.advance(1.0, false).unwrap();
}

#[test]
fn landing_legs_collide_as_cuboid_feet_and_remain_with_the_booster() {
    let craft = void_assembly::flight_rocket();
    let c = void_assembly::compile(&craft).unwrap();
    for i in 0..4 {
        let foot = c.part(&format!("foot{i}"));
        assert!((foot.pose.rotation * DVec3::Y - DVec3::Y).length() < 1e-12);
        assert_eq!(foot.definition.shape, void_assembly::Shape::Box);
        assert_eq!(
            (foot.definition.radius, foot.definition.height),
            (0.21, 0.1)
        );
    }
    let mut sim = make(&craft, false);
    let meshes = sim.fleet.vessel_collider_meshes();
    assert_eq!(meshes.len(), 14);
    assert_eq!(
        meshes
            .iter()
            .filter(|m| m.mesh.vertices.len() == 8 && m.mesh.triangles.len() == 12)
            .count(),
        4,
        "four real cuboid foot colliders"
    );
    sim.stage();
    let children = sim.stage();
    assert_eq!(children.len(), 1);
    let meshes = sim.fleet.vessel_collider_meshes();
    assert_eq!(meshes.iter().filter(|m| m.vessel == "v1").count(), 3);
    assert_eq!(
        meshes.iter().filter(|m| m.vessel == children[0]).count(),
        11
    );
    assert_eq!(
        meshes
            .iter()
            .filter(|m| m.vessel == children[0]
                && m.mesh.vertices.len() == 8
                && m.mesh.triangles.len() == 12)
            .count(),
        4
    );
}
