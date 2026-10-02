/** Rust assembly fixtures from the owning TS lab. Run from repo root with npx tsx. */
import { writeFileSync } from 'node:fs';
import { CATALOG, compile, demoCraft, freshCraft, addPart, summary, freeNodes, fuelSources, partInertiaPerKg } from '../../assembly/src/model';
const v = (p: { x: number; y: number; z: number }) => [p.x, p.y, p.z];
const catalog = CATALOG.map(d => ({ ...d, nodes: d.nodes.map(n => ({ ...n, position: v(n.position), direction: v(n.direction) })),
  modules: d.modules.map(m => m.kind === 'engine' ? { ...m, direction: v(m.direction) } : m) }));
writeFileSync(new URL('../crates/assembly/data/catalog.json', import.meta.url), JSON.stringify(catalog, null, 2) + '\n');
const crafts = [demoCraft(), addPart(freshCraft(), 'tank-small', 'p1', 'bottom', 'bottom')];
const cases = crafts.map(craft => {
  const c = compile(craft), s = summary(c);
  return { craft, rootId: c.rootId, connections: c.connections, summary: { ...s, center: v(s.center) },
    parts: c.parts.map(p => ({ id: p.id, position: v(p.pose.position), rotation: [p.pose.rotation.x,p.pose.rotation.y,p.pose.rotation.z,p.pose.rotation.w], inertia: v(partInertiaPerKg(p.definition)) })),
    free: freeNodes(c).map(n => ({ partId: n.partId, nodeId: n.node.id, position: v(n.pose.position) })),
    fuelSources: Object.fromEntries(c.parts.filter(p => p.definition.category === 'engine').map(p => [p.id, fuelSources(c,p.id,new Set())])) };
});
writeFileSync(new URL('../crates/assembly/tests/golden/assembly.json', import.meta.url), JSON.stringify(cases, null, 2) + '\n');
console.log(`assembly: ${catalog.length} definitions, ${cases.length} craft fixtures`);
