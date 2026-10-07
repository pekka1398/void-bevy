//! Several star systems in one Newtonian N-body world, as the lab's `CoupledWorld.ts`.

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

use glam::DVec3;
use void_math::hypot;
use void_orbit::{BuiltSystem, CelestialBody, HermiteBasis, SystemFrames, yoshida8_sequence};

use void_frames::{BodyId, FrameSource, SplitPosition, SystemId};

/// A system to place in the world: its built bodies (barycentric), where its barycentre is and
/// how fast it moves.
#[derive(Clone, Debug)]
pub struct SystemSeed {
    pub id: String,
    pub system: BuiltSystem,
    pub origin: SplitPosition,
    pub velocity: DVec3,
}

/// One system at one time: its barycentre's split position, velocity and acceleration, and its
/// bodies relative to it (flat x, y, z per body).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SystemState {
    pub origin: SplitPosition,
    pub velocity: DVec3,
    pub acceleration: DVec3,
    pub positions: Vec<f64>,
    pub velocities: Vec<f64>,
    pub accelerations: Vec<f64>,
}

impl SystemState {
    pub fn body_position(&self, local: usize) -> DVec3 {
        DVec3::from_slice(&self.positions[3 * local..3 * local + 3])
    }

    pub fn body_velocity(&self, local: usize) -> DVec3 {
        DVec3::from_slice(&self.velocities[3 * local..3 * local + 3])
    }
}

struct LiveSystem {
    state: SystemState,
    bodies: Vec<CelestialBody>,
    mass: f64,
    origin_correction: DVec3,
    position_correction: Vec<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Sample {
    time: f64,
    systems: Vec<SystemState>,
}

/// Which system a world body belongs to and its index there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Membership {
    pub system: usize,
    pub local: usize,
}

/// Direct Newtonian N-body, grouped only for precision: every massive body appears once. A
/// system's split origin follows its barycentre; its bodies stay local float64. Forces between
/// systems move the barycentre, and what is left of them acts on the members (external tides).
/// No monopole replacement, cut-off or sphere-of-influence switch. Orbit's Yoshida-8 stepping
/// and quintic interpolation. O(N²): not a galaxy solver.
pub struct CoupledWorld {
    pub ids: Vec<String>,
    /// All bodies, ids `system/body`, indices and parents renumbered world-wide.
    pub bodies: Vec<CelestialBody>,
    pub membership: Vec<Membership>,
    pub step_seconds: f64,
    pub sample_limit: usize,
    seed_signature: serde_json::Value,
    live: Vec<LiveSystem>,
    samples: VecDeque<Sample>,
    latest: f64,
    pub steps: u64,
}

impl CoupledWorld {
    pub fn new(seeds: Vec<SystemSeed>, step_seconds: f64, sample_limit: usize) -> Self {
        assert!(
            !seeds.is_empty()
                && step_seconds > 0.0
                && step_seconds.is_finite()
                && sample_limit >= 4,
            "CoupledWorld: invalid options"
        );
        let mut ids: Vec<String> = seeds.iter().map(|s| s.id.clone()).collect();
        ids.sort();
        ids.dedup();
        assert!(
            ids.len() == seeds.len() && seeds.iter().all(|s| !s.id.is_empty()),
            "CoupledWorld: duplicate/empty system id"
        );
        let seed_signature = serde_json::Value::Array(
            seeds
                .iter()
                .map(|s| {
                    serde_json::json!({
                        "id": s.id, "origin": s.origin, "velocity": s.velocity,
                        "positions": s.system.positions, "velocities": s.system.velocities,
                    })
                })
                .collect(),
        );
        let ids = seeds.iter().map(|s| s.id.clone()).collect();
        let mut bodies: Vec<CelestialBody> = vec![];
        let mut membership = vec![];
        let live = seeds
            .into_iter()
            .enumerate()
            .map(|(system_index, seed)| {
                assert!(seed.velocity.is_finite(), "system velocity: non-finite");
                let n = seed.system.bodies.len();
                let first = bodies.len();
                assert!(
                    n > 0 && seed.system.positions.len() == n && seed.system.velocities.len() == n,
                    "CoupledWorld: invalid body arrays"
                );
                let mut mass = 0.0;
                for body in &seed.system.bodies {
                    assert!(
                        body.mass_kg > 0.0 && body.mass_kg.is_finite(),
                        "CoupledWorld: requires finite positive masses"
                    );
                    bodies.push(CelestialBody {
                        id: format!("{}/{}", seed.id, body.id),
                        name: format!("{} {}", seed.id, body.name),
                        index: bodies.len(),
                        parent_index: body.parent_index.map(|p| first + p),
                        ..body.clone()
                    });
                    membership.push(Membership {
                        system: system_index,
                        local: body.index,
                    });
                    mass += body.mass_kg;
                }
                let flat = |v: &[DVec3]| v.iter().flat_map(|p| p.to_array()).collect::<Vec<_>>();
                let positions = flat(&seed.system.positions);
                let velocities = flat(&seed.system.velocities);
                assert!(
                    positions.iter().chain(&velocities).all(|v| v.is_finite()),
                    "CoupledWorld: non-finite state"
                );
                LiveSystem {
                    state: SystemState {
                        origin: seed.origin,
                        velocity: seed.velocity,
                        acceleration: DVec3::ZERO,
                        accelerations: vec![0.0; 3 * n],
                        positions,
                        velocities,
                    },
                    bodies: seed.system.bodies,
                    mass,
                    origin_correction: DVec3::ZERO,
                    position_correction: vec![0.0; 3 * n],
                }
            })
            .collect();
        let mut world = Self {
            ids,
            bodies,
            membership,
            step_seconds,
            sample_limit,
            live,
            seed_signature,
            samples: VecDeque::new(),
            latest: 0.0,
            steps: 0,
        };
        world.accelerate();
        world.save();
        world
    }

    /// The latest integrated time.
    pub fn time(&self) -> f64 {
        self.latest
    }

    /// The oldest retained time.
    pub fn start_time(&self) -> f64 {
        self.samples[0].time
    }

    pub fn sample_count(&self) -> usize {
        self.samples.len()
    }

    pub fn retained_bytes(&self) -> usize {
        self.samples
            .iter()
            .map(|s| {
                s.systems
                    .iter()
                    .map(|g| (g.positions.len() * 3 + 9) * 8)
                    .sum::<usize>()
            })
            .sum()
    }

    pub fn system_index(&self, id: &str) -> usize {
        self.ids
            .iter()
            .position(|s| s == id)
            .unwrap_or_else(|| panic!("CoupledWorld: unknown system {id}"))
    }

    /// Step until `t` or `budget` steps; false means `t` was not reached. No simulated time is
    /// skipped.
    pub fn extend_to(&mut self, t: f64, budget: u64) -> bool {
        assert!(
            t.is_finite() && t >= self.start_time(),
            "CoupledWorld: invalid extension"
        );
        let mut n = 0;
        while self.latest < t && n < budget {
            self.step();
            n += 1;
        }
        self.latest >= t
    }

    fn step(&mut self) {
        for w in yoshida8_sequence() {
            let dt = w * self.step_seconds;
            self.kick(dt / 2.0);
            for g in &mut self.live {
                g.state.origin = g
                    .state
                    .origin
                    .drift(g.state.velocity * dt, &mut g.origin_correction);
                for i in 0..g.state.positions.len() {
                    let y = g.state.velocities[i] * dt - g.position_correction[i];
                    let q = g.state.positions[i] + y;
                    g.position_correction[i] = (q - g.state.positions[i]) - y;
                    g.state.positions[i] = q;
                }
            }
            self.accelerate();
            self.kick(dt / 2.0);
        }
        for g in &self.live {
            assert!(g.state.velocity.is_finite(), "system velocity: non-finite");
            assert!(
                g.state
                    .positions
                    .iter()
                    .chain(&g.state.velocities)
                    .all(|v| v.is_finite()),
                "CoupledWorld: non-finite integrated state"
            );
        }
        self.latest += self.step_seconds;
        self.steps += 1;
        self.save();
    }

    fn kick(&mut self, dt: f64) {
        for g in &mut self.live {
            let s = &mut g.state;
            s.velocity += s.acceleration * dt;
            for i in 0..s.velocities.len() {
                s.velocities[i] += s.accelerations[i] * dt;
            }
        }
    }

    fn accelerate(&mut self) {
        // External and internal sums stay separate, so subtracting the barycentre's acceleration
        // cannot cancel internal gravity.
        let mut external: Vec<Vec<f64>> = self
            .live
            .iter()
            .map(|g| vec![0.0; g.state.positions.len()])
            .collect();
        for g in &mut self.live {
            g.state.accelerations.fill(0.0);
        }
        /// The separation from body a to body b (b's system shifted by `shift`), and the
        /// accelerations' scales: a gets +d·sa, b gets −d·sb.
        fn pair(
            pa: &[f64],
            a: usize,
            gm_a: f64,
            pb: &[f64],
            b: usize,
            gm_b: f64,
            shift: DVec3,
        ) -> ([f64; 3], f64, f64) {
            let (a, b) = (a * 3, b * 3);
            let d = [
                shift.x + pb[b] - pa[a],
                shift.y + pb[b + 1] - pa[a + 1],
                shift.z + pb[b + 2] - pa[a + 2],
            ];
            let r = hypot(d);
            assert!(
                r > 0.0 && r.is_finite(),
                "CoupledWorld: coincident/non-finite bodies"
            );
            let inv = 1.0 / (r * r * r);
            (d, gm_b * inv, gm_a * inv)
        }
        let n = self.live.len();
        for s in 0..n {
            let g = &mut self.live[s];
            for a in 0..g.bodies.len() {
                for b in a + 1..g.bodies.len() {
                    let (gm_a, gm_b) = (g.bodies[a].gm, g.bodies[b].gm);
                    let (d, sa, sb) = pair(
                        &g.state.positions,
                        a,
                        gm_a,
                        &g.state.positions,
                        b,
                        gm_b,
                        DVec3::ZERO,
                    );
                    for (c, dc) in d.iter().enumerate() {
                        g.state.accelerations[a * 3 + c] += dc * sa;
                        g.state.accelerations[b * 3 + c] -= dc * sb;
                    }
                }
            }
            for u in s + 1..n {
                let (gs, gu) = (&self.live[s], &self.live[u]);
                let shift = gu.state.origin.relative(&gs.state.origin);
                for a in 0..gs.bodies.len() {
                    for b in 0..gu.bodies.len() {
                        let (d, sa, sb) = pair(
                            &gs.state.positions,
                            a,
                            gs.bodies[a].gm,
                            &gu.state.positions,
                            b,
                            gu.bodies[b].gm,
                            shift,
                        );
                        for (c, dc) in d.iter().enumerate() {
                            external[s][a * 3 + c] += dc * sa;
                            external[u][b * 3 + c] -= dc * sb;
                        }
                    }
                }
            }
        }
        for (g, e) in self.live.iter_mut().zip(&external) {
            let mut mean = DVec3::ZERO;
            for (i, b) in g.bodies.iter().enumerate() {
                for c in 0..3 {
                    mean[c] += e[i * 3 + c] * (b.mass_kg / g.mass);
                }
            }
            g.state.acceleration = mean;
            for i in 0..e.len() {
                g.state.accelerations[i] += e[i] - mean[i % 3];
            }
        }
    }

    fn save(&mut self) {
        self.samples.push_back(Sample {
            time: self.latest,
            systems: self.live.iter().map(|g| g.state.clone()).collect(),
        });
        if self.samples.len() > self.sample_limit {
            let drop = 512.min(self.samples.len() - 2);
            self.samples.drain(..drop);
        }
    }

    /// Drop samples before `t`, keeping the one at or before it and at least two.
    pub fn forget_before(&mut self, t: f64) {
        assert!(t.is_finite(), "CoupledWorld: invalid prune time");
        let mut n = 0;
        while n + 1 < self.samples.len() - 1 && self.samples[n + 1].time <= t {
            n += 1;
        }
        self.samples.drain(..n);
    }

    /// Every system at `t`, interpolated between samples. Panics outside the retained interval.
    pub fn at(&self, t: f64) -> Vec<SystemState> {
        assert!(
            t >= self.start_time() && t <= self.latest,
            "CoupledWorld: query outside retained interval"
        );
        let (mut lo, mut hi) = (0, self.samples.len() - 1);
        while lo < hi {
            let m = (lo + hi).div_ceil(2);
            if self.samples[m].time <= t {
                lo = m;
            } else {
                hi = m - 1;
            }
        }
        let a = &self.samples[lo];
        if t == a.time {
            return a.systems.clone();
        }
        let b = &self.samples[lo + 1];
        let h = b.time - a.time;
        let basis = HermiteBasis::new(h, (t - a.time) / h);
        a.systems
            .iter()
            .zip(&b.systems)
            .map(|(g, next)| {
                let delta = next.origin.relative(&g.origin);
                let (mut d, mut v, mut acc) = (DVec3::ZERO, DVec3::ZERO, DVec3::ZERO);
                for k in 0..3 {
                    let args = (
                        0.0,
                        delta[k],
                        g.velocity[k],
                        next.velocity[k],
                        g.acceleration[k],
                        next.acceleration[k],
                    );
                    d[k] = basis.position(args.0, args.1, args.2, args.3, args.4, args.5);
                    v[k] = basis.velocity(args.0, args.1, args.2, args.3, args.4, args.5);
                    acc[k] = basis.acceleration(args.0, args.1, args.2, args.3, args.4, args.5);
                }
                let len = g.positions.len();
                let (mut p, mut u, mut f) = (vec![0.0; len], vec![0.0; len], vec![0.0; len]);
                for j in 0..len {
                    let args = (
                        g.positions[j],
                        next.positions[j],
                        g.velocities[j],
                        next.velocities[j],
                        g.accelerations[j],
                        next.accelerations[j],
                    );
                    p[j] = basis.position(args.0, args.1, args.2, args.3, args.4, args.5);
                    u[j] = basis.velocity(args.0, args.1, args.2, args.3, args.4, args.5);
                    f[j] = basis.acceleration(args.0, args.1, args.2, args.3, args.4, args.5);
                }
                SystemState {
                    origin: g.origin.translate(d),
                    velocity: v,
                    acceleration: acc,
                    positions: p,
                    velocities: u,
                    accelerations: f,
                }
            })
            .collect()
    }

    /// Body `index`'s split position in `state` (from `at`).
    pub fn body_position(&self, index: usize, state: &[SystemState]) -> SplitPosition {
        let m = self
            .membership
            .get(index)
            .unwrap_or_else(|| panic!("CoupledWorld: invalid body index"));
        let g = &state[m.system];
        g.origin.translate(g.body_position(m.local))
    }

    /// Gravity of every body at `p` and time `t`.
    pub fn gravity_at(&self, t: f64, p: &SplitPosition) -> DVec3 {
        self.gravity_in(&self.at(t), p)
    }

    /// Gravity of every body in `state` at `p`: the point-mass case of `void_orbit::gravity::pull`
    /// (this world requires J2 = 0), written in the multiscale lab's arithmetic (`hypot`, r·r·r)
    /// because its golden checks are bit-exact.
    pub fn gravity_in(&self, state: &[SystemState], p: &SplitPosition) -> DVec3 {
        let mut out = DVec3::ZERO;
        for (i, body) in self.bodies.iter().enumerate() {
            let d = self.body_position(i, state).relative(p);
            let r = hypot(d.to_array());
            assert!(r > 0.0, "CoupledWorld: gravity at a body centre");
            let f = body.gm / (r * r * r);
            out.x += d.x * f;
            out.y += d.y * f;
            out.z += d.z * f;
            let c = void_orbit::gravity::oblateness(body);
            if c != 0.0 {
                // Preserve the existing point-mass arithmetic (bit-exact golden fixtures),
                // while applying the same zonal field that ordinary orbit vessels feel.
                out += void_orbit::gravity::pull(0.0, c, body.rotation.axis(), -d);
            }
        }
        out
    }
}

/// The frame tree's view: each system at its split barycentre, each body relative to its own
/// system.
impl FrameSource for CoupledWorld {
    fn system_state(&self, system: SystemId, t: f64) -> (SplitPosition, DVec3) {
        let states = self.at(t);
        let s = states
            .get(system.0)
            .unwrap_or_else(|| panic!("coupled world: unknown {system:?}"));
        (s.origin, s.velocity)
    }
    fn body_in_system(&self, body: BodyId, t: f64) -> (DVec3, DVec3) {
        let m = self
            .membership
            .get(body.0)
            .expect("coupled world: unknown body");
        let g = &self.at(t)[m.system];
        (g.body_position(m.local), g.body_velocity(m.local))
    }
}

impl CoupledWorld {
    /// Every system and body as frames; `origin` names the system fleet states would be in.
    pub fn frames(&self, origin: &str) -> SystemFrames {
        SystemFrames::build(
            self.ids.len(),
            &self.bodies,
            |i| SystemId(self.membership[i].system),
            SystemId(self.system_index(origin)),
        )
    }
}

/// Exact continuation state, including drift compensation and retained interpolation samples.
/// Body definitions come from the explicitly supplied seeds and must match the stored signature.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoupledCheckpoint {
    version: u32,
    ids: Vec<String>,
    bodies: serde_json::Value,
    seeds: serde_json::Value,
    step_seconds: f64,
    sample_limit: usize,
    latest: f64,
    steps: u64,
    live: Vec<SavedSystem>,
    samples: VecDeque<Sample>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedSystem {
    state: SystemState,
    origin_correction: DVec3,
    position_correction: Vec<f64>,
}
impl CoupledWorld {
    fn body_signature(&self) -> serde_json::Value {
        serde_json::Value::Array(
            self.bodies
                .iter()
                .map(|b| {
                    serde_json::json!({
                        "id": b.id, "index": b.index, "parent": b.parent_index,
                        "mass": b.mass_kg, "gm": b.gm, "radius": b.radius_meters,
                        "j2": b.j2, "j2_radius": b.j2_reference_radius_meters,
                        "rotation": [b.rotation.period_seconds, b.rotation.obliquity_radians,
                            b.rotation.pole_longitude_radians, b.rotation.angle_at_epoch_radians],
                    })
                })
                .collect(),
        )
    }
    pub fn checkpoint(&self) -> CoupledCheckpoint {
        CoupledCheckpoint {
            version: 1,
            ids: self.ids.clone(),
            bodies: self.body_signature(),
            seeds: self.seed_signature.clone(),
            step_seconds: self.step_seconds,
            sample_limit: self.sample_limit,
            latest: self.latest,
            steps: self.steps,
            live: self
                .live
                .iter()
                .map(|g| SavedSystem {
                    state: g.state.clone(),
                    origin_correction: g.origin_correction,
                    position_correction: g.position_correction.clone(),
                })
                .collect(),
            samples: self.samples.clone(),
        }
    }
    /// Reconstruct definitions only; never reintegrate centuries from the initial epoch.
    pub fn from_checkpoint(seeds: Vec<SystemSeed>, saved: CoupledCheckpoint) -> Self {
        assert_eq!(saved.version, 1, "coupled checkpoint: unsupported version");
        let mut world = Self::new(seeds, saved.step_seconds, saved.sample_limit);
        assert_eq!(saved.ids, world.ids, "coupled checkpoint: systems changed");
        assert_eq!(
            saved.seeds, world.seed_signature,
            "coupled checkpoint: initial placement changed"
        );
        assert_eq!(
            saved.bodies,
            world.body_signature(),
            "coupled checkpoint: bodies changed"
        );
        assert!(
            saved.latest.is_finite() && saved.latest >= 0.0,
            "coupled checkpoint: invalid time"
        );
        assert_eq!(
            saved.live.len(),
            world.live.len(),
            "coupled checkpoint: wrong live systems"
        );
        assert!(
            !saved.samples.is_empty() && saved.samples.len() <= saved.sample_limit,
            "coupled checkpoint: invalid retained history"
        );
        let validate = |state: &SystemState, n: usize| {
            assert_eq!(
                state.origin,
                state.origin.translate(DVec3::ZERO),
                "coupled checkpoint: noncanonical origin"
            );
            assert!(
                state.velocity.is_finite() && state.acceleration.is_finite(),
                "coupled checkpoint: nonfinite barycentre"
            );
            for values in [&state.positions, &state.velocities, &state.accelerations] {
                assert!(
                    values.len() == 3 * n && values.iter().all(|v| v.is_finite()),
                    "coupled checkpoint: invalid body state"
                );
            }
        };
        let mut previous = None;
        for sample in &saved.samples {
            assert!(
                sample.time.is_finite() && sample.time >= 0.0 && sample.time <= saved.latest,
                "coupled checkpoint: invalid sample clock"
            );
            if let Some(t) = previous {
                assert!(sample.time > t, "coupled checkpoint: unordered samples");
                assert!(
                    ((sample.time - t) - saved.step_seconds).abs()
                        <= 8.0 * f64::EPSILON * saved.latest.max(saved.step_seconds),
                    "coupled checkpoint: inconsistent sample spacing"
                );
            }
            previous = Some(sample.time);
            assert_eq!(
                sample.systems.len(),
                world.live.len(),
                "coupled checkpoint: sample systems"
            );
            for (state, definition) in sample.systems.iter().zip(&world.live) {
                validate(state, definition.bodies.len());
            }
        }
        assert_eq!(
            saved.samples.back().unwrap().time,
            saved.latest,
            "coupled checkpoint: latest history missing"
        );
        for ((live, saved_live), newest) in world
            .live
            .iter_mut()
            .zip(saved.live)
            .zip(&saved.samples.back().unwrap().systems)
        {
            validate(&saved_live.state, live.bodies.len());
            assert_eq!(
                saved_live.state, *newest,
                "coupled checkpoint: live state differs from history"
            );
            assert!(
                saved_live.origin_correction.is_finite()
                    && saved_live.position_correction.len() == 3 * live.bodies.len()
                    && saved_live.position_correction.iter().all(|v| v.is_finite()),
                "coupled checkpoint: invalid compensation"
            );
            live.state = saved_live.state;
            live.origin_correction = saved_live.origin_correction;
            live.position_correction = saved_live.position_correction;
        }
        world.latest = saved.latest;
        world.steps = saved.steps;
        world.samples = saved.samples;
        world
    }
}
