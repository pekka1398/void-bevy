/**
 * Golden data for crates/landing's PlanetFrame, from lab/landing.
 * Run from this workspace: python3 tools/regenerate-golden.py --reference-root ../void planet_frame
 */
import { writeFileSync } from 'node:fs';
import { PlanetFrame } from '../../landing/src/physics/PlanetFrame';
import { aurelia, pebble, planetEphemeris } from '../../landing/src/planet/Planets';
import type { Vec3 } from '../../landing/src/orbitCore';

let seed = 99;
const rand = () => { seed = (seed * 1103515245 + 12345) % 2147483648; return seed / 2147483648 * 2 - 1; };
const v3 = (v: Vec3) => [v.x, v.y, v.z];

const planets = [['aurelia', aurelia()], ['pebble', pebble()]] as const;
const out = planets.map(([id, planet]) => {
  const { ephemeris, bodyIndex } = planetEphemeris(planet);
  const frame = new PlanetFrame(ephemeris, bodyIndex);
  const R = planet.terrain.radiusMeters;
  const times = [0, 1234.5, 86_400 * 3 + 0.25];
  ephemeris.extendTo(times[times.length - 1]!);
  const cases = times.flatMap((t) => Array.from({ length: 8 }, () => {
    const position = { x: rand() * 1.3 * R, y: rand() * 1.3 * R, z: rand() * 1.3 * R };
    const velocity = { x: rand() * 3000, y: rand() * 3000, z: rand() * 3000 };
    const inertial = frame.toInertial(t, { position, velocity });
    const back = frame.toBodyFixed(t, inertial);
    return { t, position: v3(position), velocity: v3(velocity), inertial: { position: v3(inertial.position), velocity: v3(inertial.velocity) },
      back: { position: v3(back.position), velocity: v3(back.velocity) }, acceleration: v3(frame.acceleration(t, position, velocity)) };
  }));
  return { id, system: planet.system, bodyId: planet.bodyId, stepSeconds: ephemeris.stepSeconds, omega: frame.omega, cases };
});

const file = new URL('../crates/landing/tests/golden/planet_frame.json', import.meta.url);
writeFileSync(file, `${JSON.stringify(out)}\n`);
console.log(`wrote ${file.pathname}: ${out.map((p) => `${p.id} ${p.cases.length} cases`).join(', ')}`);
