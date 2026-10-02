/**
 * Golden data for crates/aero, from lab/aerodynamics: the atmosphere, the wing polar, forces and
 * heat loads on the three test vehicles in random states, mass properties, thermal steps, the
 * reentry run in 10 s chunks and the lab check's single 400 s run, and the aircraft's free flight.
 * Run from the repository root: npx tsx lab/void-bevy/golden/aero.ts
 */
import { writeFileSync } from 'node:fs';
import RAPIER from '@dimforge/rapier3d-compat';
import { EARTH_AIR, VACUUM } from '../../aerodynamics/src/Atmosphere';
import { aerodynamicForces, wingPolar, type Controls, type WingAero } from '../../aerodynamics/src/Aero';
import { aircraft, capsule, assemblyRocket, resources, massProperties, aeroElements, type Vehicle } from '../../aerodynamics/src/Vehicle';
import { evaluateVehicle } from '../../aerodynamics/src/Loads';
import { advanceThermal, thermalState, type ThermalState } from '../../aerodynamics/src/Thermal';
import { AircraftFlight } from '../../aerodynamics/src/Flight';
import { EntryFlight } from '../../aerodynamics/src/Entry';
import type { Vec3, Quat } from '../../aerodynamics/src/math';

let seed = 4242;
const rand = () => { seed = (seed * 1103515245 + 12345) % 2147483648; return seed / 2147483648 * 2 - 1; };
const v3 = (v: Vec3) => [v.x, v.y, v.z];
const q4 = (q: Quat) => [q.x, q.y, q.z, q.w];
const unitQuat = (): Quat => {
  const q = { x: rand(), y: rand(), z: rand(), w: rand() }, l = Math.hypot(q.x, q.y, q.z, q.w);
  return { x: q.x / l, y: q.y / l, z: q.z / l, w: q.w / l };
};
const thermal = (t: ThermalState) => [t.skinK, t.coreK, t.ablatorKg, t.failed ? 1 : 0];

const atmosphere = [-4000, 0, 700, 5000, 10999.99, 11000.01, 15000, 20000, 25000, 32000, 40000, 47000, 50000, 51000,
  60000, 71000, 80000, 84852, 86000, 100000, 105000, 110000, 119999, 120000, 150000].map(h => {
  const a = EARTH_AIR.sample(h);
  return { altitude: h, air: [a.density, a.pressurePa, a.temperatureK, a.soundSpeed, a.viscosity] };
});

const plane = aircraft();
const wings = (['wing-left', 'fin'] as const).map(id => plane.parts.find(p => p.id === id)!.aero!.shape as WingAero);
const polar: { wing: number; alpha: number; mach: number; out: number[] }[] = [];
wings.forEach((_, w) => {
  for (let alpha = -180; alpha <= 180; alpha += 7.3) for (const mach of [0, 0.5, 0.85, 1, 1.3, 3]) {
    const p = wingPolar(wings[w]!, alpha * Math.PI / 180, mach);
    polar.push({ wing: w, alpha: alpha * Math.PI / 180, mach, out: [p.cl, p.cd, p.stall] });
  }
});

const vehicles: [string, Vehicle][] = [['aircraft', plane], ['rocket', assemblyRocket()], ['capsule', capsule()], ['bare', capsule(false)]];
const mass = vehicles.map(([id, v]) => {
  const full = resources(v), empty = resources(v);
  for (const key of empty.fuel.keys()) empty.fuel.set(key, 0);
  const a = massProperties(v, full), b = massProperties(v, empty);
  return { id, full: [a.mass, ...v3(a.center), ...v3(a.inertia)], empty: [b.mass, ...v3(b.center), ...v3(b.inertia)] };
});

const loads = vehicles.flatMap(([id, v]) => Array.from({ length: 30 }, (_, i) => {
  const data = resources(v), center = massProperties(v, data).center;
  const fast = i % 3 === 0;
  const speed = fast ? 3000 + 4000 * Math.abs(rand()) : 400 * Math.abs(rand());
  const velocity = { x: rand() * speed, y: rand() * speed, z: rand() * speed };
  const state = { center: { x: rand() * 1000, y: rand() * 1000, z: rand() * 1000 }, velocity, rotation: unitQuat(),
    angularVelocity: { x: rand(), y: rand(), z: rand() } };
  const wind = i % 4 === 0 ? { x: 0, y: 0, z: 0 } : { x: rand() * 20, y: rand() * 5, z: rand() * 20 };
  const controls: Controls = { elevator: rand(), aileron: rand(), rudder: rand() };
  const altitude = i === 5 ? 130000 : 60000 * Math.abs(rand());
  const air = EARTH_AIR.sample(altitude);
  const forces = aerodynamicForces(aeroElements(v, center), state, air, wind, controls);
  const heat = evaluateVehicle(v, data, state, air, wind, controls, 200);
  return { vehicle: id, center: v3(state.center), velocity: v3(velocity), rotation: q4(state.rotation),
    angularVelocity: v3(state.angularVelocity), wind: v3(wind), controls: [controls.elevator, controls.aileron, controls.rudder], altitude,
    force: v3(forces.force), torque: v3(forces.torque), flow: [forces.qPa, forces.speed, forces.mach],
    elements: forces.elements.map(e => ({ id: e.id, point: v3(e.point), force: v3(e.force), moment: v3(e.moment), drag: v3(e.drag), lift: v3(e.lift),
      flow: [e.speed, e.qPa, e.mach, e.alphaRadians, e.stall, e.cl, e.cd] })),
    heat: v.parts.map(p => { const h = heat.heat.get(p.id)!; return { exposed: h.env.exposed, exposure: h.env.exposure, speed: h.env.speed,
      load: [h.load.aerodynamicW, h.load.convectionW, h.load.radiationW, h.load.conductionW, h.load.fluxWm2] }; }) };
}));

const shieldSpec = capsule().parts[0]!.thermal;
const thermalRuns = [
  { name: 'shield 30 km 7 km/s', spec: shieldSpec, start: 300, env: { air: EARTH_AIR.sample(30000), speed: 7000, exposed: true, exposure: 1, backgroundK: 180 }, dt: 0.2, core: 0 },
  { name: 'small ablator', spec: { ...shieldSpec, ablator: { massKg: 0.001, activationK: 1100, latentJkg: 12e6 } }, start: 1100,
    env: { air: EARTH_AIR.sample(30000), speed: 7000, exposed: true, exposure: 1, backgroundK: 180 }, dt: 0.2, core: 0 },
  { name: 'fuselage low speed', spec: plane.parts[0]!.thermal, start: 288.15, env: { air: EARTH_AIR.sample(700), speed: 70, exposed: true, exposure: 1, backgroundK: 250 }, dt: 1 / 120, core: 0 },
  { name: 'pod heated core', spec: capsule().parts[1]!.thermal, start: 500, env: { air: VACUUM.sample(0), speed: 0, exposed: false, exposure: 0, backgroundK: 3 }, dt: 1.7, core: 300 },
].map(run => {
  const spec = run.spec, t = thermalState(spec, run.start);
  const steps = Array.from({ length: 20 }, () => {
    const b = advanceThermal(spec, t, run.env, run.dt, run.core);
    return { state: thermal(t), budget: [b.incomingJ, b.radiationJ, b.ablationJ, b.coreExternalJ, b.storedJ] };
  });
  return { name: run.name, steps };
});

const entrySample = (e: EntryFlight) => ({ time: e.time, y: [...e.y], thermal: e.vehicle.parts.map(p => thermal(e.resources.thermal.get(p.id)!)),
  maxQPa: e.maxQPa, maxFluxWm2: e.maxFluxWm2, maxG: e.maxG, heatJ: e.heatJ, acceptedSteps: e.acceptedSteps, stepHint: e.stepHint, terminal: e.terminal });
const entryChunks = [true, false].map(shield => {
  const e = new EntryFlight(capsule(shield)), samples = [entrySample(e)];
  for (let k = 0; k < 40 && !e.terminal; k++) { e.advance(10, 200000); samples.push(entrySample(e)); }
  return { shield, samples };
});
const entryWhole = [true, false].map(shield => { const e = new EntryFlight(capsule(shield)); e.advance(400, 200000); return { shield, end: entrySample(e) }; });
const steep = new EntryFlight(capsule(), { altitudeMeters: 100_000, speed: 6500, flightPathDegrees: -12, angleOfAttackDegrees: 160, bankDegrees: 25 });
const steepSamples = [entrySample(steep)];
for (let k = 0; k < 30 && !steep.terminal; k++) { steep.advance(5, 200000); steepSamples.push(entrySample(steep)); }

await RAPIER.init();
const flights = (['cruise', 'runway'] as const).map(mode => {
  const f = new AircraftFlight(RAPIER, aircraft(), mode), samples = [];
  for (let i = 0; i <= 120 * 30; i++) {
    if (i % 60 === 0) samples.push({ time: f.time, position: v3(f.position), velocity: v3(f.body.linvel()), rotation: q4(f.body.rotation()),
      fuel: f.fuelKg, thrust: f.thrustN, failure: f.failure });
    f.step({ elevator: mode === 'runway' && f.loads.aero.speed > 40 ? 0.15 : 0, aileron: 0, rudder: 0, throttle: mode === 'runway' ? 1 : 0.28, brakes: false });
  }
  f.dispose();
  return { mode, samples };
});

writeFileSync(new URL('../crates/aero/tests/golden/aero.json', import.meta.url), JSON.stringify({ atmosphere, polar, mass, loads,
  thermal: thermalRuns, entry: { chunks: entryChunks, whole: entryWhole, steep: steepSamples }, flights }) + '\n');
console.log(`aero: ${atmosphere.length} air samples, ${polar.length} polar points, ${loads.length} load states, entry ${entryWhole.map(e => `${e.end.time.toFixed(1)} s ${e.end.terminal}`).join(' / ')}`);
