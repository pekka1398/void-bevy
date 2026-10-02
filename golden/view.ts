/**
 * Golden data for crates/view, from lab/view: view states over zooms, focuses and modes; camera
 * drags, zooms and co-rotation; osculating ellipses by angle and by time; orbits in a surface frame;
 * a path frame's samples through PathCache.
 * Run from this workspace: python3 tools/regenerate-golden.py --reference-root ../void view
 */
import { writeFileSync } from 'node:fs';
import { Simulation, normalize } from '../../orbit/src/orbit';
import { PathCache, SYSTEM_PRESETS } from '../../view/src/orbitCore';
import { ellipsePoints, ellipsePointsInTime } from '../../view/src/ConicPath';
import { orbitInSurfaceFrame, PathFrame, type PathFrameKind } from '../../view/src/PathFrame';
import { cameraSpin, OrbitCamera, viewState, type FocusGeometry, type ViewState } from '../../view/src/ViewCamera';

type V = { x: number; y: number; z: number };
const v3 = (v: V) => [v.x, v.y, v.z];
const triples = (a: Float64Array) => Array.from(a);
let seed = 99;
const rand = () => { seed = (seed * 1103515245 + 12345) % 2147483648; return seed / 2147483648 * 2 - 1; };
const unit = (): V => normalize({ x: rand(), y: rand(), z: rand() });
const state = (s: ViewState) => ({ mapWeight: s.mapWeight, upWeight: s.upWeight, corotation: s.corotation, up: v3(s.up), minDistance: s.minDistance, maxDistance: s.maxDistance });

const R = 6.371e6;
const views: unknown[] = [];
for (let i = 0; i < 400; i += 1) {
  const north = unit(), radial = unit();
  const body = i % 4 === 3;
  const focus: FocusGeometry = body
    ? { kind: 'body', radial: null, north, referenceRadius: R * (0.2 + Math.abs(rand())), altitude: 0, focusRadius: R * (0.2 + Math.abs(rand())) }
    : { kind: 'vessel', radial, north, referenceRadius: R * (0.2 + Math.abs(rand())), altitude: Math.abs(rand()) * 200e3, focusRadius: 0 };
  const distance = Math.exp(Math.log(1) + (Math.abs(rand()) * Math.log(1e12)));
  const mode = i % 3 === 0 ? 'split' : 'single';
  const mapOn = mode === 'split' && (body || i % 2 === 0);
  const s = viewState(mode, mapOn, focus, distance);
  const spin = (['inertial', 'surface'] as const).map((k) => cameraSpin(s, k, i % 5, i % 7 === 0 ? i % 5 : 2));
  views.push({ focus: { ...focus, radial: focus.radial ? v3(focus.radial) : null, north: v3(north) }, distance, mode, mapOn, state: state(s), spin, focusReference: i % 5, pathReference: i % 7 === 0 ? i % 5 : 2 });
}

const cameras: unknown[] = [];
for (let i = 0; i < 40; i += 1) {
  const camera = new OrbitCamera(unit(), 10 + Math.abs(rand()) * 1e6);
  const start = { direction: v3(camera.direction), distance: camera.distance };
  const up = unit();
  const steps: unknown[] = [];
  for (let k = 0; k < 30; k += 1) {
    const what = k % 4;
    const a = rand(), b = rand();
    if (what === 0) camera.drag(a * 400, b * 900, up);
    else if (what === 1) camera.zoom(Math.exp(a * 2), 8, 2e13);
    else if (what === 2) camera.corotate(up, a * 0.3);
    else camera.clampToUp(k % 8 === 3 ? { x: -up.x, y: -up.y, z: -up.z } : up);
    steps.push({ a, b, direction: v3(camera.direction), distance: camera.distance });
  }
  cameras.push({ start, up: v3(up), steps });
}

const gm = 3.986e14;
const ellipses = Array.from({ length: 30 }, () => {
  const r = { x: 7e6 + rand() * 2e6, y: rand() * 3e6, z: rand() * 1e6 };
  const v = { x: rand() * 2e3, y: 7.4e3 + rand() * 1e3, z: rand() * 1.5e3 };
  const n = 3 + Math.floor(Math.abs(rand()) * 200);
  const inTime = ellipsePointsInTime(r, v, gm, n);
  return { r: v3(r), v: v3(v), n, byAngle: triples(ellipsePoints(r, v, gm, n)), byTime: triples(inTime.points), period: inTime.periodSeconds };
});

const sim = new Simulation({
  system: SYSTEM_PRESETS.sol, stepsPerOrbit: 256, tolerances: { positionMeters: 1e-4, velocityMetersPerSecond: 1e-7 },
  vesselStart: { homeBodyId: 'aurelia', altitudeMeters: 100e3, plane: { kind: 'equatorial', inclinationRadians: 0 } },
  engine: { thrustNewtons: 250e3, specificImpulseSeconds: 350, dryMassKg: 10e3, fuelMassKg: 30e3 },
  retentionSeconds: 86_400, predictionHorizonSeconds: 3 * 3600, planCoastSeconds: 86_400,
});
const eph = sim.ephemeris;
const home = eph.bodies[sim.bodyIndex('aurelia')]!;
const now = 12_345;
eph.extendTo(now + 86_400);
const positions = new Float64Array(eph.bodyCount * 3), velocities = new Float64Array(eph.bodyCount * 3);
eph.statesAt(now, positions, velocities);
const at = (a: Float64Array, i: number): V => ({ x: a[i * 3]!, y: a[i * 3 + 1]!, z: a[i * 3 + 2]! });
const sub = (a: V, b: V): V => ({ x: a.x - b.x, y: a.y - b.y, z: a.z - b.z });
const surfaceOrbits = eph.bodies.filter((b) => b.parentIndex !== null).map((b) => {
  const parent = b.parentIndex!;
  const points = orbitInSurfaceFrame(home, now, sub(at(positions, parent), at(positions, home.index)),
    sub(at(positions, b.index), at(positions, parent)), sub(at(velocities, b.index), at(velocities, parent)), eph.bodies[parent]!.gm + b.gm);
  const count = points.length / 3;
  // Every 97th point and the last keep the file small; the Rust check compares those.
  const kept = Array.from({ length: count }, (_, i) => i).filter((i) => i % 97 === 0 || i === count - 1);
  return { body: b.index, count, kept, points: kept.flatMap((i) => [points[i * 3]!, points[i * 3 + 1]!, points[i * 3 + 2]!]) };
});

const paths = (['inertial', 'surface'] as PathFrameKind[]).map((kind) => {
  const frame = new PathFrame(eph, kind, home.index);
  const cache = new PathCache(97.5);
  const samples: number[] = [];
  const point = (t: number): V => ({ x: 7e6 * Math.cos(t / 900), y: 7e6 * Math.sin(t / 900), z: 1e5 });
  const sample = (t: number) => { samples.push(t); return frame.at(t, point(t)); };
  cache.update(now - 3000, now + 20_000, sample);
  cache.update(now, now + 40_000, sample);
  const out = new Float32Array((cache.count + 2) * 3);
  const written = cache.writeRelative(out, { x: 1, y: 2, z: 3 }, { x: 10, y: 20, z: 30 }, { x: -1, y: -2, z: -3 }, 1);
  const axes = frame.axesAt(now);
  return { kind, count: cache.count, written, samples, vertices: Array.from(out.subarray(0, written * 3)), axes: [v3(axes.x), v3(axes.y), v3(axes.z)], at: v3(frame.at(now, { x: 1e11, y: -2e10, z: 3e9 })) };
});

const out = new URL('../crates/view/tests/golden/view.json', import.meta.url);
writeFileSync(out, `${JSON.stringify({ views, cameras, gm, ellipses, now, home: home.index, surfaceOrbits, paths })}\n`);
console.log(`wrote ${out.pathname}: ${views.length} views, ${cameras.length} cameras, ${ellipses.length} ellipses, ${surfaceOrbits.length} surface orbits (${surfaceOrbits.map((o) => o.count).join(' ')})`);
