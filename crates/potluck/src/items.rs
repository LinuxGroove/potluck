//! What players carry: every block, and items, things that are only carried
//! and held (tools and swords now; ingots, food and armour later).
//!
//! A `Thing` is either. Ids under `FIRST_ITEM` are blocks, by their block
//! id; ids from `FIRST_ITEM` up are the game's own items, listed in `ITEMS`.
//! Like block ids, item ids are saved and sent, so never renumber one; add
//! new items at the end. Players and creations go by names, which `by_name`
//! looks up among blocks first and items after.
//!
//! Pockets are 36 slots (`Slots`): the hotbar's nine, then the backpack's
//! 27. `Slots::give` fills what's there first, hotbar before backpack, and
//! `take` empties the backpack first, so what's in hand lasts.

use serde::{Deserialize, Serialize};

use crate::blocks::{self, BlockId};

pub type Thing = u16;

/// Items' ids start here, after every block's.
pub const FIRST_ITEM: Thing = 256;

pub const WOODEN_PICKAXE: Thing = 256;
pub const STONE_PICKAXE: Thing = 257;
pub const WOODEN_AXE: Thing = 258;
pub const STONE_AXE: Thing = 259;
pub const WOODEN_SHOVEL: Thing = 260;
pub const STONE_SHOVEL: Thing = 261;
pub const WOODEN_SWORD: Thing = 262;
pub const STONE_SWORD: Thing = 263;

/// How many of a block go in one slot.
pub const BLOCK_STACK: u32 = 99;
/// Slots in the hotbar, the first of the pockets.
pub const HOTBAR: usize = 9;
/// Every slot: the hotbar's and the backpack's.
pub const POCKETS: usize = 36;

/// Something only carried and held, never put down.
pub struct Item {
    pub name: &'static str,
    /// Its picture's tile in the block atlas, which is its icon.
    pub icon: &'static str,
    /// How many go in one slot.
    pub stack: u32,
}

const fn tool(name: &'static str, icon: &'static str) -> Item {
    Item { name, icon, stack: 1 }
}

/// The game's own items, from `FIRST_ITEM` on (the tools and gloomlings
/// creations say what these do).
pub static ITEMS: &[Item] = &[
    tool("Wooden pickaxe", "pickaxe_wood"),
    tool("Stone pickaxe", "pickaxe_stone"),
    tool("Wooden axe", "axe_wood"),
    tool("Stone axe", "axe_stone"),
    tool("Wooden shovel", "shovel_wood"),
    tool("Stone shovel", "shovel_stone"),
    tool("Wooden sword", "sword_wood"),
    tool("Stone sword", "sword_stone"),
];

/// A block as a thing to carry.
pub const fn of_block(b: BlockId) -> Thing {
    b as Thing
}

/// The block a thing puts down, if it's a block (and not air).
pub fn block(t: Thing) -> Option<BlockId> {
    (t > 0 && t < FIRST_ITEM).then_some(t as BlockId)
}

/// The item a thing is, if it's one of the game's items.
pub fn item(t: Thing) -> Option<&'static Item> {
    t.checked_sub(FIRST_ITEM).and_then(|i| ITEMS.get(i as usize))
}

/// Something to hold that's never put down.
pub fn is_item(t: Thing) -> bool {
    t >= FIRST_ITEM
}

pub fn name(t: Thing) -> &'static str {
    match (block(t), item(t)) {
        (Some(b), _) => blocks::block(b).name,
        (_, Some(i)) => i.name,
        _ if t == 0 => "Nothing",
        _ => "Unknown thing",
    }
}

/// The atlas tile that's its icon: a block's side, an item's picture.
pub fn icon(t: Thing) -> &'static str {
    match (block(t), item(t)) {
        (Some(b), _) => blocks::block(b).faces[1],
        (_, Some(i)) => i.icon,
        _ => "missing",
    }
}

/// How many go in one slot.
pub fn stack(t: Thing) -> u32 {
    item(t).map(|i| i.stack).unwrap_or(BLOCK_STACK)
}

/// Looks a thing up by name, as `blocks::by_name` does: blocks first
/// (the game's, then creations'), then items.
pub fn by_name(name: &str) -> Option<Thing> {
    blocks::by_name(name).filter(|&b| b != blocks::AIR).map(of_block).or_else(|| {
        let key = blocks::squash(name);
        ITEMS.iter().position(|i| blocks::squash(i.name) == key).map(|i| FIRST_ITEM + i as Thing)
    })
}

/// A thing a player can carry: not air, water or what can't be broken.
pub fn carriable(t: Thing) -> bool {
    match block(t) {
        Some(b) => b != blocks::WATER && blocks::block(b).hardness >= 0.0,
        None => item(t).is_some(),
    }
}

/// One slot of a player's pockets. In survival a slot with a count of 0 is
/// empty, whatever thing it last held; in creative the hotbar's things are
/// what the player chose, and counts don't matter.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Slot {
    pub thing: Thing,
    pub count: u32,
}

impl Slot {
    pub fn empty(&self) -> bool {
        self.count == 0
    }
}

/// A player's pockets: the hotbar's nine slots, then the backpack's 27.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slots(pub [Slot; POCKETS]);

impl Default for Slots {
    fn default() -> Self {
        Slots([Slot::default(); POCKETS])
    }
}

impl Slots {
    /// The hotbar's things, as a new player's creative hotbar has them.
    pub fn starting(hotbar: [BlockId; HOTBAR]) -> Self {
        let mut s = Slots::default();
        for (slot, b) in s.0.iter_mut().zip(hotbar) {
            slot.thing = of_block(b);
        }
        s
    }

    /// How many of a thing could go in.
    pub fn room_for(&self, t: Thing) -> u32 {
        let most = stack(t);
        self.0.iter().map(|s| if s.empty() { most } else if s.thing == t { most.saturating_sub(s.count) } else { 0 }).sum()
    }

    /// Puts up to `n` in, topping up slots with the same thing first, then
    /// empty ones, the hotbar before the backpack; returns how many went in.
    pub fn give(&mut self, t: Thing, n: u32) -> u32 {
        let most = stack(t);
        let mut left = n;
        for s in self.0.iter_mut().filter(|s| !s.empty() && s.thing == t) {
            let k = left.min(most.saturating_sub(s.count));
            s.count += k;
            left -= k;
        }
        for s in self.0.iter_mut().filter(|s| s.empty()) {
            if left == 0 {
                break;
            }
            let k = left.min(most);
            *s = Slot { thing: t, count: k };
            left -= k;
        }
        n - left
    }

    /// How many of a thing are in.
    pub fn count(&self, t: Thing) -> u32 {
        self.0.iter().filter(|s| s.thing == t).map(|s| s.count).sum()
    }

    /// Takes `n` of a thing if there are that many, the backpack's last
    /// slots first, so the hotbar keeps what's in hand.
    pub fn take(&mut self, t: Thing, n: u32) -> bool {
        if self.count(t) < n {
            return false;
        }
        let mut left = n;
        for s in self.0.iter_mut().rev().filter(|s| s.thing == t) {
            let k = left.min(s.count);
            s.count -= k;
            left -= k;
        }
        true
    }

    /// What's in, by thing, in order of thing.
    pub fn carried(&self) -> Vec<(Thing, u32)> {
        let mut out: std::collections::BTreeMap<Thing, u32> = Default::default();
        for s in self.0.iter().filter(|s| !s.empty()) {
            *out.entry(s.thing).or_default() += s.count;
        }
        out.into_iter().collect()
    }

    /// The slots with something in: (slot, thing, how many).
    pub fn filled(&self) -> Vec<(u8, Thing, u32)> {
        self.0.iter().enumerate().filter(|(_, s)| !s.empty()).map(|(i, s)| (i as u8, s.thing, s.count)).collect()
    }

    /// Pockets as `filled` lists them.
    pub fn from_filled(list: &[(u8, Thing, u32)]) -> Self {
        let mut s = Slots::default();
        for &(i, t, n) in list {
            if let Some(slot) = s.0.get_mut(i as usize) {
                *slot = Slot { thing: t, count: n.min(stack(t)) };
            }
        }
        s
    }

    /// When a hotbar slot runs out, the backpack's next lot of the same
    /// thing moves into it.
    pub fn refill(&mut self, slot: usize) {
        if slot >= HOTBAR || !self.0[slot].empty() {
            return;
        }
        let t = self.0[slot].thing;
        if let Some(from) = (HOTBAR..POCKETS).find(|&i| !self.0[i].empty() && self.0[i].thing == t) {
            self.0[slot] = self.0[from];
            self.0[from].count = 0;
        }
    }

    /// Moves what's in one slot to another (all of it, or half): into an
    /// empty slot, onto the same thing as far as it stacks, or, moving all
    /// of it, swapped with something else. Returns whether anything moved.
    pub fn shift(&mut self, from: usize, to: usize, half: bool) -> bool {
        if from == to || from >= POCKETS || to >= POCKETS || self.0[from].empty() {
            return false;
        }
        let (a, b) = (self.0[from], self.0[to]);
        let n = if half { a.count.div_ceil(2) } else { a.count };
        if b.empty() {
            self.0[to] = Slot { thing: a.thing, count: n };
            self.0[from].count -= n;
        } else if b.thing == a.thing {
            let k = n.min(stack(a.thing).saturating_sub(b.count));
            if k == 0 {
                return false;
            }
            self.0[to].count += k;
            self.0[from].count -= k;
        } else if !half {
            self.0.swap(from, to);
        } else {
            return false;
        }
        true
    }

    /// Puts the backpack in order: like things together in as few slots as
    /// they fit, blocks before items, by name. The hotbar stays as it is.
    pub fn tidy(&mut self) {
        let mut all: Vec<(Thing, u32)> = vec![];
        for s in self.0[HOTBAR..].iter().filter(|s| !s.empty()) {
            match all.iter_mut().find(|(t, _)| *t == s.thing) {
                Some(e) => e.1 += s.count,
                None => all.push((s.thing, s.count)),
            }
        }
        all.sort_by_key(|(t, _)| (is_item(*t), name(*t)));
        let mut i = HOTBAR;
        for s in &mut self.0[HOTBAR..] {
            s.count = 0;
        }
        for (t, mut n) in all {
            while n > 0 && i < POCKETS {
                let k = n.min(stack(t));
                self.0[i] = Slot { thing: t, count: k };
                n -= k;
                i += 1;
            }
        }
    }
}
