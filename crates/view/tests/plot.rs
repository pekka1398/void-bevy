use glam::DVec3;
use void_orbit::{FrameSpec, Trajectory};
use void_view::plot::{BodyPlots, PlotPath};
#[test]
fn surface_plot_keeps_a_ground_point_fixed_and_cache_rewinds() {
    let planet = void_testkit::earth_size();
    let (mut eph, body) = void_testkit::planet_ephemeris(&planet);
    eph.extend_to(3600.0);
    let radius = planet.terrain.radius_meters + 100.0;
    let omega = eph.bodies()[body].rotation.rate();
    let mut trajectory = Trajectory::new();
    for i in 0..=1024 {
        let t = 3600.0 * i as f64 / 1024.0;
        let axes = eph.bodies()[body].rotation.body_axes(t);
        let p = axes[0] * radius;
        let v = axes[1] * radius * omega;
        trajectory.append(t, &[p.x, p.y, p.z, v.x, v.y, v.z]);
    }
    let mut surface = PlotPath::default();
    let spec = FrameSpec::BodySurface { body };
    let origin = trajectory.position(0);
    surface.update(&eph, &trajectory, spec, 0, 0.0, origin, body);
    assert!(surface.points.len() > 100);
    assert!(
        surface.points.iter().all(|p| p.length() < 1e-3),
        "ground point must stay fixed in surface plot"
    );
    let original = surface.points.clone();
    surface.update(
        &eph,
        &trajectory,
        spec,
        0,
        1800.0,
        trajectory.sample(1800.0).0,
        body,
    );
    surface.update(&eph, &trajectory, spec, 0, 0.0, origin, body);
    assert_eq!(
        surface.points, original,
        "loading an earlier time must rebuild the cache"
    );
    surface.update(
        &eph,
        &trajectory,
        FrameSpec::BodyInertial { body },
        0,
        0.0,
        origin,
        body,
    );
    assert!(
        surface.points.last().unwrap().length() > 1e6,
        "same ground point moves in an inertial plot"
    );
    let mut bodies = BodyPlots::default();
    assert!(
        bodies.update(&eph, spec, 0.0, DVec3::ZERO)[body].is_empty(),
        "centred body has no trail"
    );
}
