//! Main-game splashdown fixture, using the world's real sea and terrain sampler.
use crate::session::{Action, FlightSession, Outcome};
use glam::{DQuat, DVec3};
pub fn splashdown(session: &mut FlightSession) -> String {
    splashdown_with(session, &void_assembly::reentry_capsule(), 2., 0.)
}
/// Explicit acceptance variant; speeds are fixtures, never a flight capability claim.
pub fn splashdown_with(
    session: &mut FlightSession,
    craft: &void_assembly::Craft,
    descent: f64,
    tilt: f64,
) -> String {
    splashdown_at(session, craft, descent, tilt, 0.)
}
/// Body tilt and trajectory angle are separate; both are radians from local vertical.
pub fn splashdown_at(
    session: &mut FlightSession,
    craft: &void_assembly::Craft,
    speed: f64,
    tilt: f64,
    entry_angle: f64,
) -> String {
    assert!(
        speed.is_finite()
            && speed >= 0.
            && tilt.is_finite()
            && entry_angle.is_finite()
            && (0. ..=std::f64::consts::FRAC_PI_2).contains(&entry_angle)
    );
    let sim = session.sim();
    let fleet = &sim.fleet;
    let body = sim.home;
    let level = fleet
        .environment()
        .body(body)
        .and_then(|p| p.sea_level_meters)
        .expect("splashdown requires sea");
    let radius = fleet.environment().bodies()[body].radius_meters;
    let star = fleet
        .ephemeris
        .bodies()
        .iter()
        .find(|star| {
            star.parent_index.is_none()
                && fleet.ephemeris.system_of(star.index) == fleet.ephemeris.system_of(body)
        })
        .expect("splashdown home system has no root light source");
    // Exactly the renderer's home-system light geometry, computed entirely in f64 body axes.
    // Single-body worlds explicitly author distant +X sunlight; stellar worlds use their star.
    let at = fleet.frames();
    let surface = fleet.body_frames(body).1;
    let star_position = (star.index != body).then(|| {
        at.transform(fleet.body_frames(star.index).0, surface)
            .apply_point(DVec3::ZERO)
    });
    let distant_sun = at
        .transform(fleet.origin_frame(), surface)
        .apply_direction(DVec3::X);
    let direction = (0..4096)
        .find_map(|i| {
            let z = 1. - 2. * (i as f64 + 0.5) / 4096.;
            let t = i as f64 * 2.399963229728653;
            let r = (1. - z * z).sqrt();
            let d = DVec3::new(r * t.cos(), r * t.sin(), z);
            let sun = star_position.map_or(distant_sun, |position| {
                (position - d * (radius + level + 8.)).normalize()
            });
            assert!(
                sun.is_finite() && (sun.length_squared() - 1.).abs() < 1e-9,
                "splashdown invalid sun geometry"
            );
            if d.dot(sun) <= 0.2 {
                return None;
            }
            let s = fleet.environment().surroundings_local(
                body,
                void_frames::State {
                    position: d * (radius + level + 8.),
                    velocity: DVec3::ZERO,
                },
            );
            s.ground.filter(|g| g.height < level - 50.).map(|_| d)
        })
        .expect("splashdown requires a daylight deep ocean site (sun cosine > 0.2)");
    let transform = fleet
        .frames()
        .transform(fleet.body_frames(body).1, fleet.origin_frame());
    let upright = DQuat::from_rotation_arc(DVec3::Y, direction);
    let state = transform.apply_state(void_frames::State {
        position: direction * (radius + level + 8.),
        velocity: speed * (-direction * entry_angle.cos() + upright * DVec3::X * entry_angle.sin()),
    });
    let rotation = transform.rotation() * upright * DQuat::from_rotation_z(tilt);
    let outcome = session.execute(Action::LaunchState {
        craft: craft.clone(),
        position: state.position,
        velocity: state.velocity,
        rotation,
        angular_velocity: DVec3::ZERO,
    });
    let Outcome::Spawned(id) = outcome else {
        panic!("splashdown launch refused")
    };
    session.execute(Action::Select { vessel: id.clone() });
    id
}
