/** Owning TS Fleet reference; run from repo root with npx tsx. */
import { writeFileSync } from 'node:fs';
import RAPIER from '@dimforge/rapier3d-compat';
import { createLabScene, type ScenarioId } from '../../vessels/src/LabScenes';
await RAPIER.init();
const v = (a: {x:number,y:number,z:number}) => [a.x,a.y,a.z];
const q = (a: {x:number,y:number,z:number,w:number}) => [a.x,a.y,a.z,a.w];
const out=[];
for (const scenario of ['encounter','coast','separate','join','sas'] as ScenarioId[]) {
  const {fleet}=createLabScene(RAPIER,scenario);
  if(scenario==='separate') fleet.decouple('v1/p4');
  if(scenario==='join') {
    const gap=()=>Math.hypot(...v(fleet.nodeFrame('v1/p2','bottom').position).map((x,i)=>x-v(fleet.nodeFrame('v2/p2','bottom').position)[i]!));
    while(gap()>0.08 && fleet.time<60) fleet.advance(1/60);
    fleet.join('v1/p2','bottom','v2/p2','bottom');
  }
  if(scenario==='sas') fleet.setSas('v1',true);
  fleet.advance(scenario==='encounter'?700:scenario==='coast'?600:scenario==='sas'?30:1);
  out.push({scenario,time:fleet.time,states:fleet.vesselIds().map(id=>{
    const s=fleet.snapshot(id); return { id,mode:s.mode,position:v(s.position),velocity:v(s.velocity),rotation:q(s.rotation),angularVelocity:v(s.angularVelocity),mass:s.massKg,parts:s.partIds,
      relative:v(fleet.relative(id,'v1').position) };
  }),events:fleet.events});
  fleet.free();
}
writeFileSync(new URL('../crates/vessels/tests/golden.json',import.meta.url),JSON.stringify(out,null,2)+'\n');
console.log(`vessels: ${out.length} TS Fleet scenarios`);
