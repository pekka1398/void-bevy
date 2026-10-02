/**
 * Golden data for crates/navball, from lab/navball's Navball.ts: bases, ball points, heading and
 * pitch over random attitudes and latitudes (including both poles), and horizon directions.
 * Run from the repository root: npx tsx lab/void-bevy/golden/navball.ts
 */
import { writeFileSync } from 'node:fs';
import { headingPitch, horizonDirection, navballBasis, toBall, type Vec3 } from '../../navball/src/Navball';

let seed = 4242;
const rand = () => { seed = (seed * 1103515245 + 12345) % 2147483648; return seed / 2147483648 * 2 - 1; };
const normalize = (a: Vec3): Vec3 => { const l = Math.hypot(a.x, a.y, a.z); return { x: a.x / l, y: a.y / l, z: a.z / l }; };
const cross = (a: Vec3, b: Vec3): Vec3 => ({ x: a.y * b.z - a.z * b.y, y: a.z * b.x - a.x * b.z, z: a.x * b.y - a.y * b.x });
const v3 = (v: Vec3) => [v.x, v.y, v.z];
const pole = { x: 0, y: 0, z: 1 }, primeMeridian = { x: 1, y: 0, z: 0 };

const cases = Array.from({ length: 300 }, (_, i) => {
  const latitude = i % 25 === 0 ? 90 : i % 25 === 1 ? -90 : rand() * 90, longitude = rand() * 180;
  const d = Math.PI / 180;
  const up = i % 25 < 2 ? { x: 0, y: 0, z: Math.sign(latitude) }
    : { x: Math.cos(latitude * d) * Math.cos(longitude * d), y: Math.cos(latitude * d) * Math.sin(longitude * d), z: Math.sin(latitude * d) };
  const nose = normalize({ x: rand(), y: rand(), z: rand() });
  const top = normalize(cross(cross(nose, { x: rand(), y: rand(), z: rand() }), nose));
  const basis = navballBasis({ nose, top, up, pole, primeMeridian, velocity: { x: 0, y: 0, z: 0 } });
  const direction = normalize({ x: rand(), y: rand(), z: rand() });
  const heading = rand() * 400 - 20, pitch = rand() * 90;
  return {
    nose: v3(nose), top: v3(top), up: v3(up),
    basis: [basis.right, basis.top, basis.nose, basis.up, basis.north, basis.east].map(v3),
    direction: v3(direction), ball: v3(toBall(basis, direction)), headingPitch: headingPitch(basis, direction),
    heading, pitch, horizon: v3(horizonDirection(basis, heading, pitch)),
  };
});

const out = new URL('../crates/navball/tests/golden/navball.json', import.meta.url);
writeFileSync(out, `${JSON.stringify({ pole: v3(pole), primeMeridian: v3(primeMeridian), cases })}\n`);
console.log(`wrote ${out.pathname}: ${cases.length} cases`);
