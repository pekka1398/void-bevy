# Modular aircraft first playable implementation

Branch `work/aircraft-assembly`, shared scope in playable-vehicles-and-multiscale.md.
Assembly means validated core APIs and geometry; no new VAB frontend is required.

## Behavior and ownership

`void_assembly::aircraft()` assembles a cockpit, fueled fuselage, left/right controlled
wings, two elevators, rudder, atmospheric jet and three passive Wheel gear parts.
`aircraft_airframe()` and public `mount_surface`/`mount_mirrored_pair` use the same
validated Craft/PartGraph connections a future editor uses. Full box geometry supplies
rendering, collider, principal inertia, drag face areas and contact bounds.
The airplane uses ordinary Fleet Ground/Bubble/Orbit owners and accepted fuel updates.
No aircraft runtime or artificial steering torque is introduced. Its command profile
is Aircraft, without a reaction wheel. Unsupported SAS requests return refusal and
are reflected in HUD/journal. Mixed/docked craft route controls from their root command.

Wings accept frozen pitch/yaw/roll commands through the existing wing polar; stall and
point angular velocity affect actual forces and torque. The jet uses standard engine
activation/crossfeed, finite LiquidPropellant, and an authored density envelope. The
simple density thrust law is not a complete real engine map.
Landing gear consumes the EVA/rover Wheel module, including suspension, rolling spin,
road-relative tire forces, steering, braking and reciprocal dynamic-support impulses.
Main gear supports the aircraft slightly behind its actual mass center with widened
physical struts. Tire graphics follow accepted suspension, steering and wheel spin.

Every aircraft part has an explicit ordinary Thermal module with cuboid radiating,
convection and heating areas, mass-scaled capacities and the existing failure limits.
Cuboid thermal exposure uses actual projected face area. Its wide wings cannot hide
behind a small shield merely because an obsolete cylindrical radius was used.

## Main-game acceptance

Run the main executable with `--aircraft`. This selects Terra and a declared near-flat
Hills recipe (maximum height 0.01 m), persisted in its world description and used by
both drawing and collision. This is an explicit acceptance world, not an undeclared
terrain replacement. `--planet` can select another atmospheric body; fixture conflicts
with terrain/world/load/replay/reentry/rendezvous/vacuum are rejected. `--craft` can supply
an authored airplane in that explicit runway world. Normal terrain remains available
with ordinary custom Craft/World launch paths.

- Space activates the jet; Shift/Ctrl changes throttle; X cuts it.
- W/S elevator, A/D roll, Q/E rudder and nose-wheel steering; hold B to brake.
- Pitch input is powerful at speed: brief elevator commands rotate the aircraft, release
  to neutral before a steep climb/stall. Pilot tuning remains part of GUI review.
- HUD reports airspeed, dynamic pressure, maximum section angle/stall, control input,
  fuel, thermal state and gear contact. T reports unavailable reaction-wheel SAS.
- F4 compares actual colliders, F6/F7 save/load, recording/replay preserves controls.

## Evidence and limits

Isolated -j2 targeted core checks on the branch: assembly/modules all-targets tests,
aircraft fleet-flight tests and assembly/modules/vessels/fleet-flight lib Clippy.
The main renderer/source is compiled and GUI-reviewed in the root-coordinated target.
Headless takeoff/approach uses sampled pilot commands, not direct force/attitude changes:
three-gear settling, powered taxi, physical wing takeoff, reduced-throttle descent,
wheel touchdown and braking, plus exact checkpoint/journal continuation.
These are acceptance fixtures, not automatic certification of arbitrary assemblies.
Existing wing-polar tests retain all their numerical thresholds.

No propeller/electric power, movable wing collision mesh, high-lift flaps, retractable
gear, compressor simulation, arbitrary curved-parent surface mounting or aircraft
impact/structural breakup are claimed. Surface mounts require named capacity sockets
on a cuboid face; the child mating point/normal must actually meet that face.
New geometry/poses require Craft3. Unchanged Craft2 is explicitly supported; old model
recordings/saves are refused. Aircraft model22 is reserved; root assigns final combined
checkpoint/model versions. Human acceptance has not yet occurred.

Coordinated root build/run:

```sh
cargo build -j 2 -p void-app --bin void-app
./target/debug/void-app --aircraft
```

The process opens paused. P starts simulation; Space activates the jet; hold Shift
until full throttle. Near 45–50 m/s airspeed, use brief W commands, aim for roughly
8–12 degrees nose-up and release before a steep climb. Holding W continuously is a
strong elevator command and can induce a stall. Reduce throttle and steer back toward
the surface for approach; use brief W for flare, then hold B and X after wheel contact.
Navball and AIR diagnostics are the pilot observations, not an automatic pilot.
The automated physical test uses 20 Hz sampled pilot commands for repeatability only.

At ae4928f the controlled acceptance sequence rose ~64 m at ~48 m/s, touched down at
~2.8 m/s vertical speed and stopped with brakes. Follow-up 41beb0a imports the shared
contact support-boundary correction 80091df; all three aircraft integration tests still
pass. This is headless evidence. Main GUI and human acceptance remain pending.

Final follow-up headless sequence after the contact correction reports ~64 m climb,
~48 m/s cruise, first wheel contact at ~5.3 m/s downward speed and ~35.6 m/s forward
speed, then ~0.061 m/s after braking. The previous ~2.8 m/s touchdown observation used
the older boundary velocity bookkeeping; it must not be quoted as the corrected
impact speed. The current approach fixture exercises a firm touchdown, and GUI pilot
handling/flare remains review work. No crash/structural failure claim follows from it.
