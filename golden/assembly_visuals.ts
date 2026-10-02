/** Extract the actual TS PartVisual scene; this data describes rendering, not colliders. */
import { writeFileSync } from 'node:fs';
import * as THREE from 'three';
import { CATALOG } from '../../assembly/src/model';
import { createPartVisual } from '../../assembly/src/PartVisual';
const geometry = (g: THREE.BufferGeometry) => {
  // The lab has its own Three installation: use geometry types, not cross-package instanceof.
  if (g.type === 'ConeGeometry') {
    const p = (g as THREE.ConeGeometry).parameters;
    return { kind: 'cone', radius: p.radius, height: p.height, segments: p.radialSegments };
  }
  if (g.type === 'CylinderGeometry') {
    const p = (g as THREE.CylinderGeometry).parameters;
    return { kind: 'cylinder', top: p.radiusTop, bottom: p.radiusBottom, height: p.height, segments: p.radialSegments };
  }
  if (g.type === 'BoxGeometry') {
    const p = (g as THREE.BoxGeometry).parameters;
    return { kind: 'box', size: [p.width, p.height, p.depth] };
  }
  throw new Error(`Unported visual geometry ${g.type}`);
};
const out = CATALOG.map(definition => {
  const group = createPartVisual({ id: definition.id, definition });
  const meshes = group.children.filter((o): o is THREE.Mesh => Boolean((o as THREE.Mesh).isMesh)).map(mesh => {
    const m = mesh.material as THREE.MeshStandardMaterial | THREE.MeshBasicMaterial;
    if (m.type !== 'MeshStandardMaterial' && m.type !== 'MeshBasicMaterial') throw new Error(`Unported material ${m.type}`);
    const hex = m.color.getHex(), color = [(hex >> 16 & 255) / 255, (hex >> 8 & 255) / 255, (hex & 255) / 255];
    const standard = m as THREE.MeshStandardMaterial;
    return { geometry: geometry(mesh.geometry), position: mesh.position.toArray(), rotation: mesh.quaternion.toArray(), scale: mesh.scale.toArray(), flame: mesh.name === 'flame',
      material: { color, metalness: m.type === 'MeshStandardMaterial' ? standard.metalness : 0,
        roughness: m.type === 'MeshStandardMaterial' ? standard.roughness : 1, unlit: m.type === 'MeshBasicMaterial', opacity: m.opacity } };
  });
  const edge = group.getObjectByName('selection') as THREE.LineSegments | undefined;
  if (!edge?.isLineSegments) throw new Error('Missing selection geometry');
  edge.updateMatrix();
  const p = edge.geometry.getAttribute('position');
  const selection = Array.from({ length: p.count }, (_, i) => new THREE.Vector3().fromBufferAttribute(p, i).applyMatrix4(edge.matrix).toArray());
  return { id: definition.id, meshes, selection };
});
writeFileSync(new URL('../crates/assembly-lab/data/visuals.json', import.meta.url), JSON.stringify(out, null, 2) + '\n');
console.log(`assembly visuals: ${out.length} models, ${out.reduce((n, d) => n + d.meshes.length, 0)} render meshes`);
