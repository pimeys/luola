# Luolalentely — implementation design

How the cave flyer in this repository is built, why each decision was made, and
how to verify it. Genre reference and sourcing live in
[`game_mechanics.md`](game_mechanics.md); this file describes what the code
actually does.

## 1. What runs

A single binary, `luola`:

- Three-level campaign (`levels/*.toml`) with briefing cards, results cards with
  an itemised score, in-game help and a pause screen.
- Classic *and* modern control schemes, switchable at runtime (`C`).
- Deterministic 120 Hz simulation with input-log replays that verify themselves.
- Headless modes: level validation, scripted simulation, replay verification and
  offscreen PNG screenshots.

The stack is pure Rust and asset-free: `winit` (windowing), `softbuffer` (CPU
present), `cpal` (audio), `serde` + `toml` (data). No GPU API, no C/C++ engine,
no image/sound files — every pixel is rasterized by hand, every sound is
synthesized at runtime.

## 2. Module map

| Path | Responsibility |
|---|---|
| `src/sim/` | The simulation. No rendering, no I/O, no wall-clock time. |
| `src/sim/tuning.rs` | Every feel number, in one file. |
| `src/sim/world.rs` | `World::step`: control, AI, integration, rod solver, collision, gates, objectives, scoring, checksum. |
| `src/sim/terrain.rs` | Destructible material grid: organic brush shapes (`poly`/`disc`/`chain`/`blob`), the structural rock casing, per-cell hit points, and three materials — dirt (digs at once), granular walls (take several hits and *grab* a ship until it shoots free), rock (permanent). Carving, filling, textured render spans, marched collision. |
| `src/sim/water.rs` | Water as integer mass per cell: falls, spreads, levels out; active-tile stepping. |
| `src/sim/fnv.rs` | The 64-bit digest the replay checksum is built from. |
| `src/sim/ship.rs` | Hull, fuel budget, shield state machine, rod masses. |
| `src/sim/weapons.rs` | The roster: Wings' 33 specials plus the gun, as data (`WeaponSpec`), and the loadout a base re-arms. |
| `src/sim/level.rs` | TOML schema, validation, runtime level. |
| `src/sim/validate.rs` | Flood fill: sealed cave, reachable objectives, mission distance, fuel budget, and the position of any air the ship cannot get to. |
| `src/sim/replay.rs`, `inputs.rs` | Virtual actions, input frames, bit-exact replay format. |
| `src/render/` | Software rasterizer (`fb.rs`, `font.rs`), camera, particles, effects fan-out, HUD, world scene. |
| `src/audio/` | cpal mixer and synthesizer: engine loop, weapons, explosions, klaxon. |
| `src/app.rs` | winit shell, key bindings, screens, fixed-step accumulator, present. |
| `src/headless.rs`, `src/main.rs` | Windowless drivers and the CLI. |
| `src/image.rs` | Minimal PNG writer (stored-deflate, no dependencies). |

The dependency direction is one-way: `sim` knows nothing about `render`, `audio`
or `app`, which is what makes headless verification, replay checking and the
tests possible.

## 3. The six necessary properties, and where they live

From `game_mechanics.md` §1:

1. **2D side view, gravity down** — world coordinates are screen coordinates,
   `+y` is down, gravity is `(0, +g)` per level.
2. **Player-controlled attitude coupled to thrust** — classic scheme: rotation
   sets `omega`, thrust always fires along the hull. The hull is the only thrust
   vector, and that constraint *is* the game.
3. **Continuous gravity** — applied to every body, every tick, including the
   payload and debris.
4. **Inertia, no drag, no speed clamp** — `Body::integrate` is semi-implicit
   Euler with nothing else. Drag exists only in water.
5. **Lethal terrain** — any hull vertex entering a solid cell ends the run, swept
   from the previous tick's pose so a wingtip at any speed registers. The cave can
   be dug, but touching it is still fatal.
6. **A hard budget** — fuel burns with thrust; an empty tank kills the engine and
   the shield, and the ship becomes a falling rock.

## 4. Determinism

- Fixed timestep, `1/120 s`, semi-implicit Euler, fixed operation order
  (`control → AI → integrate → rod → collide → water → objectives → timers`).
  Water steps after the collisions, so a crater dug this tick starts pouring the
  next, and before the objectives, which never read it.
- The simulation consumes `InputFrame`s (`buttons`, `move_dir`, `aim`) and
  nothing else; replays store one frame per tick with `f32` bit patterns, so
  they round-trip exactly.
- `World::checksum()` hashes the canonical state — including the shape of the
  cave and the mass of every cell of water, both folded in through per-tile
  digests that only the carved or moved tiles recompute. `--replay` re-simulates
  and refuses a mismatch, so a run that digs differently or pours differently is
  caught even when the ship itself is in the same place. Particles, camera shake
  and audio are driven from simulation *events* and never feed back, so cosmetics
  cannot desynchronise a run.
- Rendering is decoupled: the app runs the accumulator and can drop time
  (`MAX_STEPS_PER_FRAME`) without perturbing a recorded run.

## 5. Physics and feel numbers

`src/sim/tuning.rs`, all in pixels and seconds:

| Knob | Value | Why |
|---|---|---|
| Gravity `g` | 80–130 per level | "Downhill is free"; higher on later levels. |
| Thrust `T` | `2.0 g` | Enough to climb, not enough to ignore the cave. |
| Rotation `ω` | 3.0 rad/s | Slow on purpose: attitude is planned, not reacted to. |
| Modern turn rate | 6.5 rad/s | Visual-only hull follow in the twin-stick scheme. |
| Fuel burn | 4.0 /s of thrust | ~25 s of continuous thrust per tank, plus pods. |
| Hull | V: nose `(11,0)`, wingtips `(-8,∓7)`, tail notch `(-3,0)` | An AUTS-style V, 4 collision vertices, 12 swept tests/tick. |
| Bullet | 430 px/s, 1.3 s, 0.14 s cadence, 10 on screen | Inherits ship velocity, so aiming under inertia is a skill. |
| Shield | absorb 0.5 s, down 0.9 s, recharge 0.5 s | Gravity Ace's frame-level generosity: multi-hits inside the window are survivable, and the shield protects *during* the recharge animation. |
| Escape timer | 45 s | After a reactor goes critical. |
| Carried mass | +0.25 per full tank, +0.29 per Autofire magazine | Thrust is `2 g` at launch weight and rises to at most `1.30×` as the load burns or is dumped. Level fuel budgets are tuned at the reference (full) load, so a light ship is only ever quicker. |
| Terrain hit points | dirt 1, granular 6, rock ∞ | Dirt opens on the first point of damage exactly as before; a granular wall eats several shots and holds a ship that touches it until the wall around the hull is gone. |
| Gate travel | 56 px/s default | Authored `[[gate]]` slabs slide between two points; a reactor-powered or hidden one is inert until a signal starts it. |

### The payload rod

The payload is a rigid body on a distance constraint anchored **behind the
hull**, so the rod applies a torque and the pair spins — the genre's signature
chaos. The solver removes relative radial velocity with mass-weighted impulses
(`SHIP_MASS = 1.0`, `POD_MASS = 0.4`).

The mass ratio was picked from measurements, not taste. Climb rate after 0.75 s
of full thrust:

| `POD_MASS` | Ship climb (towing) | Payload climb | Verdict |
|---|---|---|---|
| 0.4 | 25.2 px/s (37% of solo) | 38.3 px/s | chosen: costly but flyable |
| 0.6 | 12.0 px/s | 25.0 px/s | drone-like, frustrating |
| 0.8 | 1.8 px/s | 14.6 px/s | effectively cannot climb |
| 1.0 | −6.4 px/s | 6.4 px/s | payload levels unwinnable |

Solo climb for reference: 67.5 px/s. `tests/simulation.rs::towing_the_payload_costs_climb_rate`
pins this band so a future tweak cannot silently make payload missions
impossible or free.

### Damage rules

| Contact | Result |
|---|---|
| Ship hull vs terrain | Shield absorbs, else death (with a shield bounce) |
| Ship hull vs **granular** wall, slow | Grabbed: velocity eaten, ship held but alive; shoot the wall around the hull to get free |
| Ship hull vs **granular** wall, above `GRAB_MAX_SPEED` | A crash like any other: shield, else death |
| Ship hull vs an active **gate/crusher** | Shield absorbs, else death |
| Projectile / drone / mine blast vs ship | Shield absorbs, else death |
| Payload vs terrain, **on the beam** | Payload destroyed; a required payload fails the mission |
| Payload vs terrain, **free** | Settles where it lands (impact sound only) |
| Landing pad, slow contact | Refuel to full, reset the shield, re-arm the special (§13) |
| Reactor hit by a shot | Damage; on destruction defences go offline, the exit opens, the collapse timer starts |

The "free payload settles" rule exists because the alternative — any payload not
under the player's control dying within seconds — made level 2 a timer race
instead of a towing puzzle. Parking the payload on a ledge to scout a corridor is
now a legal, discoverable tactic; the danger is entirely in the rod.

### Digging and water

Terrain is a grid of cells — dirt, rock or empty (`sim/terrain.rs`). Shooting dirt
removes it, so the player can tunnel, open a floor and reshape the level; rock is
permanent, and that is what keeps an authored layout meaningful once the cave is
diggable (`game_mechanics.md` §2 files destructible granular walls under Gravity
Force 2 and AUTS). A bullet crater is `CARVE_BULLET` = 3.5 px; a blast is
`CARVE_BLAST` = 30 px scaled by the explosion's power, so a mine opens a room and
a reactor going up opens a cavern. Every crater goes through `World::dig`, which
also wakes the water above it. One weapon goes the other way: `Dirtball` adds
dirt through `Terrain::fill`, the craters' inverse, which is how a pilot builds a
ledge to park a payload on (§13). Dirt does not collapse when it is undermined — a
dug tunnel keeps its shape (§12).

Water shares the same grid: every cell holds 0..=8 units of mass
(`sim/water.rs`). It falls, runs down a diagonal over a lip, spreads sideways to
level out, and does it in whole units, bottom-up, serpentine per row. Two
properties matter:

- **It is cover.** Drag eats a shot within a body's depth, so a pool is a wall a
  gun round cannot cross; only the weapons marked `water_ok` plough through, and
  the torpedo and the water cannon are the two that do (§13).
- **It is deterministic.** Integer mass, fixed visit order, never iterated to
  convergence: the walk covers exactly the tiles that are active, so the cost
  tracks how much water is *moving*, not how much exists. A settled pool costs
  nothing per tick, and the replay checksum covers the whole body of water.
- **It levels by the column, to within a unit.** Sideways flow compares how much
  water each column is carrying, so two columns that can exchange water never end
  up differing by more than one unit, and a pool cannot step down at a seam. It
  is not a pressure solver (§12), and this is the rule with the visible
  consequence: *anything standing inside a pool gets wet up to the surface around
  it*, because its column is compared by how much water it holds rather than by
  where its floor is. Levels therefore keep their pools featureless — the reactor
  level stands its core **above** the waterline rather than on a plinth inside the
  pool, and the pool's surface is set by the lip that holds it.
- **A tick's tile cap cannot lose tiles.** Activity is capped at 256 tiles a tick
  so a surge flows over several frames instead of stalling one; the tiles the cap
  skips stay queued. Dropping them instead left the water above a wide blast
  sitting in the air forever — one carve wakes every tile above it, which is far
  more than the cap — and there is a test for exactly that.
- **It is cheap, and it pays only for what moves.** A 360k-unit pool at rest costs
  0.8 µs per tick; the same pool pouring through a hole costs ~10 µs per tick
  (0.24 ms worst case), against a 120 Hz tick budget of 8333 µs.
- **It has no pressure model** (§12). Water will not push itself up a pipe, but
  it pours through a hole in the floor — which is the behaviour the genre shipped
  with, and the one the levels are built around.

## 6. Hazards, furniture and objectives

Implemented vocabulary (from §7): turrets (slew, line-of-sight, predictive aim,
shut down when the reactor dies), drones (pulsed homing thrust, die on terrain),
mines (arm after 0.6 s, proximity fuse, chained), reactors (destructible core
with a hit count), payload pods (beam-able rigid bodies), fuel pods, landing
pads, exits (optionally locked and hidden until unlocked), water bodies (reduced
gravity plus heavy drag in proportion to how deep you are, bullets stopped by the
depth, and the engine making bubbles while it is under), **gates** (a solid slab
that slides between two authored points; it stops shots, crushes what it catches
and can be inert until powered), and per-level wind.

Gates, hidden exits and the level's authored `[[signal]]` edges form one graph,
and the wiring is deliberately small. A gate's `trigger` is `always` (the
default: live from the first tick) or `reactor` (inert until the core dies). With
**no** `[[signal]]` table the classic defaults run: destroying the reactor shuts
every turret down, opens and reveals the exit, and trips every `reactor` gate. A
level that authors even one `[[signal]]` replaces those defaults and states its
own edges; each edge is *when* (`reactor` is the only wired-up trigger) → `action`
→ target `index`, with four actions today: `open_exit`, `reveal_exit`,
`start_gate` and `power_down`. A hidden exit must be named by some revealing edge
or the level fails to load, which is how an alternate exit is gated behind a
switch, a reactor kill or a bridge.

Mission shape per level: reach an open exit; if `require_pod`, the payload must
come with you (either parked inside the exit or still on the rod at the moment of
escape); if `exit_locked`, the reactor must be destroyed first, which also starts
the collapse timer. Outcomes are scored separately (`ESCAPED`, `PAYLOAD
DELIVERED`, `REACTOR DESTROYED`), mirroring Thrust's and Gravity Ace's two
routes.

## 7. Controls

| Action | Classic (default) | Modern |
|---|---|---|
| Rotate | `A`/`D` or `←`/`→` | — (hull follows the aim) |
| Thrust / move | `W`, `↑`, `Shift` | `WASD` in the stick direction |
| Fire the gun | `Space` | Left mouse or `Space` |
| Fire the special | `F` | `F` |
| Beam | `E` (hold) | Right mouse or `E` |
| Change weapon | `A`/`D` or `←`/`→`, parked on a pad | `A`/`D`, parked on a pad |
| Jettison the magazine | `X` (hold) | `X` (hold) |

`P` pause, `R` restart, `C` switch scheme, `F1` record a replay, `H`/`F2` help,
`Esc` menu. Classic is the default because attitude planning is the genre; modern
exists because accessibility matters and the mechanics themselves are unchanged.

The rotation keys change weapon instead of turning **while the ship is parked on a
pad**, which is Wings' own ritual (§13): the same buttons that do one job in flight
do the base's job while docked. In flight the special is on `F` in both schemes,
and nothing about it can be swapped until the ship is back on a base.

`X` dumps the special's remaining rounds: the ship gets lighter and quicker
immediately, so the player can trade firepower for climb rate mid-mission. The
rounds are simply gone — crossing a landing pad re-arms the magazine and puts the
mass back (§5).

## 8. Levels

TOML, `#[serde(deny_unknown_fields)]`, validated on load: non-empty name, positive
gravity and fuel, at least one exit, a player spawn that is not inside rock,
payload/reactor consistency with the mission flags, and no mission-critical object
buried in terrain.

Terrain brushes come in four shapes, because the genre's caves are piles of discs
and a level that is a stack of rectangles does not read as a cave: `poly` (the
default; flat shapes for machine-cut decks and shafts), `disc` (`center` +
`radius`), `chain` (a disc swept along a `points` spine — walls, tunnels, arches)
and `blob` (a lumpy mass: a connected core along a spine with `lumps` discs of
0.45–1.0× `radius` scattered about it, `seed` choosing which). Each kind is
checked for the fields it needs and no others, so a disc with `points` is a load
error rather than a silently empty brush. All four rasterize into the same
material grid, and rock wins over dirt where they overlap.

Brushes carry a `material`: `dirt` (the default) is diggable, `granular` is a
destructible-but-tough wall that takes `GRANULAR_HP` hits of damage and holds a
slow ship against it (see the damage table in §5), and `rock` is permanent. A
level may also author moving geometry and switchable exits: `[[gate]]` is a
`size` slab that slides from `pos` to `to` at `speed` (default `GATE_SPEED`),
optionally inert until `trigger = "reactor"`; an exit with `hidden = true` starts
invisible and must be named by a `[[signal]]` that reveals it; and a `[[signal]]`
table replaces the built-in reactor wiring with explicit `when → action → index`
edges (§6). Water is authored as pools — `[[liquid]]` with a `points` polygon —
whose cells start full and then behave like water; a pool buried entirely in
solid terrain is a load error, because it would be invisible. Per level,
`water_density` and `water_drag` (defaults 0.15 and 2.6) tune how heavy the water
feels.

The grid carries a rock casing of its own, 20 px thick and outside everything the
level authored (`terrain::CASING`), so the cave's outer wall is **structural**: a
level cannot leak, and an author cannot forget to draw the shell.
`campaign_cave_boundaries_are_rock` still checks the grid's outer ring is rock,
and `the_casing_seals_the_grid_and_is_never_dug` checks that a blast against it
removes nothing.

The authoring rule: **anything the mission's challenge rests on is rock;
anything the player is meant to reshape is dirt; granular sits between** — tough
enough that a stray round does not open it, shootable once the player commits, and
slow enough to hold a ship that clips it (the AUTS trap). Each level's validated
shape (sealed cave, reachable objectives, fuel budget) is computed on the authored
brushes — the material does not change it, because rock, dirt and granular are all
solid to the reachability fill. Beyond the casing, a handful of load-bearing
brushes stay rock (the payload level's hangar deck, the reactor level's chamber
wall) and the interior is dirt, so a player who wants a shortcut can dig one; a
few small rock boulders sit embedded in the dirt the way the reference
screenshots have them.

`luola --validate` additionally flood-fills the cave on an 8 px grid and reports:

- **sealed** — the fill must never touch the level bounding box (a hole in a wall
  means the ship can leave the level);
- **reachable** — every pod, reactor, fuel pod, pad and exit must be reachable
  from the spawn;
- **mission distance** — shortest spawn → objective → exit route, and the fuel it
  needs under a cautious-pilot model (130 px/s cruise, 50% thrust duty, 2×
  mistake allowance).

Current campaign measurements (`luola --validate levels/*.toml`):

| Level | Playfield | Terrain | Water | Hazards | Exit route | Plan vs available fuel |
|---|---|---|---|---|---|---|
| First Descent | 1760×1325 | 13 brushes, 1.41M solid cells | a trough held between two ridges at the foot of the ring — the first splash | teaching, none | 1368 px | 42 / 158 (27%) |
| Payload | 2280×1340 | 20 brushes, 1.49M solid cells | a lake on the western deck behind a rock lip; shoot the deck and it drains into the sump below | 3 turrets, 2 mines | 1968 px | 100 / 216 (46%) |
| Reactor Run | 2550×1450 | 14 brushes, 1.89M solid cells | a tank on the east deck over the reactor hall; shoot the deck and it pours ~25 px deep into the hall, behind the lip and under the core | 5 turrets, 2 drones, 3 mines | 280 px | 160 / 226 (71%) |

Every level is 99–100% reachable, and all three carry a few hundred cells of
pocketed air outside their walls — invisible from inside the cave, and reported
by `--validate` rather than left to be discovered.

`--validate` also reports the sealed **pockets** it found — how many cells, and
where. A cave authored from discs collects them where a mass leaves air trapped
under its flanks, and that air is invisible on the map until you know where to
look, so the validator names the position rather than the author hunting for it.
Level 1 is at 96% reachable with 320 pocketed cells left in the corners.

## 9. Presentation

- Internal framebuffer at `window / scale` (320 rows target), blitted with
  nearest-neighbour integer scaling and letterboxing; no GPU, no shaders.
- Terrain is a per-cell material grid (`sim/terrain.rs`), rasterized once from the
  level's brushes and re-baked only for the columns a carve touched, so drawing
  the cave is still a run blit per screen column. Cells carry a static grain
  value, which is what gives dirt and rock their texture: a flat fill reads as a
  silhouette, grain reads as ground.
- The surface is derived, not stored: any solid cell with air above it draws the
  grass band, any with air below it draws the root/overhang rim. Digging a tunnel
  therefore grows grass along it for free.
- Water is drawn per cell from its mass, translucent so the dirt reads through it,
  with a brighter line where a cell has water above and air above that.
- Everything else is lines, arcs and fills: hull, thrust plume, shield arc,
  payload, beam, turret barrels, bullets with trails, pulsing reactor, radar
  (sampled from the same grid, so it shows the tunnel you dug), off-screen
  objective indicators with distances, and a 5×7 bitmap font written from scratch.
- Particles (sparks, smoke, debris, bubbles, shock rings) and screen shake are
  spawned from simulation events by `render/fx.rs`, which also maps events onto
  audio commands.
- Gates are drawn only while they are active and unhidden, as a slab with hazard
  bars, so an unpowered one is genuinely invisible in the cave.
- Cost, measured on this machine: a full frame (simulation + textured terrain +
  water + particles + HUD) is 600-800 µs at 640×360 and 2.2-2.5 ms at 1280×720,
  both linear in pixels — 4-5% and ~14% of a 60 Hz frame budget. The simulation
  itself is 3-5 µs of that; the cave (per-cell texture plus its spans), the water
  drawn over it, the particles and the HUD split the rest, and the two cave-heavy
  levels differ by ~25% mostly in how much water is on screen. The internal
  framebuffer targets 320 rows, so the shipped cost is the 640×360 figure.
  Terrain became about four times more expensive to draw than the flat span blit
  it replaced, which is the price of a per-cell texture the player can dig into.
- Audio is synthesized: the thruster is exhaust *noise*, not a tone — two
  independent white-noise streams split by state-variable filters into the three
  bands a rocket has (a deep combustion rumble, a mid roar, and the shear-layer
  hiss above it), with the band edges and levels tracking thrust and a sparse
  crackle once the motor is pushed, so it reads as a rocket rather than a synth
  pad. Then a noise-and-sub explosion, turret and player cannon transients, a
  metallic shield ring, pickup arpeggios, a klaxon while the escape timer runs.
  No device means the engine goes inert instead of failing.

## 10. Command line

```
luola [--level P] [--scheme classic|modern] [--mute] [--weapon W]   play
luola --validate LEVEL...                                 load, seal, reach, fuel
luola --headless [--script idle|wobble|dervish] [--ticks N] [--trace] [--record R]
                                                          [--weapon W]
luola --replay R [--headless]                             verify or watch a run
luola --screenshot OUT.png [--shots N] [--size WxH] [--no-hud] [--weapon W]
luola --map OUT.png [--level P] [--size WxH]              the whole level top-down
luola --list-levels
luola --weapons                                            the roster and its numbers
```

`--weapon W` mounts one of the 33 specials at launch, overriding the level's own
choice; it is written into the replay header, so a run recorded with it verifies
against the weapon it actually flew (§13). `--weapons` prints the roster with each
weapon's ammo, reload and damage.

`--map` exists because a cave is negative space and a cave authored from discs
cannot be judged a brush at a time: it draws every cell of the grid at once (rock
grey, dirt brown, settled water blue, one marker per objective) and prints the
reachability, which is how the levels in §8 were checked while they were being
drawn. It settles the water first, so a pool that would run out of its basin shows
up as a film across the floor rather than as a lake.

## 11. Verification

`cargo test` covers, in order of what they defend:

- **Determinism** — two runs of one input log agree; a replay survives
  save → parse → re-simulate with an identical checksum, terrain and water
  included; `f32` frames round-trip bit-exactly including subnormals and signed
  zero.
- **Collision** — swept hits at speed, the render spans agreeing with the
  collision test, and a body buried in rock being pushed back out the face the
  level is on rather than out through the far wall: an escape search that treats
  the empty margin around the grid as an exit is how a ship ends up outside the
  level, and there is a test for exactly that. A slow ship against a granular wall
  is grabbed, not killed, and released when the wall around the hull is gone.
- **Digging and water** — bullets crater dirt and never rock; a granular wall
  needs its whole hit count of damage and chips one point per hit before it opens,
  so the digest moves on a chip even when no cell is removed; opening the floor
  under a pool drains it downward, conserves every unit of water and leaves the
  pool level lower; two worlds fed the same inputs stay bit-exact across carving
  and pouring, because the cave and the water are both in the checksum.
- **Feel contracts** — rotate keys turn the way they read on screen, no drag or
  speed clamp, fuel burns at the documented rate and a dry tank produces nothing,
  shield timings across the absorb/down/recharge cycle (including "no fuel, no
  shield"), the towing climb band, and the mass economy: a fresh ship is the
  reference weight, burning fuel and dumping the magazine lighten it up to the
  `MASS_AGILITY_MAX` cap, and re-arming restores it.
- **Missions** — flying to the exit completes and scores; the beam captures a
  payload, tows it and delivers it for the payload score; towing costs climb
  rate; a payload settles when free and dies when slammed into rock on the beam;
  pads refuel and repair; a reactor kill drives the authored signal graph and the
  built-in defaults; an `always` gate slides, stops bullets and crushes; signal
  and hidden-exit authoring errors are refused with reasons.
- **Content** — the shipped levels load, are sealed, have reachable objectives and
  fit their fuel budget; malformed levels are rejected with reasons.
- **Level authoring** — each brush kind rasterizes to what it says (a disc is
  round and not a box, a chain joins its ends, a blob has a body everywhere and an
  outline that varies along its spine), a blob's `seed` picks its lumps and
  reproduces them exactly otherwise, the casing seals the grid and a blast against
  it removes nothing, and the validator reports *where* the unreachable air is
  rather than only that some exists.
- **Shell and renderer** — screens advance by keyboard, every screen draws real
  pixels, restart restores the initial state, and the blit scales, letterboxes
  and clips correctly.

Offscreen screenshots (`--screenshot`) exercise the same scene and HUD code the
window draws, which is how the visuals are checked on a machine without an
attached display session.

## 12. Deliberately not implemented

Listed so the scope is explicit rather than implied:

- **Multiplayer** (split-screen or networked). The simulation is
  replay-deterministic and input-frame-driven, which is the groundwork, but no
  second ship, split view or transport exists.
- **A level editor.** The format is data-driven and validated from day one, so an
  editor is an addition, not a migration — but the tool is not here.
- **Terrain that collapses.** Dirt holds its shape when it is undermined, so a
  tunnel is a route and not a burial; a granular wall is a tough static wall, not
  a pile of falling blocks. Simulating falling material would be a second
  simulation, and the genre's own caves are full of undercuts, stalactites and
  overhangs that only exist because the dirt stays put.
- **Water pressure.** Water falls, spreads and levels out column by column; it
  will not climb a pipe, and a hard disturbance leaves a slight tilt rather than
  a perfectly flat surface. A pressure solver means iterating to convergence,
  which makes the tick cost depend on the level rather than on the input, and is
  not worth it for a cave you fly through.
- **Gamepads.** Bindings are a table in `app.rs`; adding a device is a new table,
  not a code change.
- **Lives.** Per §10 of the reference doc, fragility plus the shield plus a fast
  restart replaces them.

## 13. The weapon system

Wings v1.40's `WEAPONS.DAT` is 35 fixed-width records of 52 bytes each — a 32-byte
NUL-padded name plus five little-endian int32 fields, 1820 bytes — and 33 of them
are the selectable special weapons; `Base` and `Cannon` in the same table are the
level's furniture rather than weapons. The extraction, the verbatim table and the
count reconciliation are in
[`finnish_cave_flyer_weapons.md`](finnish_cave_flyer_weapons.md) §4, and §7 of the
same file distils the rule the whole Finnish line shares: one light gun that is
always there, plus one exotic secondary chosen while docked. `game_mechanics.md`
§13 is the catalogue of exemplars to read it against.

What Wings does *not* document is what a record's five integers mean. The manual,
the readme and every review leave the columns unnamed, so §4 reproduces the values
raw and asserts no meaning for them. Everything below about how a weapon
*behaves* is therefore **our design**, derived from its name and from what this
engine can express: the names are the `WEAPONS.DAT` records verbatim, every row of
`src/sim/weapons.rs::build` is a proposal, and nothing in this section claims
provenance it does not have.

### The two triggers

Every ship carries two weapons, and only one of them is ever a choice:

- **The gun** (`WeaponId::Gun`) is always mounted and unlimited, on `BTN_FIRE`. It
  is the reference bullet of §5 — `BULLET_SPEED`, `BULLET_LIFE`, `FIRE_COOLDOWN` —
  and it is deliberately not in the loadout, because there is nothing about it to
  spend or restock.
- **The special** is one of the 33, on `BTN_SPECIAL`, with ammo from its
  `WeaponSpec` and the spec's `reload` between shots. `Ship::loadout` holds the
  three things the choice implies: which special, how many shots are left, and the
  reload clock.

The split is §7's first constant: the weapon you always have is light, the second
trigger is the exotic one, and the ammo count is what makes that true. A special
is a budget, not a second gun; running it dry drops the pilot back to the gun.

### Bases

Wings changes the special at a base with the turn buttons, parked; so does this
game. `World::docked_on_pad()` is the whole gate — the ship alive, its speed below
`PAD_LANDING_SPEED`, and its position inside a `[[pad]]` rectangle — and while it
holds, `BTN_WEAPON_PREV`/`BTN_WEAPON_NEXT` step the level's list in roster order,
wrapping, with `WEAPON_CYCLE_COOLDOWN` between steps. A step re-arms the new
special, so the base hands it over full, and `Event::Special` records the
selection for the HUD and the audio.

Landing on a pad rearms the mounted special along with refuelling and repairing
(§5), so the base ritual is also the resupply. That is what makes it a decision
rather than a formality: leaving the cave costs time, and coming back without
having spent anything gains nothing.

### Which weapons a cave allows

A level narrows the pool with a `[weapons]` table — this game's `W_SELECT.DAT`, the
shipped 351-byte table of per-player 1/0 flags that says which weapons a match may
use (§4). `available` names the pool, parsed by `weapons::parse` so case, spaces,
underscores and hyphens are all the same, and `start` names the launch weapon:
Wings' record flagged `1`, the one already mounted when the run begins. A missing
table means all 33, starting at the first record (`Autofire`); an empty
`available`, a duplicate entry, an unknown name, or a `start` outside `available`
is a load error, and the message carries the roster.

### Five kinds, and the flags that do the rest

Thirty-three names do not get thirty-three code paths. A special is one of five
`Kind`s, and everything else about it is a flag on its `WeaponSpec`:

| `Kind` | What the trigger does | Weapons |
|---|---|---|
| `Bolt` | A projectile that flies; gravity, homing, bouncing and piercing are flags, so one path covers guns, drills and ricochets. | `Autofire`, `Dumbfire`, `IonCannon`, `Multicannon`, `Shotgun`, `Missile`, `Bats`, `Rockets`, `Torpedo`, `Nucleus`, `Digger`, `Dirtball`, `Bouncer`, `Freezer`, `Net`, `Harpoon`, `Poison`, `PoisonGas`, `Fireworks`, `Nuke` |
| `Shell` | A projectile that arcs and then detonates, on its fuse or on contact when the fuse is zero. | `Bomb`, `GrenadeLauncher`, `Splinterbomb` |
| `Place` | Lays a `Gadget` in the world, which then lives its own life in the tick loop. | `Mine`, `Landmines`, `PlasticExplosive`, `Gravitor`, `Troopers` |
| `Stream` | A short-range cone projected while the trigger is held; the cloud it leaves is what does the work. | `Hellfire`, `Watercannon` |
| `SelfEffect` | Applied to the ship instantly; nothing leaves the hull. | `Shield`, `Teleport`, `ElectricBlast` |

| Flag | What it adds |
|---|---|
| `gravity` | the fraction of the level's gravity the shot feels — shells arc, bolts do not |
| `homing` | turn rate toward the nearest hostile |
| `bounce` / `pierce` | terrain reflections, and targets pierced before the shot dies |
| `blast` / `burst` | detonation radius, and the fragments spawned when it dies |
| `count` / `spread` | projectiles per shot, and the fan they leave in |
| `cloud` + `cloud_ttl`/`_radius`/`_dps` | leaves a `CloudKind` (poison, gas, flame, water, sparks) behind |
| `carve` / `fill` | dirt removed on impact, and dirt *added* — `Dirtball` is the only builder |
| `radius` | the hit radius against turrets, drones, mines and the reactor |
| `push` | shove applied to bodies inside the shot or its cloud |
| `water_ok` | keeps working underwater |
| `effect` + `ttl` | what a hit does beyond damage (`Freeze`, `Net`, `Tether`, `Shield`, `Blink`, `Emp`) |
| `gadget` + `ttl` | what `Place` lays down, and how long it persists |
| `blink` | how far a `Blink` travels |

So `Shotgun` differs from the gun by a `count`/`spread` fan, `Missile` by
`homing`, `Digger` by `pierce` and its crater, and `Nuke` by a single shot and the
roster's largest `blast` — and none of them is a code path. A `Place` weapon hands
a `Gadget` to the world, a `Stream` weapon lays a `Cloud`, and a `SelfEffect`
never leaves the ship.

### The rules that keep them fair

- **A blast can kill the ship that fired it.** `blast_damage` hurts the pilot only
  when the blast centre is within `SELF_BLAST_FRACTION` of the radius, while an
  enemy blast hurts at the full radius. A bomb is not a point-blank melee weapon.
- **A mine you laid is a weapon, not your own trap.** `Mine::from_player` marks
  it; it arms after `GADGET_ARM_TIME` so it cannot detonate on the hull that laid
  it, ignores that ship entirely, and fires on whatever hostile comes inside
  `MINE_PROXIMITY`. Level mines and laid ones share one container, but only the
  player's count against `MAX_LAID_MINES`, so a cave cannot fill with them, and
  each carries a timer and disappears when it runs out.
- **Water stops everything but the weapons marked for it.** A shot in water is
  dragged and dies where it stops unless `water_ok` is set — `Watercannon` and
  `Torpedo` are. Streams lay clouds in front of the ship, and because each cloud
  carries its own damage accumulator (`CLOUD_TICK`), overlapping clouds overlap in
  effect: point-blank flame is lethal and distant flame is not. That is the whole
  reason `Hellfire` is a close-range weapon.
- **The Harpoon moves you without paying fuel.** While `BTN_SPECIAL` is held after
  the harpoon bites, the anchor reels the ship in (`TETHER_PULL`, capped at
  `TETHER_MAX_SPEED`) and burns no fuel — but one shot holds only its `ttl`
  seconds, so it cannot replace the thrust the levels are budgeted around, and the
  fuel budgets in §8 still mean something.
- **The Shield weapon keeps the shield charged.** `Effect::Shield` sets
  `shield_field` for the spec's `ttl` and repairs the state machine, so the bubble
  lasts its duration regardless of hits — §5's generosity, bought with ammo.
- **The Teleporter refuses a blink with nowhere to go.** `Effect::Blink` casts the
  spec's `blink` distance forward and stops short of terrain; when there is no
  room (`BLINK_MIN`) the blink does not happen and costs no ammo, because
  `apply_self_effect` reports whether it fired and only a fired effect spends.

### Replay and determinism

- `World::checksum` folds the loadout in — the special, its ammo, the reload
  clock, `shield_field`, and whether the harpoon is tethered and where — so a run
  that spent its ammo differently, or swapped weapons at a base, cannot verify
  against one that did not.
- A replay is format 2 (`# luola-replay 2`) and carries one `weapon <name>` line:
  the launch special, which comes from the level and the command line rather than
  from the pilot's inputs. Every later swap is in the input frames —
  `BTN_WEAPON_PREV`/`NEXT` are buttons like any other — so a run that changes
  weapons at a base re-simulates the change exactly.
- `--weapon <name>` mounts a special at launch and is recorded in that header, so
  a verification run reproduces the loadout; `--weapons` prints the roster.

### Deliberately not ported

Alongside the general list in §12:

- **Wings' per-match weapon banning UI.** `W_SELECT.DAT`'s role is covered by a
  level's `[weapons]` table: an author decides what a cave allows. There is no
  lobby, no per-player flags and no screen for it.
- **The five-int parameters' semantics.** The columns are unrecovered (§4), and
  naming them would assert provenance the sources do not support. The values are
  reproduced raw and the behaviours are ours.
- **Wings 2's player-authored `weapons.dat`** (§5 of the weapons doc). The sequel
  lets pilots author weapons and this repo's table is compiled in; levels are
  data-driven, weapons are not.
