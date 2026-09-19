//! Wings' weapon system: a normal gun plus one special chosen at a base.
//!
//! `docs/finnish_cave_flyer_weapons.md` §4 recovers the roster from the shipped
//! `WEAPONS.DAT` of Wings v1.40: 35 fixed-width records, of which 33 are the
//! selectable special weapons (`Base` and `Cannon` in the same table are the
//! level's furniture, not weapons). The names below are those records, verbatim.
//!
//! What Wings does *not* document is what each record's five integers mean — the
//! field meanings are unrecovered, and the manual never describes a weapon. So
//! the system is ported faithfully (one gun plus one pad-selected special, limited
//! ammo, reload proportional to power, weapons enabled per level, swapping at a
//! base) while each weapon's behaviour *in this game* is our design, derived from
//! the name and from what this engine can express. `docs/design.md` §13 lists the
//! mapping; every such decision is a proposal, not a quote.
//!
//! Nothing here knows about pixels, colours, sound or the renderer: the spec
//! table is pure data and the simulation reads it.

use crate::sim::tuning;

/// The roster, in Wings' `WEAPONS.DAT` order.
///
/// `Gun` is record-less: it is the "normal gun" every ship carries, the thing
/// Wings' manual describes as always available. The other 33 are the selectable
/// specials, numbered here as they appear in the data file (records 1-19 and
/// 22-35; 20 and 21 are `Base` and `Cannon`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum WeaponId {
    /// The always-mounted gun: unlimited ammo, no reload to speak of.
    Gun,
    Autofire,
    Dumbfire,
    Troopers,
    IonCannon,
    Multicannon,
    Shotgun,
    Splinterbomb,
    Bomb,
    Mine,
    Missile,
    Freezer,
    Poison,
    Harpoon,
    Nucleus,
    GrenadeLauncher,
    Dirtball,
    Digger,
    Hellfire,
    Torpedo,
    Landmines,
    Rockets,
    Bats,
    Teleport,
    Gravitor,
    PlasticExplosive,
    Watercannon,
    Fireworks,
    Bouncer,
    Net,
    Shield,
    ElectricBlast,
    PoisonGas,
    Nuke,
}

/// What the trigger does, which decides the whole firing path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A projectile that flies. Gravity, homing, bouncing and piercing are flags,
    /// so one path covers bullets, missiles, drills, bats and bouncers.
    Bolt,
    /// A projectile that arcs and then detonates: on its fuse, or on contact when
    /// the fuse is zero. Bombs, grenades, splinter bombs.
    Shell,
    /// Lays a gadget in the world: a mine, a cluster of troopers, a charge, a
    /// gravity well. The gadget then lives its own life in the tick loop.
    Place,
    /// A short-range cone projected while the trigger is held: flame, water, air.
    Stream,
    /// An effect on the ship itself, applied instantly: shield bubble, teleport,
    /// EMP. Everything that flies is a bolt.
    SelfEffect,
}

/// What a bolt does to whatever it touches, beyond removing hit points.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    None,
    /// Stops the target's engine for a while (`Freezer`).
    Freeze,
    /// Entangles a drone (`Net`).
    Net,
    /// Bites into the rock and reels the ship towards it (`Harpoon`).
    Tether,
    /// Keeps the ship's shield charged while it lasts (`Shield`).
    Shield,
    /// Moves the ship forward through anything it can see through (`Teleport`).
    Blink,
    /// Knocks out every turret in radius and hurts everything hostile in it
    /// (`ElectricBlast`).
    Emp,
}

/// A lingering body in the air: poison, gas, flame, a water jet, sparks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloudKind {
    /// Damages hostile things inside it, on a tick timer.
    Poison,
    /// Same, wider and longer lived.
    Gas,
    /// Short-lived flame: damage and a little digging.
    Flame,
    /// Water jet: no damage, but it shoves things around.
    Water,
    /// Fireworks: pure decoration, harmlessly bright.
    Sparks,
}

/// Something laid in the world that persists: mines, troopers, charges, wells.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GadgetKind {
    /// A proximity mine. Arms after a moment so it cannot detonate on its owner.
    Mine,
    /// The same, dropped behind the ship rather than in front (`Landmines`).
    Landmine,
    /// A timed demolition charge (`PlasticExplosive`).
    Charge,
    /// Draws drones and enemy fire towards its centre (`Gravitor`).
    Well,
    /// A pair of troopers that shoot at whatever is hostile within range.
    Troopers,
}

/// One row of the roster: everything the simulation needs to fire it, and
/// nothing about how it looks.
#[derive(Clone, Copy, Debug)]
pub struct WeaponSpec {
    pub id: WeaponId,
    /// Wings' own name for the weapon, lowercased.
    pub name: &'static str,
    pub kind: Kind,
    /// Shots per load. Zero means unlimited, which only the gun is.
    pub ammo: u16,
    /// Seconds between shots, the manual's "loading time" in our units.
    pub reload: f32,
    /// Hit points removed from a target.
    pub damage: i32,
    /// Muzzle speed, px/s.
    pub speed: f32,
    /// Seconds before the shot expires (or, for a `Shell`, its fuse).
    pub life: f32,
    /// Dirt crater radius on impact.
    pub carve: f32,
    /// Dirt *added* on impact (`Dirtball`): the only weapon that builds terrain.
    pub fill: f32,
    /// Radius used for hits against turrets, drones, mines and the reactor.
    pub radius: f32,
    /// Explosion radius on death; zero means it just stops.
    pub blast: f32,
    /// Fragments spawned when the shot dies.
    pub burst: u8,
    /// Projectiles per shot.
    pub count: u8,
    /// Total fan width when a shot fires several projectiles, in radians.
    pub spread: f32,
    /// Fraction of the level's gravity the shot feels. Shells arc, bolts do not.
    pub gravity: f32,
    /// Turn rate toward the nearest hostile, in rad/s. Zero is a dumbfire.
    pub homing: f32,
    /// Terrain reflections before the shot dies.
    pub bounce: u8,
    /// Targets pierced before the shot dies.
    pub pierce: u8,
    /// Leaves this kind of cloud where it lands, for `cloud_ttl` seconds.
    pub cloud: Option<CloudKind>,
    pub cloud_ttl: f32,
    /// How wide that cloud is.
    pub cloud_radius: f32,
    /// Hit points per second the cloud deals to each hostile inside it.
    pub cloud_dps: f32,
    /// Shove applied to bodies inside the shot or cloud, px/s².
    pub push: f32,
    /// True when the weapon works underwater. Water stops everything else.
    pub water_ok: bool,
    /// What it does to a target it touches.
    pub effect: Effect,
    /// What it lays in the world, for `Kind::Place`.
    pub gadget: Option<GadgetKind>,
    /// Duration of an effect, a gadget or a self-effect.
    pub ttl: f32,
    /// How far a blink travels, in px.
    pub blink: f32,
}

impl Default for WeaponSpec {
    fn default() -> Self {
        Self {
            id: WeaponId::Gun,
            name: "gun",
            kind: Kind::Bolt,
            ammo: 0,
            reload: tuning::FIRE_COOLDOWN,
            damage: 1,
            speed: tuning::BULLET_SPEED,
            life: tuning::BULLET_LIFE,
            carve: tuning::CARVE_BULLET,
            fill: 0.0,
            radius: 4.0,
            blast: 0.0,
            burst: 0,
            count: 1,
            spread: 0.0,
            gravity: 0.0,
            homing: 0.0,
            bounce: 0,
            pierce: 0,
            cloud: None,
            cloud_ttl: 0.0,
            cloud_radius: 0.0,
            cloud_dps: 0.0,
            push: 0.0,
            water_ok: false,
            effect: Effect::None,
            gadget: None,
            ttl: 0.0,
            blink: 0.0,
        }
    }
}

/// Every special, in roster order: what a level can enable and what a base can
/// cycle through.
pub const SPECIALS: [WeaponId; 33] = [
    WeaponId::Autofire,
    WeaponId::Dumbfire,
    WeaponId::Troopers,
    WeaponId::IonCannon,
    WeaponId::Multicannon,
    WeaponId::Shotgun,
    WeaponId::Splinterbomb,
    WeaponId::Bomb,
    WeaponId::Mine,
    WeaponId::Missile,
    WeaponId::Freezer,
    WeaponId::Poison,
    WeaponId::Harpoon,
    WeaponId::Nucleus,
    WeaponId::GrenadeLauncher,
    WeaponId::Dirtball,
    WeaponId::Digger,
    WeaponId::Hellfire,
    WeaponId::Torpedo,
    WeaponId::Landmines,
    WeaponId::Rockets,
    WeaponId::Bats,
    WeaponId::Teleport,
    WeaponId::Gravitor,
    WeaponId::PlasticExplosive,
    WeaponId::Watercannon,
    WeaponId::Fireworks,
    WeaponId::Bouncer,
    WeaponId::Net,
    WeaponId::Shield,
    WeaponId::ElectricBlast,
    WeaponId::PoisonGas,
    WeaponId::Nuke,
];

/// How many rows the table has: the gun plus 33 specials.
pub const COUNT: usize = SPECIALS.len() + 1;

static TABLE: std::sync::LazyLock<[WeaponSpec; COUNT]> = std::sync::LazyLock::new(build);

/// The spec for a weapon. Built once, then a plain table lookup.
pub fn spec(id: WeaponId) -> &'static WeaponSpec {
    &TABLE[id as usize]
}

/// Every weapon in the game, gun first.
pub fn all() -> impl Iterator<Item = WeaponId> {
    std::iter::once(WeaponId::Gun).chain(SPECIALS)
}

/// Parses a weapon name as it appears in `WEAPONS.DAT`, in a level file or on
/// the command line: case, spaces, underscores and hyphens are all the same.
pub fn parse(name: &str) -> Option<WeaponId> {
    let norm: String = name
        .trim()
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c == '_' || c == '-' { ' ' } else { c })
        .collect();
    let norm = norm.split_whitespace().collect::<Vec<_>>().join(" ");
    all().find(|id| spec(*id).name == norm)
}

/// The 33 specials as a comma-separated list, for error messages.
pub fn roster() -> String {
    SPECIALS
        .iter()
        .map(|id| spec(*id).name)
        .collect::<Vec<_>>()
        .join(", ")
}

/// True for the 33 selectable specials; false for the always-mounted gun.
pub fn selectable(id: WeaponId) -> bool {
    id != WeaponId::Gun
}

fn base() -> WeaponSpec {
    WeaponSpec::default()
}

/// The roster. Every row is a proposal for how the name behaves here — see the
/// module header and `docs/design.md` §13.
fn build() -> [WeaponSpec; COUNT] {
    let mut table = [WeaponSpec::default(); COUNT];
    {
        let mut put = |s: WeaponSpec| table[s.id as usize] = s;

        // --- the normal gun, present in every ship --------------------------
        put(base());

        // --- gun-like specials: a better gun, limited by ammo ---------------
        put(WeaponSpec {
            id: WeaponId::Autofire,
            name: "autofire",
            ammo: 240,
            reload: 0.055,
            speed: 480.0,
            life: 1.0,
            ..base()
        });
        put(WeaponSpec {
            id: WeaponId::Dumbfire,
            name: "dumbfire",
            ammo: 24,
            reload: 0.45,
            damage: 3,
            speed: 380.0,
            life: 1.8,
            carve: 8.0,
            blast: 12.0,
            gravity: 0.15,
            ..base()
        });
        put(WeaponSpec {
            id: WeaponId::IonCannon,
            name: "ion cannon",
            ammo: 14,
            reload: 0.5,
            damage: 3,
            speed: 900.0,
            life: 1.4,
            carve: 5.0,
            pierce: 3,
            ..base()
        });
        put(WeaponSpec {
            id: WeaponId::Multicannon,
            name: "multicannon",
            ammo: 30,
            reload: 0.4,
            speed: 380.0,
            life: 0.9,
            count: 8,
            spread: std::f32::consts::TAU,
            ..base()
        });
        put(WeaponSpec {
            id: WeaponId::Shotgun,
            name: "shotgun",
            ammo: 24,
            reload: 0.55,
            speed: 470.0,
            life: 0.55,
            count: 6,
            spread: 0.55,
            ..base()
        });
        // --- area weapons: clouds that keep working after the shot ----------
        put(WeaponSpec {
            id: WeaponId::Poison,
            name: "poison",
            ammo: 10,
            reload: 0.8,
            damage: 0,
            speed: 350.0,
            life: 1.0,
            carve: 2.0,
            cloud: Some(CloudKind::Poison),
            cloud_radius: 34.0,
            cloud_dps: 0.5,
            cloud_ttl: 6.0,
            ..base()
        });
        put(WeaponSpec {
            id: WeaponId::PoisonGas,
            name: "poison gas",
            ammo: 8,
            reload: 0.9,
            damage: 0,
            speed: 280.0,
            life: 1.0,
            carve: 2.0,
            cloud: Some(CloudKind::Gas),
            cloud_radius: 60.0,
            cloud_dps: 0.35,
            cloud_ttl: 10.0,
            ..base()
        });
        put(WeaponSpec {
            id: WeaponId::Hellfire,
            name: "hellfire",
            kind: Kind::Stream,
            ammo: 60,
            reload: 0.05,
            speed: 300.0,
            life: 0.35,
            carve: 2.5,
            radius: 10.0,
            cloud: Some(CloudKind::Flame),
            cloud_radius: 10.0,
            cloud_dps: 0.6,
            cloud_ttl: 0.35,
            ..base()
        });
        put(WeaponSpec {
            id: WeaponId::Watercannon,
            name: "watercannon",
            kind: Kind::Stream,
            ammo: 40,
            reload: 0.05,
            damage: 0,
            speed: 380.0,
            life: 0.3,
            carve: 2.0,
            radius: 12.0,
            cloud: Some(CloudKind::Water),
            cloud_radius: 12.0,
            cloud_dps: 0.0,
            cloud_ttl: 0.3,
            push: 120.0,
            water_ok: true,
            ..base()
        });
        put(WeaponSpec {
            id: WeaponId::Fireworks,
            name: "fireworks",
            ammo: 10,
            reload: 0.6,
            damage: 0,
            speed: 300.0,
            life: 1.4,
            burst: 14,
            cloud: Some(CloudKind::Sparks),
            cloud_radius: 26.0,
            cloud_dps: 0.0,
            cloud_ttl: 1.1,
            ..base()
        });

        // --- shells: arcs, then a blast ------------------------------------
        put(WeaponSpec {
            id: WeaponId::Bomb,
            name: "bomb",
            kind: Kind::Shell,
            ammo: 12,
            reload: 0.8,
            damage: 4,
            speed: 300.0,
            life: 2.2,
            carve: 3.0,
            blast: 34.0,
            gravity: 1.0,
            bounce: 3,
            ..base()
        });
        put(WeaponSpec {
            id: WeaponId::GrenadeLauncher,
            name: "grenade launcher",
            kind: Kind::Shell,
            ammo: 14,
            reload: 0.7,
            damage: 2,
            speed: 330.0,
            life: 1.6,
            carve: 4.0,
            blast: 26.0,
            burst: 4,
            gravity: 1.0,
            bounce: 2,
            ..base()
        });
        put(WeaponSpec {
            id: WeaponId::Splinterbomb,
            name: "splinterbomb",
            kind: Kind::Shell,
            ammo: 18,
            reload: 0.6,
            speed: 320.0,
            life: 0.5,
            carve: 3.0,
            blast: 14.0,
            burst: 6,
            gravity: 0.3,
            ..base()
        });

        // --- missiles and the swarm ----------------------------------------
        put(WeaponSpec {
            id: WeaponId::Missile,
            name: "missile",
            ammo: 10,
            reload: 1.0,
            damage: 2,
            speed: 200.0,
            life: 4.0,
            carve: 4.0,
            radius: 5.0,
            blast: 16.0,
            homing: 2.6,
            ..base()
        });
        put(WeaponSpec {
            id: WeaponId::Bats,
            damage: 1,
            name: "bats",
            ammo: 6,
            reload: 1.5,
            speed: 170.0,
            life: 6.0,
            carve: 2.0,
            radius: 5.0,
            count: 6,
            spread: 0.9,
            homing: 3.5,
            ..base()
        });
        put(WeaponSpec {
            id: WeaponId::Rockets,
            name: "rockets",
            ammo: 24,
            reload: 0.7,
            speed: 420.0,
            life: 1.2,
            carve: 3.0,
            blast: 10.0,
            count: 6,
            spread: 0.35,
            ..base()
        });
        put(WeaponSpec {
            id: WeaponId::Torpedo,
            name: "torpedo",
            ammo: 10,
            reload: 0.8,
            damage: 3,
            speed: 200.0,
            life: 2.6,
            carve: 6.0,
            radius: 6.0,
            blast: 18.0,
            gravity: 0.35,
            water_ok: true,
            ..base()
        });
        put(WeaponSpec {
            id: WeaponId::Nucleus,
            name: "nucleus",
            ammo: 8,
            reload: 1.0,
            damage: 2,
            speed: 260.0,
            life: 1.6,
            carve: 3.0,
            radius: 7.0,
            burst: 3,
            ..base()
        });

        // --- digging and building ------------------------------------------
        put(WeaponSpec {
            id: WeaponId::Digger,
            name: "digger",
            ammo: 10,
            reload: 0.5,
            speed: 240.0,
            life: 1.2,
            carve: 9.0,
            radius: 5.0,
            pierce: 6,
            ..base()
        });
        put(WeaponSpec {
            id: WeaponId::Dirtball,
            name: "dirtball",
            ammo: 20,
            reload: 0.35,
            damage: 0,
            speed: 300.0,
            life: 1.0,
            carve: 0.0,
            fill: 7.0,
            radius: 6.0,
            gravity: 0.4,
            ..base()
        });
        put(WeaponSpec {
            id: WeaponId::Bouncer,
            name: "bouncer",
            ammo: 16,
            reload: 0.4,
            speed: 380.0,
            life: 3.0,
            carve: 2.5,
            bounce: 5,
            ..base()
        });

        // --- control weapons: stop, grab, shove -----------------------------
        put(WeaponSpec {
            id: WeaponId::Freezer,
            name: "freezer",
            ammo: 12,
            reload: 0.7,
            damage: 0,
            speed: 400.0,
            life: 1.2,
            carve: 1.5,
            radius: 12.0,
            effect: Effect::Freeze,
            ttl: 4.0,
            ..base()
        });
        put(WeaponSpec {
            id: WeaponId::Net,
            damage: 0,
            name: "net",
            ammo: 8,
            reload: 0.8,
            speed: 300.0,
            life: 1.0,
            carve: 0.0,
            radius: 14.0,
            effect: Effect::Net,
            ttl: 6.0,
            ..base()
        });
        put(WeaponSpec {
            id: WeaponId::Harpoon,
            damage: 1,
            name: "harpoon",
            ammo: 8,
            reload: 0.9,
            speed: 520.0,
            life: 0.9,
            carve: 0.0,
            radius: 4.0,
            effect: Effect::Tether,
            ttl: 3.0,
            ..base()
        });

        // --- laid in the world ---------------------------------------------
        put(WeaponSpec {
            id: WeaponId::Mine,
            damage: 0,
            name: "mine",
            kind: Kind::Place,
            ammo: 8,
            reload: 0.5,
            blast: 24.0,
            gadget: Some(GadgetKind::Mine),
            ttl: 45.0,
            ..base()
        });
        put(WeaponSpec {
            id: WeaponId::Landmines,
            damage: 0,
            name: "landmines",
            kind: Kind::Place,
            ammo: 10,
            reload: 0.35,
            blast: 20.0,
            gadget: Some(GadgetKind::Landmine),
            ttl: 45.0,
            ..base()
        });
        put(WeaponSpec {
            id: WeaponId::PlasticExplosive,
            damage: 0,
            name: "plastic explosive",
            kind: Kind::Place,
            ammo: 8,
            reload: 0.4,
            blast: 30.0,
            gadget: Some(GadgetKind::Charge),
            ttl: 3.0,
            ..base()
        });
        put(WeaponSpec {
            id: WeaponId::Gravitor,
            damage: 0,
            name: "gravitor",
            kind: Kind::Place,
            ammo: 6,
            reload: 1.5,
            radius: 120.0,
            gadget: Some(GadgetKind::Well),
            ttl: 6.0,
            ..base()
        });
        put(WeaponSpec {
            id: WeaponId::Troopers,
            damage: 0,
            name: "troopers",
            kind: Kind::Place,
            ammo: 6,
            reload: 1.2,
            count: 2,
            gadget: Some(GadgetKind::Troopers),
            ttl: 30.0,
            ..base()
        });

        // --- the ship's own effects ----------------------------------------
        put(WeaponSpec {
            id: WeaponId::Shield,
            damage: 0,
            name: "shield",
            kind: Kind::SelfEffect,
            ammo: 6,
            reload: 0.6,
            effect: Effect::Shield,
            ttl: 6.0,
            ..base()
        });
        put(WeaponSpec {
            id: WeaponId::Teleport,
            damage: 0,
            name: "teleport",
            kind: Kind::SelfEffect,
            ammo: 6,
            reload: 1.5,
            effect: Effect::Blink,
            blink: 140.0,
            ..base()
        });
        put(WeaponSpec {
            id: WeaponId::ElectricBlast,
            name: "electric blast",
            kind: Kind::SelfEffect,
            ammo: 8,
            reload: 0.9,
            damage: 2,
            radius: 110.0,
            effect: Effect::Emp,
            ttl: 6.0,
            ..base()
        });

        // --- the end of the roster -----------------------------------------
        put(WeaponSpec {
            id: WeaponId::Nuke,
            name: "nuke",
            ammo: 1,
            reload: 3.0,
            damage: 12,
            speed: 240.0,
            life: 1.6,
            carve: 90.0,
            radius: 8.0,
            blast: 110.0,
            gravity: 0.1,
            ..base()
        });
    }
    table
}

/// The ship's special weapon and its ammo: the half of the loadout a base can
/// change. The gun is always there and never runs out, so it lives in the ship.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Loadout {
    pub special: WeaponId,
    pub ammo: u16,
    /// Seconds until the special can fire again.
    pub cd: f32,
}

impl Loadout {
    pub fn new(special: WeaponId) -> Self {
        let s = spec(special);
        Self {
            special,
            ammo: s.ammo,
            cd: 0.0,
        }
    }

    /// A fresh load of ammo, as a base hands out.
    pub fn rearm(&mut self) {
        self.ammo = spec(self.special).ammo;
        self.cd = 0.0;
    }

    pub fn ready(&self) -> bool {
        self.cd <= 0.0 && self.ammo > 0
    }

    pub fn empty(&self) -> bool {
        self.ammo == 0
    }

    /// Spends one shot and starts the reload. The caller has already checked
    /// [`Loadout::ready`].
    pub fn spend(&mut self) {
        self.ammo = self.ammo.saturating_sub(1);
        self.cd = spec(self.special).reload;
    }

    pub fn tick(&mut self, dt: f32) {
        self.cd = (self.cd - dt).max(0.0);
    }

    /// Steps through the weapons a level allows, in roster order and wrapping,
    /// and re-arms the new one. This is what a base's turn buttons do.
    pub fn cycle(&mut self, direction: i32, available: &[WeaponId]) {
        if available.is_empty() {
            return;
        }
        let current = available.iter().position(|w| *w == self.special);
        let next = match current {
            Some(i) => {
                let n = available.len() as i32;
                (((i as i32 + direction) % n + n) % n) as usize
            }
            None => 0,
        };
        self.special = available[next];
        self.rearm();
    }
}
