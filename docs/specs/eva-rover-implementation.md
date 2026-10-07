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

## Local EVA core milestone

Crew resides in a dedicated Seat part with its own isolated typed monopropellant tank; CrewRecord
stores identity/body+suit dry mass, not a second consumable inventory. Exit transfers that resource
quantity to a suit PartGraph craft and boarding transfers the remaining amount back. The occupied
seat also preserves packed suit thermal state; boarding/exiting cannot reset suit heating or failure.
A separate crewed rover chassis authors requiresCrew=true, while existing/autonomous commands
retain the explicit false default. Healthy occupied seats grant only the authored crew-required
capability. Exit switches human selection, clears carrier manual inputs and latches physical brakes.

Hatch clearance queries actual live native geometry. Instant endpoint transfer conserves aggregate
mass, COM, linear and angular momentum, including spinning carriers; the resulting recoil is part
of this idealized transfer. Cabin traversal/climbing are outside this milestone. Avatar collision is
an authored dynamic rectangular suited-body envelope; native normal contacts remain enabled.
Finite grounded traction/balance uses actual solved normal impulse, not proximity or a prescribed
position/velocity. The tangential actuator replaces the native suit static-friction constraint using
an explicit Min coefficient rule. Native clustered contacts and tile support identity are considered.
Jump is an instantaneous reciprocal support impulse, distinct from interval suspension loading.
Backpack rotation/translation use the existing finite RCS/resource acceptance semantics.

Vehicle steer and EVA strafe/yaw positive mean player-right. Nose+Z/top+Y is a right-handed frame
whose +X is up×forward, physically left; physics and tire drawing use the same right-turn sign.
Physical body-fixed north/east tests, not navball convention, validate the direction.

Known integration work still required after this local-only milestone: use multiscale
enter_vessel/restore_view scopes around exit/board, explicitly reject different systems, and run
remote-system transfer tests; packed Orbit wheel spin/motor torque/rails needs acceptance updates.
Passive support no longer resets native activation, but the existing single-sweep tire solver still
has about 0.09m/s parked residual on the test terrain and does not settle to sleep in 20s. A failing
regression is preserved at /tmp/void-eva-suspension-sleep-regression.rs for the next solver step;
this milestone does not claim parked sleep or full wheel owner completion.

Targeted local evidence: EVA transfer/checkpoint/replay, spinning-carrier conservation, dynamic
walk/jump, low-gravity ballistic jump and finite orbital backpack resource/checkpoint/replay pass.
Main EVA controls/avatar rendering and root TigerVNC/human acceptance are subsequent work.
