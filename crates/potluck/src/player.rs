//! The local player: walking, swimming, flying, looking, breaking and placing.

use bevy::prelude::*;

use crate::blocks::{self, BlockId, Shape, AIR, STARTER_HOTBAR, WATER};
use crate::items::{self, Slots, Thing, HOTBAR};
use crate::input::Controls;
use crate::settings::Settings;
use crate::ui::Focus;
use crate::world::{Voxels, START, WY};

pub const HALF_W: f32 = 0.3;
pub const HEIGHT: f32 = 1.8;
pub const EYE: f32 = 1.62;
const GRAVITY: f32 = 30.0;
const JUMP: f32 = 8.7;
const WALK: f32 = 4.3;
const SPRINT: f32 = 5.8;
const SNEAK: f32 = 1.6;
const FLY: f32 = 11.0;
const REACH: f32 = 6.0;
pub const MAX_HEALTH: f32 = 20.0;

#[derive(Clone, Copy, Debug)]
pub struct Hit {
    pub block: IVec3,
    pub normal: IVec3,
    pub id: BlockId,
}

#[derive(Resource)]
pub struct Me {
    /// Feet, in the middle of the body.
    pub pos: Vec3,
    pub vel: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub on_ground: bool,
    pub flying: bool,
    pub sprint: bool,
    pub in_water: bool,
    /// The hotbar's chosen slot.
    pub slot: usize,
    /// What the player carries: the hotbar's nine slots, then the
    /// backpack's 27. In creative the hotbar holds what they chose and
    /// nothing is counted.
    pub pockets: Slots,
    pub health: f32,
    fall_from: f32,
    last_jump: f64,
    pub breaking: Option<(IVec3, f32)>,
    repeat: f32,
    /// The creature being ridden.
    pub riding: Option<u64>,
    pub third_person: bool,
    pub target: Option<Hit>,
    /// The creature in the crosshair, nearer than any block.
    pub aim_body: Option<u64>,
    /// The Green's arch in the crosshair, and what the HUD says about it.
    pub aim_arch: Option<(usize, String)>,
    /// The creation that has this player's controls (`input.focus`), as
    /// the host said.
    pub focus: Option<String>,
    /// The creation whose actions were live last frame, and those held.
    pub pressing: (Option<String>, Vec<String>),
    /// What the live creation's buttons do, for the HUD.
    pub live_prompt: Option<String>,
    /// Lines creations show on this screen (`ui.show`), by (creation, id).
    pub pieces: std::collections::BTreeMap<(String, String), crate::script::Piece>,
    /// The question a creation asks this player now (`ui.ask`), as the host said.
    pub panel: Option<crate::script::Panel>,
    /// Subtitles for sounds just heard, and the seconds each has left.
    pub captions: Vec<(String, f32)>,
    /// Messages creations sent their client halves on this game, for
    /// `halves::run` to hand over.
    pub notes: Vec<crate::script::Note>,
    /// How many lots of changes to the pockets creations have made here
    /// (`world.give`, `world.take`), for the host to know a report is new.
    pub pockets_seen: u32,
    /// Hurts (less than 0) and heals creations sent, for `walk` to apply
    /// in survival.
    pub health_due: f32,
    /// Hurts creations sent that stop at some health (`world.hurt`'s
    /// `down_to`): (how much, the least it leaves), for `walk`.
    pub hurts: Vec<(f32, f32)>,
    /// How fast creations let this player heal by themselves
    /// (`world.healing`): 1 as usual, 0 not at all.
    pub healing: f32,
    /// How much faster what's in hand digs each kind of block
    /// and how hard a block it reaches (`world.digging`), and what breaking
    /// each block gives (`world.drops`), as the host last said.
    pub rules: items::Rules,
    /// Blocks this player placed and broke since the host last looked:
    /// (where, what it was, what it is now), for creations' block events.
    pub built: Vec<(IVec3, BlockId, BlockId)>,
    /// Blocks walked since the last step's sound.
    stride: f32,
    regen: f32,
    /// Seconds since the last hurt, for the HUD's flash.
    pub hurt: f32,
    /// Where the player was last hurt too badly, for the map's marker.
    pub fell_at: Option<Vec3>,
}

impl Default for Me {
    fn default() -> Self {
        Self {
            pos: Vec3::new(START.x + 0.5, 60.0, START.z + 0.5),
            vel: Vec3::ZERO,
            yaw: 0.0,
            pitch: -0.12,
            on_ground: false,
            flying: false,
            sprint: false,
            in_water: false,
            slot: 0,
            pockets: Slots::starting(STARTER_HOTBAR),
            health: MAX_HEALTH,
            fall_from: 0.0,
            last_jump: -10.0,
            breaking: None,
            repeat: 0.0,
            riding: None,
            third_person: false,
            target: None,
            aim_body: None,
            aim_arch: None,
            focus: None,
            pressing: (None, vec![]),
            live_prompt: None,
            pieces: Default::default(),
            panel: None,
            captions: vec![],
            notes: vec![],
            pockets_seen: 0,
            health_due: 0.0,
            hurts: vec![],
            healing: 1.0,
            rules: Default::default(),
            built: vec![],
            stride: 0.0,
            regen: 0.0,
            hurt: 9.0,
            fell_at: None,
        }
    }
}

impl Me {
    /// What a creation shows this player, or asks them, from the host.
    pub fn show(&mut self, change: crate::script::UiChange) {
        match change {
            crate::script::UiChange::Piece { creation, id, piece: Some(p) } => {
                self.pieces.insert((creation, id), p);
            }
            crate::script::UiChange::Piece { creation, id, piece: None } => {
                self.pieces.remove(&(creation, id));
            }
            crate::script::UiChange::Panel(p) => self.panel = p,
        }
    }

    pub fn eye(&self) -> Vec3 {
        self.pos + Vec3::Y * EYE
    }

    pub fn forward(&self) -> Vec3 {
        Quat::from_euler(EulerRot::YXZ, self.yaw, self.pitch, 0.0) * Vec3::NEG_Z
    }

    /// Puts the player on the ground at the start (or a given spot).
    pub fn respawn(&mut self, w: &Voxels, at: Option<Vec3>) {
        let p = at.unwrap_or(Vec3::new(START.x + 0.5, 0.0, START.z + 0.5));
        let top = w.top(p.x.floor() as i32, p.z.floor() as i32);
        self.pos = Vec3::new(p.x, top as f32 + 1.01, p.z);
        self.vel = Vec3::ZERO;
        self.yaw = 0.0;
        self.pitch = -0.12;
        self.health = MAX_HEALTH;
        self.fall_from = self.pos.y;
        self.riding = None;
        self.flying = false;
    }

    /// Puts the player at a spot as it is (a trip to a waypoint), keeping
    /// which way they face.
    pub fn place(&mut self, at: Vec3) {
        self.pos = at;
        self.vel = Vec3::ZERO;
        self.fall_from = at.y;
        self.flying = false;
        self.breaking = None;
    }

    /// Puts one of a thing in the pockets (survival), if there's room.
    pub fn give(&mut self, t: Thing) -> bool {
        self.pockets.give(t, 1) == 1
    }

    /// What's carried, by thing (survival).
    pub fn carried(&self) -> Vec<(Thing, u32)> {
        self.pockets.carried()
    }

    /// Things a creation gave (more) or took (fewer), as the host worked
    /// them out on its copy of these pockets.
    pub fn change_pockets(&mut self, changes: &[(Thing, i32)]) {
        for &(t, d) in changes {
            if d > 0 {
                self.pockets.give(t, (d as u32).min(crate::script::MAX_GIVE));
            } else if d < 0 {
                let n = d.unsigned_abs().min(self.pockets.count(t));
                self.pockets.take(t, n);
            }
        }
        self.pockets.refill(self.slot);
    }

    /// Notes a block this player changed, for the host to tell creations.
    fn note_built(&mut self, at: IVec3, old: BlockId, new: BlockId) {
        if self.built.len() < 256 {
            self.built.push((at, old, new));
        }
    }

    /// What's in hand: the hotbar's chosen slot, or nothing (0) when it's
    /// an empty one in survival.
    pub fn held(&self, survival: bool) -> Thing {
        let s = self.pockets.0[self.slot.min(HOTBAR - 1)];
        if survival && s.empty() { 0 } else { s.thing }
    }

    /// How much faster than by hand the thing in hand breaks a block in
    /// survival, as creations' `world.digging` said: 0 when it needs
    /// something better in hand (`world.drops`).
    pub fn dig_speed(&self, block: BlockId) -> f32 {
        let held = self.held(true);
        if !self.rules.can_break(held, block) {
            return 0.0;
        }
        self.rules.speed(held, block)
    }

    pub fn hurt(&mut self, amount: f32) {
        self.health -= amount;
        self.hurt = 0.0;
    }
}

/// Whether a box (min corner, size) overlaps anything solid.
pub fn blocked(w: &Voxels, min: Vec3, size: Vec3) -> bool {
    let a = min.floor().as_ivec3();
    let b = (min + size - Vec3::splat(1e-4)).floor().as_ivec3();
    for y in a.y..=b.y {
        for z in a.z..=b.z {
            for x in a.x..=b.x {
                if y < 0 {
                    return true;
                }
                if blocks::is_solid(w.get(x, y, z)) {
                    return true;
                }
            }
        }
    }
    false
}

/// Moves a body of the given half width and height by `d`, one axis at a
/// time, stopping at blocks. Returns which axes hit something.
pub fn move_body(w: &Voxels, pos: &mut Vec3, d: Vec3, half: f32, height: f32) -> BVec3 {
    let mut hit = BVec3::FALSE;
    let steps = (d.abs().max_element() / 0.35).ceil().max(1.0) as i32;
    let s = d / steps as f32;
    for _ in 0..steps {
        for axis in [1usize, 0, 2] {
            if s[axis] == 0.0 {
                continue;
            }
            let mut p = *pos;
            p[axis] += s[axis];
            let min = Vec3::new(p.x - half, p.y, p.z - half);
            let size = Vec3::new(half * 2.0, height, half * 2.0);
            if blocked(w, min, size) {
                match axis {
                    0 => hit.x = true,
                    1 => hit.y = true,
                    _ => hit.z = true,
                }
                // slide up to the block's face
                let lo = min[axis];
                let hi = lo + size[axis];
                if s[axis] > 0.0 {
                    if axis != 1 {
                        p[axis] = hi.floor() - half - 1e-3;
                    } else {
                        p[axis] = hi.floor() - height - 1e-3;
                    }
                } else if axis != 1 {
                    p[axis] = lo.floor() + 1.0 + half + 1e-3;
                } else {
                    p[axis] = lo.floor() + 1.0;
                }
                let min2 = Vec3::new(p.x - half, p.y, p.z - half);
                if blocked(w, min2, size) {
                    p[axis] = pos[axis];
                }
            }
            *pos = p;
        }
    }
    hit
}

/// Walks a ray through the blocks (Amanatides and Woo) to the first one that
/// can be broken.
pub fn raycast(w: &Voxels, from: Vec3, dir: Vec3, reach: f32) -> Option<Hit> {
    let mut p = from.floor().as_ivec3();
    let step = dir.signum().as_ivec3();
    let next = |v: f32, s: i32, p: i32| {
        if s > 0 { p as f32 + 1.0 - v } else { v - p as f32 }
    };
    let inv = Vec3::new(1.0 / dir.x.abs().max(1e-9), 1.0 / dir.y.abs().max(1e-9), 1.0 / dir.z.abs().max(1e-9));
    let mut t_max = Vec3::new(next(from.x, step.x, p.x) * inv.x, next(from.y, step.y, p.y) * inv.y, next(from.z, step.z, p.z) * inv.z);
    let mut normal = IVec3::ZERO;
    let mut t = 0.0;
    while t <= reach {
        let id = w.at(p);
        if id != AIR && id != WATER {
            return Some(Hit { block: p, normal, id });
        }
        if t_max.x < t_max.y && t_max.x < t_max.z {
            p.x += step.x;
            t = t_max.x;
            t_max.x += inv.x;
            normal = IVec3::new(-step.x, 0, 0);
        } else if t_max.y < t_max.z {
            p.y += step.y;
            t = t_max.y;
            t_max.y += inv.y;
            normal = IVec3::new(0, -step.y, 0);
        } else {
            p.z += step.z;
            t = t_max.z;
            t_max.z += inv.z;
            normal = IVec3::new(0, 0, -step.z);
        }
    }
    None
}

pub fn look(c: Res<Controls>, focus: Res<Focus>, mut me: ResMut<Me>) {
    if !focus.playing() {
        return;
    }
    me.yaw -= c.look.x;
    me.pitch = (me.pitch - c.look.y).clamp(-1.55, 1.55);
}

pub fn walk(
    c: Res<Controls>,
    focus: Res<Focus>,
    settings: Res<Settings>,
    voxels: Res<Voxels>,
    time: Res<Time>,
    mut me: ResMut<Me>,
    mut toasts: ResMut<crate::ui::Toasts>,
    mut heard: ResMut<crate::sound::Heard>,
) {
    let dt = time.delta_secs().min(0.05);
    let now = time.elapsed_secs_f64();
    me.hurt += dt;
    // what creations did to your health, in survival
    let due = std::mem::take(&mut me.health_due);
    if settings.survival && due < 0.0 {
        me.hurt(-due);
    } else if settings.survival && due > 0.0 {
        me.health = (me.health + due).min(MAX_HEALTH);
    }
    for (n, down_to) in std::mem::take(&mut me.hurts) {
        if settings.survival && me.health > down_to {
            let n = n.min(me.health - down_to);
            me.hurt(n);
        }
    }
    if me.riding.is_some() {
        me.vel = Vec3::ZERO;
        me.fall_from = me.pos.y;
        return;
    }
    let playing = focus.playing();
    let mv = if playing { c.mv } else { Vec2::ZERO };
    let fwd = Vec3::new(-me.yaw.sin(), 0.0, -me.yaw.cos());
    let right = Vec3::new(fwd.z * -1.0, 0.0, fwd.x);
    let wish = (fwd * mv.y + right * mv.x).clamp_length_max(1.0);

    if playing && c.sprint_pressed {
        me.sprint = !me.sprint;
    }
    if mv.y <= 0.1 {
        me.sprint = false;
    }
    if playing && c.jump_pressed {
        if !settings.survival && now - me.last_jump < 0.3 {
            me.flying = !me.flying;
            me.vel.y = 0.0;
            me.last_jump = -10.0;
        } else {
            me.last_jump = now;
        }
    }
    let feet = me.pos.floor().as_ivec3();
    let mid = (me.pos + Vec3::Y * 0.9).floor().as_ivec3();
    let was_wet = me.in_water;
    me.in_water = voxels.at(feet) == WATER || voxels.at(mid) == WATER;
    if me.in_water && !was_wet && me.vel.y < -3.0 {
        heard.game("splash", me.pos, 0.5, 1.0);
    }

    let mut vel = me.vel;
    if me.flying {
        let speed = if me.sprint { FLY * 1.8 } else { FLY };
        let target = wish * speed;
        vel.x += (target.x - vel.x) * (dt * 10.0).min(1.0);
        vel.z += (target.z - vel.z) * (dt * 10.0).min(1.0);
        let up = if playing && c.jump { 1.0 } else { 0.0 } - if playing && c.crouch { 1.0 } else { 0.0 };
        vel.y += (up * speed * 0.8 - vel.y) * (dt * 10.0).min(1.0);
    } else {
        let speed = if playing && c.crouch { SNEAK } else if me.sprint { SPRINT } else { WALK };
        let speed = if me.in_water { speed * 0.6 } else { speed };
        let target = wish * speed;
        let grip = if me.on_ground { 14.0 } else { 3.0 };
        vel.x += (target.x - vel.x) * (dt * grip).min(1.0);
        vel.z += (target.z - vel.z) * (dt * grip).min(1.0);
        if me.in_water {
            vel.y -= GRAVITY * 0.25 * dt;
            vel.y = vel.y.max(-3.0);
            if playing && c.jump {
                vel.y = (vel.y + 18.0 * dt).min(3.2);
            }
        } else {
            vel.y -= GRAVITY * dt;
            vel.y = vel.y.max(-60.0);
            if playing && c.jump && me.on_ground {
                vel.y = JUMP;
            }
        }
    }
    let mut pos = me.pos;
    let hit = move_body(&voxels, &mut pos, vel * dt, HALF_W, HEIGHT);
    let landed = hit.y && vel.y < 0.0;
    if hit.x { vel.x = 0.0 }
    if hit.z { vel.z = 0.0 }
    if hit.y { vel.y = 0.0 }
    // a little variety in each step
    let wobble = 0.92 + (now * 7.3).fract() as f32 * 0.16;
    let under = crate::sound::material(voxels.at((pos - Vec3::Y * 0.1).floor().as_ivec3()));
    // fall damage, in survival
    if landed {
        let fell = me.fall_from - pos.y;
        if fell > 1.2 && !me.in_water {
            if let Some(n) = under.step() {
                heard.game(n, pos, 0.55, wobble * 0.85);
            }
        }
        if settings.survival && !me.in_water && fell > 3.5 {
            me.hurt((fell - 3.0).floor());
        }
    }
    me.on_ground = landed;
    if landed && me.flying && !playing {
        me.flying = false;
    }
    if me.on_ground || me.in_water || me.flying || vel.y > 0.0 {
        me.fall_from = pos.y;
    }
    me.pos = pos;
    me.vel = vel;
    // footsteps, by what's underfoot
    if me.on_ground && !me.flying && !me.in_water {
        me.stride += Vec2::new(vel.x, vel.z).length() * dt;
        if me.stride > if me.sprint { 2.4 } else { 1.9 } {
            me.stride = 0.0;
            if let Some(n) = under.step() {
                heard.game(n, pos, if playing && c.crouch { 0.12 } else { 0.3 }, wobble);
            }
        }
    }
    // health comes back slowly
    if me.health < MAX_HEALTH {
        me.regen += dt * me.healing;
        if me.regen > 4.0 {
            me.regen = 0.0;
            me.health = (me.health + 1.0).min(MAX_HEALTH);
        }
    }
    if me.pos.y < -20.0 || me.health <= 0.0 {
        let msg = if me.health <= 0.0 { "You were hurt too badly. Back to the hill." } else { "You fell out of the world. Back to the hill." };
        if me.health <= 0.0 && settings.survival {
            me.fell_at = Some(me.pos);
        }
        let w = &*voxels;
        me.respawn(w, None);
        toasts.push(msg);
    }
    if me.pos.y > WY as f32 + 40.0 {
        me.pos.y = WY as f32 + 40.0;
    }
}

/// What to tell a player whose hand can't break a block yet.
pub fn needs(rules: &items::Rules, b: BlockId) -> String {
    let name = blocks::block(b).name;
    match rules.tool_for(b) {
        Some(t) => format!("{name} needs a {} or better", items::name(t).to_lowercase()),
        None => format!("{name} needs a better tool"),
    }
}

/// Breaking and placing blocks, and choosing what's in hand.
pub fn build(
    c: Res<Controls>,
    focus: Res<Focus>,
    settings: Res<Settings>,
    time: Res<Time>,
    mut voxels: ResMut<Voxels>,
    mut me: ResMut<Me>,
    herd: Res<crate::creatures::Herd>,
    mut heard: ResMut<crate::sound::Heard>,
    mut toasts: ResMut<crate::ui::Toasts>,
) {
    let dt = time.delta_secs();
    let dir = me.forward();
    me.target = raycast(&voxels, me.eye(), dir, REACH);
    if !focus.playing() || me.riding.is_some() {
        me.breaking = None;
        return;
    }
    if c.hotbar_step != 0 {
        me.slot = (me.slot as i32 + c.hotbar_step).rem_euclid(HOTBAR as i32) as usize;
    }
    if let Some(s) = c.hotbar_slot {
        me.slot = s;
    }
    let Some(hit) = me.target else {
        me.breaking = None;
        return;
    };
    if me.aim_body.is_some() {
        me.breaking = None;
        return;
    }
    // breaking
    let centre = hit.block.as_vec3() + Vec3::splat(0.5);
    let stuff = crate::sound::material(hit.id);
    if c.break_held {
        let hardness = blocks::block(hit.id).hardness;
        if hardness >= 0.0 {
            if settings.survival && !me.rules.can_break(me.held(true), hit.id) {
                // some blocks need a better tool in hand first: say so once
                // for each block tried
                if me.breaking.is_none_or(|(b, _)| b != hit.block) {
                    toasts.push(needs(&me.rules, hit.id));
                    me.breaking = Some((hit.block, 0.0));
                }
            } else if settings.survival {
                // a tool in hand digs its kind of block faster
                let step = dt * me.dig_speed(hit.id) / hardness.max(0.05);
                let progress = match me.breaking {
                    Some((b, p)) if b == hit.block => p + step,
                    _ => step,
                };
                if progress >= 1.0 {
                    if voxels.set(hit.block, AIR) {
                        me.note_built(hit.block, hit.id, AIR);
                    }
                    // what it gives: itself, or what creations say
                    let roll = crate::noise::hash2(hit.block.x ^ hit.block.y.wrapping_mul(7919), hit.block.z, (time.elapsed_secs() * 1000.0) as i32);
                    if let Some((t, n)) = me.rules.gives(hit.id, roll) {
                        if me.pockets.give(t, n) < n {
                            toasts.push("Your pockets are full");
                        }
                    }
                    me.breaking = None;
                    if let Some(n) = stuff.breaks() {
                        heard.game(n, centre, 0.7, 1.0);
                    }
                } else {
                    // a knock now and then while it gives
                    me.repeat -= dt;
                    if me.repeat <= 0.0 {
                        me.repeat = 0.25;
                        if let Some(n) = stuff.step() {
                            heard.game(n, centre, 0.4, 0.8);
                        }
                    }
                    me.breaking = Some((hit.block, progress));
                }
            } else {
                me.repeat -= dt;
                if c.break_pressed || me.repeat <= 0.0 {
                    if voxels.set(hit.block, AIR) {
                        me.note_built(hit.block, hit.id, AIR);
                    }
                    me.repeat = 0.22;
                    if let Some(n) = stuff.breaks() {
                        heard.game(n, centre, 0.6, 1.0);
                    }
                }
            }
        }
    } else {
        me.breaking = None;
        me.repeat = 0.0;
    }
    // placing
    if c.place_pressed {
        let slot = me.pockets.0[me.slot];
        let Some(id) = items::block(slot.thing) else { return };
        if settings.survival && slot.empty() {
            return;
        }
        let at = if blocks::shape(hit.id) == Shape::Cross { hit.block } else { hit.block + hit.normal };
        let here = voxels.at(at);
        if here != AIR && here != WATER && blocks::shape(here) != Shape::Cross {
            return;
        }
        if blocks::is_solid(id) {
            let min = at.as_vec3();
            let pmin = Vec3::new(me.pos.x - HALF_W, me.pos.y, me.pos.z - HALF_W);
            let psize = Vec3::new(HALF_W * 2.0, HEIGHT, HALF_W * 2.0);
            let overlap = |amin: Vec3, asize: Vec3| (amin.cmplt(min + Vec3::ONE) & (amin + asize).cmpgt(min)).all();
            if overlap(pmin, psize) {
                return;
            }
            for b in herd.bodies.values() {
                if overlap(b.pos - Vec3::new(b.half.x, 0.0, b.half.z), b.half * Vec3::new(2.0, 1.0, 2.0)) {
                    return;
                }
            }
        }
        // plants need ground under them
        if blocks::shape(id) == Shape::Cross && !blocks::is_opaque(voxels.at(at - IVec3::Y)) {
            return;
        }
        if voxels.set(at, id) {
            me.note_built(at, here, id);
            heard.game("place", at.as_vec3() + Vec3::splat(0.5), 0.6, crate::sound::material(id).knock());
            if settings.survival {
                let s = me.slot;
                me.pockets.0[s].count -= 1;
                // the backpack's next lot of it comes to hand
                me.pockets.refill(s);
            }
        }
    }
}

/// Plants with nothing under them fall away.
pub fn settle_plants(mut voxels: ResMut<Voxels>) {
    let changed: Vec<IVec3> = voxels.changes.iter().filter(|(_, id)| *id == AIR).map(|(p, _)| *p).collect();
    for p in changed {
        let up = p + IVec3::Y;
        if blocks::shape(voxels.at(up)) == Shape::Cross {
            voxels.set(up, AIR);
        }
    }
}

#[derive(Component)]
pub struct MainCamera;

pub fn place_camera(me: Res<Me>, voxels: Res<Voxels>, mut cam: Query<&mut Transform, With<MainCamera>>) {
    let Ok(mut t) = cam.single_mut() else { return };
    let rot = Quat::from_euler(EulerRot::YXZ, me.yaw, me.pitch, 0.0);
    let eye = me.eye();
    if me.third_person {
        // back off behind and above the player, stopping short of blocks
        let back = rot * Vec3::new(0.0, 0.3, 1.0).normalize();
        let mut d = 4.5;
        let mut s = 0.0;
        while s < 4.5 {
            s += 0.25;
            if blocks::is_solid(voxels.at((eye + back * s).floor().as_ivec3())) {
                d = (s - 0.4).max(0.3);
                break;
            }
        }
        t.translation = eye + back * d;
    } else {
        t.translation = eye;
    }
    t.rotation = rot;
}

/// Tells the host what this player carries whenever it changes, for
/// creations' `world.pockets`, `has`, `give` and `take`.
pub fn report_pockets(
    me: Option<ResMut<Me>>,
    settings: Res<Settings>,
    info: Res<crate::ui::WorldInfo>,
    lib: Res<crate::creations::Library>,
    net: Res<crate::net::Net>,
    mut actions: ResMut<crate::game::Actions>,
    mut last: Local<Option<(u32, bool, Vec<(u8, Thing, u32)>, u32)>>,
    mut held: Local<Option<(u32, Thing)>>,
) {
    let Some(mut me) = me else { return };
    if !info.in_world {
        return;
    }
    if last.as_ref().is_some_and(|l| l.0 != lib.world) {
        // a new world counts the host's changes from nothing
        me.pockets_seen = 0;
    }
    let now = (lib.world, settings.survival, if settings.survival { me.pockets.filled() } else { vec![] }, me.pockets_seen);
    if last.as_ref() != Some(&now) {
        actions.0.push((net.my_id, crate::game::Action::Pockets { survival: now.1, slots: now.2.clone(), seen: now.3 }));
        *last = Some(now);
    }
    // what's in hand, for `world.held`
    let hand = (lib.world, me.held(settings.survival));
    if *held != Some(hand) {
        actions.0.push((net.my_id, crate::game::Action::Held(hand.1)));
        *held = Some(hand);
    }
}
