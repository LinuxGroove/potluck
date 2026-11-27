//! The rules that tie it together: what menu choices do, using creatures,
//! running creations on the host, reloading them when they change.

use bevy::prelude::*;

use crate::creations::{open_folder, open_link, Creation, Library, Mark, Origin};
use crate::creatures::{self, Herd};
use crate::green::{self, Green, Shows};
use crate::input::{glyph, Controls};
use crate::net::{Net, Role};
use crate::noise::Rng;
use crate::player::Me;
use crate::script::{KnobValue, PlayerInfo, Scripts};
use crate::settings::Settings;
use crate::share::{Done, ShareJobs, Who};
use crate::ui::{Act, Acts, Focus, Labels, Screen, Toasts, WorldInfo};
use crate::world::Voxels;
use crate::KnobEdits;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum Action {
    Use(u64),
    Hit(u64),
    Dismount,
    Ride(Vec2, f32),
    /// One of a creation's actions (`@potluck/input`), pressed or let go.
    Press { creation: String, action: String, down: bool },
    /// An answer to a creation's question (`ui.ask`): a choice from 0, or
    /// None for closing it.
    Answer { ask: u32, choice: Option<u32> },
    /// A message from this player's client half to its creation (`@potluck/net`).
    Net(crate::script::Note),
    /// What this player carries now (survival), and how many lots of the
    /// host's changes they've had (`world.pockets`).
    Pockets { survival: bool, items: Vec<(crate::blocks::BlockId, u32)>, seen: u32 },
    /// Interact on a block, for creations' `block_used`.
    UseBlock([i32; 3]),
    /// What this player has in hand now (`world.held`); air for nothing.
    Held(crate::blocks::BlockId),
}

/// How far from a player a block they use can be.
const USE_REACH: f32 = 8.0;

/// What players did this frame: (player id, action).
#[derive(Resource, Default)]
pub struct Actions(pub Vec<(u32, Action)>);

#[derive(Resource)]
pub struct HerdRng(pub Rng);

/// Aiming at creatures, getting on and off, and the pause button.
#[allow(clippy::too_many_arguments)]
pub fn interact(
    c: Res<Controls>,
    mut focus: ResMut<Focus>,
    herd: Res<Herd>,
    mut me: ResMut<Me>,
    net: Res<Net>,
    mut actions: ResMut<Actions>,
    mut acts: ResMut<Acts>,
    green: Res<Green>,
    lib: Res<Library>,
    settings: Res<Settings>,
) {
    me.aim_body = None;
    me.aim_arch = None;
    // a creation's actions: tell the host what was pressed and let go
    let live = if focus.playing() { c.live.clone() } else { None };
    let now: Vec<String> = if live.is_some() { c.actions.clone() } else { vec![] };
    let (was_live, was) = std::mem::take(&mut me.pressing);
    for a in &was {
        if was_live != live || !now.contains(a) {
            actions.0.push((net.my_id, Action::Press { creation: was_live.clone().unwrap_or_default(), action: a.clone(), down: false }));
        }
    }
    for a in &now {
        if was_live != live || !was.contains(a) {
            actions.0.push((net.my_id, Action::Press { creation: live.clone().unwrap_or_default(), action: a.clone(), down: true }));
        }
    }
    me.live_prompt = live.as_ref().and_then(|id| lib.list.get(id)).map(|cr| {
        let what: Vec<String> = cr
            .manifest
            .actions
            .iter()
            .filter_map(|(id, a)| {
                let (pad, key) = settings.binding(&cr.id, id, a);
                let b = if c.pad && !pad.is_empty() { crate::input::button_name(&pad) } else if !key.is_empty() { key } else { crate::input::button_name(&pad) };
                (!b.is_empty()).then(|| format!("{b} {}", a.name))
            })
            .collect();
        what.join("   ")
    }).filter(|p| !p.is_empty());
    me.pressing = (live, now);
    // a creation's question: it opens once nothing else is on the screen,
    // and goes when the creation takes it back
    if focus.screen == Screen::Panel {
        match &me.panel {
            Some(p) if focus.panel.as_ref().map(|q| q.ask) != Some(p.ask) => focus.panel = Some(p.clone()),
            Some(_) => {}
            None => {
                focus.panel = None;
                focus.go(Screen::Playing);
            }
        }
    } else if focus.playing() {
        if let Some(p) = &me.panel {
            focus.panel = Some(p.clone());
            focus.go(Screen::Panel);
        }
    }
    if !focus.playing() {
        // a menu open while riding: the creature stands and waits
        if me.riding.is_some() {
            actions.0.push((net.my_id, Action::Ride(Vec2::ZERO, me.yaw)));
        }
        return;
    }
    if c.pause {
        focus.go(Screen::Pause);
        return;
    }
    let eye = me.eye();
    let dir = me.forward();
    let block_t = me.target.map(|h| (h.block.as_vec3() + Vec3::splat(0.5) - eye).length() - 0.5).unwrap_or(f32::MAX);
    let riding = me.riding;
    let pick = herd.bodies.values().filter(|b| Some(b.id) != riding).filter_map(|b| b.ray(eye, dir).map(|t| (b.id, t))).filter(|(_, t)| *t < 4.5 && *t < block_t).min_by(|a, b| a.1.total_cmp(&b.1));
    if let Some((id, _)) = pick {
        me.aim_body = Some(id);
    } else if riding.is_none() {
        // one of the Green's arches
        if let Some(i) = green::aimed(eye, dir, block_t) {
            let text = match &green.arches(&lib)[i] {
                Some(s @ Shows::Here(_)) => format!("{}\n{} Look inside", green::describe(s, &lib).1, glyph(c.pad, "interact")),
                Some(s @ Shows::Pick(_)) => format!("{}\n{} Bring it into this world", green::describe(s, &lib).1, glyph(c.pad, "interact")),
                None => "An arch for creations\nBring one into this world and it shows here".to_string(),
            };
            me.aim_arch = Some((i, text));
        }
    }
    let pid = net.my_id;
    // Share: what you ride or aim at, straight to a code
    if c.share {
        if let Some(b) = me.riding.or(me.aim_body).and_then(|id| herd.bodies.get(&id)) {
            acts.0.push(Act::Share(b.creation.clone()));
        }
    }
    if me.riding.is_some() {
        if c.interact || c.crouch {
            actions.0.push((pid, Action::Dismount));
        } else {
            actions.0.push((pid, Action::Ride(c.mv, me.yaw)));
        }
        if c.look_inside {
            if let Some(b) = me.riding.and_then(|id| herd.bodies.get(&id)) {
                focus.go(Screen::Inside(b.creation.clone()));
            }
        }
        return;
    }
    if let Some(id) = me.aim_body {
        if c.interact {
            actions.0.push((pid, Action::Use(id)));
        }
        if c.break_pressed {
            actions.0.push((pid, Action::Hit(id)));
        }
        if c.look_inside {
            if let Some(b) = herd.bodies.get(&id) {
                focus.go(Screen::Inside(b.creation.clone()));
            }
        }
    } else if let Some((i, _)) = me.aim_arch.as_ref().filter(|_| c.interact) {
        acts.0.push(Act::Arch(*i));
    } else if let Some(hit) = me.target.filter(|_| c.interact) {
        // a block: creations may answer it (a table to craft at, say)
        actions.0.push((pid, Action::UseBlock(hit.block.to_array())));
    } else if c.look_inside {
        focus.go(Screen::Library);
    }
    if c.camera {
        me.third_person = !me.third_person;
    }
}

/// On the host (or alone): creatures move, actions happen, creations tick.
#[allow(clippy::too_many_arguments)]
pub fn host_step(
    time: Res<Time>,
    mut net: ResMut<Net>,
    mut voxels: ResMut<Voxels>,
    mut herd: ResMut<Herd>,
    mut scripts: ResMut<Scripts>,
    mut actions: ResMut<Actions>,
    mut me: Option<ResMut<Me>>,
    mut toasts: ResMut<Toasts>,
    mut rng: ResMut<HerdRng>,
    settings: Res<Settings>,
    mut heard: ResMut<crate::sound::Heard>,
    clock: Res<crate::render::Clock>,
) {
    if matches!(net.role, Role::Client(_)) {
        // the host hears about this player's blocks from the blocks themselves
        if let Some(me) = me.as_mut() {
            me.built.clear();
        }
        return;
    }
    let dt = time.delta_secs().min(0.05);
    scripts.set_day(clock.time);
    let my_id = net.my_id;
    let mut players = vec![];
    if let Some(me) = &me {
        players.push(PlayerInfo { id: my_id, name: settings.name.clone(), pos: me.pos });
    }
    players.extend(net.remote_players().into_iter().map(|p| PlayerInfo { id: p.id, name: p.name.clone(), pos: p.pos }));
    scripts.players = players.clone();
    // blocks this game's own player placed and broke
    if let Some(me) = me.as_mut() {
        for (at, old, new) in std::mem::take(&mut me.built) {
            scripts.player_block(my_id, at, old, new);
        }
    }
    for (pid, a) in std::mem::take(&mut actions.0) {
        let Some(player) = players.iter().find(|p| p.id == pid).cloned() else { continue };
        match a {
            Action::Use(id) => {
                let already = herd.bodies.values().any(|b| b.rider == Some(pid));
                let Some(b) = herd.bodies.get_mut(&id) else { continue };
                if b.rideable && b.rider.is_none() && !already {
                    b.rider = Some(pid);
                    b.goal = None;
                    scripts.event(&mut voxels, &mut herd, id, "ridden", &player);
                } else if !b.rideable {
                    scripts.event(&mut voxels, &mut herd, id, "used", &player);
                }
            }
            Action::Hit(id) => scripts.event(&mut voxels, &mut herd, id, "hit", &player),
            Action::Dismount => {
                for b in herd.bodies.values_mut() {
                    if b.rider == Some(pid) {
                        b.rider = None;
                    }
                }
                herd.riders.remove(&pid);
            }
            Action::Ride(mv, yaw) => {
                herd.riders.insert(pid, (mv, yaw));
            }
            Action::Press { creation, action, down } => {
                // only while it's live for them: riding its creature, or it has their controls
                let live = herd.bodies.values().any(|b| b.creation == creation && b.rider == Some(pid)) || scripts.focus_of(pid).as_deref() == Some(creation.as_str());
                if live || !down {
                    scripts.press(pid, &creation, &action, down);
                }
            }
            Action::Answer { ask, choice } => scripts.answer(pid, ask, choice),
            Action::Net(note) => scripts.hear(pid, note),
            Action::Pockets { survival, items, seen } => scripts.set_pockets(pid, survival, &items, seen),
            Action::Held(b) => scripts.set_held(pid, b),
            Action::UseBlock(at) => {
                let at = IVec3::from_array(at);
                if (at.as_vec3() + Vec3::splat(0.5)).distance(player.pos) <= USE_REACH {
                    scripts.use_block(pid, at, voxels.at(at));
                }
            }
        }
    }
    let w = &*voxels;
    creatures::step(&mut herd, w, dt, &mut rng.0);
    let (v, h) = (&mut *voxels, &mut *herd);
    scripts.tick(v, h, dt);
    for t in std::mem::take(&mut scripts.out.toasts) {
        net.broadcast_toast(&t);
        toasts.push(t);
    }
    // creations that took or gave back a player's controls
    for (pid, creation) in scripts.focus_changes() {
        if pid == my_id && me.is_some() {
            me.as_mut().unwrap().focus = creation;
        } else {
            net.send_focus(pid, creation);
        }
    }
    // what creations show players, and ask them
    let here: Vec<u32> = players.iter().map(|p| p.id).collect();
    for (pid, change) in scripts.ui_changes(&here) {
        if pid == my_id && me.is_some() {
            me.as_mut().unwrap().show(change);
        } else {
            net.send_ui(pid, change);
        }
    }
    // blocks creations gave players or took from them
    for (pid, seq, changes) in scripts.pocket_changes() {
        if pid == my_id && me.is_some() {
            let me = me.as_mut().unwrap();
            me.change_pockets(&changes);
            me.pockets_seen = seq;
        } else {
            net.send_pockets(pid, seq, changes);
        }
    }
    // creations that hurt or healed players
    for (pid, change, down_to) in scripts.health_changes() {
        if pid == my_id && me.is_some() {
            let me = me.as_mut().unwrap();
            if down_to > 0.0 && change < 0.0 {
                me.hurts.push((-change, down_to));
            } else {
                me.health_due += change;
            }
        } else {
            net.send_health(pid, change, down_to);
        }
    }
    // how fast creations let players heal by themselves
    for (pid, rate) in scripts.healing_changes() {
        if pid == my_id && me.is_some() {
            me.as_mut().unwrap().healing = rate;
        } else {
            net.send_healing(pid, rate);
        }
    }
    // what tools dig faster, for every game here
    if let Some(digging) = scripts.digging_changes() {
        if let Some(me) = me.as_mut() {
            me.digging = digging.iter().copied().collect();
        }
        net.broadcast_digging(digging);
    }
    // messages creations sent their client halves
    for (pid, note) in scripts.net_changes(&here) {
        if pid == my_id && me.is_some() {
            me.as_mut().unwrap().notes.push(note);
        } else {
            net.send_note(pid, note);
        }
    }
    // sounds creations played, to whoever's near
    for (pid, play) in scripts.sound_changes() {
        if pid == my_id && me.is_some() {
            heard.0.push(play);
        } else {
            net.send_sound(pid, play);
        }
    }
    for (pid, v) in std::mem::take(&mut scripts.out.throws) {
        herd.riders.remove(&pid);
        if pid == my_id && me.is_some() {
            throw(me.as_mut().unwrap(), v);
        } else {
            net.throw(pid, v);
        }
    }
    let scores = std::mem::take(&mut scripts.out.scores);
    if !scores.is_empty() {
        let mut shared = scripts.shared.lock().unwrap();
        for (creation, board, lower, pid, value) in scores {
            let Some(p) = players.iter().find(|p| p.id == pid) else { continue };
            let handle = net.handle_of(pid);
            shared.boards.submit(&creation, &board, lower, &p.name, handle.as_deref(), value);
        }
    }
    for (pid, link, note) in std::mem::take(&mut scripts.out.sends) {
        // the player at this screen is the world; they can't be sent away
        if pid == my_id && me.is_some() {
            continue;
        }
        let address = net.rules().and_then(|r| r.links.get(&link).cloned());
        if let Some(address) = address {
            net.send_away(pid, &address, &note);
        }
    }
}

pub fn throw(me: &mut Me, v: Vec3) {
    me.riding = None;
    me.pos += Vec3::Y * 0.3;
    me.vel = v;
}

/// Keeps `me.riding` in step with who the creatures say is riding them.
pub fn riding(herd: Res<Herd>, net: Res<Net>, mut me: ResMut<Me>) {
    let now = herd.bodies.values().find(|b| b.rider == Some(net.my_id)).map(|b| b.id);
    if me.riding.is_some() && now.is_none() {
        // got off: step down beside it
        if let Some(b) = me.riding.and_then(|id| herd.bodies.get(&id)) {
            if me.vel.length() < 0.5 {
                let side = Vec3::new(b.forward().z, 0.0, -b.forward().x);
                me.pos = b.pos + side * (b.half.x + 0.7) + Vec3::Y * 0.3;
            }
        }
    }
    me.riding = now;
}

pub fn labels(herd: Res<Herd>, net: Res<Net>, green: Res<Green>, lib: Res<Library>, mut labels: ResMut<Labels>) {
    labels.0.clear();
    for (i, s) in green.arches(&lib).iter().enumerate() {
        if let Some(s) = s {
            labels.0.push((green::arch_at(i) + Vec3::Y * 5.6, green::describe(s, &lib).0));
        }
    }
    for b in herd.bodies.values() {
        if let Some((text, _)) = &b.says {
            labels.0.push((b.pos + Vec3::Y * (b.half.y + 0.7), text.clone()));
        }
    }
    for p in net.remote_players() {
        labels.0.push((p.pos + Vec3::Y * 2.2, p.name.clone()));
    }
}

/// Starts (or restarts) a creation in this world: on the host it runs here
/// and goes to everyone; a guest sends it to the host.
pub fn bring(id: &str, lib: &mut Library, scripts: &mut Scripts, herd: &mut Herd, net: &mut Net, toasts: &mut Toasts) {
    let Some(c) = lib.list.get(id).cloned() else { return };
    if let Role::Client(_) = net.role {
        net.send_creation(&c);
        return;
    }
    match scripts.start(&c, herd) {
        Ok(()) => {
            if !lib.running.contains(&c.id) {
                lib.running.push(c.id.clone());
            }
            if let Some(vm) = scripts.vms.get_mut(&c.id) {
                vm.error = None;
            }
            net.broadcast_creation(&c);
        }
        Err(e) => {
            if let Some(vm) = scripts.vms.get_mut(&c.id) {
                vm.error = Some(e.clone());
                vm.log.push(format!("error: {e}"));
            }
            toasts.push(format!("{} has a problem: {e}", c.manifest.name));
        }
    }
}

/// Hot reload: files saved in an editor, and knobs turned in Tweak.
#[allow(clippy::too_many_arguments)]
pub fn reloads(
    time: Res<Time>,
    mut lib: ResMut<Library>,
    mut scripts: ResMut<Scripts>,
    mut herd: ResMut<Herd>,
    mut net: ResMut<Net>,
    mut toasts: ResMut<Toasts>,
    mut edits: ResMut<KnobEdits>,
    mut focus: ResMut<Focus>,
) {
    lib.watch(time.delta_secs());
    for id in std::mem::take(&mut lib.changed) {
        let name = lib.list.get(&id).map(|c| c.manifest.name.clone()).unwrap_or_default();
        let before = std::time::Instant::now();
        bring(&id, &mut lib, &mut scripts, &mut herd, &mut net, &mut toasts);
        let ok = scripts.vms.get(&id).map(|v| v.error.is_none()).unwrap_or(false);
        if ok {
            toasts.push(format!("{name} reloaded in {} ms", before.elapsed().as_millis().max(1)));
        }
    }
    let now = time.elapsed_secs();
    for (id, key, v) in std::mem::take(&mut edits.0) {
        if let Some(c) = lib.list.get_mut(&id) {
            c.knobs.insert(key.clone(), v.clone());
        }
        // show the new value at once, then reload when the stick settles
        if let Some(vm) = scripts.vms.get_mut(&id) {
            if let Some(k) = vm.knobs.iter_mut().find(|k| k.key == key) {
                k.value = v;
            }
        }
        focus.reload_at = Some((id, now + 0.12));
    }
    if let Some((id, at)) = focus.reload_at.clone() {
        if now >= at {
            focus.reload_at = None;
            lib.save_knobs();
            if let Role::Client(_) = net.role {
                if let Some(c) = lib.list.get(&id) {
                    net.send_tweak(&id, &c.knobs);
                }
            } else {
                bring(&id, &mut lib, &mut scripts, &mut herd, &mut net, &mut toasts);
            }
        }
    }
}

/// What the menus' buttons do.
#[allow(clippy::too_many_arguments)]
pub fn do_acts(
    mut acts: ResMut<Acts>,
    mut focus: ResMut<Focus>,
    mut lib: ResMut<Library>,
    mut scripts: ResMut<Scripts>,
    mut herd: ResMut<Herd>,
    mut net: ResMut<Net>,
    mut toasts: ResMut<Toasts>,
    mut settings: ResMut<Settings>,
    mut me: ResMut<Me>,
    mut voxels: ResMut<Voxels>,
    mut info: ResMut<WorldInfo>,
    shares: Res<ShareJobs>,
    mut green: ResMut<Green>,
    mut actions: ResMut<Actions>,
    mut exit: MessageWriter<AppExit>,
) {
    // what the Green's arches can offer, asked for now and then while playing
    if info.in_world && green.asked.is_none_or(|t| t.elapsed().as_secs_f32() > green::ASK_EVERY) {
        green.asked = Some(std::time::Instant::now());
        // until a player says how old they are, only what's for all ages
        shares.green(&Who { young: settings.young || !settings.age_asked, ..Who::of(&settings) });
    }
    let mut later = vec![];
    for act in std::mem::take(&mut acts.0) {
        // the first time anything goes online, ask who's playing
        if !settings.age_asked && goes_online(&act) {
            focus.status.clear();
            focus.go(Screen::Age(Box::new(act)));
            continue;
        }
        match act {
            Act::Resume => focus.go(Screen::Playing),
            Act::SetAge(young) => {
                settings.age_asked = true;
                settings.young = young;
                settings.save();
                let back = if info.in_world { Screen::Pause } else { Screen::Title };
                if let Screen::Age(next) = std::mem::replace(&mut focus.screen, back) {
                    later.push(*next);
                }
            }
            Act::OpenParents => {
                focus.parents_open = settings.parent_pin.is_empty();
                focus.fields.remove("pin");
                focus.fields.remove("newpin");
                focus.status.clear();
                focus.go(Screen::Parents);
            }
            Act::UnlockParents(pin) => {
                let now = std::time::Instant::now();
                focus.pin_misses.retain(|t| now.duration_since(*t).as_secs() < 300);
                focus.fields.remove("pin");
                if focus.pin_misses.len() >= 5 {
                    focus.status = "That's a lot of tries: wait a few minutes.".into();
                } else if settings.pin_matches(&pin) {
                    focus.parents_open = true;
                    focus.status.clear();
                } else {
                    focus.pin_misses.push(now);
                    focus.status = "That isn't the PIN.".into();
                }
            }
            Act::SetPin(pin) => {
                let pin = pin.trim().to_string();
                if !focus.parents_open {
                    continue;
                }
                if (4..=8).contains(&pin.len()) && pin.chars().all(|c| c.is_ascii_digit()) {
                    settings.parent_pin = settings.pin_hash(&pin);
                    settings.save();
                    focus.fields.remove("newpin");
                    focus.status = "PIN set. Only someone who knows it can change these now.".into();
                } else {
                    focus.status = "A PIN is 4 to 8 numbers.".into();
                }
            }
            Act::PlayAlone => {
                info.in_world = true;
                focus.go(Screen::Playing);
            }
            Act::Host => {
                match net.host(&settings.name) {
                    Ok(addr) => {
                        toasts.push(format!("Your world is open. Friends on this network can join; or give them {addr}"));
                        info.in_world = true;
                        focus.go(Screen::Playing);
                    }
                    Err(e) => toasts.push(format!("Couldn't open the world to friends: {e}")),
                }
            }
            Act::OpenServers => {
                focus.servers.clear();
                focus.status.clear();
                focus.go(Screen::Servers);
                if settings.servers {
                    shares.servers(&Who::of(&settings));
                }
            }
            Act::OpenJoin => {
                net.listen_lan();
                focus.status.clear();
                focus.go(Screen::Join);
            }
            Act::JoinAddress(addr) => {
                let addr = addr.trim().to_string();
                if addr.is_empty() {
                    focus.status = "Type an address first, like 192.168.1.20".into();
                    continue;
                }
                // the market vouches for who we are, for worlds that ask
                focus.status = format!("Joining {addr}...");
                shares.pass(&Who::of(&settings), &addr);
            }
            Act::OpenCode => {
                focus.status.clear();
                focus.go(Screen::Code);
            }
            Act::LoadCode(code) => {
                let code = crate::share::clean_code(&code);
                if code.len() != 8 {
                    focus.status = "A code is 8 letters and numbers.".into();
                    continue;
                }
                focus.status = format!("Fetching {}...", crate::ui::pretty_code(&code));
                shares.load(&Who::of(&settings), &code, None);
            }
            Act::OpenLibrary => focus.go(Screen::Library),
            Act::OpenSettings => {
                focus.parents_open = false;
                focus.fields.insert("name".into(), settings.name.clone());
                focus.fields.insert("server".into(), settings.server.clone());
                focus.fields.insert("market".into(), settings.market.clone());
                focus.go(Screen::Settings);
            }
            Act::Inside(id) => focus.go(Screen::Inside(id)),
            Act::Tweak(id) => focus.go(Screen::Tweak(id)),
            Act::Remix(id) => match lib.remix(&id, &settings.name) {
                Ok(new_id) => {
                    // the remix takes over from the original in this world
                    if lib.running.contains(&id) {
                        for b in herd.bodies.values_mut().filter(|b| b.creation == id) {
                            b.creation = new_id.clone();
                        }
                        lib.running.retain(|r| *r != id);
                        scripts.vms.remove(&id);
                        bring(&new_id, &mut lib, &mut scripts, &mut herd, &mut net, &mut toasts);
                    }
                    if let Some(dir) = lib.list.get(&new_id).and_then(|c| c.dir.clone()) {
                        toasts.push(format!("Your remix is in {}. Change main.luau in any editor and save: the world reloads it.", short_path(&dir)));
                        open_folder(&dir);
                    }
                    focus.go(Screen::Inside(new_id));
                }
                Err(e) => toasts.push(format!("Couldn't remix it: {e}")),
            },
            Act::EditFolder(id) => {
                if let Some(dir) = lib.list.get(&id).and_then(|c| c.dir.clone()) {
                    open_folder(&dir);
                    toasts.push(format!("Opened {}", dir.display()));
                }
            }
            Act::Share(id) => {
                focus.status.clear();
                focus.go(Screen::Share(id));
            }
            Act::ShareTo(id, everyone) => {
                // younger players share with friends, by code
                let everyone = everyone && !settings.young;
                if !settings.sharing {
                    focus.status = "A parent turned sharing off.".into();
                } else if let Some(c) = lib.list.get(&id).filter(|c| c.editable()) {
                    focus.status = "Sharing...".into();
                    let code = lib.marks.get(&id).filter(|m| m.version > 0).map(|m| m.code.clone());
                    shares.share(&Who::of(&settings), c.payload(), everyone, code, c.manifest.from.clone());
                }
            }
            Act::Suggest(_) if settings.young || !settings.sharing => {
                toasts.push("Younger players share with friends, by code");
            }
            Act::Suggest(id) => {
                focus.status.clear();
                focus.fields.remove("suggest");
                focus.go(Screen::Suggest(id));
            }
            Act::SendSuggestion(id) => {
                if let Some(c) = lib.list.get(&id).filter(|c| c.editable()) {
                    focus.status = "Sending it...".into();
                    let mark = lib.marks.get(&id).filter(|m| m.version > 0);
                    let title = focus.fields.get("suggest").cloned().unwrap_or_default();
                    shares.suggest(&Who::of(&settings), c.payload(), mark.is_some_and(|m| m.everyone), mark.map(|m| m.code.clone()), c.manifest.from.clone(), title);
                }
            }
            Act::OpenSuggestions(id) => {
                focus.suggestions.clear();
                if let Some(m) = lib.marks.get(&id) {
                    focus.status = "Looking...".into();
                    shares.suggestions(&Who::of(&settings), &id, &m.code);
                }
                focus.go(Screen::Suggestions(id));
            }
            Act::TrySuggestion(code, version) => {
                focus.status = format!("Fetching {}...", crate::ui::pretty_code(&code));
                shares.load(&Who::of(&settings), &code, Some(version));
            }
            Act::Merge(id, suggestion) => {
                focus.status = "Merging...".into();
                shares.merge(&Who::of(&settings), &id, suggestion);
            }
            Act::CloseSuggestion(_, suggestion) => {
                focus.suggestions.retain(|s| s.id != suggestion);
                shares.close_suggestion(&Who::of(&settings), suggestion);
            }
            Act::Report(id) => focus.go(Screen::Report(id)),
            Act::SendReport(id, reason) => {
                if let Some(code) = lib.list.get(&id).and_then(|c| c.code.clone()) {
                    shares.report(&Who::of(&settings), &code, &reason);
                }
                focus.status.clear();
                focus.go(Screen::Inside(id));
            }
            Act::Market(next, title) => {
                focus.link = None;
                focus.status.clear();
                shares.link(&Who::of(&settings), &next, &title);
                focus.go(Screen::Link);
            }
            Act::OpenDevices => {
                focus.status.clear();
                focus.link_code = None;
                focus.fields.remove("link");
                shares.account(&Who::of(&settings));
                focus.go(Screen::Devices);
            }
            Act::MakeLinkCode => {
                focus.status = "Asking the market for a code...".into();
                shares.link_code(&Who::of(&settings));
            }
            Act::LinkDevice(code) => {
                let n = crate::share::clean_code(&code).len();
                if n != 8 && n != 20 {
                    focus.status = "A code from another device is 8 letters and numbers; a recovery code is 20.".into();
                    continue;
                }
                focus.status = "Linking...".into();
                shares.link_device(&Who::of(&settings), &code);
            }
            Act::UnlinkDevice => {
                focus.status = "Unlinking...".into();
                shares.unlink(&Who::of(&settings));
            }
            Act::OpenUrl(url) => {
                open_link(&url);
                // a link works once: opening it here uses it up
                focus.link = None;
                focus.status = "Opened in your browser.".into();
            }
            Act::Run(id) => {
                bring(&id, &mut lib, &mut scripts, &mut herd, &mut net, &mut toasts);
            }
            Act::Arch(i) => match green.arches(&lib).get(i).cloned().flatten() {
                Some(Shows::Here(id)) => focus.go(Screen::Inside(id)),
                Some(Shows::Pick(p)) => {
                    let had = lib.list.values().find(|c| c.code.as_deref() == Some(p.code.as_str())).map(|c| c.id.clone());
                    match had {
                        Some(id) => {
                            bring(&id, &mut lib, &mut scripts, &mut herd, &mut net, &mut toasts);
                            toasts.push(format!("{} is in your world", p.name));
                        }
                        None if green.fetching.is_none() => {
                            green.fetching = Some(p.code.clone());
                            toasts.push(format!("Fetching {}...", p.name));
                            shares.load(&Who::of(&settings), &p.code, None);
                        }
                        None => {}
                    }
                }
                None => {}
            },
            Act::OpenControls => {
                focus.rebinding = None;
                focus.go(Screen::Controls);
            }
            Act::Rebind(creation, action) => focus.rebinding = Some((creation, action)),
            Act::ResetControls => {
                settings.bindings.clear();
                settings.save();
            }
            Act::EnterJam(id) => {
                let code = lib.marks.get(&id).filter(|m| m.everyone && m.version > 0).map(|m| m.code.clone());
                match (focus.jam.clone(), code) {
                    (Some(jam), Some(code)) => {
                        focus.status = format!("Entering it in the {} jam...", jam.theme);
                        shares.enter_jam(&Who::of(&settings), &jam, &code);
                    }
                    _ => focus.status = "Share it to Everyone first: jam entries are on the market for anyone to try.".into(),
                }
            }
            Act::Stop(id) => {
                if !matches!(net.role, Role::Client(_)) {
                    scripts.stop(&id, &mut herd);
                    lib.running.retain(|r| *r != id);
                    net.broadcast_stop(&id);
                }
            }
            Act::ResetKnobs(id) => {
                if let Some(c) = lib.list.get_mut(&id) {
                    c.knobs.clear();
                }
                focus.reload_at = Some((id, 0.0));
            }
            Act::Back => {
                let to = match &focus.screen {
                    Screen::Servers => Screen::Join,
                    Screen::Inside(_) | Screen::Library if info.in_world => Screen::Playing,
                    _ if info.in_world => Screen::Pause,
                    _ => Screen::Title,
                };
                settings.save();
                focus.go(to);
            }
            Act::Answer(choice) => {
                // the one on the screen; the host says what's next
                if let Some(p) = focus.panel.take() {
                    actions.0.push((net.my_id, Action::Answer { ask: p.ask, choice }));
                    if me.panel.as_ref().is_some_and(|q| q.ask == p.ask) {
                        me.panel = None;
                    }
                }
                if focus.screen == Screen::Panel {
                    focus.go(Screen::Playing);
                }
            }
            Act::Leave => {
                me.focus = None;
                me.pieces.clear();
                me.panel = None;
                net.leave();
                *voxels = crate::world::generate(voxels.seed);
                herd.bodies.clear();
                start_world(&mut lib, &mut scripts, &mut herd, &mut net, &mut toasts);
                me.respawn(&voxels, None);
                info.in_world = false;
                focus.go(Screen::Title);
            }
            Act::Quit => {
                exit.write(AppExit::Success);
            }
            Act::Survival => settings.survival = !settings.survival,
            Act::InvertY => settings.invert_y = !settings.invert_y,
            Act::Lighting => {
                settings.lighting = crate::render::Tier::next(&settings.lighting).into();
                settings.save();
            }
        }
    }
    acts.0.extend(later);
}

/// Acts that reach other people: the first one asks who's playing.
fn goes_online(act: &Act) -> bool {
    matches!(act, Act::Share(_) | Act::ShareTo(..) | Act::Suggest(_) | Act::OpenServers | Act::JoinAddress(_) | Act::Market(..) | Act::OpenDevices | Act::EnterJam(_))
}

/// The creations a fresh world starts with: the base game's.
pub fn start_world(lib: &mut Library, scripts: &mut Scripts, herd: &mut Herd, net: &mut Net, toasts: &mut Toasts) {
    lib.running.clear();
    lib.world += 1;
    scripts.vms.clear();
    {
        // what creations held of players' controls and screens goes with them
        let mut sh = scripts.shared.lock().unwrap();
        sh.input = Default::default();
        sh.ui = Default::default();
    }
    let base: Vec<String> = lib.list.values().filter(|c| c.origin == Origin::Base).map(|c| c.id.clone()).collect();
    for id in base {
        bring(&id, lib, scripts, herd, net, toasts);
    }
}

/// Finished share jobs: a code to show, or a creation to bring in.
#[allow(clippy::too_many_arguments)]
pub fn share_results(
    mut settings: ResMut<Settings>,
    shares: Res<ShareJobs>,
    mut focus: ResMut<Focus>,
    mut lib: ResMut<Library>,
    mut scripts: ResMut<Scripts>,
    mut herd: ResMut<Herd>,
    mut net: ResMut<Net>,
    mut toasts: ResMut<Toasts>,
    info: Res<WorldInfo>,
    mut green: ResMut<Green>,
) {
    for done in shares.poll() {
        match done {
            Done::Shared { id, code, version, everyone, message } => {
                if version > 0 {
                    lib.mark(&id, Mark { code: code.clone(), version, everyone });
                } else if let Some(c) = lib.list.get_mut(&id) {
                    c.code = Some(code.clone());
                }
                focus.status = message;
                toasts.push(format!("Shared as {}", crate::ui::pretty_code(&code)));
            }
            Done::Loaded { code, version, payload } => match Creation::from_payload(payload, Origin::Code(code.clone())) {
                Ok(mut c) => {
                    c.code = Some(code.clone());
                    let name = c.manifest.name.clone();
                    let id = lib.add(c);
                    if version > 0 {
                        lib.marks.insert(id.clone(), Mark { code: code.clone(), version, everyone: false });
                    }
                    focus.status = String::new();
                    if info.in_world {
                        bring(&id, &mut lib, &mut scripts, &mut herd, &mut net, &mut toasts);
                        toasts.push(format!("{name} is in your world"));
                    }
                    // from an arch: stay in the world with it
                    if green.fetching.as_ref() == Some(&code) && info.in_world {
                        green.fetching = None;
                    } else {
                        focus.go(Screen::Inside(id));
                    }
                }
                Err(e) => {
                    if green.fetching.take().is_some() {
                        toasts.push(format!("Couldn't load it: {e}"));
                    }
                    focus.status = format!("Couldn't load it: {e}");
                }
            },
            Done::Link { url, title } => {
                focus.status.clear();
                focus.link = Some((url, title));
            }
            Done::Pass { address, pass } => match net.join(&address, &settings.name, pass) {
                Ok(()) => focus.status = format!("Joining {address}..."),
                Err(e) => focus.status = format!("Couldn't join {address}: {e}"),
            },
            Done::Servers(list) => {
                focus.status = if list.is_empty() { "No servers are listed on the market right now.".into() } else { String::new() };
                focus.servers = list;
            }
            Done::Suggested { id, code } => {
                if let Some(m) = lib.marks.get(&id).cloned().filter(|m| m.code == code) {
                    lib.mark(&id, m);
                }
                focus.status = "Sent. Its maker sees it under Changes others suggested, and you'll see their answer on the market.".into();
                toasts.push("Your change is on its way to the original's maker");
            }
            Done::Suggestions { id, list } => {
                if focus.screen == Screen::Suggestions(id) {
                    focus.status.clear();
                    focus.suggestions = list;
                }
            }
            Done::Merged { id, suggestion, code, version, files } => {
                let everyone = lib.marks.get(&id).is_some_and(|m| m.everyone);
                match lib.replace_files(&id, files) {
                    Ok(()) => {
                        lib.mark(&id, Mark { code, version, everyone });
                        focus.suggestions.retain(|s| s.id != suggestion);
                        focus.status = format!("Merged: version {version} is in your folder.");
                        toasts.push(format!("Merged: version {version} is in your folder"));
                    }
                    Err(e) => focus.status = format!("Merged on the market, but couldn't write it here: {e}."),
                }
            }
            Done::Account(a) => focus.account = Some(a),
            Done::LinkCode(code) => {
                focus.status.clear();
                focus.link_code = Some(code);
            }
            Done::Linked { name, handle, recovery } => {
                // the account's name, as far as names in the game go
                let game_name: String = name.chars().filter(|c| c.is_alphanumeric() || *c == '_' || *c == ' ').take(20).collect();
                if !game_name.trim().is_empty() {
                    settings.name = game_name.trim().to_string();
                    settings.save();
                }
                focus.fields.remove("link");
                focus.fields.insert("name".into(), settings.name.clone());
                focus.account = Some(crate::share::Account { handle, name: name.clone(), linked: true });
                focus.status = if recovery {
                    format!("This device plays as {name} now. Your recovery code is used up: make a new one on the market, under Your devices.")
                } else {
                    format!("This device plays as {name} now.")
                };
                toasts.push(format!("Playing as {name}"));
            }
            Done::Unlinked => {
                focus.status = "Unlinked. This device starts an account of its own.".into();
                focus.account = None;
                shares.account(&Who::of(&settings));
            }
            Done::Said(s) => {
                focus.status = s.clone();
                toasts.push(s);
            }
            Done::Green { picks, jam } => {
                green.picks = picks;
                focus.jam = jam;
            }
            Done::Entered { theme } => {
                focus.status = format!("Entered in the {theme} jam. Stars it gets until the jam ends count.");
                toasts.push(format!("Entered in the {theme} jam"));
            }
            Done::Failed(e) => {
                // a code fetched for an arch: say so in the world
                if green.fetching.take().is_some() && focus.screen == Screen::Playing {
                    toasts.push(format!("Couldn't fetch it: {e}"));
                }
                focus.status = format!("Sorry: {e}.");
            }
        }
    }
}

/// On the title screen the view turns slowly over the Green.
pub fn title_view(focus: Res<Focus>, info: Res<WorldInfo>, time: Res<Time>, mut me: ResMut<Me>) {
    if focus.screen == Screen::Title && !info.in_world {
        me.yaw += time.delta_secs() * 0.03;
    }
}

pub fn knob_number(c: &Creation, key: &str) -> Option<f64> {
    match c.knobs.get(key) {
        Some(KnobValue::Number(n)) => Some(*n),
        _ => None,
    }
}

/// A folder as a player would type it, with their home folder as `~`.
pub fn short_path(dir: &std::path::Path) -> String {
    match std::env::var_os("HOME").map(std::path::PathBuf::from) {
        Some(home) if !home.as_os_str().is_empty() && dir.starts_with(&home) => {
            format!("~/{}", dir.strip_prefix(&home).unwrap_or(dir).display())
        }
        _ => dir.display().to_string(),
    }
}
