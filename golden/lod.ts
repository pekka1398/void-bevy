/**
 * Golden data for crates/lod, from the LOD lab's own code.
 * Run from this workspace: python3 tools/regenerate-golden.py --reference-root ../void lod
 */
import { mkdirSync, writeFileSync } from 'node:fs';
import { BENCH_SCENARIOS } from '../../lod/src/app/BenchScenarios';
import { samplePlanetSurface } from '../../lod/src/app/DemoSurface';
import { HEADLESS_FOCAL_PIXELS, PLANET_PRESETS, type PlanetPreset } from '../../lod/src/app/PlanetPresets';
import { cubeToSphere } from '../../lod/src/lod/CubeSphere';
import { FACE_ADJACENCY, FACE_EDGES } from '../../lod/src/lod/FaceAdjacency';
import { PlanetLod, type LodCamera, type LodSelection } from '../../lod/src/lod/PlanetLod';
import { CUBE_FACES, tileCode, tileId, type CubeFace, type TileKey } from '../../lod/src/lod/TileKey';
import { buildTileMesh, type TileMeshData } from '../../lod/src/lod/TileMeshBuilder';
import { neighborKey, parentKey } from '../../lod/src/lod/TileNeighbors';
import { stitchEdges } from '../../lod/src/lod/TileRenderer';
import { sphereToCube, tileContaining, tilesAround } from '../../lod/src/lod/TileSearch';

const crate = new URL('../crates/lod/tests/golden/', import.meta.url);
const write = (name: string, data: unknown) => {
  const url = new URL(name, crate);
  const text = JSON.stringify(data);
  writeFileSync(url, `${text}\n`);
  console.log(`wrote ${url.pathname} (${(text.length / 1e6).toFixed(2)} MB)`);
};
const v3 = (v: { x: number; y: number; z: number }) => [v.x, v.y, v.z];
const keyOf = (k: TileKey) => [k.face, k.level, k.x, k.y];

// The lab's planet presets, as data for crates/lod's demo module (Infinity becomes null).
mkdirSync(new URL('../../presets/', crate), { recursive: true });
write('../../presets/planets.json', PLANET_PRESETS);

// Geometry: adjacency, cube <-> sphere, tile search.
const directions = [[0.3, -0.8, 0.52], [1, 1, 1], [-0.2, 0.1, -0.97], [0.62, 0.35, 0.7], [0, 0, -1], [1e-3, -1, 2e-3]];
const pad = { x: 0.62, y: 0.35, z: 0.7 };
const padUnit = (() => { const l = Math.hypot(pad.x, pad.y, pad.z); return { x: pad.x / l, y: pad.y / l, z: pad.z / l }; })();
write('geometry.json', {
  adjacency: CUBE_FACES.map((face) => FACE_EDGES.map((edge) => FACE_ADJACENCY[face][edge])),
  cubeToSphere: CUBE_FACES.flatMap((face) => [[-1, -1], [0.3, -0.7], [1, 0.25], [0, 0]].map(([u, v]) => ({ face, u, v, d: v3(cubeToSphere(face, u!, v!)) }))),
  sphereToCube: directions.map((d) => ({ d, ...sphereToCube({ x: d[0]!, y: d[1]!, z: d[2]! }) })),
  tileContaining: directions.flatMap((d) => [0, 5, 18].map((level) => ({ d, level, key: keyOf(tileContaining({ x: d[0]!, y: d[1]!, z: d[2]! }, level)) }))),
  tilesAround: [
    { point: v3({ x: padUnit.x * 6_383_000, y: padUnit.y * 6_383_000, z: padUnit.z * 6_383_000 }), reach: 3000, level: 14 },
    { point: [6_371_000, 0, 1], reach: 50_000, level: 8 },
  ].map((c) => ({ ...c, keys: tilesAround({ x: c.point[0]!, y: c.point[1]!, z: c.point[2]! }, c.reach, c.level, 6_371_000).map(keyOf) })),
});

// Meshes with the lab's demo terrain, and one stitched seam.
const mesh = (tile: TileMeshData) => ({
  key: keyOf(tile.key), origin: v3(tile.origin),
  positions: Array.from(tile.positions), normals: Array.from(tile.normals), colors: Array.from(tile.colors),
  heights: Array.from(tile.heights), grid: Array.from(tile.grid),
  minHeightMeters: tile.minHeightMeters, maxHeightMeters: tile.maxHeightMeters,
  errorMeters: tile.errorMeters, skirtDepthMeters: tile.skirtDepthMeters,
});
const build = (preset: PlanetPreset, key: TileKey) => buildTileMesh(key, (d) => samplePlanetSurface(d, preset),
  { radiusMeters: preset.radiusMeters, resolution: preset.tileResolution });
const meshCases: { preset: string; key: TileKey }[] = [
  { preset: 'seam', key: { face: 0, level: 0, x: 0, y: 0 } },
  { preset: 'seam', key: { face: 2, level: 3, x: 5, y: 1 } },
  { preset: 'seam', key: { face: 4, level: 5, x: 31, y: 0 } },
  { preset: 'landing', key: tileContaining(padUnit, 18) },
];
const fine: TileKey = { face: 4, level: 6, x: 63, y: 21 };
const coarseKey = parentKey(neighborKey(fine, 'u+'));
const fineTile = build(PLANET_PRESETS.seam, fine);
const coarseTile = build(PLANET_PRESETS.seam, coarseKey);
const coarseNode = { key: coarseKey, data: coarseTile } as unknown as Parameters<typeof stitchEdges>[1]['u+'];
const stitched = stitchEdges(fineTile, { 'u+': coarseNode }, PLANET_PRESETS.seam.tileResolution);
write('meshes.json', {
  presets: Object.fromEntries(Object.entries(PLANET_PRESETS).map(([id, p]) => [id, {
    radiusMeters: p.radiusMeters, maxSurfaceHeightMeters: p.maxSurfaceHeightMeters, tileResolution: p.tileResolution, terrain: p.terrain,
  }])),
  meshes: meshCases.map((c) => ({ preset: c.preset, ...mesh(build(PLANET_PRESETS[c.preset as 'seam'], c.key)) })),
  stitch: {
    fine: mesh(fineTile), coarse: mesh(coarseTile), edge: 'u+',
    positions: Array.from(stitched.positions), normals: Array.from(stitched.normals), heights: Array.from(stitched.heights),
  },
});

// Selection on the bench's scripted paths, with stub tiles, as lod-bench.ts.
const p = PLANET_PRESETS.landing;
const stub = (key: TileKey): TileMeshData => ({ id: tileId(key), key, origin: { x: 0, y: 0, z: 0 },
  positions: new Float32Array(), normals: new Float32Array(), colors: new Float32Array(), heights: new Float32Array(), grid: new Float32Array(),
  minHeightMeters: 0, maxHeightMeters: 0, errorMeters: 0, skirtDepthMeters: 0, buildMilliseconds: 0, sampleMilliseconds: 0, finishMilliseconds: 0 });
const SAMPLE_EVERY = 50;
const runs = [];
for (const scenario of BENCH_SCENARIOS) {
  for (const perFrame of [Infinity, 6]) {
    const lod = new PlanetLod({ radiusMeters: p.radiusMeters, minSurfaceHeightMeters: p.minSurfaceHeightMeters,
      maxSurfaceHeightMeters: p.maxSurfaceHeightMeters, occluderRadiusMeters: p.occluderRadiusMeters,
      lodSurfaceBandMeters: p.lodSurfaceBandMeters, resolution: p.tileResolution, maxLevel: p.maxLevel,
      splitDistanceRatios: p.splitDistanceRatios, maxCachedTiles: p.maxCachedTiles });
    const frames = [];
    for (let frame = 0; frame < scenario.frames; frame++) {
      const at = scenario.at(frame);
      const camera: LodCamera = { position: at.camera, focalPixels: HEADLESS_FOCAL_PIXELS, ...p.lodCamera };
      const selection: LodSelection = lod.select({ observerPositions: [at.probe], camera, distanceScale: 1, horizonCulling: true });
      const requests = perFrame === Infinity ? selection.requests
        : [...selection.requests].sort((a, b) => b.priority - a.priority).slice(0, perFrame);
      for (const request of requests) lod.acceptTile(stub(request.key));
      const record: Record<string, unknown> = {
        render: selection.render.length, requests: selection.requests.length, culled: selection.culled.horizon,
        collapses: selection.balanceCollapses.length, visited: selection.visited, built: requests.map((r) => tileCode(r.key)),
        probe: v3(at.probe), camera: v3(at.camera),
      };
      if (frame % SAMPLE_EVERY === 0) {
        record.renderCodes = selection.render.map((n) => n.code);
        record.requestList = selection.requests.map((r) => [tileCode(r.key), r.priority]);
      }
      frames.push(record);
    }
    runs.push({ scenario: scenario.name, perFrame: perFrame === Infinity ? null : perFrame, cached: lod.cachedTileCount, nodes: lod.nodeCount, frames });
  }
}
write('selection.json', {
  options: { radiusMeters: p.radiusMeters, minSurfaceHeightMeters: p.minSurfaceHeightMeters, maxSurfaceHeightMeters: p.maxSurfaceHeightMeters,
    occluderRadiusMeters: p.occluderRadiusMeters, lodSurfaceBandMeters: p.lodSurfaceBandMeters, resolution: p.tileResolution,
    maxLevel: p.maxLevel, splitDistanceRatios: p.splitDistanceRatios.map((r) => (Number.isFinite(r) ? r : null)), maxCachedTiles: p.maxCachedTiles },
  camera: { focalPixels: HEADLESS_FOCAL_PIXELS, ...p.lodCamera },
  sampleEvery: SAMPLE_EVERY,
  runs,
});
for (const run of runs) console.log(`  ${run.scenario} (${run.perFrame ?? 'all'}/frame): ${run.frames.length} frames, ${run.cached} cached, ${run.nodes} nodes`);
