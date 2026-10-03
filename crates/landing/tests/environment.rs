//! The ground frame's equation of motion is the environment's gravity plus that frame's own
//! terms: minus the planet centre's acceleration, plus centrifugal and Coriolis.
use glam::DVec3;
use void_environment::Environment;
use void_landing::{ContactFrame, PlanetFrame, planet_by_id, planet_ephemeris};
use void_orbit::{SystemFrames, gravity};

#[test]
fn planet_frame_is_environment_gravity_plus_its_frame_terms() {
    let (mut e, b) = planet_ephemeris(&planet_by_id("aurelia"));
    e.extend_to(100_000.0);
    let frame = PlanetFrame::new(&e, b);
    let env = Environment::new(&e);
    let frames = SystemFrames::new(&e);
    let (surface, origin) = (frames.surface[b], frames.origin);
    let radius = e.bodies()[b].radius_meters;
    let w = frame.spin().z;
    for (t, r, v) in [
        (0.0, DVec3::new(radius + 100.0, 0.0, 0.0), DVec3::ZERO),
        (
            3600.0,
            DVec3::new(0.3, -0.5, 0.8).normalize() * (radius + 8000.0),
            DVec3::new(120.0, -40.0, 15.0),
        ),
        (
            90_000.0,
            DVec3::new(-0.7, 0.1, -0.2).normalize() * (radius + 400e3),
            DVec3::new(10.0, 7600.0, 0.0),
        ),
    ] {
        let at = frames.tree.at(t, &e);
        let mut positions = vec![DVec3::ZERO; e.bodies().len()];
        e.positions_at(t, &mut positions);
        let mut centre = DVec3::ZERO;
        for (k, o) in e.bodies().iter().enumerate() {
            if k != b {
                centre += gravity::body_pull(o, positions[b] - positions[k]);
            }
        }
        let want = env.gravity(&at, &frames, surface, r)
            - at.transform(origin, surface).apply_direction(centre)
            + DVec3::new(
                w * w * r.x + 2.0 * w * v.y,
                w * w * r.y - 2.0 * w * v.x,
                0.0,
            );
        let got = frame.acceleration(&e, t, r, v);
        let error = (got - want).length() / got.length();
        println!("t {t}: {error:.1e}");
        assert!(error < 1e-14, "t {t}: {error}");
    }
}
