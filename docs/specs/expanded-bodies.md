# Expanded celestial bodies — exploration catalog v1

Scope: add offline exploration destinations to the main game's existing Sol analogue,
with the same Orbit/Bubble/Ground runtime, stable IDs, map/focus, world serialization,
render/terrain collision sampler and orbital launch action. N-body performance and
long-term Jupiter/Saturn stability experiments are explicitly deferred. No integrator
algorithm or tolerances changed. Shared development rules: `AGENTS.md`.

## Catalog and identity

`crates/orbit/systems/sol.json` remains the frozen 15-body golden fixture.
`sol-expanded.json` is a separate 58-body authored gameplay catalog (43 additions).
`void_orbit::expanded_sol()` loads it; `expanded_solar_scenery()` adds explicit scenery
and environments. Main-game Aurelia initialization and scene-test initialization choose
this expanded world. Ordinary landing presets, baseline `solar_scenery()` and existing
stellar-neighborhood fixtures remain unchanged. Home terrain and selected Aurelia spin
are retained. `InitialWorld::planet()` derives its system from the expanded world.

Existing stable IDs remain: ember=Io, rime=Europa, hollow=Ganymede, umber=Callisto,
haze=Titan, selene=Moon. Their expanded display names include the real equivalent.
Added stable IDs are lowercase real names, with punctuation removed:

- Ares: phobos, deimos.
- Velvet: amalthea (plus existing four Galilean equivalents).
- Halo: mimas, enceladus, tethys, dione, rhea, hyperion, iapetus, phoebe (plus haze).
- Azure: miranda, ariel, umbriel, titania, oberon.
- Abyss: proteus, triton, nereid.
- Pluto: pluto, charon, styx, nix, kerberos, hydra.
- Small bodies: ceres, vesta, pallas, hygiea, eros, bennu, ryugu; eris, haumea,
  makemake, quaoar, orcus, gonggong, sedna; halley, 67p, encke, halebopp.

This batch includes major planetary moons, not every known minor satellite. Mercury
and Venus have no moons. Dwarf planet moons beyond Pluto are outside this batch.

## Sources and initial-condition contract

Checked-in `systems/sources/expanded-catalog.json` records retrieval date, selected
satellite source rows, complete small-body API responses (including orbit solution,
epoch/equinox, physical references), explicit overrides and estimate methods.
Runtime never queries the network. `python tools/regenerate-expanded-catalog.py`
rebuilds `sol-expanded.json` deterministically from these checked-in snapshots and
the frozen baseline using only Python standard-library code. No live refresh occurs.

Primary sources:

- [JPL satellite mean elements](https://ssd.jpl.nasa.gov/sats/elem/).
- [JPL satellite physical parameters](https://ssd.jpl.nasa.gov/sats/phys_par/).
- [JPL SBDB API](https://ssd-api.jpl.nasa.gov/doc/sbdb.html), requests with `phys-par=true`
  and `full-prec=true`, query IDs preserved in provenance.
- [JPL planetary physical table](https://ssd.jpl.nasa.gov/planets/phys_par.html), for
  Eris/Haumea/Makemake radius, system mass and rotation period.
- [NASA Pluto fact sheet](https://nssdc.gsfc.nasa.gov/planetary/factsheet/plutofact.html),
  approximate Pluto orbit/physical constants. Pluto node/periapsis/phase are authored.

**Simulation t=0 is an authored game epoch, not an observational UTC/TDB date.**
JPL explicitly warns mean satellite elements are not suitable for ephemeris computation.
Their mean a/e/i/node/periapsis/phase are used to seed gameplay ellipses. Satellite
source epochs differ and are recorded; no propagation to a common date is claimed.
Laplace planes are approximated as parent equators, including Triton (its source
retrograde inclination is retained), Phoebe and Pluto moons. Nereid uses its source
ecliptic plane. Other source
planes map to the corresponding existing ecliptic/equatorial enum. Planet spin poles
remain the existing analogue poles, so this is not a precise real-system orientation.

Small-body source elements refer to heliocentric J2000 ecliptic osculating orbits at
the individually recorded SBDB TDB epochs. We reinterpret these source numerical
ellipses as **authored Jacobi** initialization: each subtree relative to parent and
earlier sibling subtrees, with their combined mass. Satellites and solar children
are sorted by semimajor axis. No claim is made that resulting Cartesian states match
Horizons, source osculating states, observed resonances or long-term system stability.
This explicit approximation avoids silently labeling heliocentric data as Jacobi data.
Future precise import must convert common-date Cartesian states into this hierarchy.

GM is converted km³/s²→m³/s² and divided by CODATA G. JPL zero/missing GM is unavailable,
not zero mass (Nereid uses an explicit 1000 kg/m³ authored spherical density estimate).
Where SBDB lacks mass, effective spherical volume times its density, or explicit
1000 kg/m³ authored density (500 kg/m³ for comets), is used. Missing TNO/comet diameters
are explicit authored scale estimates recorded per body, not measured values. Dwarf
JPL system masses are placed on the primary when omitted satellites are not modeled;
Makemake's table mass is itself a model estimate. No general unavailable-data fallback
is added: all values are committed explicitly and invalid physical data still panic.

Regular added moons use authored synchronous orbit-normal spin with source mean orbital
period; Hyperion/Phoebe/Nereid and Pluto's four small moons use explicit authored 24 h
north-pole rotation. Their actual irregular/chaotic spin is not modeled. Small-body
spin uses available SBDB periods or explicit authored 24 h; poles/phases are authored.
Pluto's 6.3872 d spin and approximate pole are not an exact Pluto–Charon coupled solution.

## Presentation, physics and compatibility

All added bodies and previously unconfigured Galilean/Titan equivalents have explicit
solid terrain, shared by rendering and collision. Shapes are spherical, with authored
craters (1% radius relief capped at 2.5 km), deterministic ID-seeded relief and authored
icy/volcanic/ochre/dark palettes. These are not measured shapes or calibrated colors.
All added atmospheres are explicitly absent; Titan atmosphere, cryovolcanism, comet
coma/tails, outgassing and non-gravitational comet acceleration remain unmodeled.

The O orbital fixture uses 25% radius altitude for bodies smaller than 1000 km radius;
existing large-body 400 km altitude is retained. This is a repeatable local test fixture,
a two-body circular initial state, not a flight/transfer or stability claim. Phobos
has an existing Laplace navigation SOI smaller than its physical radius: its default
navigation reference remains Ares even in this local fixture. Explicit map/body focus
can still select Phobos; full N-body dynamics remain unchanged. Recording `MODEL_VERSION` is **32**, rejecting
older model recordings explicitly because orbital launch action semantics changed.
World schema remains 5: the format is unchanged and new worlds embed their whole catalog.
Current-model checkpoint worlds preserve their embedded catalog; pre-32 model
checkpoints/recordings are rejected. No automatic old-save expansion/repair.

## Verification and human acceptance

Targeted headless checks: orbit tests (including frozen golden), expanded catalog
finite/positive initialization, all-body configuration/focus, representative new-body
local orbit, recording replay and checkpoint restore; baseline solar scenery and UI
presentation regression tests. No whole workspace run or long-term experiment required.
Record actual command/version/results separately on delivery. GUI screenshots do not
replace human acceptance.

Root coordinates the Bevy link. Build branch binary with `cargo build -p void-app -j 2`,
copy it to `target/acceptance/void-app-bodies` and record its checksum in
`target/acceptance/bodies-SHA256SUMS`. Run `tools/bodies-acceptance.sh phobos` (or `bennu`,
`enceladus`, `triton`, `pluto`, `halley`, `67p`) through TigerVNC. Inspect map/body labels,
focus and relative rendering, press O for a local orbital fixture, exercise pause/warp
briefly, and save/load. This checks initial exploration; long-duration warp is not
accepted as evidence of long-term system stability.

### Renderer integration correction

Main-scene integration exposed an inverse atmosphere-LUT coordinate boundary bug on
Deimos: reconstructing the top radius from squared radii produced
106200.00000000001 m, outside its exact 106200 m top. `transmittance_ray` now uses the
analytic shell radii at normalized y=0/1 and asserts normalized inputs. Physical
`transmittance_to_top` bounds remain strict. Regression tests cover Deimos, Phobos,
Bennu and 67P radii, full atmospheric/vacuum LUT construction, and rejection of invalid
coordinates/outside physical radius. Scenery golden comparison remains bit-identical.
Headless `cargo test -p void-scenery -j 2` passed all 19 tests on this branch after the
correction. This is a geometric endpoint correction, not an atmosphere-model fallback.
