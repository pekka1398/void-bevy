/**
 * Golden data for crates/terrain: scenery's layered planet and landing's hills, as the main game
 * and landing build them (landing's TerrainConfig), and tiles built on them by lab/lod.
 * Run from this workspace: python3 tools/regenerate-golden.py --reference-root ../void terrain
 */
import { mkdirSync, writeFileSync } from 'node:fs';
import { buildTileMesh, type TileKey } from '../../lod/src/lod';
import { latticeDirections } from '../../landing/src/terrain/SurfaceContract';
import { terrainFromConfig, type TerrainConfig } from '../../landing/src/terrain/TerrainConfig';
import { PLANETS } from '../../landing/src/planet/Planets';
import { DEFAULT_LAYERED } from '../../scenery/src/LayeredTerrain';

const out = new URL('../crates/terrain/tests/golden/', import.meta.url);
mkdirSync(out, { recursive: true });

// The configs the game uses: layered on Aurelia's radius, and every landing planet's hills.
const configs: Record<string, TerrainConfig> = {
  layered: { kind: 'layered', options: { ...DEFAULT_LAYERED } },
  ...Object.fromEntries(Object.entries(PLANETS).filter(([id]) => id !== 'aurelia-fast').map(([id, make]) => [id, make().terrainConfig])),
};
const directions = latticeDirections(1500);
const cells = [undefined, 30, 1000, 60_000];
const terrains = Object.entries(configs).map(([id, config]) => {
  const terrain = terrainFromConfig(config);
  return {
    id, config, name: terrain.name, radiusMeters: terrain.radiusMeters, maxHeightMeters: terrain.maxHeightMeters,
    samples: cells.map((cell) => ({
      cell: cell ?? null,
      heights: directions.map((d) => terrain.sample(d, cell).heightMeters),
      colors: directions.map((d) => [...terrain.sample(d, cell).color]),
    })),
  };
});

// Tiles on the layered planet through lab/lod's builder, as the game's workers and collision build them.
const layered = terrainFromConfig(configs.layered!);
const keys: TileKey[] = [{ face: 0, level: 2, x: 1, y: 2 }, { face: 4, level: 9, x: 300, y: 211 }, { face: 2, level: 16, x: 40000, y: 21000 }];
const tiles = keys.map((key) => {
  const tile = buildTileMesh(key, layered.sample, { radiusMeters: layered.radiusMeters, resolution: 33 });
  return { key: [key.face, key.level, key.x, key.y], origin: [tile.origin.x, tile.origin.y, tile.origin.z],
    positions: Array.from(tile.positions), normals: Array.from(tile.normals), colors: Array.from(tile.colors), heights: Array.from(tile.heights) };
});

const text = JSON.stringify({ directions: directions.map((d) => [d.x, d.y, d.z]), terrains, tiles });
writeFileSync(new URL('terrain.json', out), `${text}\n`);
console.log(`wrote terrain.json (${(text.length / 1e6).toFixed(2)} MB): ${terrains.map((t) => t.id).join(', ')}; ${tiles.length} layered tiles`);
