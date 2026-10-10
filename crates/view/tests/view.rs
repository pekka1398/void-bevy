//! The view crate: zooming, ground lock, osculating ellipses, path frames and the camera as a LOD
//! observer.

use std::f64::consts::PI;

use glam::{DQuat, DVec3};
use void_orbit::{
    Ephemeris, EphemerisOptions, SystemSpec, body_orientation, build_system, osculating_orbit,
    suggested_step_seconds,
};
use void_view::{
    FLIGHT_MAX_DISTANCE, FocusGeometry, FocusKind, MAP_MIN_DISTANCE, MIN_ANGLE_FROM_UP,
    OrbitCamera, PathCache, PathFrame, PathFrameKind, ViewMode, camera_spin, ellipse_points,
    ellipse_points_in_time, frame_to_ecliptic, orbit_in_surface_frame, view_state,
};

fn sol() -> (void_orbit::BuiltSystem, Ephemeris) {
    let path = format!("{}/../orbit/systems/sol.json", env!("CARGO_MANIFEST_DIR"));
    let system = build_system(&SystemSpec::from_json(
        &std::fs::read_to_string(&path).expect(&path),
    ));
    let ephemeris = Ephemeris::new(
        &system,
        EphemerisOptions {
            step_seconds: suggested_step_seconds(&system.bodies, 256.0),
            chunk_steps: 2048,
        },
    );
    (system, ephemeris)
}

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
    (DQuat::from_axis_angle(north().cross(DVec3::X).normalize(), 60.0 * DEG) * north()).normalize()
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
    let (system, mut eph) = sol();
    let home_index = system
        .bodies
        .iter()
        .position(|b| b.id == "aurelia")
        .unwrap();
    let home = system.bodies[home_index].clone();
    let day = home.rotation.period_seconds;
    let now = day;
    eph.extend_to(now);
    let eph = &eph;
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
