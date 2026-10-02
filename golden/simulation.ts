/**
 * Golden data for crates/orbit's Simulation, from the orbit lab's Simulation.ts, with the view lab's
 * setup (Sol, a 100 km equatorial orbit of Aurelia, the orbit lab's chemical stage). A scripted
 * session: coast, manual burns in several attitude modes, hold, warp, a planned burn flown
 * automatically, and finally a retrograde burn down to an impact.
 * Run from the repository root: npx tsx lab/void-bevy/golden/simulation.ts
 */
import { writeFileSync } from 'node:fs';
import { SYSTEM_PRESETS } from '../../orbit/src/app/SystemPresets';
import { Simulation, type AttitudeMode } from '../../orbit/src/orbit/Simulation';

const sim = new Simulation({
  system: SYSTEM_PRESETS.sol,
  stepsPerOrbit: 256,
  tolerances: { positionMeters: 1e-4, velocityMetersPerSecond: 1e-7 },
  vesselStart: { homeBodyId: 'aurelia', altitudeMeters: 100e3, plane: { kind: 'equatorial', inclinationRadians: 0 } },
  engine: { thrustNewtons: 250e3, specificImpulseSeconds: 350, dryMassKg: 10e3, fuelMassKg: 30e3 },
  retentionSeconds: 86_400,
  predictionHorizonSeconds: 3 * 3600,
  planCoastSeconds: 86_400,
});
const v3 = (v: { x: number; y: number; z: number }) => [v.x, v.y, v.z];
const checkpoints: unknown[] = [];
function record(label: string): void {
  const s = sim.vessel;
  const p = sim.prediction;
  checkpoints.push({
    label, time: sim.time, position: v3(s.position), velocity: v3(s.velocity), massKg: s.massKg,
    reference: sim.navigationReference(), effectiveThrottle: sim.effectiveThrottle, attitude: sim.attitudeMode,
    thrust: v3(sim.thrustDirection()), generation: sim.predictionGeneration,
    prediction: p.count > 0 ? { count: p.count, lastTime: p.lastTime, last: v3(p.sample(p.lastTime).position) } : null,
    impact: sim.impact ? { body: sim.impact.bodyIndex, time: sim.impact.time, at: v3(sim.impact.bodyFixedPosition) } : null,
  });
}
/** Frames of 1/60 s real time at a warp, as the page advances. */
function frames(count: number, warp: number): void {
  for (let i = 0; i < count; i += 1) {
    sim.advance(warp / 60, 20_000);
    sim.extendPrediction(4_000);
  }
}
const burn = (mode: AttitudeMode, throttle: number, count: number) => { sim.setAttitude(mode); sim.throttle = throttle; frames(count, 1); sim.throttle = 0; };

record('start');
frames(120, 100); record('coast 200 s at 100x');
burn('prograde', 1, 300); record('prograde 5 s');
burn('normal', 0.5, 120); record('normal 2 s at half');
burn('radial-out', 1, 60); record('radial-out 1 s');
sim.setAttitude('hold'); sim.throttle = 0.3; frames(120, 1); sim.throttle = 0; record('hold 2 s');
frames(300, 1000); record('coast 5000 s at 1000x');
const aurelia = sim.bodyIndex('aurelia');
sim.addManeuver({ startTime: sim.time + 600, referenceBody: aurelia, referenceMode: 'auto', prograde: 40, normal: 0, radial: 5 });
for (let i = 0; i < 50; i += 1) sim.extendPlan(4_000);
record('planned');
frames(120, 10); record('before the planned burn');
burn('retrograde', 1, 900); record('retrograde 15 s');
frames(600, 10); record('falling');
frames(300, 100); record('planned burn flown, coasting down');
frames(300, 1000); record('after impact');

const out = new URL('../crates/orbit/tests/golden/simulation.json', import.meta.url);
writeFileSync(out, `${JSON.stringify({ checkpoints })}\n`);
console.log(`wrote ${out.pathname}: ${checkpoints.length} checkpoints; impact ${JSON.stringify(sim.impact)}`);
