/**
 * Golden data for crates/scenery: lab/scenery's atmosphere tables, sky references, cloud weather and
 * noise volumes, star field and orbit view. Big buffers are written raw (little-endian) beside the JSON.
 * Run from the repository root: npx tsx lab/void-bevy/golden/scenery.ts
 */
import { mkdirSync, writeFileSync } from 'node:fs';
import { buildTransmittanceTable, earthLikeAtmosphere, skyRadiance, transmittanceCoords, transmittanceRay, type Vec3 } from '../../scenery/src/Atmosphere';
import { buildIrradianceTable, buildMultipleScatteringTable, marchSky } from '../../scenery/src/SkyTables';
import {
  buildCloudNoise, buildCloudWeather, cloudDensity, cloudShellIntervals, cloudWeather, DETAIL_SIZE, sampleCloudNoise, SHAPE_PERIOD, SHAPE_SIZE,
} from '../../scenery/src/CloudField';
import { DEFAULT_STARS, generateStars } from '../../scenery/src/Stars';
import { OrbitView } from '../../scenery/src/OrbitView';
import { DEFAULT_LAYERED } from '../../scenery/src/LayeredTerrain';

const out = new URL('../crates/scenery/tests/golden/', import.meta.url);
mkdirSync(out, { recursive: true });
const raw = (name: string, data: Float32Array | Uint8Array) => writeFileSync(new URL(name, out), Buffer.from(data.buffer, data.byteOffset, data.byteLength));

const radius = DEFAULT_LAYERED.radiusMeters;
const p = earthLikeAtmosphere(radius);
const transmittance = buildTransmittanceTable(p);
const multiple = buildMultipleScatteringTable(p, transmittance);
const irradiance = buildIrradianceTable(p, transmittance, multiple);
raw('transmittance.bin', transmittance);
raw('multiple.bin', multiple);
raw('irradiance.bin', irradiance);

const unit = (x: number, y: number, z: number): Vec3 => { const l = Math.hypot(x, y, z); return { x: x / l, y: y / l, z: z / l }; };
const rays = [
  { altitude: 2, direction: unit(0, 0, 1), sun: unit(0.3, 0, 1) },
  { altitude: 2, direction: unit(1, 0, 0.02), sun: unit(1, 0, 0.03) },
  { altitude: 10e3, direction: unit(0.2, 0.7, -0.1), sun: unit(-0.4, 0.1, 0.5) },
  { altitude: 99e3, direction: unit(0, 1, -0.2), sun: unit(0, 0.2, 1) },
  { altitude: 1500, direction: unit(-1, 0.1, 0.3), sun: unit(1, 0.2, -0.05) },
];
const sky = rays.map((ray) => ({
  ...ray,
  reference: skyRadiance(p, ray.altitude, ray.direction, ray.sun, 500),
  march: marchSky(p, transmittance, multiple, radius + ray.altitude, ray.direction, ray.sun, 32),
  marchSingle: marchSky(p, transmittance, null, radius + ray.altitude, ray.direction, ray.sun, 32),
}));
const coords = [[0.1, 0.2], [0.9, 0.95], [0.5, 0], [0, 1]].map(([x, y]) => {
  const { r, mu } = transmittanceRay(p, x!, y!);
  return { x, y, r, mu, back: transmittanceCoords(p, r, mu) };
});

const weather = buildCloudWeather();
// Every 16th row and the last (the full atlas is 8 MB).
const rows = [...Array.from({ length: 64 }, (_, i) => i * 16), 1023];
raw('weather_rows.bin', Uint8Array.from(rows.flatMap((y) => [...weather.subarray(y * 2048 * 4, (y + 1) * 2048 * 4)])));
const shape = buildCloudNoise(SHAPE_SIZE, false);
const detail = buildCloudNoise(DETAIL_SIZE, true);
raw('shape.bin', shape);
raw('detail.bin', detail);
const directions = Array.from({ length: 300 }, (_, i) => unit(Math.sin(i * 1.7) + 0.01, Math.cos(i * 2.3), Math.sin(i * 0.37)));
const weatherSamples = directions.map((d) => ({ d, w: cloudWeather(d) }));
const noiseSamples = Array.from({ length: 200 }, (_, i) => {
  const point = { x: i * 1234.5 - 70000, y: Math.sin(i) * 90000, z: i * i * 7.25 };
  return { point, values: [0, 1, 2].map((c) => sampleCloudNoise(shape, SHAPE_SIZE, point, SHAPE_PERIOD, c)) };
});
const densities = Array.from({ length: 300 }, (_, i) => {
  const args = [1000 + (i * 37) % 7500, (i * 0.137) % 1, (i * 0.291) % 1, (i * 0.613) % 1, (i * 0.871) % 1, 0.3 + (i % 7) * 0.1, (i % 3) / 2, (i % 11) * 2000, (i * 0.333) % 1] as const;
  return { args, density: cloudDensity(...args) };
});
const shells = Array.from({ length: 200 }, (_, i) => {
  const r = radius + 5000 + [-8000, 500, 4000, 7000, 3e5, 2e7][i % 6]!;
  const origin = { x: r * Math.cos(i), y: r * Math.sin(i) * 0.6, z: r * Math.sin(i) * 0.8 };
  const direction = unit(Math.sin(i * 3.1), Math.cos(i * 1.3), Math.sin(i * 0.7 + 1));
  const scene = i % 4 === 0 ? 20000 : Infinity;
  return { origin, direction, scene: Number.isFinite(scene) ? scene : null, intervals: cloudShellIntervals(origin, direction, radius + 6500, radius + 13000, scene) };
});

const { positions, colors } = generateStars(DEFAULT_STARS);
raw('star_positions.bin', positions);
raw('star_colors.bin', colors);

const view = new OrbitView(unit(0.3, -0.5, 0.8), radius + 20e3, 0.4, radius * 40);
const poses: unknown[] = [];
const record = () => { const pose = view.pose(); poses.push({ ...pose, basis: view.basis(), tilt: view.tiltRadians }); };
record();
view.panScreen(120, -40, 1, 900, 20e3); record();
view.orbitAroundCenter(0.3, -0.2); record();
view.turn(0.7, 1.1); record();
view.setRadius(radius + 3e3); record();
view.place(unit(0, 0, 1), radius + 400e3, 0.5, 0); record();
view.orbitAroundCenter(-1.2, 0.9); record();
view.turn(-2, 5); record();
view.setRadius(radius * 100); record();

writeFileSync(new URL('scenery.json', out), JSON.stringify({
  radius, params: p, sky, coords, weatherSamples, noiseSamples, densities, shells, poses,
}));
console.log('wrote', out.pathname);
