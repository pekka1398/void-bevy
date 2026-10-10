use glam::{DMat3, DQuat, DVec3};
use void_assembly::demo_craft;
use void_landing::{
    ContactFrame, ContactWorldOptions, PlanetFrame, level_for_tile_size, pebble, planet_ephemeris,
};
use void_orbit::body_orientation;
use void_vessels::*;
fn main() {
    for step in [1.0 / 60.0, 1.0 / 120.0, 1.0 / 240.0] {
        let planet = pebble();
        let (e, b) = planet_ephemeris(&planet);
        let frame = PlanetFrame::new(&e, b);
        let environment = std::sync::Arc::new(
            Environment::new(&e).with(b, BodyEnvironment::airless(planet.terrain.clone())),
        );
        let ground = GroundSpec {
            body_index: b,
            band_enter_meters: 200.0,
            band_exit_meters: 400.0,
            tiles: ContactWorldOptions {
                step_seconds: step,
                tile_level: level_for_tile_size(planet.terrain.radius_meters, 300.0),
                tile_resolution: 33,
                tile_reach_meters: 300.0,
                tile_keep_meters: 600.0,
                recenter_meters: 5000.0,
                sleeping: true,
            },
        };
        let mut f = Fleet::new(
            e,
            environment,
            0.0,
            vec![ground],
            FleetOptions {
                step_seconds: step,
                ..Default::default()
            },
        );
        let id = f.launch_landed(&demo_craft(), b, flat_site(&planet));
        f.advance(0.0);
        let sample = |f: &Fleet| {
            let s = f.snapshot(&id);
            let state = f.body_fixed_state(&id, b);
            let basis = body_orientation(&frame.body.rotation, f.time());
            let axes = DQuat::from_mat3(&DMat3::from_cols(basis[0], basis[1], basis[2]));
            let q = axes.conjugate() * s.rotation;
            let w = axes.conjugate() * s.angular_velocity - frame.spin();
            let rot = DMat3::from_quat(q);
            let inertia = rot * f.inertia(&id) * rot.transpose();
            let kinetic =
                0.5 * s.mass_kg * state.velocity.length_squared() + 0.5 * w.dot(inertia * w);
            let p = state.position;
            let potential = -frame.body.gm * s.mass_kg / p.length()
                - 0.5 * s.mass_kg * frame.omega * frame.omega * (p.x * p.x + p.y * p.y)
                - 0.5 * frame.spin().dot(inertia * frame.spin());
            let tilt = (q * DVec3::Y)
                .dot(p.normalize())
                .clamp(-1.0, 1.0)
                .acos()
                .to_degrees();
            (kinetic, potential, tilt)
        };
        let (k0, u0, _) = sample(&f);
        let mut max_gain: f64 = 0.0;
        let mut initial_steps = 0;
        for i in 1..=(60.0 / step) as usize {
            f.advance(step);
            let (k, u, tilt) = sample(&f);
            let gain = k + u - k0 - u0;
            max_gain = max_gain.max(gain);
            if i == 1 || i % ((5.0 / step) as usize) == 0 {
                println!(
                    "dt={step:.6} t={:.2} K={k:.6} dU={:.6} dE={gain:.6} max={max_gain:.6} tilt={tilt:.3} asleep={}",
                    f.time(),
                    u - u0,
                    f.rails_blocker().is_none()
                );
            }
            if f.rails_blocker().is_none() && initial_steps == 0 {
                initial_steps = i;
            }
        }
        println!("sleep time {:.3}s", initial_steps as f64 * step);
    }
}
