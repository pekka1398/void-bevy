/**
 * Golden data for crates/sas, from lab/sas's StabilityAssist.ts. Each step's input (the attitude the
 * controller saw) and output are recorded, so the Rust controller is checked on exactly the lab's
 * inputs, apart from the integrator (whose sin and cos are V8's).
 * Run from the repository root: npx tsx lab/void-bevy/golden/sas.ts
 */
import { writeFileSync } from 'node:fs';
import type { Vec3 } from '../../landing/src/orbitCore';
import type { Quaternion } from '../../landing/src/physics/ContactWorld';
import { stepAttitude, type Mat3 } from '../../landing/src/vessel/Attitude';
import { STEERING_TORQUE } from '../../landing/src/vessel/PartJointRocket';
import { attitudeError, StabilityAssist, type SasTuning } from '../../sas/src/StabilityAssist';

let seed = 77;
const rand = () => { seed = (seed * 1103515245 + 12345) % 2147483648; return seed / 2147483648 * 2 - 1; };
const unitQuat = (): Quaternion => {
  const q = { x: rand(), y: rand(), z: rand(), w: rand() };
  const l = Math.hypot(q.x, q.y, q.z, q.w);
  return { x: q.x / l, y: q.y / l, z: q.z / l, w: q.w / l };
};
const q4 = (q: Quaternion) => [q.x, q.y, q.z, q.w];
const v3 = (v: Vec3) => [v.x, v.y, v.z];

const errors = Array.from({ length: 200 }, (_, i) => {
  const a = unitQuat(), b = i % 10 === 0 ? a : unitQuat();
  return { target: q4(a), current: q4(b), error: v3(attitudeError(a, b)) };
});

const DT = 1 / 60;
// A stack-like inertia with small products of inertia.
const inertia: Mat3 = [9100, 40, -25, 40, 2300, 15, -25, 15, 9050];
interface Phase { seconds: number; pilot: Vec3; kick?: Vec3; enable?: boolean }
function run(tuning: SasTuning, start: Quaternion, spin: Vec3, phases: Phase[]) {
  const sas = new StabilityAssist(STEERING_TORQUE, tuning);
  let rotation = start, angularVelocity = spin;
  const steps: unknown[] = [];
  for (const phase of phases) {
    if (phase.enable !== undefined) sas.setEnabled(phase.enable);
    if (phase.kick) angularVelocity = { x: angularVelocity.x + phase.kick.x, y: angularVelocity.y + phase.kick.y, z: angularVelocity.z + phase.kick.z };
    for (let k = 0; k < Math.round(phase.seconds / DT); k += 1) {
      const u = sas.command({ rotation, angularVelocity, inertiaLocal: inertia }, phase.pilot, DT);
      steps.push({ rotation: q4(rotation), angularVelocity: v3(angularVelocity), pilot: v3(phase.pilot), command: v3(u), phase: sas.phase,
        target: sas.target ? q4(sas.target) : null, reset: k === 0 && phase.enable !== undefined ? phase.enable : null });
      const next = stepAttitude(rotation, angularVelocity, inertia, { x: u.x * STEERING_TORQUE, y: u.y * STEERING_TORQUE, z: u.z * STEERING_TORQUE }, DT);
      rotation = next.rotation;
      angularVelocity = next.angularVelocity;
    }
  }
  return { tuning, steps };
}

const ZERO = { x: 0, y: 0, z: 0 };
const tilted = { x: 0.12, y: -0.3, z: 0.2, w: 0.92 };
const l = Math.hypot(tilted.x, tilted.y, tilted.z, tilted.w);
const start = { x: tilted.x / l, y: tilted.y / l, z: tilted.z / l, w: tilted.w / l };
const runs = [
  run({ rateSeconds: 0.15, attitudeSeconds: 0.6, brakeFraction: 0.5, lockRate: 0.002 }, start, ZERO, [
    { seconds: 0.5, pilot: ZERO, enable: true },
    { seconds: 8, pilot: ZERO, kick: { x: 0.12, y: -0.1, z: 0.13 } },
    { seconds: 1, pilot: { x: 1, y: 0, z: 0 }, kick: { x: 0, y: 0.05, z: 0.05 } },
    { seconds: 0.5, pilot: { x: 0, y: -0.5, z: 1 } },
    { seconds: 8, pilot: ZERO },
    { seconds: 1, pilot: { x: 0.3, y: -1, z: 0 }, enable: false },
  ]),
  run({ rateSeconds: 0.2, attitudeSeconds: 1.1, brakeFraction: 0.8, lockRate: 1e-3 }, start, { x: 0.3, y: 0.2, z: -0.1 }, [
    { seconds: 10, pilot: ZERO, enable: true },
    { seconds: 6, pilot: ZERO, kick: { x: -0.6, y: 0.4, z: 0.7 } },
  ]),
];

const out = new URL('../crates/sas/tests/golden/sas.json', import.meta.url);
writeFileSync(out, `${JSON.stringify({ maxTorque: STEERING_TORQUE, inertia: [...inertia], dt: DT, errors, runs })}\n`);
console.log(`wrote ${out.pathname}: ${errors.length} errors, runs of ${runs.map((r) => r.steps.length).join(', ')} steps`);
