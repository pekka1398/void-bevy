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
The original local single-sweep milestone had about 0.09m/s parked residual. The subsequent
coupled contact milestone replaces that sweep with 32 deterministic projected Gauss-Seidel passes
on frozen live-collider geometry, including same-tire cross-axis effective mass and all upcoming
frame/thrust/air/water acceleration. Each pass replaces a tire wrench; only the final batch applies
native reciprocal impulses and commits wheel state. Finite spring/damper, tire friction circle,
rotor inertia and brake limits remain authored physical limits.

Virtual supports now use native-local pose authority like native contact constraints, avoiding
planet-absolute f64 roundtrips and free-attitude pose writes that repeatedly wake native bodies.
There is no new position/velocity approximation or relaxed sleep threshold. The restored sleep
regression verifies a settled four-tire rover sleeps, remains fixed and wakes under driver input;
a moderate real Terra incline test verifies brake hold then physical roll on brake release. A
sub-micrometer contact-impulse test verifies real small native displacement is retained. Orbit
wheel actuation/rails and shared owner completion remain subsequent work.

Targeted local evidence: EVA transfer/checkpoint/replay, spinning-carrier conservation, dynamic
walk/jump, low-gravity ballistic jump and finite orbital backpack resource/checkpoint/replay pass.
Main-game controls/avatar are committed in 65ac3f3: crewed --rover, F hatch/boarding, ground
WASD/QE, Space jump, H finite backpack, profile-specific operation hints and visible suit body
pieces inside the collision envelope. Root metadata validation passed; root TigerVNC and human
acceptance remain separate. Dry water cadence fix 7c02354 preserves native solver history instead
of rebasing unchanged timesteps due end-time subtraction rounding.

## Precise stellar crew transactions (root integration)

Exit, boarding and jumping now enter the carrier/actor's current source view and restore it on
success or refusal. Boarding across different stellar systems returns an explicit refusal before
any crew/resource change. Ground and spinning Orbit transactions in Beryl (4.24 light-years from
Sol) preserve identity, mass, COM/P/L and exact checkpoint/recording continuation. Tests reduce
COM in the common local frame before converting to split galaxy coordinates; summing an offset
onto an already rounded AU-scale global point would test reduction order instead of conservation.
The original conservation thresholds remain unchanged. Root evidence:
`/tmp/void-next-feature-evidence/eva-stellar-transfer-root.log` (2 passed).

A combined dry-contact regression exposed ULP retiming by the water stepping wrapper, which
cleared native solver history on nominal dry steps. The accepted dry cadence is now preserved;
EVA 6, vehicles 3, water 6 and aircraft 3 targeted tests pass together after that fix. Main source
has compiled, but root EVA dynamic GUI and human acceptance are still pending.

## Crewed rocket acceptance craft

`crewed_flight_rocket()` reuses the stock two-stage/RCS rocket and the same Seat module through
an authored side socket. `crates/assembly/data/crewed-rocket.json` is the exported Craft3 input,
usable directly by the existing main-game `--craft` loader. The external side seat is a declared
first-round carrier fixture, not a cabin/ladder simulation. Its flight command requires a healthy
occupied seat; the rocket RCS supply remains separate from the isolated five-kilogram backpack.
For ordinary-spacecraft EVA inspection run `void-app --rendezvous --craft
crates/assembly/data/crewed-rocket.json` and use F exit/board, H backpack and the profile HUD.
The spinning remote Orbit transaction test also uses this craft and preserves crew mass/COM/P/L.
