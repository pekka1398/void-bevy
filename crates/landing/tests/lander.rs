//! The lander: hand-off between free flight and contacts, touchdown, steering and coast prediction.
//! lab/landing's own checks (`landing-check.ts`, "P3") with its thresholds.

use std::sync::Arc;

use glam::DVec3;
use void_landing::{
    ContactWorldOptions, FrameState, Lander, LanderControl, LanderMode, LanderOptions, LanderSpec,
    PlanetFrame, level_for_tile_size, pebble, predict_coast,
};
use void_orbit::{
    AttitudeLaw, Control, Ephemeris, EphemerisOptions, PropagationRun, ThrustControl, Tolerances,
    VesselPropagator, VesselState, build_system,
};
use void_terrain::Terrain;

const FRAME_TOLERANCES: Tolerances = Tolerances {
    position_meters: 1e-6,
    velocity_meters_per_second: 1e-9,
};
const LAUNCH_SITE: DVec3 = DVec3::new(0.8, 0.55, 0.25);

fn spec() -> LanderSpec {
    LanderSpec {
        thrust_newtons: 20e3,
        specific_impulse_seconds: 300.0,
        dry_mass_kg: 1000.0,
        fuel_mass_kg: 1000.0,
        half_extents: DVec3::new(1.5, 1.0, 1.5),
        contact_shape: None,
        friction: 0.8,
        crash_tolerance_meters_per_second: None,
    }
}

fn options() -> LanderOptions {
    LanderOptions {
        contact: ContactWorldOptions {
            step_seconds: 1.0 / 60.0,
            tile_level: level_for_tile_size(100e3, 300.0),
            tile_resolution: 33,
            tile_reach_meters: 300.0,
            tile_keep_meters: 600.0,
            recenter_meters: 1000.0,
            sleeping: true,
        },
        tolerances: FRAME_TOLERANCES,
        band_enter_meters: 200.0,
        band_exit_meters: 400.0,
    }
}

fn up() -> LanderControl {
    LanderControl {
        throttle: 1.0,
        up: 1.0,
        ..Default::default()
    }
}

fn coast() -> LanderControl {
    LanderControl {
        throttle: 0.0,
        up: 1.0,
        ..Default::default()
    }
}

fn plain_pebble() -> (Ephemeris, PlanetFrame, Arc<Terrain>) {
    let base = pebble();
    let system = build_system(&base.system);
    let mut ephemeris = Ephemeris::new(
        &system,
        EphemerisOptions {
            step_seconds: 60.0,
            chunk_steps: 1024,
        },
    );
    ephemeris.extend_to(60.0);
    let frame = PlanetFrame::new(&ephemeris, 0);
    (ephemeris, frame, base.terrain)
}

fn sequence(lander: &Lander) -> String {
    let mut modes = vec!["contact".to_string()];
    modes.extend(lander.mode_changes.iter().skip(1).map(|c| {
        if c.to == LanderMode::Contact {
            "contact".into()
        } else {
            "flight".into()
        }
    }));
    modes.join(" -> ")
}

#[test]
fn hand_off_consistency_and_live_ground_contact() {
    // A 10 km hop from the ground: contact -> flight -> contact, against the orbit integrator flying
    // the same burn and coast with no terrain at all.
    let (mut eph, frame, terrain) = plain_pebble();
    let mut lander = Lander::landed(&mut eph, 0, terrain, spec(), options(), 0.0, LAUNCH_SITE);
    let start = frame.to_inertial(&eph, 0.0, lander.body_fixed_state(&eph));
    let mut run = PropagationRun::new(VesselState {
        time: 0.0,
        position: start.position,
        velocity: start.velocity,
        mass_kg: lander.mass_kg,
    });
    let mut propagator = VesselPropagator::new(&eph, FRAME_TOLERANCES);
    let thrust = ThrustControl {
        thrust_newtons: spec().thrust_newtons,
        exhaust_velocity: lander.exhaust_velocity(),
        minimum_mass_kg: spec().dry_mass_kg,
        attitude: AttitudeLaw::Surface {
            reference_body: 0,
            up: 1.0,
            prograde: 0.0,
        },
    };
    propagator.advance(
        &mut eph,
        &mut run,
        20.0,
        10_000_000,
        None,
        Some(Control::Thrust(thrust)),
    );
    lander.advance(&mut eph, 20.0, &up());
    let (mut worst, mut worst_at, mut highest, mut compared) = (0.0_f64, 0.0, 0.0_f64, 0);
    let mass_error = (lander.mass_kg - run.state().mass_kg).abs();
    while lander.time < 600.0 {
        if lander.time > 60.0 && lander.mode == LanderMode::Contact && lander.clearance(&eph) < 3.0
        {
            break;
        }
        lander.advance(&mut eph, 1.0, &coast());
        if run.impact.is_none() {
            propagator.advance(&mut eph, &mut run, lander.time, 10_000_000, None, None);
        }
        highest = highest.max(lander.clearance(&eph));
        // Compare while the lander is clear of the ground (the reference has none).
        if lander.clearance(&eph) > 20.0 && run.impact.is_none() {
            let s = run.state();
            let reference = frame.to_body_fixed(
                &eph,
                lander.time,
                FrameState {
                    position: s.position,
                    velocity: s.velocity,
                },
            );
            let e = (lander.body_fixed_state(&eph).position - reference.position).length();
            if e > worst {
                worst = e;
                worst_at = lander.time;
            }
            compared += 1;
        }
    }
    let seq = sequence(&lander);
    println!(
        "hand-off consistency: {seq}; top {:.2} km above the terrain; over {compared} samples within {worst:.2e} m of the orbit integration (worst at T+{worst_at:.0} s); fuel differs by {mass_error:.2e} kg",
        highest / 1e3
    );
    assert!(worst < 0.1 && mass_error < 1e-6 && seq == "contact -> flight -> contact");

    // Touchdown remains a live rigid body, so collision can turn and move it.
    let at_touchdown = lander.body_fixed_state(&eph).position;
    lander.advance(&mut eph, 10.0, &coast());
    let moved = (lander.body_fixed_state(&eph).position - at_touchdown).length();
    println!(
        "live ground contact: ten seconds after touchdown the unpinned craft moved {moved:.2e} m"
    );
    assert!(lander.mode == LanderMode::Contact && moved > 0.0);
}

#[test]
fn unlocked_ground_rotation() {
    let (mut eph, _, terrain) = plain_pebble();
    let mut lander = Lander::landed(&mut eph, 0, terrain, spec(), options(), 0.0, LAUNCH_SITE);
    let q0 = lander.orientation();
    lander.advance(&mut eph, 15.0, &coast());
    let alignment = q0.dot(lander.orientation()).abs();
    println!(
        "unlocked ground rotation: the live craft tips on the slope: alignment {alignment:.4}"
    );
    assert!(lander.mode == LanderMode::Contact && alignment < 0.999);
}

#[test]
fn hop_and_land() {
    // A burn to about 10 km, then a surface-retrograde landing burn.
    let (mut eph, _, terrain) = plain_pebble();
    let s = spec();
    let mut lander = Lander::landed(&mut eph, 0, terrain, s.clone(), options(), 0.0, LAUNCH_SITE);
    let site = lander.body_fixed_state(&eph).position;
    let g = 1.6;
    lander.advance(&mut eph, 20.0, &up());
    let (mut peak, mut touchdown_speed, mut last_speed) = (0.0_f64, 0.0_f64, 0.0);
    while lander.time < 1200.0 {
        let state = lander.body_fixed_state(&eph);
        if lander.time > 60.0
            && lander.mode == LanderMode::Contact
            && lander.clearance(&eph) < s.half_extents.y + 0.3
            && state.velocity.length() < 0.5
        {
            break;
        }
        let r = state.position.length();
        let v_up = state.velocity.dot(state.position) / r;
        let speed = state.velocity.length();
        let h = lander.clearance(&eph) - s.half_extents.y;
        peak = peak.max(h);
        let accel = s.thrust_newtons / lander.mass_kg;
        let mut control = coast();
        if v_up < 0.0 && h > 30.0 {
            // Burn surface-retrograde once stopping takes most of the remaining height.
            if speed * speed / (2.0 * (accel - g)) > 0.7 * h {
                control = LanderControl {
                    throttle: 1.0,
                    up: 0.0,
                    prograde: -1.0,
                    ..Default::default()
                };
            }
        } else if v_up < 0.0 || h <= 30.0 {
            // Final descent: hold 1.5 m/s down, cut the engine at touchdown.
            let want = if h > 1.5 { -1.5 } else { 0.0 };
            let throttle = if h > 0.2 {
                ((lander.mass_kg * (g + 2.0 * (want - v_up))) / s.thrust_newtons).clamp(0.0, 1.0)
            } else {
                0.0
            };
            control = LanderControl {
                throttle,
                up: 1.0,
                ..Default::default()
            };
        }
        last_speed = speed;
        if h < 0.5 {
            touchdown_speed = touchdown_speed.max(speed);
        }
        lander.advance(&mut eph, 0.1, &control);
    }
    let drift = (lander.body_fixed_state(&eph).position - site).length();
    println!(
        "hop and land: {}; peak {:.1} km, touchdown at {touchdown_speed:.2} m/s, landed {:.2} km from the launch site after {:.0} s, {:.0} kg fuel left (last speed {last_speed:.2} m/s)",
        sequence(&lander),
        peak / 1e3,
        drift / 1e3,
        lander.time,
        lander.fuel_kg()
    );
    assert!(
        lander.mode == LanderMode::Contact
            && peak > 9000.0
            && touchdown_speed < 3.0
            && lander.fuel_kg() > 0.0
    );
}

#[test]
fn coast_terrain_impact() {
    // The visual coast line uses the orbit integrator and must stop on sampled terrain.
    let (mut eph, frame, terrain) = plain_pebble();
    let d = DVec3::X;
    let r = terrain.radius_meters + terrain.height(d) + 100.0;
    let state = FrameState {
        position: DVec3::new(r, 0.0, 0.0),
        velocity: DVec3::ZERO,
    };
    let path = predict_coast(
        &mut eph,
        &frame,
        &terrain,
        FRAME_TOLERANCES,
        0.0,
        state,
        2000.0,
        100.0,
    );
    let (time, p) = path.impact.expect("the drop reaches the ground");
    let surface = terrain.radius_meters + terrain.height(p.normalize());
    let error = p.length() - surface;
    println!(
        "coast terrain impact: a 100 m drop reaches sampled terrain after {time:.2} s, height error {error:.2e} m"
    );
    assert!(time > 5.0 && time < 30.0 && error.abs() < 0.01);
}

#[test]
fn manual_thrust_steering_and_pulses() {
    let (mut eph, _, terrain) = plain_pebble();
    let p = DVec3::new(
        terrain.radius_meters + terrain.max_height_meters + 5000.0,
        0.0,
        0.0,
    );
    let state = FrameState {
        position: p,
        velocity: DVec3::ZERO,
    };
    let mut upright = Lander::flying(&mut eph, 0, terrain.clone(), spec(), options(), 0.0, state);
    let mut tilted = Lander::flying(&mut eph, 0, terrain.clone(), spec(), options(), 0.0, state);
    upright.advance(
        &mut eph,
        5.0,
        &LanderControl {
            throttle: 1.0,
            up: 1.0,
            direction: Some(DVec3::X),
            ..Default::default()
        },
    );
    tilted.advance(
        &mut eph,
        5.0,
        &LanderControl {
            throttle: 1.0,
            up: 1.0,
            direction: Some(DVec3::new(0.6, 0.8, 0.0)),
            ..Default::default()
        },
    );
    let lateral =
        tilted.body_fixed_state(&eph).position.y - upright.body_fixed_state(&eph).position.y;
    println!(
        "manual thrust direction: a five-second tilted burn displaces the craft {lateral:.2} m sideways"
    );
    assert!(lateral > 40.0 && tilted.mode == LanderMode::Flight);

    // One frame of input must be one angular impulse, not a force that keeps accumulating.
    let hover = FrameState {
        position: DVec3::new(
            terrain.radius_meters + terrain.max_height_meters + 100.0,
            0.0,
            0.0,
        ),
        velocity: DVec3::ZERO,
    };
    let mut pulse = Lander::flying(&mut eph, 0, terrain.clone(), spec(), options(), 0.0, hover);
    let q_start = pulse.orientation();
    pulse.advance(
        &mut eph,
        1.0 / 60.0,
        &LanderControl {
            turn: Some(DVec3::Z),
            ..coast()
        },
    );
    pulse.advance(&mut eph, 2.0, &coast());
    let dot = q_start.dot(pulse.orientation()).abs();
    let angle = 2.0 * dot.min(1.0).acos();
    println!(
        "one-frame steering pulse: one frame of yaw then two seconds released rotates {:.2} deg",
        angle.to_degrees()
    );
    assert!(pulse.mode == LanderMode::Contact && angle > 0.001 && angle < 0.2);

    let mut contact_up = Lander::landed(
        &mut eph,
        0,
        terrain.clone(),
        spec(),
        options(),
        0.0,
        LAUNCH_SITE,
    );
    let mut contact_tilt =
        Lander::landed(&mut eph, 0, terrain, spec(), options(), 0.0, LAUNCH_SITE);
    contact_up.advance(&mut eph, 0.5, &coast());
    contact_tilt.advance(
        &mut eph,
        0.5,
        &LanderControl {
            turn: Some(DVec3::X),
            ..coast()
        },
    );
    let burn = LanderControl {
        throttle: 1.0,
        up: 1.0,
        direction: Some(LAUNCH_SITE),
        ..Default::default()
    };
    contact_up.advance(&mut eph, 2.0, &burn);
    contact_tilt.advance(&mut eph, 2.0, &burn);
    let sideways = (contact_up.body_fixed_state(&eph).position
        - contact_tilt.body_fixed_state(&eph).position)
        .length();
    println!(
        "contact thrust steering: a two-second burn separates the tilted body from upright by {sideways:.2} m"
    );
    assert!(sideways > 0.5 && contact_tilt.mode == LanderMode::Contact);
}
