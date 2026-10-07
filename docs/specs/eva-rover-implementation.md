# EVA / rover implementation milestones

Scope and workflow: playable-vehicles-and-multiscale.md and ../../AGENTS.md.

## Rover physical milestone (2026-10-07)

The ordinary Craft3 rover fixture uses validated surface mounts, PartGraph modules and existing
Fleet Ground/Bubble/Orbit owners. Wheel is an explicitly selected raycast suspension/tire model;
the small authored hub is a chassis collider and the tire itself is a virtual contact model,
not a second dynamic owner. No model fallback occurs. A support ray queries actual live Rapier
colliders, excludes its own chassis and sensors, and accepts normals facing suspension by >0.1.

Wheel suspension spring/damper and finite rotor inertia are authored. Forward/lateral tire
impulses share the Coulomb friction circle. Brakes solve road impulse and rotor spin together;
chassis torque accounts for virtual rotor angular momentum. Dynamic supports receive reciprocal
point impulses. Tire contacts are evaluated from the same accepted boundary, with accumulated
predicted impulse response; no live body changes happen until all loads are prepared. These
operator-split impulses are applied before the existing contact step, without treating a current
tire impulse as half an atmospheric force or committing state in an orbit trial.

Wheel control and accepted suspension/spin/ground state live in PartGraph module state and are
included in direct checkpoint and journal world marks. Branch model23 / FleetCheckpoint10 reject
older schemas/rules explicitly. Root allocates final integration version centrally.

Rover motors currently have unlimited energy; no electrical or fuel subsystem is claimed.
Strong motor torque can spin tires or wheelie in low gravity; actuator limits do not change
silently by planet. Main craft command has no ideal reaction wheel.

## Main game acceptance

Run void-app --rover --planet terra (or another configured planet ID), or
--craft crates/assembly/data/rover.json. W/S drive/reverse, A/D front steering; Space latches
brakes, X toggles parking brake, and W/S releases brakes. P pause; F6/F7 direct save/load;
F4 shows actual chassis and terrain colliders. Tire meshes visibly steer, spin and move with
accepted suspension extension. HUD reports tire grounded count and drive/steer/brake.

Headless targeted evidence at b7d7015: Earth settle with four supported wheels, drive, brake,
exact checkpoint continuation and journal replay passed; affected core library Clippy passed.
Earlier f83d11e wheel/contact tests cover airborne motor with zero traction, slip energy,
friction circle, brake hold, rotor reaction, actual moving-support ray and floating-origin
recenter. Main-game source entry is delivered separately and requires root compile/GUI checks;
these headless checks do not substitute dynamic GUI or human acceptance.

EVA entry/exit, walking/jumping/jetpack and rover seat transfer are the next milestone on this
same branch; not claimed complete by the rover checkpoint.
