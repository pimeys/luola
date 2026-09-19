//! The world: one integrator, one constraint solver, one collision pass.
//!
//! `World::step` is the *only* place game state advances. It is driven at a
//! fixed 120 Hz with an `InputFrame` and nothing else, which is what makes
//! replays, record verification and (eventually) netcode possible
//! (`docs/game_mechanics.md` §9, §12).

use std::sync::Arc;

use crate::math::{Rng, V2, angle_diff, approach_angle};
use crate::sim::entities::{
    Bullet, Cloud, Drone, Exit, FuelPod, Gadget, Gate, Mine, Pad, Pod, Reactor, Turret,
};
use crate::sim::events::{Event, Failure, Outcome};
use crate::sim::fnv::Fnv;
use crate::sim::inputs::{
    BTN_BEAM, BTN_DUMP, BTN_FIRE, BTN_ROTATE_CCW, BTN_ROTATE_CW, BTN_SPECIAL, BTN_THRUST,
    BTN_WEAPON_NEXT, BTN_WEAPON_PREV, InputFrame, Scheme,
};
use crate::sim::level::{Level, SignalAction, Trigger};
use crate::sim::ship::{Shield, Ship};
use crate::sim::terrain::{Carve, MAT_GRANULAR, Terrain};
use crate::sim::tuning;
use crate::sim::tuning::DT;
use crate::sim::water::Water;
use crate::sim::weapons::{self, CloudKind, Effect, GadgetKind, Kind, Loadout, WeaponId};

/// Where the run is in its life cycle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RunState {
    Flying,
    /// The hull is gone; the wreck is still on screen for a moment.
    ShipLost {
        t: f32,
    },
    Failed {
        reason: Failure,
        t: f32,
    },
    Complete {
        outcome: Outcome,
        t: f32,
    },
}

impl RunState {
    pub fn is_flying(self) -> bool {
        matches!(self, RunState::Flying)
    }

    pub fn is_over(self) -> bool {
        !self.is_flying()
    }

    /// Seconds spent in the end state; used to time the results card.
    pub fn end_time(self) -> f32 {
        match self {
            RunState::Flying => 0.0,
            RunState::ShipLost { t }
            | RunState::Failed { t, .. }
            | RunState::Complete { t, .. } => t,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Stats {
    pub ticks: u64,
    pub shots: u32,
    pub hits: u32,
    pub turrets_destroyed: u32,
    pub drones_destroyed: u32,
    pub mines_destroyed: u32,
    pub damage_taken: u32,
}

/// Score breakdown, kept itemised so the results card can explain it (§6).
#[derive(Clone, Copy, Debug, Default)]
pub struct Score {
    pub fuel: i32,
    pub time: i32,
    pub turrets: i32,
    pub drones: i32,
    pub mines: i32,
    pub reactor: i32,
    pub payload: i32,
    pub escape: i32,
}

impl Score {
    pub fn total(&self) -> i32 {
        self.fuel
            + self.time
            + self.turrets
            + self.drones
            + self.mines
            + self.reactor
            + self.payload
            + self.escape
    }

    pub fn lines(&self) -> [(&'static str, i32); 8] {
        [
            ("FUEL REMAINING", self.fuel),
            ("TIME BONUS", self.time),
            ("TURRETS", self.turrets),
            ("DRONES", self.drones),
            ("MINES", self.mines),
            ("REACTOR", self.reactor),
            ("PAYLOAD", self.payload),
            ("ESCAPE", self.escape),
        ]
    }
}

/// Something the HUD can point at: radar blip or off-screen indicator (§8.5).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Marker {
    pub p: V2,
    pub kind: MarkerKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkerKind {
    Pod,
    Reactor,
    Exit,
    Fuel,
    Pad,
    Turret,
    Drone,
    Mine,
    Ship,
}

pub struct World {
    pub level: Arc<Level>,
    /// The cave as it is *now*: `Level::terrain` is the pristine template the
    /// level was authored with, and every blast since has gone into this copy.
    pub terrain: Terrain,
    /// Water on the same grid, seeded from the level's pools.
    pub water: Water,
    pub scheme: Scheme,
    pub seed: u64,
    pub tick: u64,
    pub rng: Rng,

    pub ship: Ship,
    pub pods: Vec<Pod>,
    pub turrets: Vec<Turret>,
    pub drones: Vec<Drone>,
    pub mines: Vec<Mine>,
    pub reactors: Vec<Reactor>,
    pub fuel_pods: Vec<FuelPod>,
    pub pads: Vec<Pad>,
    pub exits: Vec<Exit>,
    /// Moving geometry: gates and crushers.
    pub gates: Vec<Gate>,
    pub bullets: Vec<Bullet>,
    /// Lingering weapon clouds: poison, gas, flame, water, sparks.
    pub clouds: Vec<Cloud>,
    /// Things the special weapons laid down: charges, wells, troopers.
    pub gadgets: Vec<Gadget>,

    pub state: RunState,
    pub stats: Stats,
    pub score: Score,
    /// Time left before the cave collapses, once a reactor is critical.
    pub escape: Option<f32>,
    /// Rate limits pad events so the sound does not machine-gun.
    pad_cooldown: f32,
    /// Rate limits weapon cycling, so holding a turn key steps one weapon.
    weapon_cd: f32,
    /// Whether the ship was in water on the previous tick, for the entry splash.
    ship_wet: bool,
    events: Vec<Event>,
}

impl World {
    pub fn new(level: Arc<Level>, scheme: Scheme, seed: u64) -> Self {
        let mut ship = Ship::new(level.player.pos, level.player.angle, level.start_fuel);
        ship.loadout = Loadout::new(level.start_weapon);
        let pods = level
            .pods
            .iter()
            .map(|s| Pod::new(s.pos, s.angle))
            .collect();
        let turrets = level
            .turrets
            .iter()
            .map(|s| Turret::new(s.pos, s.angle))
            .collect();
        let drones = level
            .drones
            .iter()
            .map(|s| Drone::new(s.pos, s.angle))
            .collect();
        let mines = level.mines.iter().map(|s| Mine::new(s.pos)).collect();
        let reactors = level.reactors.iter().map(|s| Reactor::new(s.pos)).collect();
        let fuel_pods = level
            .fuel_pods
            .iter()
            .map(|f| FuelPod {
                p: f.pos,
                amount: f.amount,
                taken: false,
                phase: 0.0,
            })
            .collect();
        let pads = level.pads.iter().map(|a| Pad { rect: a.rect }).collect();
        let exits = level
            .exits
            .iter()
            .enumerate()
            .map(|(i, a)| {
                let hidden =
                    level.exit_locked || level.exit_hidden.get(i).copied().unwrap_or(false);
                Exit {
                    rect: a.rect,
                    open: !level.exit_locked && !hidden,
                    hidden,
                    phase: 0.0,
                }
            })
            .collect();
        let gates = level
            .gates
            .iter()
            .map(|g| Gate::new(g.from, g.to, g.size, g.speed, g.trigger, g.hidden, g.phase))
            .collect();

        // The cave this run digs into, and the water it was authored with.
        let terrain = level.terrain.clone();
        let mut water = Water::new(&terrain, level.water);
        for pool in &level.pools {
            water.fill(&terrain, pool);
        }

        Self {
            level,
            terrain,
            water,
            scheme,
            seed,
            tick: 0,
            rng: Rng::new(seed),
            ship,
            pods,
            turrets,
            drones,
            mines,
            reactors,
            fuel_pods,
            pads,
            exits,
            gates,
            bullets: Vec::new(),
            clouds: Vec::new(),
            gadgets: Vec::new(),
            state: RunState::Flying,
            stats: Stats::default(),
            score: Score::default(),
            escape: None,
            pad_cooldown: 0.0,
            weapon_cd: 0.0,
            ship_wet: false,
            events: Vec::new(),
        }
    }

    /// Mounts a specific special on the ship at launch.
    ///
    /// This is the `--weapon` override and the replay header talking: the level
    /// says what a cave normally starts you with, and this says what *this* run
    /// starts with.
    pub fn arm(&mut self, weapon: WeaponId) {
        self.ship.loadout = Loadout::new(weapon);
    }

    // ------------------------------------------------------------ queries --

    pub fn events(&self) -> &[Event] {
        &self.events
    }

    pub fn elapsed(&self) -> f32 {
        self.tick as f32 * DT
    }

    pub fn pod_required(&self) -> bool {
        self.level.require_pod
    }

    /// Pods still on the mission.
    pub fn pods_alive(&self) -> usize {
        self.pods.iter().filter(|p| p.alive).count()
    }

    /// True when the mission's payload is accounted for: parked in an open exit,
    /// or still on the beam at the moment of escape. Carrying it out on the rod
    /// is the skill route, so it has to score like the delivery it is.
    pub fn payload_secured(&self) -> bool {
        self.pods.iter().any(|p| {
            p.alive
                && (p.delivered
                    || (p.attached && p.body.p.dist(self.ship.body.p) < tuning::BEAM_BREAK))
        })
    }

    /// True when the ship is parked on a base: slow, alive, inside a pad. This
    /// is what gates weapon swapping, exactly as it gates refuelling.
    pub fn docked_on_pad(&self) -> bool {
        self.ship.alive
            && self.ship.body.v.len() < tuning::PAD_LANDING_SPEED
            && self.pads.iter().any(|p| p.rect.contains(self.ship.body.p))
    }

    /// Mines the player has laid, for the cap that keeps a cave from filling up.
    pub fn laid_mines(&self) -> usize {
        self.mines.iter().filter(|m| m.from_player).count()
    }

    pub fn reactor_alive(&self) -> bool {
        self.reactors.iter().any(|r| !r.destroyed)
    }

    pub fn exit_open(&self) -> bool {
        self.exits.iter().any(|e| e.open)
    }

    /// What the HUD should point the player at right now.
    pub fn markers(&self) -> Vec<Marker> {
        let mut out = Vec::new();
        for p in &self.pods {
            if p.alive && !p.delivered {
                out.push(Marker {
                    p: p.body.p,
                    kind: MarkerKind::Pod,
                });
            }
        }
        for r in &self.reactors {
            if !r.destroyed {
                out.push(Marker {
                    p: r.p,
                    kind: MarkerKind::Reactor,
                });
            }
        }
        for e in &self.exits {
            if e.hidden {
                continue;
            }
            out.push(Marker {
                p: e.rect.center(),
                kind: MarkerKind::Exit,
            });
        }
        for f in &self.fuel_pods {
            if !f.taken {
                out.push(Marker {
                    p: f.p,
                    kind: MarkerKind::Fuel,
                });
            }
        }
        for a in &self.pads {
            out.push(Marker {
                p: a.rect.center(),
                kind: MarkerKind::Pad,
            });
        }
        for t in &self.turrets {
            if t.alive && t.powered {
                out.push(Marker {
                    p: t.p,
                    kind: MarkerKind::Turret,
                });
            }
        }
        for d in &self.drones {
            if d.alive {
                out.push(Marker {
                    p: d.body.p,
                    kind: MarkerKind::Drone,
                });
            }
        }
        for m in &self.mines {
            if m.alive {
                out.push(Marker {
                    p: m.body.p,
                    kind: MarkerKind::Mine,
                });
            }
        }
        out
    }

    /// Priority target for the objective needle: payload, then reactor, then exit.
    pub fn objective_target(&self) -> Option<Marker> {
        let carriers: Vec<Marker> = self
            .markers()
            .into_iter()
            .filter(|m| match m.kind {
                MarkerKind::Pod => self.level.require_pod,
                MarkerKind::Reactor => self.level.exit_locked,
                MarkerKind::Exit => true,
                _ => false,
            })
            .collect();
        let order = |k: MarkerKind| match k {
            MarkerKind::Pod => 0,
            MarkerKind::Reactor => 1,
            MarkerKind::Exit => 2,
            _ => 3,
        };
        carriers.into_iter().min_by_key(|m| order(m.kind))
    }

    // --------------------------------------------------------------- step --

    /// Advances exactly one fixed tick.
    pub fn step(&mut self, input: InputFrame) {
        self.events.clear();
        match self.state {
            RunState::Flying => self.step_flying(input),
            RunState::ShipLost { ref mut t }
            | RunState::Failed { ref mut t, .. }
            | RunState::Complete { ref mut t, .. } => *t += DT,
        }
        self.tick += 1;
        self.animate();
    }

    fn step_flying(&mut self, input: InputFrame) {
        self.stats.ticks = self.tick;
        self.control_ship(input);
        if !matches!(self.state, RunState::Flying) {
            return;
        }
        self.step_turrets();
        self.step_drones();
        self.step_gadgets();
        self.step_gates();
        self.integrate();
        self.solve_rod();
        self.collide();
        self.step_clouds();
        // After the blasts of this tick, so the water starts pouring the moment
        // a floor opens, and before the objectives, which never read it.
        self.step_medium();
        self.step_objectives();
        self.step_timers();
    }

    /// Cosmetic phases. Kept in the simulation so replays and screenshots match.
    fn animate(&mut self) {
        for f in self.fuel_pods.iter_mut() {
            f.phase = (f.phase + DT) % 4.0;
        }
        for e in self.exits.iter_mut() {
            e.phase = (e.phase + DT) % 2.0;
        }
        for r in self.reactors.iter_mut() {
            r.phase = (r.phase + DT) % 2.0;
        }
    }

    fn control_ship(&mut self, input: InputFrame) {
        let mut thrust_power = 0.0f32;
        let mut gun_shot: Option<(V2, f32)> = None;

        {
            let ship = &mut self.ship;
            ship.prev = ship.body;
            ship.thrusting = false;
            ship.thrust_dir = V2::ZERO;
            if ship.alive {
                match self.scheme {
                    Scheme::Classic => {
                        // Screen space has +y down, so a *positive* angle turns
                        // clockwise on screen: the CCW key must spin negative.
                        let dir = f32::from(input.has(BTN_ROTATE_CW))
                            - f32::from(input.has(BTN_ROTATE_CCW));
                        ship.set_spin(dir);
                        if input.has(BTN_THRUST) && ship.has_fuel() {
                            ship.thrust_dir = ship.body.forward();
                            ship.thrusting = true;
                            ship.burn(DT);
                        }
                    }
                    Scheme::Modern => {
                        ship.body.omega = 0.0;
                        ship.turn_towards(input.aim, tuning::MODERN_TURN_RATE, DT);
                        let dir = input.move_dir;
                        if dir.len_sq() > 1e-6 && ship.has_fuel() {
                            ship.thrust_dir = dir.normalized();
                            ship.thrusting = true;
                            ship.burn(DT);
                        }
                    }
                }
                if ship.thrusting {
                    thrust_power = 1.0;
                }
                if input.has(BTN_FIRE) && ship.can_fire() {
                    ship.fire_cd = tuning::FIRE_COOLDOWN;
                    let dir = match self.scheme {
                        Scheme::Classic => ship.body.angle,
                        Scheme::Modern => input.aim,
                    };
                    gun_shot = Some((ship.body.p + V2::from_angle(dir) * 11.0, dir));
                }
            }
            ship.tick_timers(DT);
        }

        if thrust_power > 0.0 {
            self.events.push(Event::Thrust {
                p: self.ship.body.p - self.ship.thrust_dir * 6.0,
                angle: self.ship.thrust_dir.angle(),
                power: thrust_power,
            });
        }
        if let Some((p, dir)) = gun_shot {
            let b = Bullet::fire(WeaponId::Gun, p, dir, self.ship.body.v, true);
            self.spawn_shot(b);
        }

        // The special: one of the roster, limited by ammo, swapped at a base.
        let aim = match self.scheme {
            Scheme::Classic => self.ship.body.angle,
            Scheme::Modern => input.aim,
        };
        if input.has(BTN_SPECIAL) && self.ship.alive && self.ship.loadout.ready() {
            self.fire_special(aim);
        }
        // The stripped-for-speed choice: dump the special's magazine to fly
        // lighter. The gun is never dumped, and a base can rearm.
        if input.has(BTN_DUMP) && self.ship.alive && self.ship.loadout.ammo > 0 {
            self.ship.loadout.ammo = 0;
            let p = self.ship.body.p;
            self.events.push(Event::Jettison { p });
        }
        self.step_tether(input.has(BTN_SPECIAL));
        self.cycle_weapon(input);
        self.update_beam(input.has(BTN_BEAM));
    }

    /// Fires the selected special, by the kind of thing it is.
    fn fire_special(&mut self, dir: f32) {
        let weapon = self.ship.loadout.special;
        let s = weapons::spec(weapon);
        let p = self.ship.body.p + V2::from_angle(dir) * 12.0;
        match s.kind {
            Kind::Bolt | Kind::Shell => {
                self.spawn_salvo(weapon, p, dir);
                self.ship.loadout.spend();
                self.events.push(Event::Special {
                    weapon,
                    p,
                    angle: dir,
                });
            }
            Kind::Place => {
                if self.place_gadget(weapon, dir) {
                    self.ship.loadout.spend();
                    self.events.push(Event::Special {
                        weapon,
                        p,
                        angle: dir,
                    });
                }
            }
            Kind::Stream => {
                self.spawn_stream(weapon, p, dir);
                self.ship.loadout.spend();
                self.events.push(Event::Special {
                    weapon,
                    p,
                    angle: dir,
                });
            }
            Kind::SelfEffect => {
                if self.apply_self_effect(weapon, dir) {
                    self.ship.loadout.spend();
                    self.events.push(Event::Special {
                        weapon,
                        p,
                        angle: dir,
                    });
                }
            }
        }
    }

    /// Fires the `count` projectiles of one shot, fanned over its `spread`.
    ///
    /// A full turn (`Multicannon`) goes all the way round; anything narrower is
    /// a fan centred on the aim.
    fn spawn_salvo(&mut self, weapon: WeaponId, p: V2, dir: f32) {
        let s = weapons::spec(weapon);
        let count = s.count.max(1) as usize;
        let all_round = (s.spread - std::f32::consts::TAU).abs() < 1e-3;
        for i in 0..count {
            let offset = if count == 1 {
                0.0
            } else if all_round {
                i as f32 * std::f32::consts::TAU / count as f32
            } else {
                (i as f32 / (count as f32 - 1.0) - 0.5) * s.spread
            };
            let b = Bullet::fire(weapon, p, dir + offset, self.ship.body.v, true);
            self.spawn_shot(b);
        }
    }

    /// Pushes a shot into the world: the cap keeps a stream weapon from growing
    /// the projectile list without bound.
    fn spawn_shot(&mut self, b: Bullet) {
        self.stats.shots += 1;
        self.events.push(Event::Bullet {
            p: b.p,
            angle: b.angle(),
            from_player: b.from_player,
            weapon: b.weapon,
        });
        if self.bullets.len() < tuning::MAX_SHOTS {
            self.bullets.push(b);
        }
    }

    /// A stream weapon lays a short-lived cloud in front of the ship: flame,
    /// water, air. The cloud is what does the work.
    fn spawn_stream(&mut self, weapon: WeaponId, p: V2, dir: f32) {
        let s = weapons::spec(weapon);
        let kind = s.cloud.unwrap_or(CloudKind::Flame);
        let c = Cloud {
            p: p + V2::from_angle(dir) * 10.0,
            v: V2::from_angle(dir) * 90.0,
            kind,
            radius: s.cloud_radius.max(8.0),
            ttl: s.cloud_ttl,
            age: 0.0,
            dps: s.cloud_dps,
            push: s.push,
            from_player: true,
            acc: 0.0,
            weapon,
        };
        self.spawn_cloud(c);
    }

    fn spawn_cloud(&mut self, c: Cloud) {
        self.events.push(Event::Cloud {
            p: c.p,
            radius: c.radius,
            kind: c.kind,
        });
        if self.clouds.len() < tuning::MAX_CLOUDS {
            self.clouds.push(c);
        }
    }

    /// Lays a mine, a charge, a well or a pair of troopers. Returns whether
    /// anything was actually placed, so a refused placement costs no ammo.
    fn place_gadget(&mut self, weapon: WeaponId, dir: f32) -> bool {
        let s = weapons::spec(weapon);
        let forward = V2::from_angle(dir);
        let Some(kind) = s.gadget else {
            return false;
        };
        match kind {
            GadgetKind::Mine | GadgetKind::Landmine => {
                // Landmines go out of the back, mines out of the nose.
                let p = self.ship.body.p
                    + match kind {
                        GadgetKind::Landmine => -forward * 18.0,
                        _ => forward * 18.0,
                    };
                if self.laid_mines() >= tuning::MAX_LAID_MINES {
                    return false;
                }
                self.mines.push(Mine::laid(p, s.ttl, s.blast));
                self.events.push(Event::Gadget { p, kind });
            }
            _ => {
                if self.gadgets.len() >= tuning::MAX_GADGETS {
                    return false;
                }
                let count = if kind == GadgetKind::Troopers {
                    s.count.max(1) as usize
                } else {
                    1
                };
                for i in 0..count {
                    let offset = (i as f32 - (count as f32 - 1.0) * 0.5) * 14.0;
                    let p = self.ship.body.p + forward * 14.0 + forward.perp() * offset;
                    self.gadgets.push(Gadget {
                        p,
                        prev: p,
                        v: self.ship.body.v,
                        kind,
                        weapon,
                        from_player: true,
                        age: 0.0,
                        ttl: s.ttl,
                        armed: false,
                        resting: false,
                        blast: s.blast,
                        radius: s.radius,
                        cd: 0.0,
                    });
                    self.events.push(Event::Gadget { p, kind });
                }
            }
        }
        true
    }

    /// The ship's own effects: a shield bubble, a blink, an electric blast.
    /// Returns whether the effect happened, so a refused blink costs no ammo.
    fn apply_self_effect(&mut self, weapon: WeaponId, dir: f32) -> bool {
        let s = weapons::spec(weapon);
        let p = self.ship.body.p;
        match s.effect {
            Effect::Shield => {
                self.ship.shield_field = s.ttl;
                self.ship.shield.repair();
                true
            }
            Effect::Blink => {
                let forward = V2::from_angle(dir);
                let from = self.ship.body.p;
                let want = from + forward * s.blink;
                // Blink to just short of whatever stops it; refuse a blink with
                // nowhere to go rather than charging for a dud.
                let to = match self.terrain.segment_hit(from, want) {
                    Some(hit) => {
                        if hit.p.dist(from) < tuning::BLINK_MIN {
                            return false;
                        }
                        hit.p - forward * tuning::BLINK_CLEARANCE
                    }
                    None => want,
                };
                self.ship.body.p = to;
                self.ship.prev = self.ship.body;
                self.events.push(Event::Blink { from, to });
                true
            }
            Effect::Emp => {
                self.events.push(Event::Emp {
                    p,
                    radius: s.radius,
                });
                self.emp_blast(p, s.radius, s.damage, s.ttl);
                true
            }
            _ => false,
        }
    }

    /// Knocks out the defences around a point and hurts everything hostile in
    /// range. The electric blast's own shape.
    fn emp_blast(&mut self, p: V2, radius: f32, damage: i32, ttl: f32) {
        let mut kills: Vec<(V2, bool)> = Vec::new();
        for t in self.turrets.iter_mut() {
            if !t.alive || t.p.dist(p) > radius + tuning::TURRET_RADIUS {
                continue;
            }
            t.emp = t.emp.max(ttl);
            t.hp -= damage;
            if t.hp <= 0 {
                t.alive = false;
                kills.push((t.p, true));
            }
        }
        for d in self.drones.iter_mut() {
            if d.alive && d.body.p.dist(p) <= radius + tuning::DRONE_RADIUS {
                d.hp -= damage;
                if d.hp <= 0 {
                    d.alive = false;
                    kills.push((d.body.p, false));
                }
            }
        }
        for (k, is_turret) in kills {
            if is_turret {
                self.stats.turrets_destroyed += 1;
                self.events.push(Event::TurretDestroyed { p: k });
            } else {
                self.stats.drones_destroyed += 1;
                self.events.push(Event::DroneDestroyed { p: k });
            }
            self.explode(k, 0.5);
        }
    }

    /// The harpoon's tether: while the trigger is held the ship is reeled in
    /// towards the anchor. Reeling burns no fuel — the rock does the pulling —
    /// but each shot holds for a few seconds, so it is a movement trick rather
    /// than a way to skip the fuel economy the levels are built around.
    fn step_tether(&mut self, held: bool) {
        let Some((anchor, _)) = self.ship.tether else {
            return;
        };
        if !held || !self.ship.alive {
            self.ship.tether = None;
            self.events.push(Event::Tether {
                p: anchor,
                on: false,
            });
            return;
        }
        let to = anchor - self.ship.body.p;
        let d = to.len();
        if d < 10.0 {
            self.ship.tether = None;
            self.events.push(Event::Tether {
                p: anchor,
                on: false,
            });
            return;
        }
        let pull = to / d * tuning::TETHER_PULL;
        self.ship.body.v += pull * DT;
        let cap = tuning::TETHER_MAX_SPEED;
        if self.ship.body.v.len() > cap {
            self.ship.body.v = self.ship.body.v.normalized() * cap;
        }
    }

    /// Steps the special weapon through the level's list, at a base only: this
    /// is Wings' "change weapon with the turn buttons while docked".
    fn cycle_weapon(&mut self, input: InputFrame) {
        self.weapon_cd = (self.weapon_cd - DT).max(0.0);
        let dir = i32::from(input.has(BTN_WEAPON_NEXT)) - i32::from(input.has(BTN_WEAPON_PREV));
        if dir == 0 || self.weapon_cd > 0.0 || self.level.weapons.len() < 2 {
            return;
        }
        if !self.docked_on_pad() {
            return;
        }
        self.weapon_cd = tuning::WEAPON_CYCLE_COOLDOWN;
        self.ship.loadout.cycle(dir, &self.level.weapons);
        self.pad_cooldown = 0.4;
        self.events.push(Event::Special {
            weapon: self.ship.loadout.special,
            p: self.ship.body.p,
            angle: self.ship.body.angle,
        });
    }

    fn update_beam(&mut self, held: bool) {
        let Some(idx) = self.ship.attached else {
            if !held || !self.ship.alive {
                return;
            }
            let ship_p = self.ship.body.p;
            let mut best: Option<(usize, f32)> = None;
            for (i, pod) in self.pods.iter().enumerate() {
                if !pod.alive || pod.attached {
                    continue;
                }
                let d = pod.body.p.dist(ship_p);
                if d <= tuning::BEAM_RANGE && best.map(|(_, bd)| d < bd).unwrap_or(true) {
                    // No beaming through rock.
                    if self.terrain.segment_hit(ship_p, pod.body.p).is_none() {
                        best = Some((i, d));
                    }
                }
            }
            if let Some((i, d)) = best {
                self.pods[i].attached = true;
                self.pods[i].rod_len = d.max(26.0);
                self.ship.attached = Some(i);
                let p = self.pods[i].body.p;
                self.events.push(Event::Beam { on: true });
                self.events.push(Event::PodAttached { p });
            }
            return;
        };

        let pod = self.pods[idx];
        let too_far = pod.body.p.dist(self.ship.body.p) > tuning::BEAM_BREAK;
        if !held || !pod.alive || too_far || !self.ship.alive {
            self.pods[idx].attached = false;
            self.ship.attached = None;
            self.events.push(Event::Beam { on: false });
        }
    }

    /// Advances every live gate. Inactive gates are inert and invisible.
    fn step_gates(&mut self) {
        for g in self.gates.iter_mut() {
            g.step(DT);
        }
    }

    fn step_turrets(&mut self) {
        let ship_p = self.ship.body.p;
        let ship_v = self.ship.body.v;
        let chase = self.ship.alive;
        let mut shots: Vec<Bullet> = Vec::new();
        for t in self.turrets.iter_mut() {
            if !t.alive {
                continue;
            }
            t.cooldown = (t.cooldown - DT).max(0.0);
            t.emp = (t.emp - DT).max(0.0);
            t.frozen = (t.frozen - DT).max(0.0);
            if !chase || !t.operational() {
                continue;
            }
            let to = ship_p + ship_v * tuning::TURRET_LEAD - t.p;
            let dist = to.len();
            if dist > tuning::TURRET_RANGE {
                continue;
            }
            let target = to.angle();
            t.aim = approach_angle(t.aim, target, tuning::TURRET_TURN_RATE * DT);
            if t.cooldown <= 0.0
                && angle_diff(t.aim, target).abs() < 0.12
                && self.terrain.segment_hit(t.p, ship_p).is_none()
            {
                t.cooldown = tuning::TURRET_FIRE_COOLDOWN;
                let mut b = Bullet::fire(WeaponId::Gun, t.p, t.aim, V2::ZERO, false);
                b.v = V2::from_angle(t.aim) * tuning::TURRET_BULLET_SPEED;
                b.life = 2.2;
                b.carve = tuning::CARVE_BULLET;
                shots.push(b);
            }
        }
        for b in shots {
            self.events.push(Event::Bullet {
                p: b.p,
                angle: b.angle(),
                from_player: false,
                weapon: b.weapon,
            });
            self.bullets.push(b);
        }
    }

    fn step_drones(&mut self) {
        let ship_p = self.ship.body.p;
        let chase = self.ship.alive;
        for d in self.drones.iter_mut() {
            if !d.alive {
                continue;
            }
            d.prev = d.body.p;
            d.age += DT;
            d.frozen = (d.frozen - DT).max(0.0);
            d.netted = (d.netted - DT).max(0.0);
            // Pulsed thrust: a drone that never coasts cannot be dodged.
            d.thrusting = (d.age % 1.3) < 0.85;
            // A frozen or netted drone keeps its momentum and nothing else.
            if !d.mobile() {
                d.thrusting = false;
                continue;
            }
            if chase {
                let to = ship_p - d.body.p;
                if to.len() < 700.0 {
                    d.body.angle =
                        approach_angle(d.body.angle, to.angle(), tuning::DRONE_TURN_RATE * DT);
                }
                d.thrusting = d.thrusting || to.len() < 120.0;
            } else {
                d.thrusting = false;
            }
        }
    }

    fn integrate(&mut self) {
        let Self {
            level,
            water,
            ship,
            pods,
            drones,
            mines,
            bullets,
            clouds,
            gadgets,
            ..
        } = self;
        let gravity = level.gravity;
        let wind = level.wind;

        if ship.alive {
            let (mut acc, drag) = ambient(water, gravity, wind, ship.body.p);
            if ship.thrusting {
                acc += ship.thrust_dir * ship.thrust_available(gravity);
            }
            ship.body.integrate(acc, DT);
            apply_drag(&mut ship.body, drag);
        }
        for pod in pods.iter_mut() {
            if !pod.alive {
                continue;
            }
            pod.prev = pod.body;
            let (acc, drag) = ambient(water, gravity, wind, pod.body.p);
            pod.body.integrate(acc, DT);
            apply_drag(&mut pod.body, drag);
        }
        for d in drones.iter_mut() {
            if !d.alive {
                continue;
            }
            let (mut acc, drag) = ambient(water, gravity, wind, d.body.p);
            if d.thrusting {
                acc += d.body.forward() * gravity * tuning::DRONE_THRUST_RATIO;
            }
            d.body.integrate(acc, DT);
            apply_drag(&mut d.body, drag);
        }
        for m in mines.iter_mut() {
            if !m.alive {
                continue;
            }
            m.prev = m.body.p;
            m.age += DT;
            let (acc, drag) = ambient(water, gravity, wind, m.body.p);
            m.body.integrate(acc, DT);
            apply_drag(&mut m.body, drag);
        }
        for b in bullets.iter_mut() {
            b.prev = b.p;
            b.age += DT;
            if b.gravity > 0.0 {
                b.v += V2::new(0.0, gravity * b.gravity) * DT;
            }
            b.p += b.v * DT;
            b.life -= DT;
            // Water stops bullets: a pool is cover, not just scenery. Weapons
            // that work underwater (`Torpedo`, `Watercannon`) plough through
            // instead; everything else is dragged to a halt and dies there,
            // which is where its cloud is left.
            if water.mass_at(b.p) > 0 && !weapons::spec(b.weapon).water_ok {
                b.v *= 1.0 / (1.0 + tuning::WATER_BULLET_DRAG * DT);
                if b.v.len() < tuning::WATER_SHOT_FLOOR {
                    b.life = 0.0;
                }
            }
        }
        for c in clouds.iter_mut() {
            c.age += DT;
            c.ttl -= DT;
            c.p += c.v * DT;
            c.v *= 1.0 / (1.0 + 1.4 * DT);
            c.acc += c.dps * DT;
        }
        for g in gadgets.iter_mut() {
            g.prev = g.p;
            g.age += DT;
            g.ttl -= DT;
            if !g.resting {
                let (acc, drag) = ambient(water, gravity, wind, g.p);
                g.v += acc * DT;
                g.v *= 1.0 / (1.0 + drag * DT);
                g.p += g.v * DT;
            }
        }
    }

    /// Rigid rod between ship and payload: the genre's signature chaos (§4.2).
    fn solve_rod(&mut self) {
        let Some(idx) = self.ship.attached else {
            return;
        };
        if !self.pods[idx].alive {
            self.ship.attached = None;
            return;
        }
        let ship = &mut self.ship;
        let pod = &mut self.pods[idx];
        let w_ship = 1.0 / tuning::SHIP_MASS;
        let w_pod = 1.0 / tuning::POD_MASS;

        // The rod is anchored behind the hull, so it applies a torque to the ship.
        let anchor = ship.body.to_world(V2::new(-tuning::ROD_ANCHOR, 0.0));
        let r = anchor - ship.body.p;
        let d = pod.body.p - anchor;
        let dist = d.len().max(1e-4);
        let dir = d / dist;
        let err = dist - pod.rod_len;

        let total_w = w_ship + w_pod;
        pod.body.p -= dir * (err * (w_pod / total_w));
        ship.body.p += dir * (err * (w_ship / total_w));

        // Effective mass includes how far off-centre the rod pulls.
        let lever = r.cross(dir);
        let k = w_ship + w_pod + lever * lever * tuning::SHIP_INV_INERTIA;
        let rel = pod.body.v - (ship.body.v + r.perp() * ship.body.omega);
        let lambda = -rel.dot(dir) / k;

        let impulse = dir * lambda;
        ship.body.v -= impulse * w_ship;
        ship.body.omega -= r.cross(impulse) * tuning::SHIP_INV_INERTIA;
        pod.body.v += impulse * w_pod;
        ship.body.omega = ship.body.omega.clamp(-8.0, 8.0);
    }

    fn collide(&mut self) {
        self.collide_ship_terrain();
        self.collide_pods();
        self.collide_gates();
        self.collide_drones();
        self.collide_mines();
        self.collide_bullets();
        self.cleanup();
    }

    fn collide_ship_terrain(&mut self) {
        if !self.ship.alive {
            return;
        }
        let prev = self.ship.prev_hull();
        let cur = self.ship.hull();
        let mut hit = None;
        // Containment net first: a hull vertex left inside the mass would tunnel
        // through the wall on this tick, because there is no edge to cross.
        for vertex in cur {
            if let Some(h) = self.terrain.penetration(vertex, self.ship.body.v) {
                // Slide the hull out along the escape direction until the vertex
                // is comfortably clear of the surface — unless it is a granular
                // wall holding it, which reaches the face and stops there.
                if h.material != MAT_GRANULAR {
                    self.ship.body.p += h.normal * (h.depth + 3.0);
                }
                hit = Some(h);
                break;
            }
        }
        for i in 0..cur.len() {
            if hit.is_some() {
                break;
            }
            if let Some(h) = self.terrain.segment_hit(prev[i], cur[i]) {
                hit = Some(h);
                break;
            }
        }
        if hit.is_none() {
            // Static overlap: hull edges at either end of the tick.
            for i in 0..cur.len() {
                let j = (i + 1) % cur.len();
                if let Some(h) = self.terrain.segment_hit(cur[i], cur[j]) {
                    hit = Some(h);
                    break;
                }
            }
        }
        let Some(hit) = hit else {
            if self.ship.grabbed {
                self.ship.grabbed = false;
                let p = self.ship.body.p;
                self.events.push(Event::Grab { p, on: false });
            }
            return;
        };

        // A granular wall grabs instead of killing: the AUTS trap, escaped by
        // shooting the wall away. Hitting one at speed is still a crash.
        if hit.material == MAT_GRANULAR {
            let speed = self.ship.body.v.len();
            if speed <= tuning::GRAB_MAX_SPEED {
                if !self.ship.grabbed {
                    self.ship.grabbed = true;
                    self.events.push(Event::Grab { p: hit.p, on: true });
                }
                if hit.depth > 0.0 {
                    self.ship.body.p += hit.normal * hit.depth;
                }
                self.ship.body.v *= tuning::GRAB_DAMP;
                return;
            }
            // Too fast to hold: chip the wall and let the normal rules decide.
            self.strike(hit.p, tuning::CARVE_BULLET * 1.5, 4);
        }

        if self.ship.shield.absorb(self.ship.has_fuel()) {
            let speed = self.ship.body.v.len();
            self.ship.body.v = hit.normal * speed * tuning::SHIELD_BOUNCE;
            self.ship.body.p += hit.normal * 2.0;
            self.stats.damage_taken += 1;
            self.events.push(Event::ShieldHit {
                p: hit.p,
                angle: hit.normal.angle(),
            });
        } else {
            self.destroy_ship();
        }
    }

    fn collide_pods(&mut self) {
        let required = self.level.require_pod;
        for i in 0..self.pods.len() {
            if !self.pods[i].alive {
                continue;
            }
            let speed = self.pods[i].body.v.len();
            let Some(hit) = self.pod_terrain_hit(i) else {
                continue;
            };
            if self.pods[i].attached {
                // On the beam the payload is a wrecking ball: the rod is the
                // level's boss, and hitting rock with it ends the mission.
                self.lose_pod(i, required);
            } else {
                // Free payload: it settles where it lands. Parking it on a ledge
                // to scout ahead is a tactic, not a mistake.
                let pod = &mut self.pods[i];
                if speed > tuning::POD_REST_SPEED {
                    let p = pod.body.p;
                    self.events.push(Event::Landing { p, speed });
                }
                pod.body.v = V2::ZERO;
                pod.body.omega = 0.0;
                pod.body.p += hit.normal * (hit.depth + 2.0);
            }
        }
    }

    /// A moving slab is lethal machinery: the shield eats the hit if it can,
    /// otherwise the crusher takes the ship. A payload that meets one is lost
    /// on the beam and settles when free, exactly like terrain contact.
    fn collide_gates(&mut self) {
        if self.gates.is_empty() {
            return;
        }
        if self.ship.alive {
            let hull = self.ship.hull();
            let mut hit = None;
            for g in &self.gates {
                if !g.active {
                    continue;
                }
                if g.rect.contains(self.ship.body.p) || hull.iter().any(|v| g.rect.contains(*v)) {
                    hit = Some(g.rect.center());
                    break;
                }
            }
            if let Some(p) = hit {
                self.ship_contact(p);
            }
        }
        if !self.ship.alive {
            return;
        }
        let required = self.level.require_pod;
        for i in 0..self.pods.len() {
            if !self.pods[i].alive {
                continue;
            }
            let samples = pod_samples(&self.pods[i].body);
            let inside = samples
                .iter()
                .any(|s| self.gates.iter().any(|g| g.active && g.rect.contains(*s)));
            if !inside {
                continue;
            }
            if self.pods[i].attached {
                self.lose_pod(i, required);
            } else {
                let pod = &mut self.pods[i];
                pod.body.v = V2::ZERO;
                pod.body.omega = 0.0;
            }
        }
    }

    /// Terrain contact for the payload, using the rod-independent hull samples.
    fn pod_terrain_hit(&self, i: usize) -> Option<crate::sim::terrain::Hit> {
        let pod = &self.pods[i];
        if let Some(hit) = self.terrain.penetration(pod.body.p, pod.body.v) {
            return Some(hit);
        }
        let samples = pod_samples(&pod.body);
        let prev_samples = pod_samples(&pod.prev);
        for k in 0..samples.len() {
            if let Some(hit) = self.terrain.segment_hit(prev_samples[k], samples[k]) {
                return Some(hit);
            }
        }
        for k in 0..samples.len() {
            let j = (k + 1) % samples.len();
            if let Some(hit) = self.terrain.segment_hit(samples[k], samples[j]) {
                return Some(hit);
            }
        }
        None
    }

    /// The payload is fragile: any contact with rock destroys it, and a mission
    /// that needed it is failed (`docs/game_mechanics.md` §4.3).
    fn lose_pod(&mut self, i: usize, required: bool) {
        self.pods[i].alive = false;
        self.pods[i].attached = false;
        if self.ship.attached == Some(i) {
            self.ship.attached = None;
        }
        let p = self.pods[i].body.p;
        self.events.push(Event::PodLost { p });
        self.explode(p, 0.7);
        if required {
            self.fail(Failure::PayloadLost);
        }
    }

    fn collide_drones(&mut self) {
        let ship_p = self.ship.body.p;
        let ship_alive = self.ship.alive;
        for i in 0..self.drones.len() {
            if !self.drones[i].alive {
                continue;
            }
            let prev = self.drones[i].prev;
            let cur = self.drones[i].body.p;
            if self
                .level
                .terrain
                .penetration(cur, self.drones[i].body.v)
                .is_some()
                || self.terrain.segment_hit(prev, cur).is_some()
            {
                self.drones[i].alive = false;
                self.stats.drones_destroyed += 1;
                self.events.push(Event::DroneDestroyed { p: cur });
                self.explode(cur, 0.5);
                continue;
            }
            if ship_alive && cur.dist(ship_p) < tuning::DRONE_RADIUS + tuning::SHIP_RADIUS {
                self.drones[i].alive = false;
                self.stats.drones_destroyed += 1;
                self.events.push(Event::DroneDestroyed { p: cur });
                self.explode(cur, 0.6);
                self.ship_contact(cur);
            }
        }
    }

    fn collide_mines(&mut self) {
        let ship_p = self.ship.body.p;
        let ship_alive = self.ship.alive;
        let mut detonate: Vec<usize> = Vec::new();
        for i in 0..self.mines.len() {
            if !self.mines[i].alive {
                continue;
            }
            self.mines[i].ttl -= DT;
            if self.mines[i].expired() {
                self.mines[i].alive = false;
                continue;
            }
            if !self.mines[i].resting {
                let prev = self.mines[i].prev;
                let cur = self.mines[i].body.p;
                if let Some(h) = self.terrain.segment_hit(prev, cur) {
                    self.mines[i].body.p = h.p - h.normal * 1.0;
                    self.mines[i].body.v = V2::ZERO;
                    self.mines[i].resting = true;
                }
            }
            if self.mines[i].age <= tuning::GADGET_ARM_TIME {
                continue;
            }
            let p = self.mines[i].body.p;
            if self.mines[i].from_player {
                // A mine the player laid waits for something hostile: its owner
                // can fly straight over it. That is what makes it a weapon
                // rather than a trap for the pilot who placed it.
                if self.hostile_near(p, tuning::MINE_PROXIMITY) {
                    detonate.push(i);
                }
            } else if ship_alive && p.dist(ship_p) < tuning::MINE_PROXIMITY {
                detonate.push(i);
            }
        }
        for i in detonate {
            self.detonate_mine(i);
        }
    }

    /// True when anything hostile to the player is within `radius` of `p`.
    fn hostile_near(&self, p: V2, radius: f32) -> bool {
        self.turrets
            .iter()
            .any(|t| t.alive && t.p.dist(p) < radius + tuning::TURRET_RADIUS)
            || self
                .drones
                .iter()
                .any(|d| d.alive && d.body.p.dist(p) < radius + tuning::DRONE_RADIUS)
            || self
                .reactors
                .iter()
                .any(|r| !r.destroyed && r.p.dist(p) < radius + tuning::REACTOR_RADIUS)
    }

    fn detonate_mine(&mut self, index: usize) {
        let mine = self.mines[index];
        let p = mine.body.p;
        let radius = mine.blast;
        self.mines[index].alive = false;
        self.mines[index].resting = true;
        self.stats.mines_destroyed += 1;
        self.events.push(Event::MineBlast { p });
        // A mine takes the floor with it: this is the blast that opens a level.
        self.dig(p, tuning::CARVE_BLAST * 0.8);

        // Chain the neighbours of the same owner: one mine clears a shaft.
        for j in 0..self.mines.len() {
            if j != index
                && self.mines[j].alive
                && self.mines[j].from_player == mine.from_player
                && self.mines[j].body.p.dist(p) < radius
            {
                self.mines[j].alive = false;
                self.stats.mines_destroyed += 1;
                self.events.push(Event::MineBlast {
                    p: self.mines[j].body.p,
                });
            }
        }
        self.blast_damage(p, radius, tuning::MINE_BLAST_DAMAGE, mine.from_player);
    }

    /// One blast's worth of damage, applied to whichever side fired it.
    ///
    /// Blasts are the only weapon effect that is symmetric: a player blast
    /// hurts anything hostile and can catch the ship that fired it, a hostile
    /// blast hurts the ship. That single rule keeps bombs honest.
    fn blast_damage(&mut self, p: V2, radius: f32, damage: i32, from_player: bool) {
        if from_player {
            let mut kills: Vec<(V2, bool)> = Vec::new();
            for t in self.turrets.iter_mut() {
                if !t.alive || t.p.dist(p) > radius + tuning::TURRET_RADIUS {
                    continue;
                }
                t.hp -= damage;
                if t.hp <= 0 {
                    t.alive = false;
                    kills.push((t.p, true));
                }
            }
            for d in self.drones.iter_mut() {
                if d.alive && d.body.p.dist(p) <= radius + tuning::DRONE_RADIUS {
                    d.alive = false;
                    kills.push((d.body.p, false));
                }
            }
            let mut reactor_broken = false;
            for r in self.reactors.iter_mut() {
                if !r.destroyed && r.p.dist(p) <= radius + tuning::REACTOR_RADIUS {
                    r.hp -= damage;
                    if r.hp <= 0 {
                        r.destroyed = true;
                        r.critical = tuning::ESCAPE_LIMIT;
                        reactor_broken = true;
                        kills.push((r.p, true));
                    }
                }
            }
            for (p, is_turret) in kills {
                if is_turret {
                    self.stats.turrets_destroyed += 1;
                    self.events.push(Event::TurretDestroyed { p });
                } else {
                    self.stats.drones_destroyed += 1;
                    self.events.push(Event::DroneDestroyed { p });
                }
                self.explode(p, 0.6);
            }
            if reactor_broken {
                self.event_reactor_destroyed();
            }
        }
        // Standing in your own blast is your own fault, and it is what keeps a
        // bomb from being a point-blank melee weapon.
        let hit_own_ship = if from_player {
            p.dist(self.ship.body.p) < radius * tuning::SELF_BLAST_FRACTION
        } else {
            p.dist(self.ship.body.p) < radius
        };
        if self.ship.alive && hit_own_ship {
            self.ship_contact(p);
        }
    }

    /// A weapon blast: crater, flash, then the damage.
    fn blast_at(&mut self, p: V2, radius: f32, damage: i32, carve: f32, from_player: bool) {
        if carve > 0.0 {
            self.dig(p, carve);
        }
        self.events.push(Event::Explosion {
            p,
            power: (radius / tuning::CARVE_BLAST).clamp(0.3, 2.0),
        });
        self.blast_damage(p, radius, damage, from_player);
    }

    /// `Dirtball`'s crater, run backwards: dirt is *added*, and the water above
    /// it wakes because the space it was sitting in just moved.
    fn fill_terrain(&mut self, p: V2, radius: f32) {
        let filled = self.terrain.fill(p, radius);
        if filled > 0 {
            self.water.wake(
                p.x as i32 - radius as i32,
                p.y as i32 - radius as i32,
                p.x as i32 + radius as i32,
                p.y as i32 + radius as i32,
            );
            self.events.push(Event::Fill { p, radius });
        }
    }

    fn collide_bullets(&mut self) {
        for i in 0..self.bullets.len() {
            if self.bullets[i].life <= 0.0 {
                continue;
            }
            if self.bullet_terrain(i) {
                continue;
            }
            if self.bullet_targets(i) {
                continue;
            }
            // A shot that simply ran out of time still leaves its cloud.
            if self.bullets[i].life <= 0.0 {
                self.leave_cloud(i);
            }
        }
    }

    /// Terrain interaction for one shot. Returns true when the shot is finished.
    fn bullet_terrain(&mut self, i: usize) -> bool {
        let b = self.bullets[i];
        // A gate is machinery, not terrain: a shot stops on it and never digs.
        for g in &self.gates {
            if g.active && g.rect.segment_hits(b.prev, b.p) {
                self.events.push(Event::Impact {
                    p: b.p,
                    normal: V2::new(0.0, -1.0),
                });
                self.kill_shot(i);
                return true;
            }
        }
        let Some(h) = self.terrain.segment_hit(b.prev, b.p) else {
            return false;
        };
        let effect_ttl = weapons::spec(b.weapon).ttl;

        // The harpoon: bite the rock and hold. No crater, no damage — the
        // reward is the pull.
        if b.effect == Effect::Tether {
            self.ship.tether = Some((h.p, effect_ttl.max(1.0)));
            self.kill_shot(i);
            self.events.push(Event::Tether { p: h.p, on: true });
            return true;
        }

        // The dirtball: the only shot that builds terrain instead of removing it.
        if b.fill > 0.0 {
            self.fill_terrain(h.p, b.fill);
            self.kill_shot(i);
            return true;
        }

        // A bouncer reflects and carries on, losing a little speed.
        if b.bounce > 0 {
            let v = b.v;
            let reflected = v - h.normal * (2.0 * v.dot(h.normal));
            let bullet = &mut self.bullets[i];
            bullet.bounce -= 1;
            bullet.v = reflected * tuning::BOUNCE_RESTITUTION;
            bullet.p = h.p + h.normal * 2.0;
            bullet.prev = bullet.p;
            self.events.push(Event::Impact {
                p: h.p,
                normal: h.normal,
            });
            self.strike(h.p, b.carve * 0.5, b.damage);
            return false;
        }

        // A drill bores through: it keeps digging until its shots run out, which
        // is what makes the `Digger` a tunnelling tool rather than a gun.
        if b.pierce > 0 {
            self.bullets[i].pierce -= 1;
            self.events.push(Event::Impact {
                p: h.p,
                normal: h.normal,
            });
            self.strike(h.p, b.carve, b.damage);
            self.bullets[i].prev = self.bullets[i].p;
            return false;
        }

        if b.blast > 0.0 {
            self.detonate_shot(i);
            return true;
        }

        self.events.push(Event::Impact {
            p: h.p,
            normal: h.normal,
        });
        // Every weapon digs. This crater is what lets the player tunnel, and it
        // is why the cave is made of cells. Granular walls only chip.
        self.strike(h.p, b.carve, b.damage);
        self.kill_shot(i);
        true
    }

    /// Target interaction for one shot: turrets, drones, the reactor, hostile
    /// mines, and — for enemy fire — the ship. Returns true when it is finished.
    fn bullet_targets(&mut self, i: usize) -> bool {
        let b = self.bullets[i];
        if !b.from_player {
            if self.ship.alive && b.p.dist(self.ship.body.p) < tuning::SHIP_RADIUS + b.radius {
                self.kill_shot(i);
                self.ship_contact(b.p);
                return true;
            }
            return false;
        }

        let effect_ttl = weapons::spec(b.weapon).ttl;

        if let Some(t) = self
            .turrets
            .iter_mut()
            .find(|t| t.alive && t.p.dist(b.p) < tuning::TURRET_RADIUS + b.radius)
        {
            t.hp -= b.damage;
            t.frozen = t.frozen.max(if b.effect == Effect::Freeze {
                effect_ttl
            } else {
                0.0
            });
            self.stats.hits += 1;
            let p = t.p;
            if t.hp <= 0 {
                t.alive = false;
                self.stats.turrets_destroyed += 1;
                self.events.push(Event::TurretDestroyed { p });
                self.explode(p, 0.6);
            } else {
                self.events.push(Event::Impact {
                    p: b.p,
                    normal: -b.v.normalized(),
                });
            }
            if b.effect == Effect::Freeze {
                self.events.push(Event::Freeze { p });
            }
            return self.resolve_hit(i);
        }

        if let Some(d) = self
            .drones
            .iter_mut()
            .find(|d| d.alive && d.body.p.dist(b.p) < tuning::DRONE_RADIUS + b.radius)
        {
            d.hp -= b.damage;
            let p = d.body.p;
            match b.effect {
                Effect::Freeze => {
                    d.frozen = d.frozen.max(effect_ttl);
                    self.events.push(Event::Freeze { p });
                }
                Effect::Net => {
                    d.netted = d.netted.max(effect_ttl.max(tuning::NET_TIME));
                    d.thrusting = false;
                    self.events.push(Event::NetHit { p });
                }
                _ => {}
            }
            if d.hp <= 0 {
                d.alive = false;
                self.stats.drones_destroyed += 1;
                self.events.push(Event::DroneDestroyed { p });
                self.explode(p, 0.5);
            } else if b.effect == Effect::None {
                self.events.push(Event::Impact {
                    p: b.p,
                    normal: -b.v.normalized(),
                });
            }
            self.stats.hits += 1;
            return self.resolve_hit(i);
        }

        if let Some(r) = self
            .reactors
            .iter_mut()
            .find(|r| !r.destroyed && r.p.dist(b.p) < tuning::REACTOR_RADIUS + b.radius)
        {
            r.hp -= b.damage;
            self.stats.hits += 1;
            let p = r.p;
            if r.hp <= 0 {
                r.destroyed = true;
                r.critical = tuning::ESCAPE_LIMIT;
                self.events.push(Event::ReactorDestroyed { p });
                self.explode(p, 1.0);
                self.event_reactor_destroyed();
            } else {
                self.events.push(Event::ReactorHit { p });
            }
            return self.resolve_hit(i);
        }

        // A hostile mine can be shot off the wall.
        if let Some(m) = self.mines.iter_mut().find(|m| {
            m.alive && !m.from_player && m.body.p.dist(b.p) < tuning::MINE_RADIUS + b.radius
        }) {
            m.alive = false;
            self.stats.hits += 1;
            let p = m.body.p;
            self.explode(p, 0.3);
            return self.resolve_hit(i);
        }

        false
    }

    /// What happens when a shot has just hit something: it blows up, it carries
    /// on and loses a point of piercing, or it is spent.
    fn resolve_hit(&mut self, i: usize) -> bool {
        let b = self.bullets[i];
        if b.blast > 0.0 {
            self.detonate_shot(i);
            return true;
        }
        if self.bullets[i].pierce > 0 {
            self.bullets[i].pierce -= 1;
            return false;
        }
        self.kill_shot(i);
        true
    }

    /// Ends a shot, leaving the cloud it carries where it stopped.
    fn kill_shot(&mut self, i: usize) {
        self.bullets[i].life = 0.0;
        self.leave_cloud(i);
    }

    /// Turns a shot's `blast` and `burst` into a crater, a flash, damage and a
    /// spray of fragments.
    fn detonate_shot(&mut self, i: usize) {
        let b = self.bullets[i];
        self.kill_shot(i);
        self.blast_at(b.p, b.blast, b.damage.max(1), b.carve, b.from_player);
        if b.burst > 0 {
            for k in 0..b.burst {
                let angle = k as f32 * std::f32::consts::TAU / b.burst as f32;
                let frag = Bullet::fragment(&b, b.p, angle);
                self.spawn_shot(frag);
            }
        }
    }

    /// The cloud a shot leaves behind, if it carries one.
    fn leave_cloud(&mut self, i: usize) {
        let b = self.bullets[i];
        let Some(kind) = b.cloud else {
            return;
        };
        let s = weapons::spec(b.weapon);
        let c = Cloud {
            p: b.p,
            v: b.v * 0.12,
            kind,
            radius: s.cloud_radius.max(b.radius),
            ttl: b.cloud_ttl,
            age: 0.0,
            dps: s.cloud_dps,
            push: b.push,
            from_player: b.from_player,
            acc: 0.0,
            weapon: b.weapon,
        };
        self.spawn_cloud(c);
    }

    /// Damage over time from the clouds: each cloud carries an accumulator, and
    /// every whole hit point it has built up is dealt to everything hostile in
    /// its radius. Overlapping clouds therefore do overlap in effect, which is
    /// what makes a stream weapon at point-blank range a real threat.
    fn step_clouds(&mut self) {
        for i in 0..self.clouds.len() {
            let c = self.clouds[i];
            if c.push > 0.0 {
                self.cloud_push(c.p, c.radius, c.push, c.from_player);
            }
            let whole = c.acc.floor();
            if whole < 1.0 {
                continue;
            }
            let hits = whole.min(4.0) as i32;
            self.clouds[i].acc -= whole;
            self.cloud_damage(c.p, c.radius, hits, c.from_player);
            // Flame eats at the rock it is licking, a bite at a time.
            if c.kind == CloudKind::Flame {
                self.dig(c.p, tuning::FLAME_CARVE);
            }
        }
        self.clouds.retain(|c| c.ttl > 0.0);
    }

    /// A cloud's damage, applied to whichever side owns it.
    fn cloud_damage(&mut self, p: V2, radius: f32, damage: i32, from_player: bool) {
        if !from_player {
            if self.ship.alive && p.dist(self.ship.body.p) < radius + tuning::SHIP_RADIUS {
                self.ship_contact(p);
            }
            return;
        }
        let mut kills: Vec<(V2, bool)> = Vec::new();
        for t in self.turrets.iter_mut() {
            if !t.alive || t.p.dist(p) > radius + tuning::TURRET_RADIUS {
                continue;
            }
            t.hp -= damage;
            if t.hp <= 0 {
                t.alive = false;
                kills.push((t.p, true));
            }
        }
        for d in self.drones.iter_mut() {
            if d.alive && d.body.p.dist(p) <= radius + tuning::DRONE_RADIUS {
                d.hp -= damage;
                if d.hp <= 0 {
                    d.alive = false;
                    kills.push((d.body.p, false));
                }
            }
        }
        for (p, is_turret) in kills {
            if is_turret {
                self.stats.turrets_destroyed += 1;
                self.events.push(Event::TurretDestroyed { p });
            } else {
                self.stats.drones_destroyed += 1;
                self.events.push(Event::DroneDestroyed { p });
            }
            self.explode(p, 0.4);
        }
    }

    /// A cloud shoving things around: the water jet and the air cannon.
    fn cloud_push(&mut self, p: V2, radius: f32, push: f32, from_player: bool) {
        if !from_player {
            if self.ship.alive && p.dist(self.ship.body.p) < radius + tuning::SHIP_RADIUS {
                let dir = (self.ship.body.p - p).normalized();
                self.ship.body.v += dir * push * DT;
            }
            return;
        }
        for d in self.drones.iter_mut() {
            if d.alive && d.body.p.dist(p) <= radius + tuning::DRONE_RADIUS {
                let dir = (d.body.p - p).normalized();
                d.body.v += dir * push * DT;
            }
        }
    }

    /// Laid gadgets: charges tick down to a blast, wells pull, troopers shoot.
    fn step_gadgets(&mut self) {
        let mut shots: Vec<Bullet> = Vec::new();
        let mut blasts: Vec<(V2, f32, WeaponId)> = Vec::new();
        for i in 0..self.gadgets.len() {
            let g = self.gadgets[i];
            if g.ttl <= 0.0 {
                if g.kind == GadgetKind::Charge {
                    blasts.push((g.p, g.blast, g.weapon));
                }
                continue;
            }
            if !self.gadgets[i].resting && self.terrain.segment_hit(g.prev, g.p).is_some() {
                self.gadgets[i].resting = true;
                self.gadgets[i].v = V2::ZERO;
            }
            match g.kind {
                GadgetKind::Well => self.well_pull(g.p, g.radius),
                GadgetKind::Troopers => {
                    if let Some(b) = self.trooper_shot(i) {
                        shots.push(b);
                    }
                }
                _ => {}
            }
        }
        for (p, radius, weapon) in blasts {
            let s = weapons::spec(weapon);
            self.blast_at(p, radius, s.damage.max(2), radius * 0.6, true);
        }
        for b in shots {
            self.spawn_shot(b);
        }
        self.gadgets.retain(|g| g.ttl > 0.0);
    }

    /// The gravity well: drones and enemy fire are dragged towards it. Nothing
    /// pulls the player's own ship, which would make the weapon a trap.
    fn well_pull(&mut self, centre: V2, radius: f32) {
        for d in self.drones.iter_mut() {
            if !d.alive || d.body.p.dist(centre) > radius {
                continue;
            }
            let dir = (centre - d.body.p).normalized();
            d.body.v += dir * tuning::WELL_PULL * DT;
        }
        for b in self.bullets.iter_mut() {
            if b.from_player || b.p.dist(centre) > radius {
                continue;
            }
            let dir = (centre - b.p).normalized();
            b.v += dir * tuning::WELL_PULL * DT;
        }
    }

    /// One trooper firing at whatever hostile it can see. The shot is a plain
    /// gun round, so a deployed squad fights with the same weapon the pilot has.
    fn trooper_shot(&mut self, i: usize) -> Option<Bullet> {
        let g = self.gadgets[i];
        if g.armed && g.cd > 0.0 {
            self.gadgets[i].cd -= DT;
            return None;
        }
        self.gadgets[i].armed = true;
        if !g.resting && g.age < 0.2 {
            return None;
        }
        let mut best: Option<(V2, f32)> = None;
        for t in self.turrets.iter() {
            if t.alive {
                let d = t.p.dist(g.p);
                if d <= tuning::TROOPER_RANGE && best.map(|(_, bd)| d < bd).unwrap_or(true) {
                    best = Some((t.p, d));
                }
            }
        }
        for d in self.drones.iter() {
            if d.alive {
                let dist = d.body.p.dist(g.p);
                if dist <= tuning::TROOPER_RANGE && best.map(|(_, bd)| dist < bd).unwrap_or(true) {
                    best = Some((d.body.p, dist));
                }
            }
        }
        let (target, _) = best?;
        if self.terrain.segment_hit(g.p, target).is_some() {
            return None;
        }
        self.gadgets[i].cd = tuning::TROOPER_FIRE_COOLDOWN;
        Some(Bullet::fire(
            WeaponId::Gun,
            g.p,
            (target - g.p).angle(),
            V2::ZERO,
            true,
        ))
    }

    /// Contact damage: the shield may eat it, otherwise the ship is gone.
    fn ship_contact(&mut self, p: V2) {
        if !self.ship.alive {
            return;
        }
        if self.ship.shield.absorb(self.ship.has_fuel()) {
            self.stats.damage_taken += 1;
            self.events.push(Event::ShieldHit {
                p,
                angle: (p - self.ship.body.p).angle(),
            });
        } else {
            self.destroy_ship();
        }
    }

    fn destroy_ship(&mut self) {
        if !self.ship.alive {
            return;
        }
        self.ship.alive = false;
        self.ship.attached = None;
        let p = self.ship.body.p;
        self.explode(p, 1.0);
        self.events.push(Event::ShipLost { p });
        self.state = RunState::ShipLost { t: 0.0 };
    }

    fn fail(&mut self, reason: Failure) {
        if self.state.is_over() {
            return;
        }
        self.state = RunState::Failed { reason, t: 0.0 };
    }

    fn event_reactor_destroyed(&mut self) {
        // The reactor raises the level's signal graph. With no authored signals
        // the classic default runs — defences offline, exits revealed,
        // reactor-gated machinery powered — and an authored graph replaces that
        // default edge for edge (`docs/design.md` §7 signals).
        if self.level.signals.is_empty() {
            for t in self.turrets.iter_mut() {
                t.powered = false;
            }
            for i in 0..self.exits.len() {
                self.open_exit(i);
            }
            for g in self.gates.iter_mut() {
                if g.trigger == Trigger::Reactor {
                    g.activate();
                }
            }
        } else {
            let signals = self.level.signals.clone();
            for s in signals {
                match s.action {
                    SignalAction::OpenExit => self.open_exit(s.index),
                    SignalAction::RevealExit => {
                        if let Some(e) = self.exits.get_mut(s.index) {
                            e.hidden = false;
                        }
                    }
                    SignalAction::StartGate => {
                        if let Some(g) = self.gates.get_mut(s.index) {
                            g.activate();
                        }
                    }
                    SignalAction::PowerDown => {
                        for t in self.turrets.iter_mut() {
                            t.powered = false;
                        }
                    }
                }
            }
        }
        self.escape = Some(tuning::ESCAPE_LIMIT);
    }

    /// Opens and reveals one exit and emits the signal's event. Idempotent, so
    /// a graph with several edges onto the same exit is harmless.
    fn open_exit(&mut self, i: usize) {
        let Some(e) = self.exits.get_mut(i) else {
            return;
        };
        let was = e.open;
        e.open = true;
        e.hidden = false;
        if !was {
            let p = e.rect.center();
            self.events.push(Event::ExitOpen { p });
        }
    }

    fn step_objectives(&mut self) {
        let ship_p = self.ship.body.p;
        let ship_alive = self.ship.alive;

        if ship_alive {
            for i in 0..self.fuel_pods.len() {
                if self.fuel_pods[i].taken {
                    continue;
                }
                let p = self.fuel_pods[i].p;
                if p.dist(ship_p) < tuning::PICKUP_RADIUS {
                    let amount = self.fuel_pods[i].amount;
                    self.fuel_pods[i].taken = true;
                    let gained = self.ship.refuel(amount);
                    self.events.push(Event::Pickup { p, fuel: true });
                    self.events.push(Event::Refuel { p, amount: gained });
                }
            }

            self.pad_cooldown = (self.pad_cooldown - DT).max(0.0);
            let slow = self.ship.body.v.len() < tuning::PAD_LANDING_SPEED;
            if slow && self.pad_cooldown <= 0.0 {
                let fuel_low = self.ship.fuel < self.ship.fuel_capacity - 0.5;
                let shield_low = !matches!(self.ship.shield, Shield::Charged);
                // A base in Wings repairs the ship *and* reloads the special;
                // that is what makes leaving to rearm a real decision.
                let unarmed =
                    self.ship.loadout.ammo < weapons::spec(self.ship.loadout.special).ammo;
                if fuel_low || shield_low || unarmed {
                    let pads = self.pads.clone();
                    if let Some(pad) = pads.iter().find(|p| p.rect.contains(ship_p)) {
                        self.ship.refuel_full();
                        self.ship.shield.repair();
                        self.ship.loadout.rearm();
                        self.pad_cooldown = 0.4;
                        self.events.push(Event::PadLanding {
                            p: pad.rect.center(),
                        });
                    }
                }
            }
        }

        // Payload delivery: on the beam inside the exit, or parked in it.
        for exit in self.exits.clone() {
            if !exit.open {
                continue;
            }
            for pod in self.pods.iter_mut() {
                if pod.alive && exit.rect.contains(pod.body.p) {
                    pod.delivered = true;
                }
            }
            if ship_alive && exit.rect.contains(ship_p) {
                let pods_ok = !self.level.require_pod || self.payload_secured();
                if pods_ok {
                    self.complete();
                }
            }
        }
    }

    fn complete(&mut self) {
        if self.state.is_over() {
            return;
        }
        let beamed = self.payload_secured();
        let outcome = if self.reactors.iter().any(|r| r.destroyed) {
            Outcome::ReactorDestroyed
        } else if beamed {
            Outcome::PodBeamed
        } else {
            Outcome::Escaped
        };
        let seconds = self.elapsed();
        let time_bonus =
            ((tuning::SCORE_PAR_TIME - seconds).max(0.0) * tuning::SCORE_TIME_BONUS as f32) as i32;
        self.score = Score {
            fuel: (self.ship.fuel * tuning::SCORE_FUEL as f32) as i32,
            time: time_bonus,
            turrets: self.stats.turrets_destroyed as i32 * tuning::SCORE_TURRET,
            drones: self.stats.drones_destroyed as i32 * tuning::SCORE_DRONE,
            mines: self.stats.mines_destroyed as i32 * tuning::SCORE_MINE,
            reactor: if outcome == Outcome::ReactorDestroyed
                || self.reactors.iter().any(|r| r.destroyed)
            {
                tuning::SCORE_REACTOR
            } else {
                0
            },
            payload: if beamed { tuning::SCORE_POD } else { 0 },
            escape: tuning::SCORE_ESCAPE,
        };
        let p = self.ship.body.p;
        self.state = RunState::Complete { outcome, t: 0.0 };
        self.events.push(Event::LevelComplete { p });
    }

    fn step_timers(&mut self) {
        if let Some(t) = self.escape.as_mut() {
            *t -= DT;
            let pulse = ((tuning::ESCAPE_LIMIT - *t) % 0.6) < DT;
            if pulse {
                let p = self.ship.body.p;
                self.events.push(Event::AlarmPulse { p });
            }
            if *t <= 0.0 {
                self.escape = Some(0.0);
                let p = self.ship.body.p;
                self.explode(p, 1.0);
                self.fail(Failure::CaveCollapsed);
            }
        }
    }

    // ------------------------------------------------------- destruction ---

    /// One tick of water flow, plus the medium's feedback: the splash when the
    /// ship drops in, and the bubbles its engine makes while it is under.
    fn step_medium(&mut self) {
        self.water.step(&self.terrain);

        let submerged = self.water.submerged_at(self.ship.body.p);
        let wet = submerged > 0.3;
        if wet && !self.ship_wet {
            let p = self.ship.body.p;
            self.events.push(Event::Splash {
                p,
                power: submerged,
            });
        }
        self.ship_wet = wet;
        if self.ship.alive && self.ship.thrusting && submerged > 0.15 && self.tick.is_multiple_of(8)
        {
            self.events.push(Event::Bubble {
                p: self.ship.body.p - self.ship.thrust_dir * 8.0,
            });
        }
    }

    /// An explosion: the event everything reacts to, plus the crater it leaves.
    fn explode(&mut self, p: V2, power: f32) {
        self.events.push(Event::Explosion { p, power });
        self.dig(p, tuning::CARVE_BLAST * power);
    }

    /// Digs a crater of `radius` and wakes the water that can now pour into it.
    fn dig(&mut self, p: V2, radius: f32) -> Carve {
        let carve = if radius >= tuning::CARVE_MIN {
            self.terrain.carve(p, radius)
        } else {
            Carve::NONE
        };
        if carve.hit_terrain() {
            self.water.wake(carve.x0, carve.y0, carve.x1, carve.y1);
            self.events.push(Event::Carve {
                p,
                radius,
                material: carve.material,
            });
        }
        carve
    }

    /// Chips solid terrain with `amount` points of damage.
    ///
    /// Dirt (one hit point) vanishes on the first shot exactly as it always
    /// did; granular walls absorb several. Water only wakes where a cell
    /// actually broke, so chipping a sealed wall does not stir the pool behind
    /// it. Every shot does at least one point, so no weapon stops digging.
    fn strike(&mut self, p: V2, radius: f32, amount: i32) -> Carve {
        let carve = if radius >= tuning::CARVE_MIN {
            self.terrain.damage(p, radius, amount.clamp(1, 255) as u8)
        } else {
            Carve::NONE
        };
        if carve.removed > 0 {
            self.water.wake(carve.x0, carve.y0, carve.x1, carve.y1);
            self.events.push(Event::Carve {
                p,
                radius,
                material: carve.material,
            });
        }
        carve
    }

    fn cleanup(&mut self) {
        self.bullets.retain(|b| b.life > 0.0);
        self.clouds.retain(|c| c.ttl > 0.0);
        self.gadgets.retain(|g| g.ttl > 0.0);
        self.turrets.retain(|t| t.alive);
        self.drones.retain(|d| d.alive);
        self.mines.retain(|m| m.alive);
        // Pods are kept: the wreck is worth showing.
    }

    // ---------------------------------------------------------- checksum ---

    /// Canonical state hash used by replays and record verification.
    pub fn checksum(&self) -> u64 {
        let mut h = Fnv::new();
        h.u64(self.tick);
        h.u64(self.seed);
        h.u8(self.scheme as u8);
        let s = &self.ship;
        h.f32(s.body.p.x);
        h.f32(s.body.p.y);
        h.f32(s.body.v.x);
        h.f32(s.body.v.y);
        h.f32(s.body.angle);
        h.f32(s.body.omega);
        h.f32(s.fuel);
        h.u8(u8::from(s.alive));
        h.u8(match s.shield {
            Shield::Charged => 0,
            Shield::Absorbing { .. } => 1,
            Shield::Down { .. } => 2,
            Shield::Recharging { .. } => 3,
        });
        h.u8(match s.attached {
            Some(i) => i as u8 + 1,
            None => 0,
        });
        // The loadout is state the run depends on: a replay that swapped
        // weapons must reproduce the same choice, the same ammo and the same
        // reload clock, or the checksum has to disagree.
        h.u8(s.loadout.special as u8);
        h.u32(s.loadout.ammo as u32);
        h.f32(s.loadout.cd);
        h.f32(s.shield_field);
        h.u8(u8::from(s.grabbed));
        h.u8(u8::from(s.tether.is_some()));
        if let Some((anchor, hold)) = s.tether {
            h.f32(anchor.x);
            h.f32(anchor.y);
            h.f32(hold);
        }
        for p in &self.pods {
            h.f32(p.body.p.x);
            h.f32(p.body.p.y);
            h.f32(p.body.angle);
            h.u8(u8::from(p.alive));
            h.u8(u8::from(p.delivered));
            h.u8(u8::from(p.attached));
        }
        for t in &self.turrets {
            h.f32(t.aim);
            h.f32(t.cooldown);
            h.u8(t.hp as u8);
            h.f32(t.emp);
            h.f32(t.frozen);
        }
        for d in &self.drones {
            h.f32(d.body.p.x);
            h.f32(d.body.p.y);
            h.f32(d.body.v.x);
            h.f32(d.body.v.y);
            h.f32(d.body.angle);
            h.f32(d.frozen);
            h.f32(d.netted);
        }
        for c in &self.clouds {
            h.f32(c.p.x);
            h.f32(c.p.y);
            h.f32(c.radius);
            h.f32(c.ttl);
            h.f32(c.acc);
            h.u8(c.kind as u8);
        }
        for g in &self.gadgets {
            h.f32(g.p.x);
            h.f32(g.p.y);
            h.f32(g.ttl);
            h.f32(g.cd);
            h.u8(u8::from(g.resting));
            h.u8(g.kind as u8);
        }
        for m in &self.mines {
            h.f32(m.body.p.x);
            h.f32(m.body.p.y);
            h.u8(u8::from(m.from_player));
            h.f32(m.ttl);
        }
        for r in &self.reactors {
            h.u8(r.hp as u8);
            h.f32(r.critical);
        }
        for f in &self.fuel_pods {
            h.u8(u8::from(f.taken));
        }
        for e in &self.exits {
            h.u8(u8::from(e.open));
            h.u8(u8::from(e.hidden));
        }
        for g in &self.gates {
            h.f32(g.t);
            h.f32(g.dir);
            h.u8(u8::from(g.active));
            h.u8(u8::from(g.hidden));
        }
        for b in &self.bullets {
            h.f32(b.p.x);
            h.f32(b.p.y);
            h.f32(b.life);
            h.u8(u8::from(b.from_player));
            h.u8(b.weapon as u8);
            h.u8(b.pierce);
            h.u8(b.bounce);
            h.f32(b.age);
        }
        h.u64(self.rng_state_hint());
        // The cave the run dug and the water it moved are part of the run.
        h.u64(self.terrain.digest());
        h.u64(self.water.digest());
        h.finish()
    }

    fn rng_state_hint(&self) -> u64 {
        // The generator is only advanced by content that is already hashed, so a
        // single probe value keeps the digest cheap but still sensitive.
        self.rng.clone().next_u32() as u64
    }
}

fn apply_drag(body: &mut crate::sim::body::Body, drag: f32) {
    if drag > 0.0 {
        body.v *= 1.0 / (1.0 + drag * DT);
    }
}

/// Gravity, wind and water response at a point.
///
/// Water cuts gravity and adds drag in proportion to how deep the point is in
/// it, so a dive is survivable but slow and getting out costs thrust.
fn ambient(water: &Water, gravity: f32, wind: V2, p: V2) -> (V2, f32) {
    let base = V2::new(0.0, gravity) + wind;
    let depth = water.submerged_at(p);
    if depth <= 0.0 {
        return (base, 0.0);
    }
    let params = water.params();
    (
        base * crate::math::lerp(1.0, params.density, depth),
        params.drag * depth,
    )
}

/// Six hull samples for payload/terrain contact (the pod is a box on a rod).
fn pod_samples(body: &crate::sim::body::Body) -> [V2; 6] {
    const R: f32 = tuning::POD_RADIUS;
    [
        body.to_world(V2::new(R, 0.0)),
        body.to_world(V2::new(R * 0.6, R)),
        body.to_world(V2::new(-R * 0.6, R)),
        body.to_world(V2::new(-R, 0.0)),
        body.to_world(V2::new(-R * 0.6, -R)),
        body.to_world(V2::new(R * 0.6, -R)),
    ]
}
