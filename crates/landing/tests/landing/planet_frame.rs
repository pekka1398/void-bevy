//! The planet frame turns with the planet: ground at rest in it moves at Ω × r in space.

use glam::DVec3;
use void_frames::{BodyId, BodyStates, State};
use void_landing::PlanetFrame;
use void_testkit::{aurelia, planet_ephemeris};

#[test]
fn ground_at_rest_moves_with_the_planets_spin() {
    let planet = aurelia();
    let (mut ephemeris, index) = planet_ephemeris(&planet);
    let frame = PlanetFrame::new(&ephemeris, index);
    let r = planet.terrain.radius_meters;
    for t in [0.0, 1234.5, 86_400.0 * 3.3] {
        ephemeris.extend_to(t);
        let (centre, centre_velocity) = ephemeris.body_state(BodyId(index), t);
        let spin_axis = frame.body.rotation.axis();
        for local in [DVec3::X, DVec3::Y, DVec3::new(0.3, -0.5, 0.8).normalize()] {
            let ground = State {
                position: local * r,
                velocity: DVec3::ZERO,
            };
            let space = frame.to_inertial(&ephemeris, t, ground);
            let offset = space.position - centre;
            let expected = spin_axis * frame.omega;
            // The inertial position is barycentric, an astronomical unit out: rounding there is
            // about 3e-5 m.
            assert!(
                (offset.length() - r).abs() < 1e-4,
                "radius {}",
                offset.length()
            );
            assert!(
                (space.velocity - centre_velocity - expected.cross(offset)).length() < 1e-9,
                "t {t}: ground velocity"
            );
            let back = frame.to_body_fixed(&ephemeris, t, space);
            assert!((back.position - ground.position).length() < 1e-4);
            assert!(back.velocity.length() < 1e-9);
        }
    }
}
