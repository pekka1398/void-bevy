/** Headless geometry emitted by the actual TS SceneView. The tiny DOM stub supplies only
 * marker elements; no renderer/window is opened. Run at repo root with npx tsx.
 */
import { writeFileSync } from 'node:fs';
import * as THREE from 'three';
import { Simulation } from '../../orbit/src/orbit/Simulation';
import { SceneView } from '../../orbit/src/app/SceneView';
import { SYSTEM_PRESETS } from '../../orbit/src/app/SystemPresets';
import type { FrameSpec } from '../../orbit/src/orbit/ReferenceFrames';
class Marker {
 style: Record<string,string> = {};className='';innerHTML='';offsetWidth=120;
 classList={add(){},remove(){},toggle(){}};
 private children=new Map<string,Marker>();
 querySelector(selector:string){if(!this.children.has(selector))this.children.set(selector,new Marker());return this.children.get(selector)!;}
 append(){}addEventListener(){}textContent='';
}
Object.assign(globalThis,{document:{createElement(){return new Marker();}}});
const sim=new Simulation({system:SYSTEM_PRESETS.sol,stepsPerOrbit:256,tolerances:{positionMeters:1e-4,velocityMetersPerSecond:1e-7},vesselStart:{homeBodyId:'aurelia',altitudeMeters:400e3,plane:{kind:'orbit-of',bodyId:'selene'}},engine:{thrustNewtons:250e3,specificImpulseSeconds:350,dryMassKg:10e3,fuelMassKg:30e3},retentionSeconds:31*86400,predictionHorizonSeconds:43200,planCoastSeconds:7*86400});
const home=sim.bodyIndex('aurelia'),moon=sim.bodyIndex('selene');
sim.advance(120,20000);
sim.addManeuver({startTime:sim.time+600,referenceBody:home,referenceMode:'auto',prograde:40,normal:0,radial:5});
for(let i=0;i<50;i++){sim.extendPrediction(4000);sim.extendPlan(4000);}
const camera=new THREE.PerspectiveCamera(50,1.5,1,1e15);camera.position.set(0,30000,60000);camera.lookAt(0,0,0);camera.updateMatrixWorld();
const frames:FrameSpec[]=[{kind:'barycentric'},{kind:'body-inertial',body:home},{kind:'body-surface',body:home},{kind:'two-body-rotating',primary:home,secondary:moon}];
const cases=frames.map(frame=>{
 const view=new SceneView(sim,new Marker() as unknown as HTMLElement,frame,30*86400,21600,()=>{});view.setTarget(moon);view.update({kind:'body',index:home},camera,1440,960,100);
 const raw=view as unknown as Record<string,any>;
 const line=(mesh:THREE.Line)=>{
  if(!mesh.visible)return {visible:false};
  const count=mesh.geometry.drawRange.count;
  if(!Number.isFinite(count))throw new Error('visible path has no draw range');
  const p=mesh.geometry.getAttribute('position').array,c=mesh.geometry.getAttribute('color')?.array;
  const indices=[...new Set([0,...Array.from({length:Math.ceil(count/97)},(_,i)=>i*97),count-1])].filter(i=>i>=0&&i<count);
  return {visible:true,count,indices,points:indices.map(i=>[p[i*3],-p[i*3+2]!,p[i*3+1]]),colors:c?indices.map(i=>[c[i*3],c[i*3+1],c[i*3+2]]):null};
 };
 return {frame,history:line(raw.vesselTrail),prediction:line(raw.predictionLine),plan:line(raw.planLine),target:line(raw.targetLine),bodyPaths:raw.bodies.map((v:any)=>line(v.trail)),endTime:sim.plan.trajectory.lastTime};
});
writeFileSync(new URL('../crates/orbit-lab/tests/scene.json',import.meta.url),JSON.stringify({cases})+'\n');
console.log(`wrote actual SceneView paths in ${cases.length} frames`);
