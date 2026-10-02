/**
 * System specs and golden data for crates/orbit, from the orbit lab.
 * Run from this workspace: python3 tools/regenerate-golden.py --reference-root ../void orbit
 */
import { mkdirSync, writeFileSync } from 'node:fs';
import { SYSTEM_PRESETS } from '../../orbit/src/app/SystemPresets';
import { Ephemeris, suggestedStepSeconds } from '../../orbit/src/orbit/Ephemeris';
import { osculatingOrbit, solveKeplerElliptic, stateFromElements, type EllipticElements } from '../../orbit/src/orbit/Kepler';
import { buildSystem } from '../../orbit/src/orbit/SystemSpec';

const crate = new URL('../crates/orbit/', import.meta.url);
mkdirSync(new URL('systems/', crate), { recursive: true });
mkdirSync(new URL('tests/golden/', crate), { recursive: true });
const write = (path: string, data: unknown) => {
  const url = new URL(path, crate);
  writeFileSync(url, `${JSON.stringify(data, null, 1)}\n`);
  console.log(`wrote ${url.pathname}`);
};
const flat = (a: Float64Array) => Array.from(a);

// As the orbit lab's page (src/app/main.ts) and Simulation.
const STEPS_PER_ORBIT = 256;
const CHUNK_STEPS = 2048;
const DAY = 86_400;

for (const [id, spec] of Object.entries(SYSTEM_PRESETS)) {
  write(`systems/${id}.json`, spec);
  const system = buildSystem(spec);
  const stepSeconds = suggestedStepSeconds(system.bodies, STEPS_PER_ORBIT);
  const ephemeris = new Ephemeris(system, { stepSeconds, chunkSteps: CHUNK_STEPS });
  const energy0 = ephemeris.currentEnergy();
  const times = [0.37 * stepSeconds, DAY + 0.123, 10 * DAY, 100 * DAY + 0.5];
  const positions = new Float64Array(system.bodies.length * 3);
  const velocities = new Float64Array(system.bodies.length * 3);
  const states = times.map((t) => {
    ephemeris.extendTo(t);
    ephemeris.statesAt(t, positions, velocities);
    return { t, positions: flat(positions), velocities: flat(velocities) };
  });
  write(`tests/golden/${id}.json`, {
    stepsPerOrbit: STEPS_PER_ORBIT,
    chunkSteps: CHUNK_STEPS,
    stepSeconds,
    bodies: system.bodies.map((b) => ({
      id: b.id, gm: b.gm, rotation: b.rotation, parentIndex: b.parentIndex,
      orbitPeriodSeconds: b.orbitPeriodSeconds, periapsisFraction: b.periapsisFraction, sphereOfInfluenceMeters: b.sphereOfInfluenceMeters,
    })),
    positions: flat(system.positions),
    velocities: flat(system.velocities),
    states,
    endTime: ephemeris.endTime,
    energy: { start: energy0, end: ephemeris.currentEnergy() },
    angularMomentum: ephemeris.currentAngularMomentum(),
  });
}

const elements: EllipticElements[] = [
  { semiMajorAxisMeters: 1.496e11, eccentricity: 0.0167, inclinationRadians: 0, longitudeOfAscendingNodeRadians: 0, argumentOfPeriapsisRadians: 1.796, meanAnomalyRadians: 6.24 },
  { semiMajorAxisMeters: 3.844e8, eccentricity: 0.0549, inclinationRadians: 0.0898, longitudeOfAscendingNodeRadians: 2.18, argumentOfPeriapsisRadians: 5.55, meanAnomalyRadians: -40.1 },
  { semiMajorAxisMeters: 2.2e7, eccentricity: 0.93, inclinationRadians: 2.9, longitudeOfAscendingNodeRadians: -1, argumentOfPeriapsisRadians: 0.3, meanAnomalyRadians: 0.02 },
];
const gm = 3.986004418e14;
write('tests/golden/kepler.json', {
  gm,
  cases: elements.map((el) => {
    const s = stateFromElements(el, gm);
    return {
      elements: el,
      eccentricAnomaly: solveKeplerElliptic(el.meanAnomalyRadians, el.eccentricity),
      position: [s.position.x, s.position.y, s.position.z],
      velocity: [s.velocity.x, s.velocity.y, s.velocity.z],
      osculating: osculatingOrbit(s.position, s.velocity, gm),
    };
  }),
});
