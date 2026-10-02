//! The game's air: the aero crate's atmosphere driving the landing crate's rocket. These check the
//! wiring and the size of the effect, not the aerodynamics, which void-aero's own checks cover.

use glam::DVec3;
use void_app::aero_field::RocketAir;
use void_landing::{
    DemoRocket, LanderControl, PartJointRocket, demo_rocket, pebble, planet_ephemeris,
};
use void_orbit::Ephemeris;

struct Ascent {
    speed: f64,
    altitude: f64,
}

/// Burn the booster straight up from the launch site, with air or without, and report the state at
/// burnout. Attitude is held along the launch vertical, so the two differ only by the air.
fn ascend(with_air: bool) -> Ascent {
    let planet = void_app::flight::game_planet_by_id("aurelia", None);
    let (mut ephemeris, index) = planet_ephemeris(&planet.planet);
    let mut demo: DemoRocket = demo_rocket(&planet.planet.terrain);
    if let Some(site) = planet.launch_site {
        demo.launch_site = site;
    }
    let mut rocket = PartJointRocket::landed(
        &mut ephemeris,
        index,
        planet.planet.terrain.clone(),
        demo.full.clone(),
        demo.upper.clone(),
        demo.booster.clone(),
        demo.options,
        demo.launch_site,
    );
    if with_air {
        let air = RocketAir::for_planet(&planet.planet, &demo);
        assert!(air.is_some(), "Aurelia is an Earth analogue and has air");
        rocket.set_air_field(air);
    }
    let control = LanderControl {
        throttle: 1.0,
        up: 1.0,
        turn: Some(DVec3::ZERO),
        ..Default::default()
    };
    let mut t = 0.0;
    while t < 150.0 && rocket.part_fuel_kg(void_landing::RocketPart::Booster) > 0.0 {
        rocket.advance(&mut ephemeris, 0.25, &control, None);
        t += 0.25;
    }
    let state = rocket.body_fixed_state(&ephemeris);
    Ascent {
        speed: state.velocity.length(),
        altitude: state.position.length() - planet.planet.terrain.radius_meters,
    }
}

#[test]
fn an_airless_world_has_no_air_field() {
    let planet = pebble();
    let demo = demo_rocket(&planet.terrain);
    assert!(
        RocketAir::for_planet(&planet, &demo).is_none(),
        "Pebble is airless, so it must have no field at all rather than a field of zeroes"
    );
}

#[test]
fn air_costs_the_ascent_speed_and_height() {
    let vacuum = ascend(false);
    let air = ascend(true);
    println!(
        "burnout: vacuum {:.0} m/s at {:.0} m, air {:.0} m/s at {:.0} m",
        vacuum.speed, vacuum.altitude, air.speed, air.altitude
    );
    // The demo rocket is sized for a vacuum ascent, so Earth's air takes a large part of it. The
    // bounds are wide: they check that drag is applied and is of the right order, not its value.
    assert!(
        air.speed < 0.75 * vacuum.speed && air.speed > 0.25 * vacuum.speed,
        "air burnout speed {:.0} m/s against {:.0} m/s in vacuum",
        air.speed,
        vacuum.speed
    );
    assert!(
        air.altitude < 0.75 * vacuum.altitude,
        "air burnout altitude {:.0} m against {:.0} m in vacuum",
        air.altitude,
        vacuum.altitude
    );
}

#[test]
fn there_is_no_drag_above_the_atmosphere() {
    let planet = void_app::flight::game_planet_by_id("aurelia", None);
    let (ephemeris, _) = planet_ephemeris(&planet.planet);
    let _: &Ephemeris = &ephemeris;
    let demo = demo_rocket(&planet.planet.terrain);
    let air = RocketAir::for_planet(&planet.planet, &demo).expect("Aurelia has air");
    let radius = planet.planet.terrain.radius_meters;
    let fast = DVec3::new(0.0, 7800.0, 0.0);
    let high = void_landing::FrameState {
        position: DVec3::new(radius + 200_000.0, 0.0, 0.0),
        velocity: fast,
    };
    assert_eq!(
        air.force(
            &[void_landing::RocketPart::Upper],
            high,
            glam::DQuat::IDENTITY,
            1400.0
        ),
        DVec3::ZERO,
        "200 km is above the atmosphere's 120 km ceiling"
    );
    let low = void_landing::FrameState {
        position: DVec3::new(radius + 5_000.0, 0.0, 0.0),
        velocity: fast,
    };
    assert!(
        air.force(
            &[void_landing::RocketPart::Upper],
            low,
            glam::DQuat::IDENTITY,
            1400.0
        )
        .length()
            > 1e6,
        "7.8 km/s at 5 km is far past any real flight, so the force must be enormous"
    );
}

#[test]
fn ambient_pressure_drives_the_nozzle() {
    let planet = void_app::flight::game_planet_by_id("aurelia", None);
    let demo = demo_rocket(&planet.planet.terrain);
    let air = RocketAir::for_planet(&planet.planet, &demo).expect("Aurelia has air");
    let radius = planet.planet.terrain.radius_meters;
    let at = |altitude: f64| air.pressure_pa(DVec3::new(radius + altitude, 0.0, 0.0));
    assert!(
        (at(0.0) - 101_325.0).abs() < 1.0,
        "sea level is one standard atmosphere, not {:.0} Pa",
        at(0.0)
    );
    // ISA tabulates against geopotential altitude, and 11 km geometric is 10 981 m geopotential,
    // so the pressure there is a little above the table's 22 632 Pa at the tropopause.
    assert!(
        (at(11_000.0) - 22_700.0).abs() < 20.0,
        "{:.0} Pa at 11 km",
        at(11_000.0)
    );
    assert_eq!(at(200_000.0), 0.0, "there is no air left at 200 km");

    // The booster keeps 90% of its vacuum thrust at sea level, as its exit area says it should.
    let spec = &demo.booster;
    let sea_level = spec.thrust_newtons - spec.nozzle_exit_area_m2 * at(0.0);
    let ratio = sea_level / spec.thrust_newtons;
    println!(
        "booster: {:.1} kN at sea level against {:.1} kN in vacuum ({:.1}%), Isp {:.0} s",
        sea_level / 1e3,
        spec.thrust_newtons / 1e3,
        ratio * 100.0,
        spec.specific_impulse_seconds * ratio
    );
    assert!((0.85..0.95).contains(&ratio));
}
