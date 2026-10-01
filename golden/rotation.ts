/**
 * Golden data for crates/rotation, from lab/rotation's RotatingFrame.ts.
 * Run from the repository root: npx tsx lab/void-bevy/golden/rotation.ts
 */
import { writeFileSync } from 'node:fs';
import { fictitiousTorque, freeRotationStep, inertiaIn, rotationStep, type Mat3, type Quaternion, type Vec3 } from '../../rotation/src/RotatingFrame';

let seed = 2024;
const rand = () => { seed = (seed * 1103515245 + 12345) % 2147483648; return seed / 2147483648 * 2 - 1; };
const vec = (scale: number): Vec3 => ({ x: rand() * scale, y: rand() * scale, z: rand() * scale });
const unitQuat = (): Quaternion => {
  const q = { x: rand(), y: rand(), z: rand(), w: rand() };
  const l = Math.hypot(q.x, q.y, q.z, q.w);
  return { x: q.x / l, y: q.y / l, z: q.z / l, w: q.w / l };
};
/** A symmetric positive-definite inertia: a diagonal turned by a random rotation. */
const inertia = (): Mat3 => inertiaIn(unitQuat(), [1 + Math.abs(rand()) * 4, 0, 0, 0, 2 + Math.abs(rand()) * 4, 0, 0, 0, 3 + Math.abs(rand()) * 4]);
const q3 = (v: Vec3) => [v.x, v.y, v.z];
const q4 = (q: Quaternion) => [q.x, q.y, q.z, q.w];

// Earth's spin and a fast planet's, about z and about a tilted axis.
const spins: Vec3[] = [{ x: 0, y: 0, z: 7.292e-5 }, { x: 0, y: 0, z: 7.27e-4 }, { x: 1e-4, y: -3e-4, z: 5e-4 }, { x: 0, y: 0, z: 0 }];
const cases = Array.from({ length: 40 }, (_, i) => {
  const spin = spins[i % spins.length]!;
  const I = inertia(), q = unitQuat(), w = vec(i % 3 === 0 ? 2 : 0.05), tau = vec(i % 2 ? 50 : 0), dt = [1 / 60, 0.02, 1][i % 3]!;
  const free = freeRotationStep(q, w, I, spin, dt);
  const step = rotationStep(q, w, I, tau, spin, dt);
  return {
    inertia: [...I], rotation: q4(q), angularVelocity: q3(w), torque: q3(tau), spin: q3(spin), dt,
    torqueOut: q3(fictitiousTorque(inertiaIn(q, I), w, spin)),
    inertiaIn: [...inertiaIn(q, I)],
    free: { rotation: q4(free.rotation), angularVelocity: q3(free.angularVelocity) },
    step: { rotation: q4(step.rotation), angularVelocity: q3(step.angularVelocity) },
  };
});

// A long tumble: 10 minutes at 60 Hz in Earth's frame, with a torque in the first minute.
const I: Mat3 = [2, 0.1, 0, 0.1, 3, -0.2, 0, -0.2, 4];
let state = { rotation: { x: 0.1, y: 0.2, z: 0.3, w: Math.sqrt(1 - 0.14) }, angularVelocity: { x: 0.3, y: -0.7, z: 1.1 } };
const trace: { t: number; rotation: number[]; angularVelocity: number[] }[] = [];
for (let n = 1; n <= 36_000; n++) {
  const torque = n <= 3600 ? { x: 0.4, y: 0, z: -0.2 } : { x: 0, y: 0, z: 0 };
  state = rotationStep(state.rotation, state.angularVelocity, I, torque, spins[0]!, 1 / 60);
  if (n % 3600 === 0) trace.push({ t: n / 60, rotation: q4(state.rotation), angularVelocity: q3(state.angularVelocity) });
}

const out = new URL('../crates/rotation/tests/golden/rotation.json', import.meta.url);
writeFileSync(out, `${JSON.stringify({ cases, tumble: { inertia: [...I], trace } })}\n`);
console.log(`wrote ${out.pathname}: ${cases.length} cases, ${trace.length} tumble samples`);
