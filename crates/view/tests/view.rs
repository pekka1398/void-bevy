//! The view crate against lab/view: golden data from `golden/view.ts`, then the lab's own checks
//! (`view-check.ts`) with its thresholds.

use std::f64::consts::PI;

use glam::DVec3;
use serde_json::Value;
use void_orbit::{
    EngineSpec, Simulation, SimulationOptions, StartPlane, SystemSpec, Tolerances, VesselStartSpec,
    body_orientation, osculating_orbit,
};
use void_view::{
    FLIGHT_MAX_DISTANCE, FocusGeometry, FocusKind, MAP_MIN_DISTANCE, MIN_ANGLE_FROM_UP,
    OrbitCamera, PathCache, PathFrame, PathFrameKind, ViewMode, camera_spin, ellipse_points,
    ellipse_points_in_time, frame_to_ecliptic, orbit_in_surface_frame, rotate, view_state,
};

/// The orbit lab's surface-frame orbit, angle = epoch + (2π / period) · t. Past the first turn
/// this rounds where the native `Spin::angle` (exact remainder) does not; the golden data is
/// checked against this reproduction, and the native function against it within the rounding.
fn lab_orbit_in_surface_frame(
    frame_body: &void_orbit::CelestialBody,
    now: f64,
    parent_offset: DVec3,
    relative_position: DVec3,
    relative_velocity: DVec3,
    gm: f64,
    count: usize,
) -> Vec<DVec3> {
    let (points, period_seconds) =
        ellipse_points_in_time(relative_position, relative_velocity, gm, count);
    let [node, quadrature, pole] = frame_body.rotation.equatorial_basis();
    let spin = 2.0 * std::f64::consts::PI / frame_body.rotation.period_seconds;
    (0..=count)
        .map(|i| {
            let v = parent_offset + points[i % count];
            let along = v.x * node.x + v.y * node.y + v.z * node.z;
            let across = v.x * quadrature.x + v.y * quadrature.y + v.z * quadrature.z;
            let angle = frame_body.rotation.angle_at_epoch_radians
                + spin * (now + period_seconds * i as f64 / count as f64);
            let (c, s) = (angle.cos(), angle.sin());
            DVec3::new(
                c * along + s * across,
                -s * along + c * across,
                v.x * pole.x + v.y * pole.y + v.z * pole.z,
            )
        })
        .collect()
}
fn golden() -> Value {
    let path = format!("{}/tests/golden/view.json", env!("CARGO_MANIFEST_DIR"));
    serde_json::from_str(&std::fs::read_to_string(&path).expect(&path)).expect(&path)
}

fn f(v: &Value) -> f64 {
    v.as_f64().unwrap_or_else(|| panic!("not a number: {v}"))
}

fn v3(v: &Value) -> DVec3 {
    DVec3::new(f(&v[0]), f(&v[1]), f(&v[2]))
}

fn triples(v: &Value) -> Vec<DVec3> {
    let a: Vec<f64> = v.as_array().unwrap().iter().map(f).collect();
    a.chunks(3).map(|c| DVec3::new(c[0], c[1], c[2])).collect()
}

/// Largest difference relative to `scale`.
fn relative(a: DVec3, b: DVec3, scale: f64) -> f64 {
    (a - b).abs().max_element() / scale
}

fn view_lab_simulation() -> Simulation {
    let path = format!("{}/../orbit/systems/sol.json", env!("CARGO_MANIFEST_DIR"));
    Simulation::new(SimulationOptions {
        system: SystemSpec::from_json(&std::fs::read_to_string(&path).expect(&path)),
        steps_per_orbit: 256.0,
        tolerances: Tolerances {
            position_meters: 1e-4,
            velocity_meters_per_second: 1e-7,
        },
        vessel_start: VesselStartSpec {
            home_body_id: "aurelia".into(),
            altitude_meters: 100e3,
            plane: StartPlane::Equatorial {
                inclination_radians: 0.0,
            },
        },
        engine: EngineSpec {
            thrust_newtons: 250e3,
            specific_impulse_seconds: 350.0,
            dry_mass_kg: 10e3,
            fuel_mass_kg: 30e3,
        },
        retention_seconds: 86_400.0,
        prediction_horizon_seconds: 3.0 * 3600.0,
        plan_coast_seconds: 86_400.0,
    })
}

// --- Against the lab ----------------------------------------------------------------------------

#[test]
fn view_states_and_camera_match_the_lab() {
    let g = golden();
    let mut worst = 0.0_f64;
    for c in g["views"].as_array().unwrap() {
        let fc = &c["focus"];
        let focus = FocusGeometry {
            kind: if fc["kind"] == "body" {
                FocusKind::Body
            } else {
                FocusKind::Vessel
            },
            radial: (!fc["radial"].is_null()).then(|| v3(&fc["radial"])),
            north: v3(&fc["north"]),
            reference_radius: f(&fc["referenceRadius"]),
            altitude: f(&fc["altitude"]),
            focus_radius: f(&fc["focusRadius"]),
        };
        let mode = if c["mode"] == "split" {
            ViewMode::Split
        } else {
            ViewMode::Single
        };
        let s = view_state(
            mode,
            c["mapOn"].as_bool().unwrap(),
            &focus,
            f(&c["distance"]),
        );
        let l = &c["state"];
        worst = worst
            .max((s.map_weight - f(&l["mapWeight"])).abs())
            .max((s.up_weight - f(&l["upWeight"])).abs())
            .max((s.corotation - f(&l["corotation"])).abs())
            .max(relative(s.up, v3(&l["up"]), 1.0));
        assert_eq!(s.min_distance, f(&l["minDistance"]));
        assert_eq!(s.max_distance, f(&l["maxDistance"]));
        let references = (
            c["focusReference"].as_u64().unwrap() as usize,
            c["pathReference"].as_u64().unwrap() as usize,
        );
        for (k, kind) in [PathFrameKind::Inertial, PathFrameKind::Surface]
            .into_iter()
            .enumerate()
        {
            let (body, weight) = camera_spin(&s, kind, references.0, references.1);
            assert_eq!(body as u64, c["spin"][k]["body"].as_u64().unwrap());
            worst = worst.max((weight - f(&c["spin"][k]["weight"])).abs());
        }
    }
    let mut camera_worst = 0.0_f64;
    for c in g["cameras"].as_array().unwrap() {
        let up = v3(&c["up"]);
        let mut camera = OrbitCamera::new(v3(&c["start"]["direction"]), f(&c["start"]["distance"]));
        for (k, step) in c["steps"].as_array().unwrap().iter().enumerate() {
            let (a, b) = (f(&step["a"]), f(&step["b"]));
            match k % 4 {
                0 => camera.drag(a * 400.0, b * 900.0, up),
                1 => camera.zoom(f64::exp(a * 2.0), 8.0, 2e13),
                2 => camera.corotate(up, a * 0.3),
                _ => camera.clamp_to_up(if k % 8 == 3 { -up } else { up }),
            }
            camera_worst = camera_worst
                .max(relative(camera.direction, v3(&step["direction"]), 1.0))
                .max((camera.distance - f(&step["distance"])).abs() / camera.distance);
        }
    }
    println!(
        "{} view states within {worst:.1e}; {} cameras through 30 drags, zooms, turns and clamps within {camera_worst:.1e}",
        g["views"].as_array().unwrap().len(),
        g["cameras"].as_array().unwrap().len()
    );
    assert!(worst < 1e-12 && camera_worst < 1e-12);
}

#[test]
fn ellipses_match_the_lab() {
    let g = golden();
    let gm = f(&g["gm"]);
    let (mut angle, mut time) = (0.0_f64, 0.0_f64);
    for c in g["ellipses"].as_array().unwrap() {
        let (r, v, n) = (v3(&c["r"]), v3(&c["v"]), c["n"].as_u64().unwrap() as usize);
        let scale = r.length();
        for (a, b) in ellipse_points(r, v, gm, n)
            .iter()
            .zip(triples(&c["byAngle"]))
        {
            angle = angle.max(relative(*a, b, scale));
        }
        let (points, period) = ellipse_points_in_time(r, v, gm, n);
        assert!((period - f(&c["period"])).abs() < 1e-12 * period);
        for (a, b) in points.iter().zip(triples(&c["byTime"])) {
            time = time.max(relative(*a, b, scale));
        }
    }
    println!("ellipses by angle within {angle:.1e}, by time within {time:.1e} (relative)");
    assert!(angle < 1e-13 && time < 1e-12);
}

#[test]
fn surface_frame_orbits_and_path_frames_match_the_lab() {
    let g = golden();
    let mut sim = view_lab_simulation();
    let now = f(&g["now"]);
    let home = g["home"].as_u64().unwrap() as usize;
    sim.ephemeris.extend_to(now + 86_400.0);
    let eph = &sim.ephemeris;
    let bodies = eph.bodies().to_vec();
    let n = bodies.len();
    let (mut p, mut v) = (vec![DVec3::ZERO; n], vec![DVec3::ZERO; n]);
    eph.states_at(now, &mut p, Some(&mut v));
    let (mut worst, mut native) = (0.0_f64, 0.0_f64);
    for o in g["surfaceOrbits"].as_array().unwrap() {
        let b = o["body"].as_u64().unwrap() as usize;
        let parent = bodies[b].parent_index.unwrap();
        let points = orbit_in_surface_frame(
            &bodies[home],
            now,
            p[parent] - p[home],
            p[b] - p[parent],
            v[b] - v[parent],
            bodies[parent].gm + bodies[b].gm,
        );
        assert_eq!(
            points.len() as u64,
            o["count"].as_u64().unwrap(),
            "{}",
            bodies[b].id
        );
        let lab_points = lab_orbit_in_surface_frame(
            &bodies[home],
            now,
            p[parent] - p[home],
            p[b] - p[parent],
            v[b] - v[parent],
            bodies[parent].gm + bodies[b].gm,
            points.len() - 1,
        );
        let scale = (p[b] - p[home]).length() + (p[b] - p[parent]).length();
        for (k, lab) in o["kept"]
            .as_array()
            .unwrap()
            .iter()
            .zip(triples(&o["points"]))
        {
            let k = k.as_u64().unwrap() as usize;
            worst = worst.max(relative(lab_points[k], lab, scale));
        }
        for (a, b) in points.iter().zip(&lab_points) {
            native = native.max(relative(*a, *b, scale));
        }
    }
    println!(
        "surface-frame orbits of {} bodies: lab formula within {worst:.1e}, native exact angle within {native:.1e} of it (relative)",
        bodies.len() - 1
    );
    assert!(worst < 1e-11);
    // A year of a fast-turning frame is thousands of turns; the lab's 2π t / P loses about
    // turns · 2π · 2⁻⁵³ of angle, which the exact remainder keeps.
    assert!(native < 1e-9, "{native:e}");

    for c in g["paths"].as_array().unwrap() {
        let kind = if c["kind"] == "surface" {
            PathFrameKind::Surface
        } else {
            PathFrameKind::Inertial
        };
        let frame = PathFrame::new(eph, kind, home);
        let mut cache = PathCache::new(97.5);
        let mut samples = Vec::new();
        let point = |t: f64| DVec3::new(7e6 * (t / 900.0).cos(), 7e6 * (t / 900.0).sin(), 1e5);
        let mut sample = |t: f64| {
            samples.push(t);
            frame.at(eph, t, point(t))
        };
        cache.update(now - 3000.0, now + 20_000.0, &mut sample);
        cache.update(now, now + 40_000.0, &mut sample);
        let lab_samples: Vec<f64> = c["samples"].as_array().unwrap().iter().map(f).collect();
        assert_eq!(samples, lab_samples, "{kind:?}: sample times");
        assert_eq!(cache.count() as u64, c["count"].as_u64().unwrap());
        let mut out = Vec::new();
        cache.write_relative(
            &mut out,
            DVec3::new(1.0, 2.0, 3.0),
            Some(DVec3::new(10.0, 20.0, 30.0)),
            Some(DVec3::new(-1.0, -2.0, -3.0)),
        );
        assert_eq!(out.len() as u64, c["written"].as_u64().unwrap());
        // The lab writes three.js axes (x, z, −y) in f32.
        let mut vertex_worst = 0.0_f64;
        for (a, b) in out.iter().zip(triples(&c["vertices"])) {
            let three = DVec3::new(a.x as f32 as f64, a.z as f32 as f64, (-a.y) as f32 as f64);
            vertex_worst = vertex_worst.max(relative(three, b, 7e6));
        }
        let axes = frame.axes_at(now);
        for (k, axis) in axes.iter().enumerate() {
            assert!(relative(*axis, v3(&c["axes"][k]), 1.0) < 1e-12);
        }
        let at = frame.at(eph, now, DVec3::new(1e11, -2e10, 3e9));
        let at_error = relative(at, v3(&c["at"]), 1e11);
        println!(
            "{kind:?} path frame: {} samples on the lab's grid; vertices within {vertex_worst:.1e}, a far point within {at_error:.1e} (relative)",
            samples.len()
        );
        assert!(vertex_worst < 1e-6 && at_error < 1e-12);
    }
}

// --- The lab's checks -----------------------------------------------------------------------------

const DEG: f64 = PI / 180.0;
const R: f64 = 6.371e6;

fn angle(a: DVec3, b: DVec3) -> f64 {
    (a.dot(b) / (a.length() * b.length()))
        .clamp(-1.0, 1.0)
        .acos()
}

fn north() -> DVec3 {
    DVec3::new(0.0, 0.4, 0.92).normalize()
}

/// A vessel 60 degrees from the pole: flight "up" and map "up" differ a lot.
fn radial() -> DVec3 {
    rotate(north(), north().cross(DVec3::X).normalize(), 60.0 * DEG).normalize()
}

fn vessel_at(altitude: f64) -> FocusGeometry {
    FocusGeometry {
        kind: FocusKind::Vessel,
        radial: Some(radial()),
        north: north(),
        reference_radius: R,
        altitude,
        focus_radius: 0.0,
    }
}

#[test]
fn single_view_zoom_out_is_smooth() {
    let mut camera = OrbitCamera::new(DVec3::new(1.0, -0.3, 0.2).normalize(), 5.0);
    let mut previous = view_state(ViewMode::Single, false, &vessel_at(0.0), camera.distance);
    assert!(previous.map_weight == 0.0 && previous.corotation == 1.0);
    assert!(angle(previous.up, radial()) < 1e-12);
    let (mut worst_up, mut worst_direction) = (0.0_f64, 0.0_f64);
    let (mut map_while_upright, mut up_while_map_in) = (false, false);
    camera.clamp_to_up(previous.up);
    while camera.distance < 1e11 {
        let before = camera.direction;
        camera.zoom(1.01, previous.min_distance, previous.max_distance);
        let next = view_state(ViewMode::Single, false, &vessel_at(0.0), camera.distance);
        camera.clamp_to_up(next.up);
        worst_up = worst_up.max(angle(previous.up, next.up));
        worst_direction = worst_direction.max(angle(before, camera.direction));
        assert!(next.map_weight >= previous.map_weight);
        assert!(next.up_weight >= previous.up_weight);
        // The map comes in first; the up does not start turning before it is fully in.
        assert!(next.up_weight == 0.0 || next.map_weight == 1.0, "{next:?}");
        map_while_upright |= next.map_weight > 0.0 && next.map_weight < 1.0;
        up_while_map_in |= next.up_weight > 0.0 && next.up_weight < 1.0;
        assert!(next.corotation <= previous.corotation);
        previous = next;
    }
    assert!(previous.map_weight == 1.0 && previous.up_weight == 1.0 && previous.corotation == 0.0);
    assert!(map_while_upright && up_while_map_in);
    assert!(angle(previous.up, north()) < 1e-12);
    println!(
        "single view zoom 5 m -> 1e11 m: largest step up {:.3} deg, direction {:.3} deg",
        worst_up / DEG,
        worst_direction / DEG
    );
    assert!(worst_up < 0.5 * DEG && worst_direction < 0.5 * DEG);
}

#[test]
fn ground_lock_drag_and_split_ranges() {
    // An orbiting vessel's close-up is inertial; a landed one's follows the ground.
    let orbiting = view_state(ViewMode::Single, false, &vessel_at(100e3), 30.0);
    assert!(orbiting.map_weight == 0.0 && orbiting.corotation == 0.0);
    assert_eq!(
        view_state(ViewMode::Single, false, &vessel_at(10e3), 30.0).corotation,
        1.0
    );

    // Co-rotating fully for one spin period returns the camera to where it started.
    let mut camera = OrbitCamera::new(DVec3::new(0.2, -1.0, 0.5).normalize(), 30.0);
    let start = camera.direction;
    for _ in 0..1000 {
        camera.corotate(north(), 2.0 * PI / 1000.0);
    }
    assert!(angle(start, camera.direction) < 1e-9);

    // Dragging never passes over the pole and azimuth drags keep the elevation.
    let mut camera = OrbitCamera::new(DVec3::new(1.0, 0.0, 0.1).normalize(), 30.0);
    let up = DVec3::Z;
    let elevation = angle(camera.direction, up);
    camera.drag(300.0, 0.0, up);
    assert!((angle(camera.direction, up) - elevation).abs() < 1e-12);
    camera.drag(0.0, 5000.0, up);
    assert!((angle(camera.direction, up) - MIN_ANGLE_FROM_UP).abs() < 1e-9);
    camera.drag(0.0, -10000.0, up);
    assert!((angle(camera.direction, up) - (PI - MIN_ANGLE_FROM_UP)).abs() < 1e-9);
    camera.drag(0.0, 5000.0, up);
    camera.clamp_to_up(-DVec3::Z);
    assert!(angle(camera.direction, -DVec3::Z) >= MIN_ANGLE_FROM_UP - 1e-12);

    // Split view: flight and map each keep their own zoom range.
    let flight = view_state(ViewMode::Split, false, &vessel_at(100e3), 1e9);
    assert!(flight.max_distance == FLIGHT_MAX_DISTANCE && flight.map_weight == 0.0);
    let map = view_state(ViewMode::Split, true, &vessel_at(100e3), 10.0);
    assert!(map.min_distance == MAP_MIN_DISTANCE && map.map_weight == 1.0 && map.corotation == 0.0);
    let body = FocusGeometry {
        kind: FocusKind::Body,
        radial: None,
        north: north(),
        reference_radius: R,
        altitude: 0.0,
        focus_radius: R,
    };
    assert!(
        std::panic::catch_unwind(|| view_state(ViewMode::Split, false, &body, 4.0 * R)).is_err()
    );
    let body_map = view_state(ViewMode::Split, true, &body, 4.0 * R);
    assert_eq!(body_map.min_distance, (1.02 * R).max(R + MAP_MIN_DISTANCE));

    // The camera follows the path frame on the map, and only the ground lock otherwise.
    let far = view_state(ViewMode::Single, false, &vessel_at(100e3), 1e9);
    let close = view_state(ViewMode::Single, false, &vessel_at(100e3), 30.0);
    let landed = view_state(ViewMode::Single, false, &vessel_at(0.0), 30.0);
    let same =
        |(b, w): (usize, f64), body: usize, weight: f64| b == body && (w - weight).abs() < 1e-12;
    assert!(same(
        camera_spin(&far, PathFrameKind::Inertial, 3, 3),
        3,
        0.0
    ));
    assert!(same(
        camera_spin(&far, PathFrameKind::Surface, 3, 3),
        3,
        1.0
    ));
    assert!(same(
        camera_spin(&far, PathFrameKind::Surface, 5, 3),
        3,
        1.0
    ));
    assert!(same(
        camera_spin(&close, PathFrameKind::Surface, 3, 3),
        3,
        0.0
    ));
    assert!(same(
        camera_spin(&landed, PathFrameKind::Surface, 3, 3),
        3,
        1.0
    ));
    assert!(same(
        camera_spin(&landed, PathFrameKind::Inertial, 3, 3),
        3,
        1.0
    ));
}

#[test]
fn osculating_ellipses() {
    let gm = 3.986e14;
    let (r, v) = (DVec3::new(7e6, 1e5, 2e5), DVec3::new(-200.0, 7.9e3, 1.5e3));
    let osc = osculating_orbit(r, v, gm);
    let n = 256;
    let mut nearest = f64::INFINITY;
    for p in ellipse_points(r, v, gm, n) {
        let d = p.length();
        assert!(
            d >= osc.periapsis_radius_meters * (1.0 - 1e-12)
                && d <= osc.apoapsis_radius_meters * (1.0 + 1e-12)
        );
        assert!(p.dot(r.cross(v)).abs() < 1e-3 * d * r.cross(v).length());
        nearest = nearest.min((p - r).length());
    }
    let chord = 2.0 * PI * osc.apoapsis_radius_meters / n as f64;
    assert!(nearest < chord);
    let circle = ellipse_points(
        DVec3::new(7e6, 0.0, 0.0),
        DVec3::new(0.0, (gm / 7e6).sqrt(), 0.0),
        gm,
        64,
    );
    assert!((circle[0].x - 7e6).abs() < 1e-3 && circle[0].y.abs() < 1e-3);
    assert!(
        std::panic::catch_unwind(|| ellipse_points(r, DVec3::new(0.0, 12e3, 0.0), gm, 64)).is_err()
    );

    // Time-sampled ellipse: point k is where an independent two-body RK4 is k/n of a period on.
    let (r0, v0) = (DVec3::new(7e6, 1e5, 2e5), DVec3::new(-900.0, 8.6e3, 1.5e3));
    let n = 64;
    let (points, period) = ellipse_points_in_time(r0, v0, gm, n);
    assert!((period - osculating_orbit(r0, v0, gm).period_seconds).abs() < 1e-6 * period);
    let accel = |p: DVec3| -gm / p.length().powi(3) * p;
    let (mut p, mut v, mut worst) = (r0, v0, 0.0_f64);
    let steps = 400;
    let h = period / n as f64 / steps as f64;
    for point in &points {
        worst = worst.max((p - *point).length());
        for _ in 0..steps {
            let a1 = accel(p);
            let (p2, v2) = (p + 0.5 * h * v, v + 0.5 * h * a1);
            let a2 = accel(p2);
            let (p3, v3) = (p + 0.5 * h * v2, v + 0.5 * h * a2);
            let a3 = accel(p3);
            let (p4, v4) = (p + h * v3, v + h * a3);
            let a4 = accel(p4);
            p += h / 6.0 * (v + 2.0 * v2 + 2.0 * v3 + v4);
            v += h / 6.0 * (a1 + 2.0 * a2 + 2.0 * a3 + a4);
        }
    }
    println!("time-sampled ellipse: {n} points within {worst:.1e} m of RK4");
    assert!(worst < 1.0);
}

#[test]
fn path_frames_and_surface_orbits() {
    // Path frames through the map's own steps (PathCache, then the turn by the axes now). Samples
    // span a day of Aurelia's spin; "now" is the end.
    let mut sim = view_lab_simulation();
    let home_index = sim.body_index("aurelia");
    let home = sim.system.bodies[home_index].clone();
    let day = home.rotation.period_seconds;
    let now = day;
    sim.ephemeris.extend_to(now);
    let eph = &sim.ephemeris;
    let centre = |t: f64| eph.body_position(home_index, t);
    let ground = DVec3::new(0.6, -0.5, 0.62) * home.radius_meters;
    let on_ground =
        |t: f64| centre(t) + frame_to_ecliptic(&body_orientation(&home.rotation, t), ground);
    let r = home.radius_meters + 300e3;
    let n = (home.gm / r.powi(3)).sqrt();
    let orbiting = |t: f64| centre(t) + DVec3::new(r * (n * t).cos(), r * (n * t).sin(), 0.0);
    let draw = |kind: PathFrameKind, path: &dyn Fn(f64) -> DVec3, origin: DVec3| {
        let frame = PathFrame::new(eph, kind, home_index);
        let mut cache = PathCache::new(day / 500.0);
        cache.update(0.0, now, |t| frame.at(eph, t, path(t)));
        let mut out = Vec::new();
        cache.write_relative(
            &mut out,
            frame.at(eph, now, origin),
            Some(frame.at(eph, now, path(now))),
            None,
        );
        let axes = frame.axes_at(now);
        let vertices: Vec<DVec3> = out.iter().map(|&p| frame_to_ecliptic(&axes, p)).collect();
        let mut times = vec![now];
        times.extend(cache.samples().map(|(t, _)| t));
        (vertices, times)
    };
    let (fixed, _) = draw(PathFrameKind::Surface, &on_ground, on_ground(now));
    let spread = fixed.iter().map(|p| p.length()).fold(0.0, f64::max);
    assert!(
        fixed.len() > 400 && spread < 1e-3,
        "{} vertices, {spread} m",
        fixed.len()
    );
    let (swept, _) = draw(PathFrameKind::Inertial, &on_ground, on_ground(now));
    let sweep = swept.iter().map(|p| p.length()).fold(0.0, f64::max);
    assert!(sweep > home.radius_meters);

    let focus = centre(now);
    let (inertial, times) = draw(PathFrameKind::Inertial, &orbiting, focus);
    let (surface, _) = draw(PathFrameKind::Surface, &orbiting, focus);
    let now_axes = body_orientation(&home.rotation, now);
    let (mut worst_inertial, mut worst_surface) = (0.0_f64, 0.0_f64);
    for i in 1..inertial.len() {
        let t = times[i];
        let relative = orbiting(t) - centre(t);
        worst_inertial = worst_inertial.max((inertial[i] - relative).length());
        let a = body_orientation(&home.rotation, t);
        let fixed = DVec3::new(relative.dot(a[0]), relative.dot(a[1]), relative.dot(a[2]));
        worst_surface =
            worst_surface.max((surface[i] - frame_to_ecliptic(&now_axes, fixed)).length());
    }
    println!(
        "path frames: ground point {spread:.1e} m in the surface frame ({:.0} km sweep inertial); orbit worst {worst_inertial:.1e} m inertial, {worst_surface:.1e} m surface",
        sweep / 1e3
    );
    assert!(worst_inertial < 1.0 && worst_surface < 1.0);

    // Orbits in the surface frame: a stationary orbit is a point; a moon's petals turned back
    // are its inertial ellipse.
    let [eq_x, eq_y, _] = home.rotation.equatorial_basis();
    let circular = |radius: f64| (eq_x * radius, eq_y * (home.gm / radius).sqrt());
    let at = 12_345.0;
    let (sr, sv) = circular((home.gm * day * day / (4.0 * PI * PI)).cbrt());
    let still = orbit_in_surface_frame(&home, at, DVec3::ZERO, sr, sv, home.gm);
    let drift = still
        .iter()
        .map(|p| (*p - still[0]).length())
        .fold(0.0, f64::max);
    assert!(drift < 1.0, "stationary drift {drift} m");
    let (mr, mv) = circular(60.0 * home.radius_meters);
    let petals = orbit_in_surface_frame(&home, at, DVec3::ZERO, mr, mv, home.gm);
    let count = petals.len() - 1;
    let (inertial, period) = ellipse_points_in_time(mr, mv, home.gm, count);
    let mut worst = 0.0_f64;
    for (i, p) in petals.iter().enumerate() {
        let back = frame_to_ecliptic(
            &body_orientation(&home.rotation, at + period * i as f64 / count as f64),
            *p,
        );
        worst = worst.max((back - inertial[i % count]).length());
    }
    println!(
        "surface-frame orbits: stationary drift {drift:.1e} m over {} points; moon {count} points over {:.1} turns, back to inertial within {worst:.1e} m",
        still.len(),
        period / day
    );
    assert!(worst < 1.0);
    assert!(count >= (64.0 * period / day).ceil() as usize || count == 16_384);
}

#[test]
fn camera_as_a_second_lod_observer() {
    use void_landing::{ContactWorldOptions, landing_lod_options, level_for_tile_size};
    use void_lod::{LodView, PlanetLod, TileMeshOptions, build_tile_mesh, tile_containing};
    use void_terrain::{HillsOptions, Terrain, TerrainConfig};

    // From far out, the planet's face toward the camera is drawn, though it is below the vessel's
    // horizon; without the camera it is culled.
    let radius = 100e3;
    let terrain = Terrain::from_config(&TerrainConfig::Hills(HillsOptions {
        name: "check hills".into(),
        radius_meters: radius,
        max_height_meters: 3000.0,
        wavelength_meters: 8000.0,
        octaves: 6,
    }));
    let contact = ContactWorldOptions {
        step_seconds: 1.0 / 60.0,
        tile_level: level_for_tile_size(radius, 300.0),
        tile_resolution: 17,
        tile_reach_meters: 300.0,
        tile_keep_meters: 600.0,
        recenter_meters: 1000.0,
        sleeping: true,
    };
    let options = landing_lod_options(&terrain, &contact);
    let vessel = DVec3::new(radius + 100e3, 0.0, 0.0);
    let camera_far = DVec3::new(-10.0 * radius, 0.3 * radius, 0.0);
    let settle = |lod: &mut PlanetLod, view: &LodView| {
        for _ in 0..400 {
            let selection = lod.select(view);
            if selection.requests.is_empty() {
                return selection;
            }
            for request in selection.requests.iter().take(64) {
                lod.accept_tile(std::sync::Arc::new(build_tile_mesh(
                    request.key,
                    &terrain,
                    TileMeshOptions {
                        radius_meters: radius,
                        resolution: contact.tile_resolution,
                    },
                )));
            }
        }
        panic!("LOD did not settle in 400 rounds");
    };
    let far_side = camera_far.normalize();
    let drawn_over = |selection: &void_lod::LodSelection, direction: DVec3| -> i64 {
        for level in (0..=options.max_level).rev() {
            let key = tile_containing(direction, level);
            if selection.render.contains(&key.code()) {
                return level as i64;
            }
        }
        -1
    };
    let view = |observers: Vec<DVec3>| LodView {
        observer_positions: observers,
        camera: None,
        distance_scale: 1.0,
        horizon_culling: true,
    };
    let vessel_only = settle(&mut PlanetLod::new(options.clone()), &view(vec![vessel]));
    let with_camera = settle(
        &mut PlanetLod::new(options.clone()),
        &view(vec![vessel, camera_far]),
    );
    let under = vessel.normalize();
    println!(
        "camera observer: tiles {} (vessel only) -> {}; under vessel L{}, far side L{}",
        vessel_only.render.len(),
        with_camera.render.len(),
        drawn_over(&with_camera, under),
        drawn_over(&with_camera, far_side)
    );
    assert_eq!(drawn_over(&vessel_only, far_side), -1);
    assert!(drawn_over(&with_camera, far_side) >= 0);
    assert_eq!(
        drawn_over(&with_camera, under),
        drawn_over(&vessel_only, under)
    );
}
