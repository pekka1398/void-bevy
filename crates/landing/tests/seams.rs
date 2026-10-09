//! Swept invariants at the seams of the planet frame and the contact world. The checks in
//! `contact.rs` each hold one scenario; these hold the same properties over many states instead,
//! to catch a seam that is only right for the one case that happens to be written down.
//!
//! The states are drawn from a fixed seed, printed with every run: a sweep that fails once and
//! passes next time is worse than no sweep at all, so nothing here draws from the clock.

use std::sync::Arc;

use glam::DVec3;
use void_landing::{ContactFrame, FrameState, PlanetFrame, pebble};
use void_orbit::{
    BodySpec, EllipticElements, Ephemeris, EphemerisOptions, GravityField, OrbitPlane,
    PropagationRun, RotationSpec, SpinSpec, SystemSpec, Tolerances, VesselPropagator, VesselState,
    build_system, suggested_step_seconds,
};
use void_terrain::Terrain;

const SEED: u64 = 0x5EED_1A17_D00D_F00D;
const FRAME_TOLERANCES: Tolerances = Tolerances {
    position_meters: 1e-6,
    velocity_meters_per_second: 1e-9,
};

/// SplitMix64, so the sweep is reproducible without a dependency and without the generator itself
/// being something to debug: it is seven lines and its output is a documented constant stream.
struct Rng(u64);

impl Rng {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in [0, 1).
    fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    fn range(&mut self, low: f64, high: f64) -> f64 {
        low + (high - low) * self.unit()
    }

    /// Log-uniform, so a sweep over altitudes spends as many samples in the first kilometre as in
    /// the last hundred. The seams are at the bottom of the range and a linear draw would miss them.
    fn log_range(&mut self, low: f64, high: f64) -> f64 {
        (low.ln() + (high.ln() - low.ln()) * self.unit()).exp()
    }

    /// Uniform on the sphere, by the cylindrical projection: z is uniform and the longitude is too.
    fn direction(&mut self) -> DVec3 {
        let z = self.range(-1.0, 1.0);
        let phi = self.range(0.0, std::f64::consts::TAU);
        let r = (1.0 - z * z).max(0.0).sqrt();
        DVec3::new(r * phi.cos(), r * phi.sin(), z)
    }
}

struct Env {
    name: &'static str,
    ephemeris: Ephemeris,
    frame: PlanetFrame,
    #[allow(dead_code)]
    terrain: Arc<Terrain>,
}

/// Pebble with an exaggerated J2 and a small moon, as `contact.rs` uses: every term in the frame's
/// acceleration is non-zero, so an error in any one of them shows up.
fn harsh_pebble() -> Env {
    let base = pebble();
    let mut root = base.system.root.clone();
    root.gravity_field = Some(GravityField {
        j2: 0.01,
        reference_radius_meters: 100e3,
    });
    root.children = vec![BodySpec {
        id: "pip".into(),
        name: "Pip".into(),
        color: "#999".into(),
        mass_kg: 2e19,
        radius_meters: 10e3,
        rotation: RotationSpec::Spin(SpinSpec {
            period_seconds: 36_000.0,
            obliquity_radians: 0.0,
            pole_longitude_radians: 0.0,
            angle_at_epoch_radians: 0.0,
        }),
        orbit: Some(EllipticElements {
            semi_major_axis_meters: 400e3,
            eccentricity: 0.0,
            inclination_radians: 0.3,
            longitude_of_ascending_node_radians: 0.0,
            argument_of_periapsis_radians: 0.0,
            mean_anomaly_radians: 1.0,
        }),
        orbit_plane: Some(OrbitPlane::Ecliptic),
        gravity_field: None,
        children: Vec::new(),
    }];
    let system = build_system(&SystemSpec {
        name: "harsh pebble".into(),
        root,
    });
    let mut ephemeris = Ephemeris::new(
        &system,
        EphemerisOptions {
            step_seconds: suggested_step_seconds(&system.bodies, 256.0),
            chunk_steps: 1024,
        },
    );
    ephemeris.extend_to(6000.0);
    let frame = PlanetFrame::new(&ephemeris, 0);
    Env {
        name: "harsh pebble",
        ephemeris,
        frame,
        terrain: base.terrain,
    }
}

/// Aurelia, the game's Earth analogue, in the whole Sol system: sixty times Pebble's radius, and
/// its centre is an astronomical unit from the frame the inertial side is written in, which is where
/// the arithmetic actually hurts. This is the case the game runs, so it sets the real thresholds.
fn aurelia() -> Env {
    let base = void_landing::aurelia();
    let (mut ephemeris, index) = void_landing::planet_ephemeris(&base);
    ephemeris.extend_to(6000.0);
    let frame = PlanetFrame::new(&ephemeris, index);
    Env {
        name: "aurelia",
        ephemeris,
        frame,
        terrain: base.terrain,
    }
}

/// A body-fixed state somewhere over the planet: any latitude, from the deck to well past orbit,
/// moving in any direction at anything from a walk to escape speed.
fn sample(rng: &mut Rng, frame: &PlanetFrame) -> (f64, FrameState) {
    let d = rng.direction();
    let altitude = rng.log_range(1.0, 500e3);
    let speed = rng.log_range(1.0, 2.0 * (frame.body.gm / frame.body.radius_meters).sqrt());
    (
        rng.range(0.0, 5000.0),
        FrameState {
            position: d * (frame.body.radius_meters + altitude),
            velocity: rng.direction() * speed,
        },
    )
}

/// The smallest position difference the arithmetic can represent where the cancelling happens, which
/// is at whichever is larger: the planet's own distance from the frame origin, or the vessel's
/// distance from the planet. Aurelia sits an astronomical unit out, so there the floor is 1.5e11 m ×
/// f64's epsilon, about 33 µm, and no amount of care in the frame code can beat it; Pebble sits all
/// but at the origin, so there the vessel's own radius sets it instead. Thresholds below are stated
/// as multiples of this rather than in absolute metres, so they travel with the planet and the orbit
/// instead of being retuned per scenario.
fn precision_floor(env: &Env, t: f64, position: DVec3) -> f64 {
    let mut positions = vec![DVec3::ZERO; env.ephemeris.bodies().len()];
    env.ephemeris.positions_at(t, &mut positions);
    f64::EPSILON
        * positions[env.frame.body.index]
            .length()
            .max(position.length())
            .max(1.0)
}

/// The largest of a sweep, and the state that produced it, so a failure names its own case.
struct Worst {
    value: f64,
    at: String,
    seen: usize,
}

impl Worst {
    fn new() -> Self {
        Self {
            value: 0.0,
            at: "no samples".into(),
            seen: 0,
        }
    }

    fn offer(&mut self, value: f64, at: impl FnOnce() -> String) {
        self.seen += 1;
        if value > self.value || self.seen == 1 {
            self.value = value;
            self.at = at();
        }
    }
}

#[test]
fn frame_transforms_round_trip_from_anywhere() {
    // The conversion is r = Rᵀ(p − c), v = Rᵀ(u − c′) − ω × r and back. Position error is absolute
    // metres, which is what matters on the ground, but velocity error has to be read against the
    // frame term it cancels: ω × r is 465 m/s on Earth's equator and far more out at the samples'
    // 500 km, so a fixed metres-per-second bound would be measuring the planet's size.
    let mut rng = Rng(SEED);
    let (mut worst_position, mut worst_velocity) = (Worst::new(), Worst::new());
    for env in [harsh_pebble(), aurelia()] {
        for _ in 0..4000 {
            let (t, s) = sample(&mut rng, &env.frame);
            let there = env.frame.to_inertial(&env.ephemeris, t, s);
            let back = env.frame.to_body_fixed(&env.ephemeris, t, there);
            let scale = s.velocity.length() + env.frame.omega.abs() * s.position.length();
            let floor = precision_floor(&env, t, s.position);
            worst_position.offer((back.position - s.position).length() / floor, || {
                format!(
                    "{} at T+{t:.0} s, {:.0} km up, floor {floor:.2e} m",
                    env.name,
                    (s.position.length() - env.frame.body.radius_meters) / 1e3
                )
            });
            // Velocity cancels at the same two scales as position: the planet's own speed through
            // the frame — 30 km/s for Aurelia — against the vessel's own, which near the ground is
            // only the 465 m/s the surface is being carried at. Measured in ulps of the larger.
            let carried = env
                .frame
                .to_inertial(
                    &env.ephemeris,
                    t,
                    FrameState {
                        position: s.position,
                        velocity: DVec3::ZERO,
                    },
                )
                .velocity
                .length();
            // Plus the part inherited from position: the velocity conversion carries a −ω × r term,
            // so whatever the position lost comes back multiplied by ω. At Aurelia that is the
            // larger of the two — 33 µm of position is 2.4 nm/s of velocity — and it is not
            // something the velocity code could avoid without the position being exact first.
            let velocity_floor =
                f64::EPSILON * carried.max(scale).max(1.0) + env.frame.omega.abs() * floor;
            worst_velocity.offer((back.velocity - s.velocity).length() / velocity_floor, || {
                format!(
                    "{} at T+{t:.0} s, {scale:.0} m/s in frame against {carried:.0} m/s carried, floor {velocity_floor:.2e} m/s",
                    env.name
                )
            });
        }
    }
    println!(
        "frame round trip over 8000 states: position within {:.1} ulp of its cancelling scale (worst {}), velocity within {:.1} ulp of its own (worst {})",
        worst_position.value, worst_position.at, worst_velocity.value, worst_velocity.at
    );
    // A few ulps, not a few metres: the conversion subtracts the planet's centre and adds it back,
    // so it cannot do better, and holding it to single-digit ulps says the frame code adds nothing
    // of its own. In metres this is tens of microns at Aurelia, which is three orders below the
    // metre scale two docking craft care about.
    assert!(
        worst_position.value < 8.0,
        "round trip lost {:.1} ulp at {}",
        worst_position.value,
        worst_position.at
    );
    assert!(
        worst_velocity.value < 8.0,
        "round trip lost {:.1} ulp of velocity at {}",
        worst_velocity.value,
        worst_velocity.at
    );
}

#[test]
fn the_rotating_frame_equations_follow_the_inertial_ones_from_anywhere() {
    // Integrating in the rotating frame with the centrifugal and Coriolis terms must give the same
    // arc as integrating in the inertial frame and converting, from any state rather than from the
    // one hop contact.rs flies. The arc is short so the sweep is affordable; what it is looking for
    // is a term that is wrong or missing, which shows immediately, not slow drift.
    let mut rng = Rng(SEED ^ 0x0000_0000_000A_11CE);
    let arc = 10.0;
    let h = 0.005;
    let mut worst = Worst::new();
    for mut env in [harsh_pebble(), aurelia()] {
        for _ in 0..150 {
            let (t0, start) = sample(&mut rng, &env.frame);
            // The inertial reference: the orbit crate's own adaptive integrator.
            let inertial = env.frame.to_inertial(&env.ephemeris, t0, start);
            let mut run = PropagationRun::new(VesselState {
                time: t0,
                position: inertial.position,
                velocity: inertial.velocity,
                mass_kg: 1000.0,
            });
            let mut propagator = VesselPropagator::new(&env.ephemeris, FRAME_TOLERANCES);
            propagator.advance(
                &mut env.ephemeris,
                &mut run,
                t0 + arc,
                10_000_000,
                None,
                None,
            );
            if run.impact.is_some() {
                continue;
            }
            let s = run.state();
            let reference = env.frame.to_body_fixed(
                &env.ephemeris,
                t0 + arc,
                FrameState {
                    position: s.position,
                    velocity: s.velocity,
                },
            );
            // RK4 in the rotating frame, the same scheme contact.rs uses.
            let eph = &env.ephemeris;
            let acc = |t: f64, r: DVec3, v: DVec3| env.frame.acceleration(eph, t, r, v);
            let (mut t, mut r, mut v) = (t0, start.position, start.velocity);
            while t < t0 + arc - 1e-9 {
                let (k1v, k1r) = (acc(t, r, v), v);
                let (k2v, k2r) = (
                    acc(t + h / 2.0, r + k1r * (h / 2.0), v + k1v * (h / 2.0)),
                    v + k1v * (h / 2.0),
                );
                let (k3v, k3r) = (
                    acc(t + h / 2.0, r + k2r * (h / 2.0), v + k2v * (h / 2.0)),
                    v + k2v * (h / 2.0),
                );
                let (k4v, k4r) = (acc(t + h, r + k3r * h, v + k3v * h), v + k3v * h);
                r += ((k1r + k4r) + (k2r + k3r) * 2.0) * (h / 6.0);
                v += ((k1v + k4v) + (k2v + k3v) * 2.0) * (h / 6.0);
                t += h;
            }
            // Two terms, because a sample that barely moves is dominated by the arithmetic and a
            // fast one by the schemes disagreeing: the ulps the conversions cannot avoid, plus a
            // part per billion of the distance covered. The measure is the error over their sum, so
            // one number covers a drifting metre and a 20 km arc alike.
            let flown = (reference.position - start.position).length();
            let budget = 8.0 * precision_floor(&env, t0 + arc, start.position) + 1e-9 * flown;
            worst.offer((r - reference.position).length() / budget, || {
                format!(
                    "{} at T+{t0:.0} s, {:.0} km up at {:.0} m/s, flew {flown:.0} m, budget {budget:.2e} m",
                    env.name,
                    (start.position.length() - env.frame.body.radius_meters) / 1e3,
                    start.velocity.length()
                )
            });
        }
    }
    println!(
        "rotating-frame equations over 300 ten-second arcs: within {:.2} of the arithmetic-plus-scheme budget (worst {})",
        worst.value, worst.at
    );
    assert!(
        worst.value < 1.0,
        "rotating-frame arc is {:.2} times its budget at {}",
        worst.value,
        worst.at
    );
}

fn contact_options() -> void_landing::ContactWorldOptions {
    void_landing::ContactWorldOptions {
        step_seconds: 1.0 / 60.0,
        tile_level: void_landing::level_for_tile_size(100e3, 300.0),
        tile_resolution: 33,
        tile_reach_meters: 300.0,
        tile_keep_meters: 600.0,
        recenter_meters: 1000.0,
        sleeping: true,
    }
}

#[test]
fn moving_the_floating_origin_never_moves_the_craft() {
    // `contact.rs` moves the origin 800 m once and asks the state not to change. The reason there
    // is an origin to move is that Rapier works in f32: at planet-radius coordinates it cannot
    // resolve a centimetre, so bodies are kept near a local origin that follows them. That makes the
    // error budget proportional to how far the origin is allowed to drift, which is what this sweeps
    // — the game sets `recenter_meters`, and this is the number that setting costs.
    let mut rng = Rng(SEED ^ 0x0000_0000_000F_0001);
    let base = pebble();
    let system = build_system(&base.system);
    let mut worst_position = Worst::new();
    let mut worst_velocity = Worst::new();
    for _ in 0..40 {
        let mut eph = Ephemeris::new(
            &system,
            EphemerisOptions {
                step_seconds: 60.0,
                chunk_steps: 1024,
            },
        );
        eph.extend_to(60.0);
        let frame = PlanetFrame::new(&eph, 0);
        let site = rng.direction();
        let radius = base.terrain.radius_meters + base.terrain.height(site);
        let at = site * (radius + rng.range(5.0, 500.0));
        let mut world = void_landing::ContactWorld::new(
            frame,
            Some(base.terrain.clone()),
            contact_options(),
            0.0,
            at,
            &mut eph,
        );
        let body = world.add_body(
            &eph,
            &void_landing::ContactBodySpec {
                shape: void_landing::BodyShape::Simple(void_landing::SimpleShape::Ball {
                    radius: 1.0,
                }),
                mass_kg: 500.0,
                friction: 0.6,
                restitution: 0.2,
                lock_rotations: false,
            },
            FrameState {
                position: at,
                velocity: rng.direction() * rng.range(0.0, 20.0),
            },
            glam::DQuat::IDENTITY,
            DVec3::ZERO,
        );
        for _ in 0..rng.range(1.0, 200.0) as usize {
            world.step(&mut eph, None);
        }
        let before = world.state(&eph, body, DVec3::ZERO);
        // Anywhere inside the distance the world is willing to let a body wander from its origin.
        let shift = rng.direction() * rng.range(1.0, contact_options().recenter_meters);
        world.recenter(world.origin + shift);
        let after = world.state(&eph, body, DVec3::ZERO);
        // f32 resolves about seven digits, so a body a kilometre from its origin is quantised at a
        // tenth of a millimetre: that is what a move would cost if the reported state came out of
        // Rapier's pose. It does not — the world keeps the body-fixed position in an f64 record and
        // only the local pose is f32 — so the move is free, and the sweep holds it to free rather
        // than to the f32 budget. The error is reported in units of that budget to say how much
        // would be given up if that ever changed.
        let floor = f32::EPSILON as f64 * shift.length().max(1.0);
        worst_position.offer((before.position - after.position).length() / floor, || {
            format!(
                "a {:.0} m move with the body {:.0} m up, floor {floor:.2e} m",
                shift.length(),
                before.position.length() - radius
            )
        });
        worst_velocity.offer(
            (before.velocity - after.velocity).length() / (f32::EPSILON as f64 * 20.0),
            || format!("a {:.0} m move", shift.length()),
        );
    }
    println!(
        "floating origin over 40 moves: position within {:.1} f32 ulp of the move (worst {}), velocity within {:.1} f32 ulp of the body's speed (worst {})",
        worst_position.value, worst_position.at, worst_velocity.value, worst_velocity.at
    );
    assert_eq!(worst_position.seen, 40, "every case must be measured");
    assert_eq!(
        (worst_position.value, worst_velocity.value),
        (0.0, 0.0),
        "moving the origin changed the state, by {:.1} f32 ulp of position ({}) and {:.1} of \
         velocity ({}): the body-fixed state is no longer independent of where the origin sits",
        worst_position.value,
        worst_position.at,
        worst_velocity.value,
        worst_velocity.at
    );
}
