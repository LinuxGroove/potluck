//! The rules that tie it together: what menu choices do, using creatures,
//! running creations on the host, reloading them when they change.

use bevy::prelude::*;

use crate::creations::{open_folder, open_link, Creation, Library, Mark, Origin};
use crate::creatures::{self, Herd};
use crate::input::Controls;
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
}

/// What players did this frame: (player id, action).
#[derive(Resource, Default)]
pub struct Actions(pub Vec<(u32, Action)>);

#[derive(Resource)]
pub struct HerdRng(pub Rng);

/// Aiming at creatures, getting on and off, and the pause button.
pub fn interact(c: Res<Controls>, mut focus: ResMut<Focus>, herd: Res<Herd>, mut me: ResMut<Me>, net: Res<Net>, mut actions: ResMut<Actions>, mut acts: ResMut<Acts>) {
    me.aim_body = None;
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
) {
    if matches!(net.role, Role::Client(_)) {
        return;
    }
    let dt = time.delta_secs().min(0.05);
    let my_id = net.my_id;
    let mut players = vec![];
    if let Some(me) = &me {
        players.push(PlayerInfo { id: my_id, name: settings.name.clone(), pos: me.pos });
    }
    players.extend(net.remote_players().into_iter().map(|p| PlayerInfo { id: p.id, name: p.name.clone(), pos: p.pos }));
    scripts.players = players.clone();
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
    for (pid, v) in std::mem::take(&mut scripts.out.throws) {
        herd.riders.remove(&pid);
        if pid == my_id && me.is_some() {
            throw(me.as_mut().unwrap(), v);
        } else {
            net.throw(pid, v);
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

pub fn labels(herd: Res<Herd>, net: Res<Net>, mut labels: ResMut<Labels>) {
    labels.0.clear();
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
    mut exit: MessageWriter<AppExit>,
) {
    for act in std::mem::take(&mut acts.0) {
        match act {
            Act::Resume => focus.go(Screen::Playing),
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
                match net.join(&addr, &settings.name) {
                    Ok(()) => focus.status = format!("Joining {addr}..."),
                    Err(e) => focus.status = format!("Couldn't join {addr}: {e}"),
                }
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
                if let Some(c) = lib.list.get(&id).filter(|c| c.editable()) {
                    focus.status = "Sharing...".into();
                    let code = lib.marks.get(&id).filter(|m| m.version > 0).map(|m| m.code.clone());
                    shares.share(&Who::of(&settings), c.payload(), everyone, code, c.manifest.from.clone());
                }
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
            Act::OpenUrl(url) => {
                open_link(&url);
                // a link works once: opening it here uses it up
                focus.link = None;
                focus.status = "Opened in your browser.".into();
            }
            Act::Run(id) => {
                bring(&id, &mut lib, &mut scripts, &mut herd, &mut net, &mut toasts);
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
                    Screen::Inside(_) | Screen::Library if info.in_world => Screen::Playing,
                    _ if info.in_world => Screen::Pause,
                    _ => Screen::Title,
                };
                settings.save();
                focus.go(to);
            }
            Act::Leave => {
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
}

/// The creations a fresh world starts with: the base game's.
pub fn start_world(lib: &mut Library, scripts: &mut Scripts, herd: &mut Herd, net: &mut Net, toasts: &mut Toasts) {
    lib.running.clear();
    scripts.vms.clear();
    let base: Vec<String> = lib.list.values().filter(|c| c.origin == Origin::Base).map(|c| c.id.clone()).collect();
    for id in base {
        bring(&id, lib, scripts, herd, net, toasts);
    }
}

/// Finished share jobs: a code to show, or a creation to bring in.
#[allow(clippy::too_many_arguments)]
pub fn share_results(
    shares: Res<ShareJobs>,
    mut focus: ResMut<Focus>,
    mut lib: ResMut<Library>,
    mut scripts: ResMut<Scripts>,
    mut herd: ResMut<Herd>,
    mut net: ResMut<Net>,
    mut toasts: ResMut<Toasts>,
    info: Res<WorldInfo>,
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
                    focus.go(Screen::Inside(id));
                }
                Err(e) => focus.status = format!("Couldn't load it: {e}"),
            },
            Done::Link { url, title } => {
                focus.status.clear();
                focus.link = Some((url, title));
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
            Done::Said(s) => {
                focus.status = s.clone();
                toasts.push(s);
            }
            Done::Failed(e) => {
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
