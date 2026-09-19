# Luolalentely — genre mechanics reference

Working notes for building a cave flyer ("luolalentely", *gravity shooter*, *Thrust-type*, *Gravitor*).
Compiled from the Finnish/English Wikipedia cluster, MobyGames, Pelit, the Gravity Force 2 project
pages, the Turboraketti fan site, Gravity Ace's design docs/devlogs, and the games' own manuals/readmes.

Claim discipline: everything stated as fact is sourced below. Tuning numbers that sources do not give are
marked **[proposal]** — they are starting points for our own iteration, not quoted constants.

---

## 1. Definition

> "This sub-genre goes by many names, such as *Gravity shooters*, *Cave-flyers*, *Thrust-types*, *Gravitors*.
> A type of shoot 'em up in which the protagonist pilots a rotatable craft with thrusters while being subject
> to gravity and, often, the craft's inertia. Gravity shooters feature multi-directional, 360-degree movement
> and shooting. They also feature free-scrolling stages that allow the players to roam freely around the map
> and approach targets however they wish. The action is presented in a side-view and usually takes place in
> enclosed, cavern-like environments … filled with various types of enemies and hazards."
> — MobyGames genre group 6133

> "Keskeisenä elementtinä on inertia- ja painovoimalakeja noudattavan avaruusaluksen ohjailu kaksiulotteisessa,
> luolamaisessa maastossa."
> — Yhteishyvä, *Konsolipelisanakirja* (2012), the source the Finnish Wikipedia article is built on

> "Pelaaja ohjaa painovoiman vaikutuksen alaista alusta kääntämällä sitä ja käyttämällä työntövoimaa."
> — fi.wikipedia, *Turboraketti* / *Rocket Zone*

Six necessary properties. Drop any one and it is a different genre:

1. **2D side view**, free-scrolling or single-screen, ground/gravity is *down*.
2. **Craft orientation is player-controlled** and is coupled to thrust direction (in classic mode also to
   fire direction).
3. **Gravity acts continuously** on the craft — flying is a perpetual fight against falling.
4. **Inertia**: velocity persists; there is no instant stop, no friction unless a level says so.
5. **Terrain is lethal**, usually instantly. The cave is the primary antagonist.
6. **Fuel or another hard budget** bounds how much thrust you get, forcing planning.

Not the same genre: *Lunar Lander* (descent/landing simulation, no dogfight, no free roaming — treated as a
separate genre by fi.wikipedia, which files it under "Muut lajityypit"), scrolling shoot 'em ups (no gravity,
no rotation), platformers, twin-stick arena shooters without gravity (the "modern controls" variant of Gravity
Ace sits closer to this, see §5).

---

## 2. Lineage

| Year | Title | Contribution |
|---|---|---|
| 1962 | **Spacewar!** (Russell, Graetz, Wiitanen, PDP-1) | Vector 2D, gravity well, wrap-around arena, local multiplayer, first gamepad, silent fire button |
| 1969 | *Lunar Landing Game* (Jim Storer, FOCAL, PDP-8) | Text lunar lander genre: gravity + limited fuel |
| 1973 | *Moonlander* (Jack Burness, DEC GT-40) | First graphical lander; vector cross-section of terrain |
| 1979 | *Lunar Lander* (Atari), *Asteroids* (Atari) | The two halves: gravity+fuel, and rotate-and-thrust with 360° shooting |
| **1982** | **Gravitar** (Mike Hally, Rich Adam, Atari) | **The genre's template**: Lunar Lander gravity × Asteroids controls, side-view precision flying, turrets, reactors, fuel, escalating stage modifiers |
| 1986 | **Thrust** (Jeremy C. Smith, Superior Software) | Vectorised the template, added the tractor-beam pod (coupled-body physics), made the genre popular across every 8-bit machine; "thrust-style" became a term |
| 1987 | *Oids* (FTL) | Gave it a mission structure (rescue androids), shields, weapons with ammo economy |
| 1989 | *Gravity Force* (Kingsoft, Amiga) | Commercial Thrust clone; 1994 fan sequel *Gravity Force 2* added destructible walls, single-player missions, split-screen **and** null-modem 2P |
| 1992 | **Turboraketti** (Heikki Kosola, Amiga) | Finnish multiplier: split-screen deathmatch + **Turboranking** race/score economy; started the Finnish wave |
| 1995 | *AUTS* (The Kudos, DOS) | 2–4 players on one keyboard, destructible and granular walls, landing pads for repair/weapon swap |
| 1996 | *Wings* (Miika Virpioja) | Deathmatch + Last Man Standing, serial/modem play, 33 special weapons |
| 2006 | *Wings 2* | Networked cave flyer, 6 modes, eject-from-ship, level and weapon editors |
| 2014 | *Gravity Force 2* source released (CC BY-SA 4.0) | Whole 11k-line 68k assembly codebase public |
| 2019– | **Gravity Ace** | Modern reference implementation: level editor, dual control schemes, shield design, devlog documenting the tuning |
| 2026 | *Rocket Zone* (Timo Pantsari, browser) | Current Finnish entry; cites Turboraketti, Gravity Force, Thrust |

Regional shape: the genre was born in US arcades (1982), popularised on European 8-bit home computers (1986),
flourished in the Amiga/ST/DOS shareware scene 1989–1999, and survived through hobbyists and indie
reinterpretations. Finland is a distinct outlier — Pelit calls Turboraketti the best-known domestic
representative and notes it "synnytti Suomen shareware-skenessä useita muita luolalentelyvariantteja";
the *Luettelo suomalaisista shareware- ja freewarepeleistä* lists 18 Finnish cave flyers
(A2, Assault Wing, AUTS, Avaruusromu, Fuse, KaBoom, KOPS, Orb Wars, PP 2, Rocket Chase, Spruit, Turbis,
Turboraketti, Turbåraketten, V-Wing, Wham!, Wings, Wings 2).

---

## 3. How it looks

- **Side view, terrain as silhouette.** The cave is a filled polygon mass; the play space is the negative
  space. Historically vector or line-drawn (Thrust, Gravitar, Turboraketti, AUTS' V-shapes), now pixel art.
- **Everything reads as motion.** Ships are 3–12 px triangles/flying-Vs; the visual payload is the *thrust
  plume*, the *rotation*, a line trail behind bullets, screen shake — Gravity Ace's devlog is largely about
  these layers because they carry all readability.
- **HUD** is minimal and legible at a glance: fuel gauge, shield/recharge ring, weapon + ammo, lives/race timer,
  score. Original *Thrust* had 3 lives; Gravity Ace dropped lives for fragile-ship-plus-shield.
- **Camera**: free-scrolling levels (MobyGames: "free-scrolling stages"), sometimes several screens wide, with a
  radar/needle for off-map objectives. Fixed-screen variants: Gravitar's single-reactor chambers, early
  single-screen dogfight maps.
- **Level furniture**: turrets, reactors/energy cores, landing/repair pads, fuel pods, gates, doors, moving
  walls, granular or liquid media, ice, stalactites, props.

---

## 4. The physics model

The whole genre is one integrator plus a collision test. Get this right and everything else is content.

### 4.1 Equations of motion

Per fixed timestep, per ship (semi-implicit Euler is stable and is what feels right):

```
a = g + thrust * (cos θ, sin θ)          # g = (0, +g) in screen coords
v = v + a * dt
p = p + v * dt
θ = θ + ω * dt                            # ω = ±rotate_rate while rotate key held
```

Notes that decide the feel:

- **No drag, no max speed clamp.** Inertia is the genre. If you clamp speed you punish the skill of
  arriving fast and precisely; if you add drag you soften the gravity problem into a hover problem.
- **Thrust-to-gravity ratio is the single most important number.** With `T/g ≈ 1` the ship barely climbs and
  every manoeuvre is a commitment; at `T/g > 2` the cave stops being a threat. Classic commercial values sit
  in the low multiple **[proposal: start at `T = 2.0 g`, iterate]**.
- **Rotation rate must be slow enough to punish.** Gravitar's reputation for difficulty is explicitly
  attributed to a ship that "rotates too slowly to allow a player to correct their mistake"; the genre's
  skill is *planning the attitude*, not reacting.
- **Gravity may be per-level** (Gravity Ace exposes gravity and starting fuel as level properties), and the
  classic escalation trick is modifying it: Gravitar round 1 normal, round 2 *repulsive* (inverted), round 3
  invisible terrain, round 4 both.
- **Fuel is the throttle budget.** Thrust burns fuel; with the tank empty the ship is a falling rock.
  Gravity Ace: "your engines are powerful but they use a lot of fuel … you'll need fuel to make your escape so
  conserve enough for the return trip."

### 4.2 Coupled bodies

The pod/tractor beam is the genre's signature second-order problem:

> "The ship and pod are subject to gravity and inertia, and being connected by a stiff rod can end up spinning
> around each other, out of control. Hitting the walls of the cave with the ship or the pod results in death."
> — en.wikipedia, *Thrust*

Two implementation choices, both shipped:

- **Rigid/rod constraint** (Thrust, Gravity Ace's fixed joint): the payload is a second rigid body on a
  distance constraint. Angry spin is emergent and is the whole point.
- **Soft spring/beam** (wobblier, friendlier) **[proposal: only if playtests show the rod is too brutal]**.

Crew and cargo are also the level's "key": *Oids* lands on flat ground to pick up rescued androids;
Gravity Ace beams a reactor core out and hauls it to the exit.

### 4.3 Collision and damage

Three models, in increasing generosity:

| Model | Games | Feel |
|---|---|---|
| **Instant death on terrain contact** | Thrust, Gravity Force, Gravitar, most Amiga/DOS era | Maximum tension, maximum punishment; requires generous restart loops |
| **Instant death, shield absorbs one hit then recharges** | Gravity Ace | Modern default; see below for the timing trick |
| **HP / bouncing walls / granular media** | Oids, AUTS (walls take damage, granular walls trap you) | Softer, supports longer sessions and multi-hour structures |

Gravity Ace's shield design is worth copying exactly — it cheats in the player's favour at the frame level:

- one hit destroys the ship normally; the shield eats the first hit and then goes down;
- on hit: shield begins disappearing (600 ms animation), sound plays for a full second, **but collision stays
  active for the first 500 ms**, so rapid multi-hits can be absorbed;
- on recharge: shield *starts absorbing collisions at the beginning of the 500 ms visible-recharge
  animation*, i.e. it protects before the player can see it.
- the shield does not disable on trivial contacts (asteroids, debris); it deactivates when fuel runs out.

Result quoted by the author: "narrow escapes", "bounce off those walls just before my shield died" — the
player reads unfair luck, and it is scripted.

**[proposal] starting damage matrix for our game**

| Contact | Result |
|---|---|
| Terrain, at any speed | Ship death (shield rules above) |
| Projectile | Ship death / shield pop |
| Payload hits terrain | Payload lost; mission fails or ship explodes (Thrust kills both) |
| Granular wall | Ship sticks, can shoot itself free (AUTS) |
| Landing pad | Repair, refuel, weapon switch |

---

## 5. Controls

### 5.1 Classic: rotate + thrust ("tank controls")

Three to five bindings, no aiming:

| Action | Typical binding |
|---|---|
| Rotate CCW / CW | `Z`/`X`, `A`/`S`, `←`/`→` |
| Thrust | `Space`, `↑`, `Shift` |
| Fire | `Enter`/`Return`, `B` |
| Shield / beam / special | extra key, hold-to-use |
| Pause | `P` |

(*Thrust*-style web clone, spwebgames.com, documents exactly this table.)

Properties: aiming is a *consequence* of flying; firing and thrusting compete for the same attitude; the skill
ceiling is attitude planning under gravity. Turboraketti is played with joysticks, split screen, one machine.

### 5.2 Modern: twin-stick / mouse aim

Gravity Ace's default. Movement and aim are decoupled: left stick (or WASD) moves, right stick (or mouse) aims;
"the ship moves instantly in the direction you move the stick. Ship rotation is visual only and can be ignored."
Classic mode is retained as an explicit, discoverable option ("for retro purists … more difficult but also gives
a more retro vibe") — and the author notes unifying the schemes into one option menu mattered because the old
dual mapping "appeared like a BUG".

Trade-offs:

| | Classic | Modern |
|---|---|---|
| Skill ceiling | Very high; attitude planning | High; positioning and aim |
| Accessibility | Hostile to new players | Readable immediately |
| Identity | *Is* the genre | Genre-flavoured twin-stick |
| Multiplayer fairness | Symmetric, timeless | Better for 2-stick dogfights |

**[proposal]** Ship classic as the default for a purist luolalentely and offer modern as an accessibility option;
if the game is multiplayer-first, invert that.

### 5.3 Split duties (co-op)

Gravity Ace's input rework made one-ship co-op free: all devices stay live, so one player flies while another
shoots and beams. The author flags it as an explicit future mode with locked action→input bindings. Cheap,
distinctive, and it fits the Finnish heritage (2–4 players around one keyboard: Turboraketti, AUTS, Wings).

---

## 6. Verbs, missions and scoring

Two families, historically:

**A. Mission/objective (single-player).**
- *Gravitar*: pick a planet, destroy cannon towers while managing fuel, shoot the reactor core and escape
  before the timer expires (bonus points + 7500 fuel); four escalating universe variants.
- *Thrust*: shoot turrets to stop fire, shoot the reactor to make it critical and destroy the planet's
  defences, tractor the pod, fly it up out of the cave; the escape is timed.
- *Oids*: destroy android factories, land, pick up Oids, deliver them to the mothership.
- *Gravity Ace*: fly in, take out drones/turrets, beam the reactor core, get out with enough fuel.

**B. Arena (multiplayer-first).**
- *Turboraketti*: split-screen deathmatch with a **Turboranking** meta-layer — points for kills, for
  time-trial runs on five per-level race routes, for surviving ships; handicap points between good and bad
  players; the winner takes the pot, the loser loses it. Readmes and the fan site are explicit that the game
  is mostly *tactics*: strip weapons and fuel to be fastest for the race, accept being defenceless; the
  opponent may mine your route and your pad while you're in the menu.
- *Gravity Force 2* / *Gravity Power*: dogfight and **race** modes, levels shared and edited by the community.
- *AUTS*: 2–4 players, last-man-standing, one base gun plus one special weapon chosen at pads.
- *Wings 2*: six modes, network or split-screen, eject and fight on foot.

**Design lesson from Turboraketti**: the score economy (mass-vs-speed, handicap, route racing, mineable routes)
is what keeps a 1992 two-button game alive for decades. A pure deathmatch has a short life; an economy with
multiple ways to score per match has a long one.

---

## 7. Hazards and level vocabulary

- **Turrets / cannon towers** — line-of-fire obstacles that also gate the objective (Gravitar, Thrust).
- **Reactors / cores** — the mission object; destroying one ends the level by force, beaming it out is the
  skill route; each may have separate `Destroyed` and `Beamed` outcomes to script.
- **Drones / homing enemies** — mobile pressure that punishes hovering.
- **Mines** — area denial; in multiplayer, denial of race routes and landing pads.
- **Granular / sticky walls** — collide-and-stick, can be shot free (AUTS).
- **Destructible walls** — tunnels open under fire (Gravity Force 2, AUTS); can also close.
- **Liquids, wind, ice** — media that change control response or buoyancy; Turboraketti has wind and a liquid
  the ship can dive into but moves slowly in.
- **Moving geometry** — crushers, gates, hidden power-ups (`Hidden` until the core is destroyed or beamed).
- **Signals** — Gravity Ace connects events: `Destroyed` → reveal exit, buttons → laser gates, cooldown and
  phase properties for repeating behaviour. This is the cheapest way to author puzzles without code.
- **Invisible / inverted-gravity stages** — Gravitar's endgame modifiers; extremely effective, use sparingly.

---

## 8. Level design patterns

1. **Chamber + throat.** A wide cave to fight in, joined by a corridor narrower than the ship's turning circle.
   The corridor is the level's boss: you must already be aligned when you enter it.
2. **Gravity-assisted routing.** Down is free; up costs fuel. Design one-way drops and expensive climbs;
   Gravity Ace's own tip list says "gravity can help you move downward without fuel" and "use your engine in
   short bursts; let inertia work for you".
3. **Fuel accounting as the real timer.** Place refuelling stations and fuel pods so that the *return* leg is
   the puzzle. The classic failure state is winning the objective and then not being able to leave.
4. **Payload-aware geometry.** If the beam exists, doors must be wide enough for ship + pod, and the rod's
   swinging radius must be accounted for; the interesting levels make the pod a hazard.
5. **Readable hazard placement.** Gravity Ace added off-screen indicators for important friendly objects after
   players destroyed things they could not see — "it didn't feel fair".
6. **Reverse the teaching.** Players arrive trained by platformers: Gravity Ace's tutorial was rewritten so the
   first movement objective goes *left*, purely to break the run-right reflex.
7. **Geometry as keys.** Gravity Ace's shipped levels are built from overlapping wall polygons (each ≤500
   vertices for performance) with gaps too small to fly through — cheap way to build large caves with a coarse
   brush.
8. **Author in an editor, not in code.** Gravity Force 2 shipped a level editor and community levels; Gravity
   Ace's editor is the same tool the designers used, with campaigns, undo, vertex editing, object properties
   (hidden, phase, cooldown), signal wiring and dialog nodes. A cave flyer without an editor has a
   fraction of its content budget.

---

## 9. Multiplayer

Historical progression, all of it still valid:

| Transport | Examples | Notes |
|---|---|---|
| Shared keyboard, split screen | Turboraketti (2), AUTS (2–4), Wings (4) | Cheapest, loudest, best local feel; keyboard ghosting is a documented real-world failure mode (AUTS) |
| Serial / null modem | Gravity Force 2 v1.10, Wings | Two machines, 90s style |
| LAN / internet | Wings 2, Gravity Force 2 (2026 web remake) | Gravity Force 2's web port now does online play, AI opponents, local split screen, and spectating |
| Same-machine "modern" | Rocket Zone (2026) | Single-player vs AI + local 2P; browser delivery, keyboard/gamepad/joystick |

Determinism matters the moment you go networked or want replays/score verification: fixed timestep, no
floating uncertainty in the integrator, integer or fixed-point where the original design allows. **[proposal]**
for a Rust implementation: fixed `dt` (e.g. 1/120 s) accumulator, `f32` fixed-order integration, and an
input-log replay format from day one — it makes netcode, AI training and "world record time" leaderboards
(the GF2 community maintains race records) possible for free.

---

## 10. Why the genre works — the best parts

1. **A tiny rule set with a huge skill space.** Four inputs, one integrator, one collision test; mastery is
   unbounded. The result is the "10 000 hours" property Turboraketti's fan base describes: the game only
   *starts* after thousands of rounds.
2. **The world is the primary antagonist.** No AI needed to make the game hard. Terrain is dumb, honest and
   lethal, which makes failure feel attributable to the player rather than to the designer.
3. **Every manoeuvre is a commitment.** Gravity + inertia mean you choose trajectories, not positions. This is
   what separates it from arcade shooters and why a well-flying run is *watchable* ("ammattilaisen lentelyä
   on ilo seurata").
4. **Fuel converts time into a resource.** Free-roaming becomes a routing problem; the level turns into a
   planning puzzle without a single puzzle object.
5. **Coupled-body physics (the pod) creates emergent chaos** from one constraint — the cheapest generator of
   stories the genre has.
6. **Asymmetric risk/reward in multiplayer.** Turboraketti's mass-vs-speed tradeoff makes loadout a bluffing
   game; gunless rockets race faster and mine their own routes.
7. **Extremely high content-to-effort ratio.** One cave, a handful of tile/particle types and an editor yields
   hours; the genre has no animation, no pathfinding and no dialogue requirements.
8. **Self-balancing difficulty** via *approximately* analogue means: harsher gravity, less fuel, more turrets,
   a narrower throat — all one-number changes.
9. **Legibility.** The screen is honest: no hidden HP bars, no damage rolls. The genre's fairness reputation
   (Gravitar's designers assumed nobody could ever complete it, and were wrong) rests on this.
10. **A known, faithful audience.** The genre has living communities around 30-year-old shareware, maintained
    level archives, world-record tables and modern reimplementations; it is niche but it is *permanent*.

### What goes wrong (pitfalls)

- **Unfair deaths**: dying to something off-screen, or to a hit you could not have seen. Fix with indicators,
  shield-timing generosity, and telegraphed enemies.
- **The rod is too harsh**: pod deaths in corridors make the game feel arbitrary; re-tune the constraint, the
  corridor widths, or the camera.
- **Lives systems in a fragile-ship game**: original *Thrust* gave 3 lives for the whole campaign; the Gravity
  Ace author explicitly rejected that as "punishingly difficult" and used fragility + shield + no lives instead.
- **Clamped speed / friction everywhere**: it feels responsive in a prototype and destroys the genre's identity.
- **Content sprawl**: with no drag, players find shortcuts and sequence breaks; levels must be validated with
  players, not in the editor.
- **Aiming-first design**: if the ship can shoot in any direction while hovering (modern controls without
  gravity pressure tuned up), the game degrades into a twin-stick shooter wearing a spacesuit.

---

## 11. Adjustment cheatsheet (per-ship / per-level knobs)

Derived from the shipped designs above. Values in brackets are **[proposal]** starting points for iteration,
not sourced constants.

| Knob | Range seen in the genre | Effect |
|---|---|---|
| Gravity `g` | per-level property (Gravity Ace); inverted in Gravitar's round 2 | Difficulty, "downhill is free" |
| Thrust `T` | `T ≈ 2 g` **[proposal]** | Climb capability; the core feel number |
| Rotation rate `ω` | deliberately slow (Gravitar's difficulty) | Who is in control: planner or reactor |
| Fuel capacity / burn | level property, refill stations + pods | Time budget, route planning |
| Shield | none → single-hit + recharge (500/600 ms / 1 s timings) → HP | Forgiveness |
| Lives | 3 per campaign (Thrust) → none (Gravity Ace) | Session structure |
| Payload | none → rigid rod (Thrust) → soft beam **[proposal]** | Chaos, mission structure |
| Terrain lethality | instant death → damage → sticky/slow media | Tension |
| Level size | single screen → multi-screen free scroll | Exploration, editor load |
| Player count | 1 → 4 (AUTS) | Social weight |

---

## 12. Implementation notes for this repo (Rust)

- **Fixed timestep**, semi-implicit Euler; render interpolation optional. Determinism is worth more than
  0.5 ms of smoothing here (replays, records, netcode, AI).
- **Entities with identical integrator**: ship, payload/pod, drones, debris, mines. One `Body { p, v, θ, ω }`
  struct plus per-entity force accumulators; the constraint solver handles the beam only.
- **Terrain**: polygon mass with swept collision against the ship's hull points (a V: nose, two wingtips
  and a tail notch) rather than the bounding circle — the *feel* of the genre is dying to a wingtip clipping
  a wall. Broadphase: uniform grid or BSP over level polygons.
- **Data-driven levels** from day one (JSON/TOML/`gfb`-style tile + object lists) so an editor can be added
  without a format migration. Author per-level: gravity, start fuel, object list, signal graph, dialog.
- **Scripted generosity** belongs in the physics layer, not content: shield collision windows, first-hit
  grace, off-screen hazard indicators.
- **Inputs**: virtual actions (`rotate_cw`, `rotate_ccw`, `thrust`, `fire`, `beam`, `special`), so classic /
  modern / split-duty schemes are a binding table, not branches in game code.
- **Audio-first for the ship**: the continuous engine loop is heard for the whole session; the reference
  implementation split it into startup / running / shutdown samples.
- **HUD**: fuel + shield ring + objective direction. Nothing else is load-bearing.

---

## 13. Catalogue of exemplars to play/read

| Game | Year / platform | Why study it |
|---|---|---|
| *Spacewar!* | 1962, PDP-1 | Gravity, dogfight, local multiplayer, the first fire button |
| *Gravitar* | 1982, arcade / 2600 | Genre template; stage modifiers; fuel-for-points reactor |
| *Thrust* | 1986, BBC Micro + every 8-bit | The canonical caves; the rod; the escape timer |
| *Oids* | 1987, Atari ST / Mac / Amiga | Mission structure, shields, ammo, "gutsy, brainy… with strategic depth" |
| *Gravity Force* / *Gravity Force 2* / *Gravity Power* | 1989–1994, Amiga | Destructible walls, split-screen + serial dogfight, editor, **source available** |
| *Turboraketti* | 1992, Amiga | Finnish multiplayer benchmark; race economy; tactics |
| *AUTS* | 1995, DOS | 4-player shared keyboard; granular and destructible walls; landing pads |
| *Wings* / *Wings 2* | 1996/2006, DOS/Win+Linux | Networked cave flyer, editors, six modes |
| *Solar Jetman* | 1990, NES | Console branch; cites *Oids* |
| *XPilot* / *SubSpace* / *Roketz* | 1990s, PC | The internet-multiplayer branch of thrust + gravity |
| *PixelJunk Shooter*, *Gravity Crash* | 2009– | Modern console reinterpretations with physics materials |
| *Gravity Ace* | 2019–, Win/Linux | Live design documentation; dual controls; shield timing; editor |
| *Gravity Force 2* (web) | 2026, browser | AI opponents, online play, spectating on top of 1992 assets |
| *Rocket Zone* | 2026, browser | Current Finnish entry, single machine, cites the three classics |

The **weapons** of the Finnish entries in that table — full lists, loadout screens, the extraction
method and per-game source evidence — are catalogued in
[`finnish_cave_flyer_weapons.md`](finnish_cave_flyer_weapons.md).

---

## 14. Sources

- fi.wikipedia: [Luolalentely](https://fi.wikipedia.org/wiki/Luolalentely), [Turboraketti](https://fi.wikipedia.org/wiki/Turboraketti), [AUTS](https://fi.wikipedia.org/wiki/AUTS_(videopeli)), [Gravitar](https://fi.wikipedia.org/wiki/Gravitar), [Wings 2](https://fi.wikipedia.org/wiki/Wings_2), [Wings](https://fi.wikipedia.org/wiki/Wings_(videopeli)), [Lunar Lander](https://fi.wikipedia.org/wiki/Lunar_Lander), [Rocket Zone](https://fi.wikipedia.org/wiki/Rocket_Zone), [Luettelo suomalaisista shareware- ja freewarepeleistä](https://fi.wikipedia.org/wiki/Luettelo_suomalaisista_shareware-_ja_freewarepeleist%C3%A4)
- en.wikipedia: [Thrust (video game)](https://en.wikipedia.org/wiki/Thrust_(video_game)), [Gravity Force](https://en.wikipedia.org/wiki/Gravity_Force), [Oids](https://en.wikipedia.org/wiki/Oids), [Gravitar](https://en.wikipedia.org/wiki/Gravitar), [Lunar Lander (video game genre)](https://en.wikipedia.org/wiki/Lunar_Lander_(video_game_genre))
- [MobyGames: Genre — Cave-flyers and Thrust variants](https://www.mobygames.com/group/6133/genre-cave-flyers-and-thrust-variants/) (64 games listed)
- Juho Kuorikoski, "Luolalentelyt ennen ja nyt", *Pelit*, 15.3.2014 — https://www.pelit.fi/artikkelit/luolalentelyt-ennen-ja-nyt/
- Yhteishyvä / Markus Laakso, "Konsolipelisanakirja", 27.10.2012 (archived) — the citation behind the Finnish Wikipedia lead
- [The Gravity-Force 2 Homepage](http://www.lysator.liu.se/~jensa/gf2/) — releases, level editor, CC BY-SA source code, [web remake](https://play.gravityforce2.com) with controls
- [Turboraketti fan site](https://www.turboraketti.org/tr/index.html) — game systems, loadout/tactics, downloads
- [Gravity Ace player guide](https://gravityace.com/docs/play/), [controls](https://gravityace.com/docs/controls/), [level editor](https://gravityace.com/docs/editor/), [devlog archive](https://gravityace.com/devlog/) (shield design, classic controls, history of Thrust-likes, tutorial/level design)
- [spwebgames.com/thrust](https://spwebgames.com/thrust/) — classic control table for a Thrust-style clone
- [Rocket Zone](https://rocketzone.io/fi/) — 2026 browser cave flyer, Finnish
- [itch.io cave-flyer tag](https://itch.io/games/tag-cave-flyer) — current indie activity in the genre
