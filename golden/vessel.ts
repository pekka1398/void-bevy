/**
 * Golden data for crates/orbit's vessel propagator, apsides, dominance and flight plan.
 * Run from this workspace: python3 tools/regenerate-golden.py --reference-root ../void vessel
 */
import { writeFileSync } from 'node:fs';
import { SYSTEM_PRESETS } from '../../orbit/src/app/SystemPresets';
import { findApsides } from '../../orbit/src/orbit/Apsides';
import { DominanceTree } from '../../orbit/src/orbit/Dominance';
import { Ephemeris, suggestedStepSeconds } from '../../orbit/src/orbit/Ephemeris';
import { FlightPlan, type ManeuverSpec } from '../../orbit/src/orbit/FlightPlan';
import { buildSystem } from '../../orbit/src/orbit/SystemSpec';
import { Trajectory } from '../../orbit/src/orbit/Trajectory';
import {
  PropagationRun, VesselPropagator, type PropagationControl, type VesselState,
} from '../../orbit/src/orbit/VesselPropagator';

const TOLERANCES = { positionMeters: 1e-4, velocityMetersPerSecond: 1e-7 };
const ENGINE = { thrustNewtons: 250e3, exhaustVelocity: 350 * 9.80665, dryMassKg: 10e3 };
const T0 = 1000;
const DAY = 86_400;

const system = buildSystem(SYSTEM_PRESETS.sol);
const ephemeris = new Ephemeris(system, { stepSeconds: suggestedStepSeconds(system.bodies, 256), chunkSteps: 2048 });
const home = system.bodies.findIndex((b) => b.id === 'aurelia');
const moon = system.bodies.findIndex((b) => b.id === 'selene');
ephemeris.extendTo(T0);
const center = ephemeris.bodyState(home, T0);
const body = system.bodies[home]!;

const v3 = (v: { x: number; y: number; z: number }) => [v.x, v.y, v.z];
const add = (a: { x: number; y: number; z: number }, b: number[]) => ({ x: a.x + b[0]!, y: a.y + b[1]!, z: a.z + b[2]! });

/** A 400 km orbit inclined 0.3 rad from the ecliptic, relative to Aurelia. */
const r = body.radiusMeters + 400e3;
const speed = Math.sqrt(body.gm / r);
const start: VesselState = {
  time: T0,
  position: add(center.position, [r, 0, 0]),
  velocity: add(center.velocity, [0, speed * Math.cos(0.3), speed * Math.sin(0.3)]),
  massKg: 40e3,
};
const stateOf = (run: PropagationRun) => ({ time: run.time, y: Array.from(run.y), impact: run.impact });

function leg(name: string, from: VesselState, duration: number, control: PropagationControl | null) {
  const propagator = new VesselPropagator(ephemeris, TOLERANCES);
  const run = new PropagationRun(from);
  const trajectory = new Trajectory();
  trajectory.append(run.time, run.y);
  const outcome = propagator.advance(run, from.time + duration, 1_000_000, trajectory, control);
  const mid = 0.5 * (trajectory.firstTime + trajectory.lastTime);
  const sample = trajectory.sample(mid);
  return {
    name, from, duration, control, outcome,
    end: stateOf(run),
    accepted: propagator.acceptedSteps, rejected: propagator.rejectedSteps, samples: trajectory.count,
    mid: { t: mid, position: v3(sample.position), velocity: v3(sample.velocity) },
    apsides: findApsides(trajectory, ephemeris, home, from.time, 8).map((a) => ({ ...a, position: v3(a.position) })),
  };
}

const legs = [
  leg('coast', start, 2 * DAY, null),
  leg('frenet', start, 300, { ...ENGINE, minimumMassKg: ENGINE.dryMassKg, attitude: { kind: 'frenet', referenceBody: home, tangent: 0.8, normal: 0.36, radial: -0.48 } }),
  leg('surface', start, 120, { ...ENGINE, minimumMassKg: ENGINE.dryMassKg, attitude: { kind: 'surface', referenceBody: home, up: 1, prograde: -0.3 } }),
  leg('inertial', start, 200, { ...ENGINE, minimumMassKg: ENGINE.dryMassKg, attitude: { kind: 'inertial', direction: { x: 0.3 / Math.sqrt(0.98), y: -0.5 / Math.sqrt(0.98), z: 0.8 / Math.sqrt(0.98) } } }),
  leg('force', start, 600, { force: { x: 1000, y: -2000, z: 500 }, massFlowKgPerSecond: 0.5, minimumMassKg: ENGINE.dryMassKg }),
  // Low, slow and falling: hits Aurelia.
  leg('impact', {
    time: T0,
    position: add(center.position, [body.radiusMeters + 300e3, 0, 0]),
    velocity: add(center.velocity, [-1000, 0.3 * speed, 0]),
    massKg: 40e3,
  }, DAY, null),
];

// Flight plan: one burn, then apsis placement, then a second burn and a blocked third.
const plan = new FlightPlan(ephemeris, TOLERANCES, ENGINE, DAY);
plan.rebase(new PropagationRun(start));
const burn = (startTime: number, prograde: number, normal: number, radial: number): ManeuverSpec =>
  ({ startTime, referenceBody: home, referenceMode: 'fixed', prograde, normal, radial });
plan.add(burn(T0 + 1800, 800, 50, -20));
const apoapsisStart = plan.startAtApsis(0, 'apoapsis', T0);
const periapsisStart = plan.startAtApsis(0, 'periapsis', T0);
plan.add(burn(T0 + 4 * 3600, -300, 0, 0));
const secondPeriapsis = plan.startAtApsis(1, 'periapsis', T0);
plan.add(burn(T0 + 4 * 3600 + 10, 10, 0, 0));
while (!plan.complete) plan.extend(5000);
const at = T0 + 4 * 3600;
const planPosition = plan.positionAt(at);
const statuses = Array.from({ length: plan.count }, (_, i) => plan.status(i));
const last = plan.trajectory.count - 1;

const dominance = new DominanceTree(system.bodies);
const positions = new Float64Array(system.bodies.length * 3);
ephemeris.positionsAt(T0, positions);
const at3 = (i: number) => ({ x: positions[i * 3]!, y: positions[i * 3 + 1]!, z: positions[i * 3 + 2]! });
const points = [start.position, add(at3(moon), [2e6, 0, 0]), add(at3(home), [3e9, 0, 0]), { x: 0, y: 0, z: 0 }];

const out = new URL('../crates/orbit/tests/golden/vessel.json', import.meta.url);
writeFileSync(out, `${JSON.stringify({
  tolerances: TOLERANCES,
  engine: ENGINE,
  home,
  moon,
  start: { time: start.time, position: v3(start.position), velocity: v3(start.velocity), massKg: start.massKg },
  legs: legs.map((l) => ({ ...l, from: { time: l.from.time, position: v3(l.from.position), velocity: v3(l.from.velocity), massKg: l.from.massKg } })),
  plan: {
    coast: DAY,
    apoapsisStart, periapsisStart, secondPeriapsis,
    statuses,
    end: { time: plan.trajectory.time(last), position: v3(plan.trajectory.position(last)), velocity: v3(plan.trajectory.velocity(last)) },
    samples: plan.trajectory.count,
    positionAt: { t: at, position: planPosition ? v3(planPosition) : null },
    impact: plan.impact,
  },
  dominance: { time: T0, points: points.map(v3), dominant: points.map((p) => dominance.dominant(positions, p)) },
}, null, 1)}\n`);
console.log(`wrote ${out.pathname}`);
for (const l of legs) console.log(`  ${l.name}: ${l.outcome.kind}, ${l.accepted} accepted, ${l.rejected} rejected, ${l.apsides.length} apsides`);
console.log(`  plan: ${statuses.map((s) => (s.ok ? 'ok' : s.reason)).join(' / ')}; apoapsis ${JSON.stringify(apoapsisStart)}, periapsis ${JSON.stringify(periapsisStart)}`);
