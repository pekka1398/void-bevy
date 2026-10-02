/**
 * Golden data for crates/multiscale, from lab/multiscale: split-position arithmetic (bigint cells
 * as decimal strings), the coupled world's states (compact three-system fixture and the wide
 * light-year fixture), and the coasting probe's flights with their frame hand-offs.
 * Run from this workspace: python3 tools/regenerate-golden.py --reference-root ../void multiscale
 */
import { writeFileSync } from 'node:fs';
import { buildSystem, YEAR, type BuiltSystem, type SystemSpec, type Vec3 } from '../../multiscale/src/cores';
import { position, translate, compose, difference, relative, drift, serialize, type SplitPosition } from '../../multiscale/src/SplitPosition';
import { CoupledWorld, type SystemSeed, type SystemState } from '../../multiscale/src/CoupledWorld';
import { absolute, reframe } from '../../multiscale/src/Frames';
import { Traveller } from '../../multiscale/src/Traveller';
import { wideWorld, transfer } from '../../multiscale/src/Fixtures';

let seed = 99;
const rand = () => { seed = (seed * 1103515245 + 12345) % 2147483648; return seed / 2147483648 * 2 - 1; };
const v3 = (v: Vec3) => [v.x, v.y, v.z];
const split = (p: SplitPosition) => ({ cell: [p.cell.x, p.cell.y, p.cell.z].map(String), offset: v3(p.offset) });
const big = (scale: number) => BigInt(Math.round(rand() * scale)) * 10n ** 12n + BigInt(Math.round(rand() * 1e12));
const randomSplit = (): SplitPosition => position({ x: rand() * 3e9, y: rand() * 3e9, z: rand() * 3e9 },
  { x: big(1e12), y: big(1e12), z: big(1e3) });
const randomVec = (scale: number): Vec3 => ({ x: rand() * scale, y: rand() * scale, z: rand() * scale });

const splits = Array.from({ length: 200 }, (_, i) => {
  const a = randomSplit(), b = i % 3 === 0 ? translate(a, randomVec(1e7)) : randomSplit(), d = randomVec(i % 2 ? 1e10 : 3);
  const near = translate(a, randomVec(1e5));
  const before = randomVec(1e-9), correction = { ...before }, drifted = drift(a, d, correction);
  return { a: split(a), b: split(b), delta: v3(d), raw: v3(randomVec(1e13)),
    translate: split(translate(a, d)), compose: split(compose(a, b)), difference: split(difference(a, b)),
    near: split(near), relative: v3(relative(near, a)), drift: split(drifted), before: v3(before), correction: v3(correction), text: serialize(a) };
});
// Carry at the raw constructor: offsets far outside a cell.
const raw = splits.map(s => split(position({ x: s.raw[0]!, y: s.raw[1]!, z: s.raw[2]! })));

const states = (gs: readonly SystemState[]) => gs.map(g => ({ origin: split(g.origin), velocity: v3(g.velocity), acceleration: v3(g.acceleration),
  positions: [...g.positions], velocities: [...g.velocities], accelerations: [...g.accelerations] }));

function smallSystem(id: string, mass = 1e25): BuiltSystem {
  const rotation = { periodSeconds: 86400, obliquityRadians: 0, poleLongitudeRadians: 0, angleAtEpochRadians: 0 };
  const spec: SystemSpec = { name: id, root: { id: 'star', name: 'star', massKg: mass, radiusMeters: 1e6, color: '#fff', rotation,
    children: [{ id: 'planet', name: 'planet', massKg: mass * 1e-4, radiusMeters: 1e5, color: '#aaf', rotation, orbitPlane: 'ecliptic',
      orbit: { semiMajorAxisMeters: 2e8, eccentricity: 0.1, inclinationRadians: 0.2, longitudeOfAscendingNodeRadians: 0.3, argumentOfPeriapsisRadians: 0.4, meanAnomalyRadians: 0.5 }, children: [] }] } };
  return buildSystem(spec);
}
const huge = position({ x: 123.125, y: -9.25, z: 7 }, { x: 10n ** 24n, y: -(10n ** 23n), z: 10n ** 22n });
function compactSeeds(anchor = position()): SystemSeed[] {
  return [{ id: 'A', system: smallSystem('A'), origin: compose(anchor, position()), velocity: { x: 20, y: 30, z: 0 } },
    { id: 'B', system: smallSystem('B', 8e24), origin: compose(anchor, position({ x: 4e9, y: 8e8, z: 0 })), velocity: { x: -30, y: 10, z: 5 } },
    { id: 'C', system: smallSystem('C', 1.2e25), origin: compose(anchor, position({ x: -5e9, y: 3e9, z: 1e9 })), velocity: { x: 5, y: -10, z: 0 } }];
}
const compact = [position(), huge].map(anchor => {
  const world = new CoupledWorld(compactSeeds(anchor), 10);
  world.extendTo(2000);
  return { samples: [0, 10, 123.25, 999.9, 2000].map(t => ({ t, systems: states(world.at(t)),
    gravity: v3(world.gravityAt(t, translate(world.at(t)[0]!.origin, { x: 8e8, y: 1e9, z: 2e8 }))) })) };
});

const wide = wideWorld();
wide.extendTo(10 * YEAR);
const wideSamples = [0, 0.5 * 86400, YEAR, 10 * YEAR].map(t => ({ t, systems: states(wide.at(t)) }));

const frameState = { frame: 'Aster', position: position({ x: 1.125, y: 0.01, z: -2.25 }), velocity: { x: 1.5, y: -0.25, z: 0.125 } };
let next = frameState;
const reframed = [];
for (let i = 0; i < 5; i++) {
  next = reframe(wide, 50, next, i % 2 ? 'Aster' : 'Beryl');
  reframed.push({ frame: next.frame, position: split(next.position), velocity: v3(next.velocity), absolute: split(absolute(wide, 50, next).position) });
}

const flightRecord = (f: Traveller) => ({ time: f.time, steps: f.steps, frame: f.state.frame, position: split(f.state.position), velocity: v3(f.state.velocity),
  terminal: f.terminal, events: f.events.map(e => ({ ...e })) });
const compactWorld = new CoupledWorld(compactSeeds(), 10);
const compactFlight = new Traveller(compactWorld, 0, { frame: 'A', position: position({ x: 8e8, y: 2e9, z: 0 }), velocity: { x: 500_000, y: 0, z: 0 } }, 100);
const compactLegs = [1000, 4500, 9000].map(t => { compactFlight.advanceTo(t, 10_000); return flightRecord(compactFlight); });

const longWorld = wideWorld(), longFlight = transfer(longWorld);
const longLegs = [YEAR, 102 * YEAR, 210 * YEAR].map(t => { longFlight.advanceTo(t, 100_000); return flightRecord(longFlight); });

writeFileSync(new URL('../crates/multiscale/tests/golden/multiscale.json', import.meta.url), JSON.stringify({
  splits, raw, compact, wide: wideSamples, reframed, compactFlight: compactLegs, longFlight: { legs: longLegs, worldSteps: longWorld.steps },
}) + '\n');
console.log(`multiscale: ${splits.length} split cases; compact flight ${compactFlight.steps} steps; long flight ${longFlight.steps} steps, ${longWorld.steps} world steps, frame ${longFlight.state.frame}`);
