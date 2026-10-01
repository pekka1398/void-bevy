/**
 * Golden data for crates/frames, from the orbit lab's own frame code.
 * Run from the repository root: npx tsx lab/void-bevy/golden/frames.ts
 */
import { writeFileSync } from 'node:fs';
import { bodyOrientation, equatorialAxes } from '../../orbit/src/orbit/BodyRotation';
import { toFrame } from '../../orbit/src/orbit/ReferenceFrames';
import type { CelestialBody } from '../../orbit/src/orbit/SystemSpec';

const spins = [
  { periodSeconds: 86_164.1, obliquityRadians: 0.4091, poleLongitudeRadians: -Math.PI / 2, angleAtEpochRadians: 1.2 },
  { periodSeconds: 2_360_591.5, obliquityRadians: 0, poleLongitudeRadians: 0, angleAtEpochRadians: 0 },
  { periodSeconds: 20_997_360, obliquityRadians: 3.0960, poleLongitudeRadians: 1.3, angleAtEpochRadians: -0.4 },
  { periodSeconds: 8_616.4, obliquityRadians: 1.2, poleLongitudeRadians: 2.9, angleAtEpochRadians: 5.9 },
];
// Times stay small enough that t / period loses nothing the exact remainder would keep.
const times = [0, 1, 3_600.5, 86_400, 1.0e6, 3.3e6];
const points = [
  { x: 1.496e11, y: -2.2e9, z: 4.1e7 },
  { x: -6.371e6, y: 1.0, z: 3.0e6 },
];
const origin = { x: 1.4959e11, y: -2.21e9, z: 4.0e7 };

const asBody = (rotation: (typeof spins)[number]) => ({ rotation }) as unknown as CelestialBody;
const basis = (b: { x: { x: number; y: number; z: number }; y: typeof b.x; z: typeof b.x }) =>
  [b.x, b.y, b.z].map((v) => [v.x, v.y, v.z]);

const cases = spins.map((spin) => {
  const body = asBody(spin);
  return {
    spin,
    equatorial: basis(equatorialAxes(body)),
    orientations: times.map((t) => {
      const axes = bodyOrientation(body, t);
      return { t, axes: basis(axes), toFrame: points.map((p) => { const q = toFrame({ origin, axes }, p); return [q.x, q.y, q.z]; }) };
    }),
  };
});

const out = new URL('../crates/frames/tests/golden/frames.json', import.meta.url);
writeFileSync(out, `${JSON.stringify({ origin: [origin.x, origin.y, origin.z], points: points.map((p) => [p.x, p.y, p.z]), cases }, null, 1)}\n`);
console.log(`wrote ${out.pathname}: ${cases.length} spins × ${times.length} times`);
