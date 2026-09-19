# Finnish cave-flyer weapons — the original guns, sourced

Companion to `game_mechanics.md` (§13 catalogue, §14 sources). Where that document covers genre
*mechanics*, this one covers the **weapons** the Finnish cave-flyers actually shipped: names,
counts, how they are chosen, and how their economies work.

Claim discipline, same as the parent document: everything stated as fact is quoted or extracted from
a primary source below. Reconstruction and arithmetic on top of a source is marked **[inference]**.
Things no source establishes are marked **[unrecovered]** — they are not guesses.

Why this was worth digging: the primary sources are not articles. For AUTS, Wings and Rocket Zone the
weapon lists survive in a **file inside the shipped game** — a manual, a data table, a JS bundle. For
Turboraketti they do not survive at all, because its UI text is stored as bitmaps and the author's
source was lost; the strongest artefact is the equipment screen itself.

---

## 1. What exists, and how much of it is recoverable

| Game | Author | Year / platform | Weapon system | Recoverability |
|---|---|---|---|---|
| **Turboraketti** | Heikki Kosola | 1992 beta / 1993 v2.0, Amiga | 2 slots × 4 types, ammo + fuel loadout | types only — the game has **no weapon names anywhere** |
| **AUTS** | The Kudos (Jaakko Lyytinen) | 1995, MS-DOS, shareware | fixed nose gun + 1 of 20 specials | **full list**, from the game's own `AUTS.DOC` |
| **Wings** | Miika Virpioja | 1996, MS-DOS, shareware | normal gun + 1 of 33 specials | **full list**, extracted from `WEAPONS.DAT` |
| **Wings 2** | Miika Virpioja | 2006, Windows/Linux, freeware | `weapons.dat`, player-authorable | names from HUD screenshots and the official forum only |
| **Rocket Zone** | Timo Pantsari | 2026, browser, free | Gun slot (4 types) + Heavy slot (4 types) | **full table**, extracted from the shipped bundle |

The 1990s Finnish clone wave — **V-Wing** (Simo Siiriä 1997), **KOPS** (Jetro Lauha 1996),
**KaBoom** (Sanisalo & Wiik 1996), **Spruit** (ATR Software 1996), **Orb Wars** (Simo Savolainen 1997),
**A2** (Olli Lyytinen 1998, AUTS sequel), **Turbis** (1999, unofficial PC Turboraketti),
**Wham!**, **Fuse**, **PP 2**, **Rocket Chase**, **Avaruusromu**, **Turbåraketten** — is confirmed as
this genre by fi.wikipedia's shareware list and the Pelit article, but **[unrecovered]**: no manual,
readme or review reachable enumerates any of their weapons. Remedy's cancelled **Guntech** (≈80 %
complete, Virgin publishing deal) is the same: a weapon-bearing cave-flyer whose weapons are unknown.

---

## 2. Turboraketti (Heikki Kosola, Amiga, 1992 beta 0.99 / 1993 v2.0)

The genre's Finnish origin point: *"trendi alkoi vuonna 1992 Amigalle julkaistun Turboraketin myötä"*
(fi.wikipedia, Luolalentely).

### 2.1 The loadout screen

Reached by landing on your home platform and pulling the joystick down. Transcribed from the
equipment-menu screenshot and the fan translation of the game's own strings:

```
ASE 1 - AMMUKSIA  [ 0 1 2 3 4 5 6 7 8 ]   TYYPPI  1 2 3 4      <- main armament
ASE 2 - AMMUKSIA  [ 0 1 2 3 4 5 6 7 8 ]   TYYPPI  5 6 7 8      <- special armament
BENSIINIÄ         [ bar ]
                                          LOAD / SATSI 1 / SATSI 2 / SAVE
```

- Two slots. **ASE 1** selects types **1–4**, **ASE 2** selects types **5–8**.
- Each slot has its own ammo count (`AMMUKSIA`), and fuel (`BENSIINIÄ`) is set on the same screen.
- Two saved presets (`SATSI 1` / `SATSI 2`) plus LOAD/SAVE.
- In flight: joystick forward fires the primary, joystick down fires the special (FRGCB).
- Ammo and fuel are re-loadable at your base, and armament can be changed mid-match by landing again.

### 2.2 Counts per version

- Beta **0.99**: four weapons — two main + two specials.
- Release **v2.0**: eight weapons — four main + four specials.
  — FRGCB: *"in the beta version, there are only four weapons available (two main weapons and two
  specials), and the number was upgraded to eight (four main and four specials) for the full version."*
- fi.wikipedia's summary (*"kevyempi sarjatuliase että raskaampi tykki, joita molempia on valittavana
  neljä erilaista"* = a lighter machine gun and a heavier cannon, four variants of each) matches the
  4 + 4 split, but that section carries **no citations** — treat it as Wikipedia's own reading.

### 2.3 The weapons have no names

The committed result, and the reason this file quotes screens instead of a list: **Turboraketti never
names its weapons**. A native speaker's translation of the fully disassembled in-game strings — the
EAB thread, backed by `absence`'s listing of every text string — yields only:

`Load` · `Satsi 1` / `Satsi 2` (batch 1/2) · `Save` · `Ase ½ Ammuksia` (weapon 1/2 – ammo type) ·
`Tyyppi` (type) · `Benziiniä` (gasoline — *"inexplicably written with a z"*, per the translator)

Weapons are therefore chosen **by number and icon**. Any tidy Finnish weapon names circulating online
are invented.

### 2.4 What the icons show

The eight icons preview the shot pattern (read off the equipment screen at 3× zoom):

| Type | Icon | Reads as |
|---|---|---|
| 1 | ship + dense, evenly spaced dot trail | rapid straight-line pellet stream |
| 2 | ship + clustered dots near the muzzle | burst / small spread |
| 3 | ship + two widely separated dots | sparse, slow pairs |
| 4 | ship + a single distant dot | one slow projectile |
| 5 | ship + dots curling into a closed ring | ring / circular burst |
| 6 | ship + dots forming an arc | fan spread |
| 7 | both ships drawn, dots zig-zagging between them | an anti-ship pattern |
| 8 | both ships drawn, dots in a figure-eight with a ring | an anti-ship pattern |

Icons 1–4 are drawn with the firing ship alone; 5–8 involve the enemy ship — which is consistent with
1–4 being the light forward guns and 5–8 the exotic specials.

### 2.5 Attested individual weapons

- **Mines.** Pelit: after a player strips the ship to race, the opponent may have *"käynyt
  miinoittamassa haastereitin"* — gone and mined the speedrace route. The fan site's slang list
  includes a phrase for being chased by an opponent *"joka ampuu miinoja"* (who is shooting mines).
- **Bombs.** The faithful PC remake (`Turboraketti Remake 2020`, by TapanilanKTT, made with Kosola's
  permission) names *"Bomb number 5"* (veteran players recall chain-killing an opponent trapped on his
  own base) and *"Bomb number 6 (mines)"* — bouncing, with a long timeout.
- **Homing missile.** The same changelog distinguishes the original's flashing homing missile; Lemon
  Amiga players remember *"the seeker bullet"* opponents fled, often crashing while evading.
- **A cannon whose shots fragment into shrapnel** on contact with a hull (remake author's note).
- **A pulse cannon with a second "spray" mode** (remake changelog: *"Spread of the spray (pulse cannon
  mode 2) widened"*).

**[inference]** Mapping the remake's numbering onto the Amiga's types — e.g. Bomb 5 ≈ Tyyppi 5, mines ≈
Tyyppi 6, both in the ASE 2 row — is reconstruction from the remake plus the menu layout. The Amiga
original labels them 1–8 and nothing else.

### 2.6 Player slang attached to type numbers

The official fan site's vocabulary list tags specific types (players name what the game will not):

| Slang | Meaning | Type |
|---|---|---|
| *Ruoska* ("whip") | — | ase nro. **1** |
| *Ropotin* | — | ase nro. **2** |
| *Kananpaska* ("chicken shit") | — | ase nro. **4** |
| *Leuhka* | — | ase nro. **8** |

### 2.7 The loadout tradeoff (the real design)

Driven by **how much you carry**, not which type you mount:

- fi.wikipedia: *"Alukset ovat sitä ketterämpiä mitä kevyempiä ne ovat, joten nopeuskisaa kannattaa
  lähteä suorittamaan mahdollisimman vähäisellä ammus- ja polttoainemäärällä"* — the lighter the ship,
  the more agile; race with as little ammo and fuel as possible.
- Pelit: *"aseita ja polttoainetta voi jättää rannalle vaikka kesken pelin, jolloin purkista tulee
  nopea, mutta puolustuskyvytön"* — leave weapons and fuel behind mid-match and the ship is fast but
  defenceless. The counter-play is mining the racing route during that pit stop.
- FRGCB: carried mass affects lap times; higher speed demands knowing the maps; the alternative is
  arriving prepared for dogfighting.

Stripping means setting **both** `ASE – AMMUKSIA` rows to 0 and cutting fuel — the type selectors stay
where they are, but there is nothing to fire.

---

## 3. AUTS — The Ultimate Stress Relief Game (The Kudos, MS-DOS, 1995)

Every ship carries a fixed nose **GUN** plus **one** special weapon, chosen in the PLAYER SETUP menu
(second icon from the top). Specials are swapped and the ship repaired/re-energised at landing pads;
`R` randomises weaponry. There is **no ammo**: specials are balanced by a cooldown the manual calls
*"loading time"*, and by a qualitative damage tier.

The manual's own framing of the gun:

> GUN: attached to every ship along with one of the special ones — fires small pellets from the nose of
> the ship — autofire not supported — ONLY REGULAR PELLETS ARE OPERATIVE UNDERWATER ( gun, rearturret,
> multicannon ) — cause damage: A LITTLE

The game's own weapon names are **English**; the only Finnish vocabulary is the category words
*perusase* / *tykki* (base gun) and *erikoisase* (special weapon).

### 3.1 `AUTS.DOC` v1.20 — GUN + 20 specials

| # | Weapon | Behaviour (manual's words) | Loading time |
|---|---|---|---|
| 1 | **Gun** | small pellets from the nose, no autofire, works underwater; damage A LITTLE | — |
| 2 | **Cloaker** | hides you, jams the missile locking computer; on/off; damage NONE | — |
| 3 | **Magnofilter** | magnetic field emitter, shields at 80 %, v1.20 also speeds up your own pellets, docking impossible while on; damage NONE | — |
| 4 | **Rearturret** | rear-facing turret, autofire; damage A LITTLE; works underwater | — |
| 5 | **Multicannon** | regular pellets fired all around you; damage A LITTLE; works underwater | SHORT |
| 6 | **Rubber Bullets** | advanced version of the regular gun; damage A LITTLE | — |
| 7 | **Mine** | contact warhead that floats in the air; damage ENORMOUS | MEDIUM |
| 8 | **Freezer** | freezes enemies; deep-frozen ships cannot be controlled and are more fragile; damage SOME | SHORT |
| 9 | **Atom Weapon** | three neutrons orbiting your ship; deadly at short range; damage MODERATE | — |
| 10 | **Dirtclod** | slime onto the enemies' screens to blind them; damage NONE (visual) | SHORT |
| 11 | **Headspinner** *(new in v1.20)* | biological weapon, deceives the sense of balance — screen spin; damage MENTAL DIZZYNESS | SHORT |
| 12 | **Nucleus** | chain-reaction devices, detonated by shooting them with pellets; damage UNPREDICTABLE | — |
| 13 | **Troopers** | deploys armed soldiers who shoot scatter fire; max 16 at a time; damage SLIGHT→MEDIUM | SHORT |
| 14 | **Hell Fire** | short-range continuous burner on the nose; damage REMARKABLE | — |
| 15 | **Machinegun** | long-range high-velocity bullets, *"boosted up to 2500 rnds/min (!) which results in decreased control during firing"*; damage MEDIUM | — |
| 16 | **Sonicboom** | ultra-high-frequency emitter, intensity falls off with distance; damage SOME | LONG |
| 17 | **Fan** | turbo-spin fan that blows the enemy; direct damage NONE, *"via ground collisions: SEVERE"* | — |
| 18 | **Toxic Dump** | radioactive waste clouds; damage HARSH | MODERATE |
| 19 | **Dumbfire** | a large clod of powder, *"extremely deadly in experienced hands"*; damage MUCH | MEDIUM |
| 20 | **Missile** | *"a seeking missile but not very smart"*; damage MEDIUM | LONG |
| 21 | **Blackhole** *(new in v1.20)* | absorbs all matter and emits a strong gravity field; damage NONE/MUCH | LUDICROUSLY LONG |

Version note: v1.16 (May/95) is the same list **minus Headspinner and Blackhole** (gun + 19), and its
Magnofilter entry lacks the pellet-speed-boost line. MobyGames' *"arsenal of two dozen imaginative
weapons"* is a loose count of the v1.20 list, not a literal 24.

Non-gun content in the same slot list: Mine, Troopers, Fan, Blackhole, Toxic Dump, Nucleus, Cloaker and
Magnofilter are devices rather than guns. Terrain: walls take damage from ship fire; *"rakeisia
seiniä"* (granular walls) trap a ship until it shoots itself free. **[unrecovered]**: which specific
weapons damage terrain.

Sources: [`AUTS.DOC` v1.20](https://archive.org/download/auts120/auts120.zip/AUTS.DOC) ·
[v1.16](https://archive.org/download/swizzle_demu_AUTS/AUTS.zip/AUTS.DOC) ·
[fi.wikipedia](https://fi.wikipedia.org/wiki/AUTS_(videopeli)).

---

## 4. Wings (Miika Virpioja, MS-DOS, 1996/97)

A normal gun plus **one** special weapon chosen at a base (the turning buttons cycle it); the weapon
flagged `1` is the starting one and weapons flagged `X` can be swapped later. fi.wikipedia counts
**33 special weapons**; en.wikipedia repeats the figure ("the player can select one of 33 weapons"),
citing a MikroBitti 10/1996 review. Some were locked in the unregistered shareware.

### 4.1 `WEAPONS.DAT` (v1.40) — 35 records, verbatim

The list is not in the manual — it is a fixed-width table inside the shipped game: **35 records × 52
bytes** (32-byte NUL-padded ASCII name + five little-endian int32 fields), 1820 bytes total. Extracted
from `wings140.zip`:

| # | Name | Fields | # | Name | Fields |
|---|---|---|---|---|---|
| 1 | Autofire | 1, 0, 0, 0, 0 | 19 | Torpedo | 1, 0, 0, 0, 40 |
| 2 | Dumbfire | 1, 0, 0, 0, 150 | 20 | Base | 1, 0, 0, 0, 0 |
| 3 | Troopers | 0, 128, 0, 0, 0 | 21 | Cannon | 1, 0, 0, 70, 0 |
| 4 | Ion cannon | 6, 0, 0, 0, 0 | 22 | Landmines | 1, 350, 8, 0, 0 |
| 5 | Multicannon | 1, 0, 0, 0, 0 | 23 | Rockets | 1, 400, 6, 0, 0 |
| 6 | Shotgun | 1, 0, 0, 0, 100 | 24 | Bats | 18, 0, 0, 0, 0 |
| 7 | Splinterbomb | 1, 0, 0, 0, 0 | 25 | Teleport | 1, 0, 0, 0, 0 |
| 8 | Bomb | 1, 0, 0, 0, 0 | 26 | Gravitor | 1, 0, 0, 0, 0 |
| 9 | Mine | 1, 0, 0, 0, 0 | 27 | Plastic explosive | 1, 0, 0, 0, 0 |
| 10 | Missile | 1, 0, 0, 0, 130 | 28 | Watercannon | 500, 0, 0, 0, 5 |
| 11 | Freezer | 1, 0, 0, 0, 0 | 29 | Fireworks | 1, 0, 0, 0, 0 |
| 12 | Poison | 1, 0, 0, 1000, 0 | 30 | Bouncer | 1, 0, 0, 10, 150 |
| 13 | Harpoon | 1, 0, 0, 500, 100 | 31 | Net | 1, 0, 0, 1000, 0 |
| 14 | Nucleus | 1, 0, 0, 0, 0 | 32 | Shield | 50, 0, 0, 0, 0 |
| 15 | Grenade launcher | 1, 250, 5, 0, 25 | 33 | Electric blast | 250, 0, 0, 0, 0 |
| 16 | Dirtball | 1, 0, 0, 0, 95 | 34 | Poison gas | 1, 450, 4, 0, 0 |
| 17 | Digger | 1, 0, 0, 180, 0 | 35 | Nuke | 1, 0, 0, 400, 0 |
| 18 | Hellfire | 500, 0, 0, 0, 1 | | | |

**Count reconciliation [inference]:** 35 records against the documented 33 selectable specials leaves
`Base` and `Cannon` as the two non-weapon entries in the same table — consistent with the manual's
bases and with a contemporaneous review noting that bases can be built with cannons. So: **33 special
weapons + Base + Cannon**.

Reproduction (the table is auditable in three lines — `WEAPONS.DAT` is plain fixed-width data, not
compressed):

```python
import struct, zipfile
w = zipfile.ZipFile('wings140.zip').read('WEAPONS.DAT')
for i in range(len(w) // 52):                      # 35 records
    r = w[i * 52:(i + 1) * 52]
    print(r[:32].split(b'\x00')[0].decode(), struct.unpack('<5i', r[32:]))
```

**Field meanings [unrecovered]:** the five ints are unexplained by the manual, the readme or any
review. The shape is suggestive — six entries with a large second value and a small third (Landmines
1,350,8 · Rockets 1,400,6 · Poison gas 1,450,4 · Grenade launcher 1,250,5) look like reload + salvo
size, and a handful of single large values (Hellfire 500, Watercannon 500, Electric blast 250, Shield
50) look like a per-weapon magnitude — but nothing in the sources names the columns, so they are
reproduced raw and no meaning is asserted.

Other shipped data files: `W_SELECT.DAT` (351 bytes of per-player 1/0 flags — which weapons a match may
use), `SHIPS.DAT`, `WINGS.DAT`.

Economy, per a review of the game: infinite basic ammo with one limited special, reload proportional to
the weapon's power, and non-gun specials including a weightless-maker, a drill, and base-building with
cannons and soldiers.

Sources: [`wings140.zip` (`WINGS.DOC`, `WEAPONS.DAT`, `W_SELECT.DAT`)](https://archive.org/download/wings_dos/wings140.zip) ·
[en.wikipedia](https://en.wikipedia.org/wiki/Wings_(1996_video_game)) ·
[fi.wikipedia](https://fi.wikipedia.org/wiki/Wings_(videopeli)) ·
[Suomipelit review](https://web.archive.org/web/20060513095808/http://www.suomipelit.com/index.php?c=peliarvostelu&id=39).

---

## 5. Wings 2 (Miika Virpioja, 2006, freeware since 2008)

Weapons live in a **`weapons.dat`** controlling which weapons a match uses; the official forum notes
that in a network game every client must carry an identical copy. The sequel also allows player-made
levels **and weapons** — the closest thing in the Finnish line to the open weapon-data approach.

Names legible in the HUD of the official screenshots:

**Gun · Assault Rifle · MachineGun · Rocket · Grenades · Bazooka · Longbow · Dumbfire · Wildfire ·
Minesweeper**

The official forum adds, by name: **Gravity gun · Shuriken · PlasticExplosives · Digger · Wormholes**,
a net/EMP weapon, and mines (wormholes as teleporters, the digger disabled in some modes). The
community wiki that once held the full table and the manual is only archived as a PHP error, and the
weapon-authoring thread is unarchived — so the table itself is **[unrecovered]**.

Wings 2 is also unusual in the genre in letting the pilot eject, fight on foot, or take over robots,
guns and turrets.

Sources: [wings2.net](https://www.wings2.net/) · [archived forum thread](https://web.archive.org/web/20070107043019/http://www.wings2.net/forum/index.php?topic=646.new) ·
[fi.wikipedia](https://fi.wikipedia.org/wiki/Wings_2).

---

## 6. Rocket Zone (Timo Pantsari, browser, 2026) — the genre's current Finnish entry

The site names the genre ("luolalentoräiskintä") and its inspirations: **Turboraketti, Gravity Force ja
Thrust**. Arena *Vacuumos* is playable; *Cryola* and *Subterrus* are shown but locked.

The weapon table is a static object in the shipped bundle (`WEAPON_TYPES`) — eight weapons in two
slots:

```
1: Straight Gun      category weapon1   icon straight
2: Spray Gun         category weapon1   icon spray
3: Dual Wingtip      category weapon1   icon dual
4: Triple Gun        category weapon1   icon triple
5: Proximity Bomb    category weapon2   icon proximity     (splits -- "proximity_bomb_split")
6: Bouncy Bomb       category weapon2   icon heatseeker    (icon labels for 6/7 are swapped in the code)
7: Heatseeker        category weapon2   icon bouncy
8: Ball Lightning    category weapon2   icon cloud
```

- The HUD and the input help call the slots **Gun** and **Heavy**; they fire on separate keys.
- **Gun ammo** is a bar clamped to **250** rounds; **Heavy ammo** is a discrete **0–8** selector.
- The service menu that sets them also sets **fuel, clamped 10–100 %** — Heavy ammo is bought against
  fuel, and the loadout is applied at launch, i.e. it is a pre-round decision.
- Selection happens in that menu **only while the ship sits still on a spawn pad** (the visibility test
  is on-pad *and* velocity below a small threshold) — the same docked-loadout ritual as Turboraketti.

**[unrecovered]**: per-weapon damage, velocity, homing rate and split counts live in the game worker
and were not extractable from the retrievable bundle text.

Sources: [rocketzone.io/fi](https://rocketzone.io/fi/) · [rocketzone.io/en](https://rocketzone.io/en/) ·
shipped bundle `assets/immutable/main-*.js`.

---

## 7. What the Finnish line actually shares

Four constants across all five titles — these are the transferable design rules, more than any
individual weapon:

1. **One light forward gun that is always available, plus one exotic secondary you choose.** AUTS: gun
   + 1 of 20. Wings: gun + 1 of 33. Turboraketti: 4 light + 4 special. Rocket Zone: 4 light + 4 heavy.
   Rocket Zone, 34 years later, re-derived Kosola's 1992 split exactly.
2. **The choice happens docked, never mid-air.** Pad → menu → take off; AUTS and Wings let you
   re-choose by landing again; Rocket Zone only shows the menu while you are parked and nearly still.
3. **The loadout is a weight/budget decision, not only a damage decision.** Turboraketti lets you dump
   ammo and fuel mid-match for speed; Rocket Zone trades Heavy ammo against a fuel percentage; Wings
   reload is proportional to weapon power; AUTS charges cooldown instead of ammo.
4. **Terrain is part of the weapon system.** AUTS walls break under fire and granular walls grab you
   until you shoot yourself free; Wings ships a Digger, a Mine and a level editor; Turboraketti's cannon
   fragments into shrapnel on hull contact.

And one asymmetry worth noting: **the Finnish games are generous with weapon count and stingy with
weapon naming.** AUTS ships 21 named entries, Wings 35 records, Rocket Zone 8 — while Turboraketti,
the game that started the wave, ships eight weapons the player only ever sees as icons.

### For this repo

Two concrete suggestions implied by the above, not yet implemented in `docs/design.md`:

- A **pad-docked loadout screen** with a **light/heavy split** (4 + 4 as in Turboraketti and Rocket
  Zone), rather than the current single bullet type.
- **Ammo bought against fuel** as one budget on that screen, so the loadout is a weight/endurance
  decision like Turboraketti's mass-vs-speed tradeoff.

---

## 8. Sources

- **Turboraketti**
  - [FRGCB FRGR #19 (2026)](http://frgcb.blogspot.com/2026/06/frgr-19-turboraketti-heikki-kosola-1992.html) — versions, controls, 4 → 8 weapons, mass-vs-speed, ship-modification screens.
  - [Equipment-menu screenshot](http://turboraketti.bplaced.net/turboraketti_008.png) and the [TurboRAKETTI Shrine](http://turboraketti.bplaced.net/index.html).
  - [Full text translation of the game's strings](http://turboraketti.bplaced.net/TURBORAKETTI-TRANSLATION.htm).
  - [EAB: "Translation from Finnish to English (Turboraketti)"](https://web.archive.org/web/20240114190527/https://eab.abime.net/showthread.php?t=47673) — disassembly of every in-game string; the `Benziiniä` z note.
  - [Official fan site (Lauri Ahonen, 1998–2003)](https://www.turboraketti.org/tr/index.html) — loadout, tactics, slang list; [downloads](https://www.turboraketti.org/tr/download.html) (`tr2_uae.zip`, `tr099beta.lha`).
  - [Pelit, Juho Kuorikoski, "Luolalentelyt ennen ja nyt" (2014)](https://www.pelit.fi/artikkelit/luolalentelyt-ennen-ja-nyt/) — mines, stripping the ship, the clone wave, Guntech.
  - [Turboraketti Remake 2020 (TapanilanKTT)](https://tapanilanktt.itch.io/turboraketti-remake-2020) — bomb/mine/homing/pulse-cannon behaviour names; [Lemon Amiga](https://www.lemonamiga.com/game/turboraketti-2) — the "seeker bullet".
  - fi.wikipedia: [Turboraketti](https://fi.wikipedia.org/wiki/Turboraketti) (uncited gameplay summary).
- **AUTS**: [`AUTS.DOC` v1.20](https://archive.org/download/auts120/auts120.zip/AUTS.DOC) · [v1.16](https://archive.org/download/swizzle_demu_AUTS/AUTS.zip/AUTS.DOC) · [fi.wikipedia](https://fi.wikipedia.org/wiki/AUTS_(videopeli)) · [MobyGames via Wayback](https://web.archive.org/web/20220509065943/https://www.mobygames.com/game/dos/auts-the-ultimate-stress-relief-game) · [Mikrobitti 1/96](https://archive.org/download/mblehdet94-00/MB_1996_djvu.txt)
- **Wings**: [`wings140.zip`](https://archive.org/download/wings_dos/wings140.zip) (`WINGS.DOC`, `WEAPONS.DAT`, `W_SELECT.DAT`) · [en.wikipedia](https://en.wikipedia.org/wiki/Wings_(1996_video_game)) · [fi.wikipedia](https://fi.wikipedia.org/wiki/Wings_(videopeli)) · [Suomipelit review via Wayback](https://web.archive.org/web/20060513095808/http://www.suomipelit.com/index.php?c=peliarvostelu&id=39) · [MobyGames weapon-banning screen](https://www.mobygames.com/game/173238/wings/screenshots/dos/1070536/)
- **Wings 2**: [wings2.net](https://www.wings2.net/) · [forum thread on `weapons.dat`](https://web.archive.org/web/20070107043019/http://www.wings2.net/forum/index.php?topic=646.new) · [fi.wikipedia](https://fi.wikipedia.org/wiki/Wings_2)
- **Rocket Zone**: [rocketzone.io/fi](https://rocketzone.io/fi/) · [rocketzone.io/en](https://rocketzone.io/en/) · [llms.txt](https://rocketzone.io/llms.txt) · shipped bundle `assets/immutable/main-*.js`
- **The wider line**: [fi.wikipedia — Luolalentely](https://fi.wikipedia.org/wiki/Luolalentely) · [fi.wikipedia — Luettelo suomalaisista shareware- ja freewarepeleistä](https://fi.wikipedia.org/wiki/Luettelo_suomalaisista_shareware-_ja_freewarepeleistä) (the `luolalentely` rows used for the clone wave)
