/** Four plotting frames from TS, including the previously missed two-body frame.
 * Run from this workspace: python3 tools/regenerate-golden.py --reference-root ../void reference_frames
 */
import { writeFileSync } from 'node:fs';
import { buildSystem } from '../../orbit/src/orbit/SystemSpec';
import { Ephemeris, suggestedStepSeconds } from '../../orbit/src/orbit/Ephemeris';
import { FrameEvaluator, toFrame, directionToFrame, type FrameSpec } from '../../orbit/src/orbit/ReferenceFrames';
import { SYSTEM_PRESETS } from '../../orbit/src/app/SystemPresets';
const vector = (v: { x: number; y: number; z: number }) => [v.x,v.y,v.z];
const systems = Object.entries(SYSTEM_PRESETS).map(([id, spec]) => {
  const system = buildSystem(spec);
  const home = system.bodies.find(b => b.id === (id === 'sol' ? 'aurelia' : 'aurelia-veil'))!.index;
  const moon = system.bodies.find(b => b.id === (id === 'sol' ? 'selene' : 'lumen'))!.index;
  const eph = new Ephemeris(system, { stepSeconds: suggestedStepSeconds(system.bodies, 256), chunkSteps: 2048 });
  eph.extendTo(60 * 86400);
  const specs: FrameSpec[] = [{kind:'barycentric'}, {kind:'body-inertial',body:home}, {kind:'body-surface',body:home}, {kind:'two-body-rotating',primary:home,secondary:moon}];
  const cases = specs.flatMap(frame => [0,1234.5,86400,30*86400,60*86400].map(t => {
    const evalFrame = new FrameEvaluator(eph, frame);
    const state = evalFrame.evaluate(t);
    const period = evalFrame.rotationPeriodSeconds(t);
    return { frame,t,origin:vector(state.origin),axes:[state.axes.x,state.axes.y,state.axes.z].map(vector),period:Number.isFinite(period)?period:null,
      bodies:system.bodies.map(b => vector(toFrame(state,eph.bodyPosition(b.index,t)))),
      direction:vector(directionToFrame(state,{x:0.2,y:-0.3,z:0.7})) };
  }));
  return { id,home,moon,cases };
});
writeFileSync(new URL('../crates/orbit/tests/golden/reference_frames.json',import.meta.url),JSON.stringify({systems})+'\n');
