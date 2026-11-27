//! Friends' worlds: one player hosts, the rest join over the network. The
//! host owns the world and runs every creation; when someone joins they get
//! the world and its creations, so there's nothing to install. Messages are
//! JSON in length-prefixed frames over TCP, and hosts announce themselves on
//! the local network with a UDP beacon.

use std::collections::{BTreeMap, HashMap};
use std::net::UdpSocket;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::creations::{Creation, Library, Origin, Payload};
use crate::creatures::{Body, Herd, Part};
use crate::game::{throw, Action, Actions};
use crate::mesh::ChunkEntities;
use crate::player::Me;
use crate::render::Clock;
use crate::script::Scripts;
use crate::settings::Settings;
use crate::ui::{Focus, Screen, Toasts, WorldInfo};
use crate::transport::{self, TlsFiles, Wire};
use crate::world::Voxels;

pub const PORT: u16 = 47126;
const BEACON: u16 = 47127;
pub const PROTOCOL: u32 = 6;
/// How far away (in blocks, across the ground) a player hears about
/// creatures and other players.
pub const NEAR: f32 = 144.0;
/// Beyond this, players and creatures are sent a quarter as often.
const CLOSE: f32 = 32.0;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct PlayerSnap {
    pub id: u32,
    /// Sent when a player first comes into view, or changes name.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    pub pos: [f32; 3],
    pub yaw: f32,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct BodySnap {
    pub id: u64,
    /// Sent when a creature first comes into view.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub creation: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub kind: String,
    pub pos: [f32; 3],
    pub yaw: f32,
    pub phase: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rider: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub says: Option<String>,
    pub rev: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Look {
    pub id: u64,
    pub rev: u32,
    pub parts: Vec<Part>,
    pub half: [f32; 3],
    pub seat: f32,
    pub rideable: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum Msg {
    /// `pass` is the market's word on who this is, for worlds that ask.
    Hello {
        name: String,
        protocol: u32,
        #[serde(default)]
        pass: Option<String>,
    },
    Welcome { id: u32, seed: u32, time: f32, host: String },
    Refused(String),
    Creation(Payload),
    Stop(String),
    Blocks(Vec<(i32, i32, i32, u8)>),
    Me { pos: [f32; 3], yaw: f32 },
    /// Players near you that moved, and those gone from view.
    People { list: Vec<PlayerSnap>, gone: Vec<u32> },
    /// Creatures near you that changed, and those gone from view.
    Bodies { list: Vec<BodySnap>, gone: Vec<u64> },
    /// Everyone in the world, by name, now and then.
    Roster(Vec<String>),
    /// Off to another server, with what the creation said about it.
    Go { address: String, note: String },
    Looks(Vec<Look>),
    Act(Action),
    Thrown([f32; 3]),
    Toast(String),
    Tweak { id: String, knobs: BTreeMap<String, serde_json::Value> },
    /// The creation that has your controls now (`input.focus`), or none.
    Focus(Option<String>),
    /// A line a creation shows on your screen (`ui.show`), or hides with None.
    Piece { creation: String, id: String, piece: Option<crate::script::Piece> },
    /// The question a creation asks you now (`ui.ask`), or none.
    Panel(Option<crate::script::Panel>),
    /// A sound a creation played near you, or for you (`sound.play`).
    Sound(crate::sound::Play),
}

pub enum Event {
    Msg(u32, Msg),
    World(u32, Vec<u8>),
    Joined(u32, Sender<Vec<u8>>),
    Gone(u32),
}

pub fn decode(w: Wire) -> Option<Event> {
    match w {
        Wire::Joined(id, s) => Some(Event::Joined(id, s)),
        Wire::Gone(id) => Some(Event::Gone(id)),
        Wire::Data(id, b) => match b.first() {
            Some(1) => Some(Event::World(id, b[1..].to_vec())),
            Some(0) => serde_json::from_slice(&b[1..]).ok().map(|m| Event::Msg(id, m)),
            _ => None,
        },
    }
}

pub fn frame_msg(m: &Msg) -> Vec<u8> {
    let mut out = vec![0u8];
    out.extend_from_slice(&serde_json::to_vec(m).unwrap_or_default());
    out
}

fn frame_world(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len() + 1);
    out.push(1);
    out.extend_from_slice(bytes);
    out
}

/// How to open a world to others.
pub struct HostOptions {
    pub name: String,
    pub port: u16,
    /// A certificate, its key and the port to serve TLS (wss and https) on.
    pub tls: Option<(PathBuf, PathBuf, u16)>,
    /// Announce it on the local network.
    pub beacon: bool,
    pub rules: crate::hosting::Rules,
}

pub struct Remote {
    pub id: u32,
    pub name: String,
    /// The market handle, when the player came with a pass.
    pub handle: Option<String>,
    pub pos: Vec3,
    pub yaw: f32,
    out: Option<Sender<Vec<u8>>>,
    welcomed: bool,
    /// What this player has been told, so only changes go out.
    seen: Seen,
}

#[derive(Default)]
struct Seen {
    bodies: HashMap<u64, BodySnap>,
    revs: HashMap<u64, u32>,
    players: HashMap<u32, PlayerSnap>,
    /// sends so far, for sending far things less often
    tick: u32,
}

/// Centimetres are plenty, and shorter to send.
fn r2(v: f32) -> f32 {
    (v * 100.0).round() / 100.0
}

fn r2v(v: Vec3) -> [f32; 3] {
    [r2(v.x), r2(v.y), r2(v.z)]
}

fn remote(id: u32, name: String, out: Option<Sender<Vec<u8>>>) -> Remote {
    Remote { id, name, handle: None, pos: Vec3::ZERO, yaw: 0.0, out, welcomed: false, seen: Seen::default() }
}

pub struct HostNet {
    events: Mutex<Receiver<Wire>>,
    status: Arc<Mutex<String>>,
    name: String,
    since_status: f32,
    players: BTreeMap<u32, Remote>,
    stop_beacon: Arc<Mutex<bool>>,
    pub rules: crate::hosting::Rules,
    since_roster: f32,
    /// Bytes sent this second, and the last whole second's.
    sent: (usize, usize, f32),
}

pub struct ClientNet {
    events: Mutex<Receiver<Wire>>,
    out: Sender<Vec<u8>>,
    pub host_name: String,
    players: BTreeMap<u32, Remote>,
    welcomed: bool,
    addr: String,
    /// Everyone in the world, as the host last said.
    pub roster: Vec<String>,
}

pub enum Role {
    Solo,
    Host(HostNet),
    Client(ClientNet),
}

#[derive(Resource)]
pub struct Net {
    pub role: Role,
    pub my_id: u32,
    lan: Option<Mutex<Receiver<(String, String)>>>,
    since_send: f32,
    /// Messages to send at the next chance (host: to everyone).
    pending: Vec<Msg>,
}

impl Default for Net {
    fn default() -> Self {
        Self { role: Role::Solo, my_id: 1, lan: None, since_send: 0.0, pending: vec![] }
    }
}

/// This machine's address on the local network, for telling friends.
pub fn local_ip() -> String {
    UdpSocket::bind("0.0.0.0:0")
        .and_then(|s| {
            s.connect("10.255.255.255:1")?;
            s.local_addr()
        })
        .map(|a| a.ip().to_string())
        .unwrap_or_else(|_| "127.0.0.1".into())
}

impl Net {
    pub fn remote_players(&self) -> Vec<&Remote> {
        match &self.role {
            Role::Solo => vec![],
            Role::Host(h) => h.players.values().filter(|p| p.welcomed).collect(),
            Role::Client(c) => c.players.values().collect(),
        }
    }

    pub fn host(&mut self, name: &str) -> Result<String, String> {
        if let Role::Host(_) = self.role {
            return Ok(format!("{}:{PORT}", local_ip()));
        }
        self.host_with(HostOptions { name: format!("{name}'s world"), port: PORT, tls: None, beacon: true, rules: Default::default() })
    }

    pub fn host_with(&mut self, o: HostOptions) -> Result<String, String> {
        self.leave();
        let (tx, rx) = channel();
        let status = Arc::new(Mutex::new("{}".to_string()));
        let next = Arc::new(Mutex::new(1u32));
        if let Some((cert, key, tls_port)) = &o.tls {
            let cfg = transport::tls_config(&TlsFiles { cert, key })?;
            transport::listen(*tls_port, Some(cfg), tx.clone(), status.clone(), next.clone())?;
        }
        transport::listen(o.port, None, tx, status.clone(), next)?;
        let stop = Arc::new(Mutex::new(false));
        if o.beacon {
            // tell the local network every second
            let stop2 = stop.clone();
            let label = format!("POTLUCK{PROTOCOL}|{}|{}", o.name, o.port);
            std::thread::spawn(move || {
                let Ok(s) = UdpSocket::bind("0.0.0.0:0") else { return };
                let _ = s.set_broadcast(true);
                while !*stop2.lock().unwrap() {
                    let _ = s.send_to(label.as_bytes(), ("255.255.255.255", BEACON));
                    std::thread::sleep(Duration::from_secs(1));
                }
            });
        }
        self.role = Role::Host(HostNet { events: Mutex::new(rx), status, name: o.name, since_status: 9.0, players: BTreeMap::new(), stop_beacon: stop, rules: o.rules, since_roster: 9.0, sent: (0, 0, 0.0) });
        self.my_id = 1;
        Ok(format!("{}:{}", local_ip(), o.port))
    }

    pub fn join(&mut self, addr: &str, name: &str, pass: Option<String>) -> Result<(), String> {
        self.leave();
        let (tx, rx) = channel();
        let out = transport::connect(addr, PORT, tx)?;
        let _ = out.send(frame_msg(&Msg::Hello { name: name.to_string(), protocol: PROTOCOL, pass }));
        self.role = Role::Client(ClientNet { events: Mutex::new(rx), out, host_name: String::new(), players: BTreeMap::new(), welcomed: false, addr: addr.to_string(), roster: vec![] });
        Ok(())
    }

    /// Sends a player to another server.
    pub fn send_away(&self, pid: u32, address: &str, note: &str) {
        if let Role::Host(h) = &self.role {
            if let Some(o) = h.players.get(&pid).and_then(|p| p.out.as_ref()) {
                let _ = o.send(frame_msg(&Msg::Go { address: address.to_string(), note: note.to_string() }));
            }
        }
    }

    /// A remote player's market handle, if they came with a pass.
    pub fn handle_of(&self, pid: u32) -> Option<String> {
        match &self.role {
            Role::Host(h) => h.players.get(&pid).and_then(|p| p.handle.clone()),
            _ => None,
        }
    }

    pub fn rules(&self) -> Option<&crate::hosting::Rules> {
        match &self.role {
            Role::Host(h) => Some(&h.rules),
            _ => None,
        }
    }

    pub fn leave(&mut self) {
        if let Role::Host(h) = &self.role {
            *h.stop_beacon.lock().unwrap() = true;
        }
        self.role = Role::Solo;
        self.my_id = 1;
    }

    /// Starts listening for hosts' beacons on the local network.
    pub fn listen_lan(&mut self) {
        if self.lan.is_some() {
            return;
        }
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let Ok(s) = UdpSocket::bind(("0.0.0.0", BEACON)) else { return };
            let mut buf = [0u8; 256];
            while let Ok((n, from)) = s.recv_from(&mut buf) {
                let text = String::from_utf8_lossy(&buf[..n]).to_string();
                let parts: Vec<&str> = text.split('|').collect();
                if parts.len() == 3 && parts[0] == format!("POTLUCK{PROTOCOL}") {
                    if tx.send((parts[1].to_string(), format!("{}:{}", from.ip(), parts[2]))).is_err() {
                        break;
                    }
                }
            }
        });
        self.lan = Some(Mutex::new(rx));
    }

    fn send_host(&self, m: &Msg) {
        if let Role::Client(c) = &self.role {
            let _ = c.out.send(frame_msg(m));
        }
    }

    fn broadcast(&mut self, m: &Msg) {
        if let Role::Host(h) = &mut self.role {
            let f = frame_msg(m);
            for p in h.players.values().filter(|p| p.welcomed) {
                if let Some(o) = &p.out {
                    h.sent.0 += f.len();
                    let _ = o.send(f.clone());
                }
            }
        }
    }

    pub fn broadcast_creation(&mut self, c: &Creation) {
        self.pending.push(Msg::Creation(c.payload()));
    }
    pub fn broadcast_stop(&mut self, id: &str) {
        self.pending.push(Msg::Stop(id.to_string()));
    }
    pub fn broadcast_toast(&mut self, t: &str) {
        self.pending.push(Msg::Toast(t.to_string()));
    }
    pub fn send_creation(&self, c: &Creation) {
        self.send_host(&Msg::Creation(c.payload()));
    }
    pub fn send_tweak(&self, id: &str, knobs: &BTreeMap<String, crate::script::KnobValue>) {
        let knobs = knobs.iter().map(|(k, v)| (k.clone(), crate::creations::knob_to_json(v))).collect();
        self.send_host(&Msg::Tweak { id: id.to_string(), knobs });
    }
    pub fn throw(&self, pid: u32, v: Vec3) {
        if let Role::Host(h) = &self.role {
            if let Some(o) = h.players.get(&pid).and_then(|p| p.out.as_ref()) {
                let _ = o.send(frame_msg(&Msg::Thrown(v.to_array())));
            }
        }
    }
}

fn snap_bodies(herd: &Herd) -> Vec<BodySnap> {
    herd.bodies
        .values()
        .map(|b| BodySnap {
            id: b.id,
            creation: b.creation.clone(),
            kind: b.kind.clone(),
            pos: r2v(b.pos),
            yaw: r2(b.yaw),
            phase: r2(b.phase),
            rider: b.rider,
            says: b.says.as_ref().map(|s| s.0.clone()),
            rev: b.rev,
        })
        .collect()
}

fn look_of(b: &Body) -> Look {
    Look { id: b.id, rev: b.rev, parts: b.parts.clone(), half: b.half.to_array(), seat: b.seat, rideable: b.rideable }
}

/// Worlds found on the local network show in the Join screen.
pub fn lan_list(net: Res<Net>, mut focus: ResMut<Focus>) {
    if let Some(rx) = &net.lan {
        for (name, addr) in rx.lock().unwrap().try_iter() {
            if !focus.lan.iter().any(|(_, a)| *a == addr) {
                focus.lan.push((name, addr));
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn host_net(
    time: Res<Time>,
    mut net: ResMut<Net>,
    mut voxels: ResMut<Voxels>,
    mut herd: ResMut<Herd>,
    mut lib: ResMut<Library>,
    mut scripts: ResMut<Scripts>,
    mut actions: ResMut<Actions>,
    mut toasts: ResMut<Toasts>,
    me: Option<Res<Me>>,
    clock: Res<Clock>,
    settings: Res<Settings>,
    mut info: ResMut<WorldInfo>,
    ticks: Res<crate::hosting::TickTimes>,
    mut heard: ResMut<crate::sound::Heard>,
) {
    let pending = std::mem::take(&mut net.pending);
    let Role::Host(_) = &net.role else {
        voxels.changes.clear();
        if !matches!(net.role, Role::Client(_)) {
            info.line = String::new();
        }
        return;
    };
    for m in &pending {
        net.broadcast(m);
    }
    let events: Vec<Event> = match &net.role {
        Role::Host(h) => h.events.lock().unwrap().try_iter().filter_map(decode).collect(),
        _ => vec![],
    };
    // blocks a player may not change go back as they were, to them alone
    let mut undo: Vec<(u32, Vec<(i32, i32, i32, u8)>)> = vec![];
    for ev in events {
        match ev {
            Event::Joined(id, out) => {
                if let Role::Host(h) = &mut net.role {
                    h.players.insert(id, remote(id, format!("guest {id}"), Some(out)));
                }
            }
            Event::Gone(id) => {
                let gone = match &mut net.role {
                    Role::Host(h) => h.players.remove(&id).map(|p| (p.name, p.welcomed)),
                    _ => None,
                };
                if let Some((n, true)) = gone {
                    toasts.push(format!("{n} left"));
                    for b in herd.bodies.values_mut() {
                        if b.rider == Some(id) {
                            b.rider = None;
                        }
                    }
                    herd.riders.remove(&id);
                }
            }
            Event::World(..) => {}
            Event::Msg(id, m) => match m {
                Msg::Hello { name, protocol, pass } => {
                    let Role::Host(h) = &mut net.role else { continue };
                    let here = h.players.values().filter(|p| p.welcomed).count() + usize::from(me.is_some());
                    let rules = h.rules.clone();
                    let Some(p) = h.players.get_mut(&id) else { continue };
                    let Some(out) = p.out.clone() else { continue };
                    if p.welcomed {
                        continue;
                    }
                    if protocol != PROTOCOL {
                        let _ = out.send(frame_msg(&Msg::Refused("This world runs a different version of Potluck.".into())));
                        continue;
                    }
                    let (shown, handle) = match rules.admit(&name, pass.as_deref(), here) {
                        Ok(w) => w,
                        Err(why) => {
                            let _ = out.send(frame_msg(&Msg::Refused(why)));
                            continue;
                        }
                    };
                    p.name = shown;
                    p.handle = handle;
                    // where they'll appear, until they say where they are
                    p.pos = Vec3::new(crate::world::START.x, 0.0, crate::world::START.z);
                    // the world, then everything in it
                    let world = frame_world(&voxels.encode());
                    let mut sent = world.len();
                    let _ = out.send(frame_msg(&Msg::Welcome { id, seed: voxels.seed, time: clock.time, host: h.name.clone() }));
                    let _ = out.send(world);
                    for cid in &lib.running {
                        if let Some(c) = lib.list.get(cid) {
                            let f = frame_msg(&Msg::Creation(c.payload()));
                            sent += f.len();
                            let _ = out.send(f);
                        }
                    }
                    p.welcomed = true;
                    h.sent.0 += sent;
                    h.since_roster = 9.0;
                    toasts.push(format!("{} joined", p.name));
                }
                Msg::Blocks(list) => {
                    let build = net.rules().map(|r| r.build).unwrap_or(true);
                    if build {
                        let few = list.len() <= 8;
                        for (x, y, z, b) in list {
                            let p = IVec3::new(x, y, z);
                            if few {
                                block_sound(&mut heard, p, voxels.at(p), b);
                            }
                            voxels.set(p, b);
                        }
                    } else {
                        undo.push((id, list.iter().map(|&(x, y, z, _)| (x, y, z, voxels.at(IVec3::new(x, y, z)))).collect()));
                    }
                }
                Msg::Me { pos, yaw } => {
                    if let Role::Host(h) = &mut net.role {
                        if let Some(p) = h.players.get_mut(&id).filter(|p| p.welcomed) {
                            p.pos = Vec3::from_array(pos);
                            p.yaw = yaw;
                        }
                    }
                }
                Msg::Act(a) => actions.0.push((id, a)),
                Msg::Creation(p) => {
                    let (who, handle, welcomed) = match &net.role {
                        Role::Host(h) => h.players.get(&id).map(|p| (p.name.clone(), p.handle.clone(), p.welcomed)).unwrap_or_default(),
                        _ => Default::default(),
                    };
                    if !welcomed {
                        continue;
                    }
                    if !net.rules().is_none_or(|r| r.may_bring(handle.as_deref())) {
                        net.tell(id, "This world doesn't take creations from players.");
                        continue;
                    }
                    match Creation::from_payload(p, Origin::Friend(who.clone())) {
                        Ok(c) => {
                            let name = c.manifest.name.clone();
                            let cid = lib.add(c);
                            crate::game::bring(&cid, &mut lib, &mut scripts, &mut herd, &mut net, &mut toasts);
                            toasts.push(format!("{who} brought {name}"));
                        }
                        Err(e) => toasts.push(format!("{who}'s creation didn't load: {e}")),
                    }
                }
                Msg::Tweak { id: cid, knobs } => {
                    let handle = net.handle_of(id);
                    if !net.rules().is_none_or(|r| r.may_bring(handle.as_deref())) {
                        continue;
                    }
                    if let Some(c) = lib.list.get_mut(&cid) {
                        c.knobs = knobs.iter().filter_map(|(k, v)| Some((k.clone(), crate::creations::knob_from_json(v)?))).collect();
                        crate::game::bring(&cid, &mut lib, &mut scripts, &mut herd, &mut net, &mut toasts);
                    }
                }
                _ => {}
            },
        }
    }
    for (pid, list) in undo {
        net.tell(pid, "This world doesn't let players change blocks.");
        if let Role::Host(h) = &net.role {
            if let Some(o) = h.players.get(&pid).and_then(|p| p.out.as_ref()) {
                let _ = o.send(frame_msg(&Msg::Blocks(list)));
            }
        }
    }
    // twenty times a second: blocks, and what changed near each player
    net.since_send += time.delta_secs();
    let changes: Vec<(i32, i32, i32, u8)> = voxels.changes.drain(..).map(|(p, b)| (p.x, p.y, p.z, b)).collect();
    if !changes.is_empty() {
        net.broadcast(&Msg::Blocks(changes));
    }
    if net.since_send >= 0.05 {
        net.since_send = 0.0;
        let mut everyone = vec![];
        if let Some(me) = &me {
            everyone.push(PlayerSnap { id: 1, name: settings.name.clone(), pos: r2v(me.pos), yaw: r2(me.yaw) });
        }
        everyone.extend(net.remote_players().iter().map(|p| PlayerSnap { id: p.id, name: p.name.clone(), pos: r2v(p.pos), yaw: r2(p.yaw) }));
        let bodies = snap_bodies(&herd);
        if let Role::Host(h) = &mut net.role {
            let mut sent = 0;
            for p in h.players.values_mut().filter(|p| p.welcomed) {
                let Some(out) = &p.out else { continue };
                for m in news_for(p.id, p.pos, &mut p.seen, &everyone, &bodies, &herd) {
                    let f = frame_msg(&m);
                    sent += f.len();
                    let _ = out.send(f);
                }
            }
            h.sent.0 += sent;
        }
    }
    let names: Vec<String> = net.remote_players().iter().map(|p| p.name.clone()).collect();
    let n = names.len();
    info.line = format!("Hosting at {}  ·  {} {}", local_ip(), n, if n == 1 { "friend" } else { "friends" });
    let creations: Vec<String> = lib.running.iter().filter_map(|id| lib.list.get(id)).map(|c| format!("{} by {}", c.manifest.name, c.manifest.by)).collect();
    let dt = time.delta_secs();
    let roster = {
        let Role::Host(h) = &mut net.role else { return };
        h.sent.2 += dt;
        if h.sent.2 >= 1.0 {
            h.sent = (0, h.sent.0, 0.0);
        }
        h.since_roster += dt;
        let mut all = names;
        if me.is_some() {
            all.insert(0, settings.name.clone());
        }
        h.since_status += dt;
        if h.since_status > 1.0 {
            h.since_status = 0.0;
            *h.status.lock().unwrap() = serde_json::json!({
                "name": h.name, "players": all, "max_players": h.rules.max_players, "creations": creations,
                "creatures": herd.bodies.len(), "version": crate::share::version(), "protocol": PROTOCOL,
                "tick_ms": ticks.average_ms(), "tick_ms_max": ticks.max_ms(), "bytes_per_second": h.sent.1,
                "up_seconds": ticks.up_seconds(),
            })
            .to_string();
        }
        if h.since_roster > 2.0 {
            h.since_roster = 0.0;
            Some(all)
        } else {
            None
        }
    };
    if let Some(all) = roster {
        net.broadcast(&Msg::Roster(all));
    }
}

impl Net {
    /// Tells a player which creation has their controls now.
    pub fn send_focus(&self, pid: u32, creation: Option<String>) {
        if let Role::Host(h) = &self.role {
            if let Some(o) = h.players.get(&pid).and_then(|p| p.out.as_ref()) {
                let _ = o.send(frame_msg(&Msg::Focus(creation)));
            }
        }
    }

    /// A sound for one player to hear.
    pub fn send_sound(&self, pid: u32, play: crate::sound::Play) {
        if let Role::Host(h) = &self.role {
            if let Some(o) = h.players.get(&pid).and_then(|p| p.out.as_ref()) {
                let _ = o.send(frame_msg(&Msg::Sound(play)));
            }
        }
    }

    /// What a creation shows one player, or asks them.
    pub fn send_ui(&self, pid: u32, change: crate::script::UiChange) {
        if let Role::Host(h) = &self.role {
            if let Some(o) = h.players.get(&pid).and_then(|p| p.out.as_ref()) {
                let m = match change {
                    crate::script::UiChange::Piece { creation, id, piece } => Msg::Piece { creation, id, piece },
                    crate::script::UiChange::Panel(p) => Msg::Panel(p),
                };
                let _ = o.send(frame_msg(&m));
            }
        }
    }

    /// A line for one player's screen.
    pub fn tell(&self, pid: u32, text: &str) {
        if let Role::Host(h) = &self.role {
            if let Some(o) = h.players.get(&pid).and_then(|p| p.out.as_ref()) {
                let _ = o.send(frame_msg(&Msg::Toast(text.to_string())));
            }
        }
    }

    /// Bytes sent to players in the last whole second.
    pub fn bytes_per_second(&self) -> usize {
        match &self.role {
            Role::Host(h) => h.sent.1,
            _ => 0,
        }
    }
}

fn near(a: Vec3, b: [f32; 3]) -> bool {
    let (dx, dz) = (a.x - b[0], a.z - b[2]);
    dx * dx + dz * dz <= NEAR * NEAR
}

/// What one player needs to hear: players and creatures near them that
/// changed since they were last told, those gone from view, and the look
/// of any creature they haven't seen in its current look.
fn news_for(me: u32, at: Vec3, seen: &mut Seen, everyone: &[PlayerSnap], bodies: &[BodySnap], herd: &Herd) -> Vec<Msg> {
    seen.tick = seen.tick.wrapping_add(1);
    let turn = seen.tick;
    // far things move on a quarter of the sends, spread over them
    let later = |id: u64, pos: [f32; 3]| {
        let (dx, dz) = (at.x - pos[0], at.z - pos[2]);
        dx * dx + dz * dz > CLOSE * CLOSE && (turn as u64 + id) % 4 != 0
    };
    let mut out = vec![];
    let mut list = vec![];
    let mut here = std::collections::HashSet::new();
    for p in everyone.iter().filter(|p| p.id != me && near(at, p.pos)) {
        here.insert(p.id);
        let old = seen.players.get(&p.id);
        if old.is_some() && later(p.id as u64, p.pos) {
            continue;
        }
        if old.is_none_or(|old| moved(old.pos, p.pos) || (old.yaw - p.yaw).abs() > 0.01 || old.name != p.name) {
            let mut snap = p.clone();
            if old.is_some_and(|o| o.name == p.name) {
                snap.name.clear();
            }
            seen.players.insert(p.id, p.clone());
            list.push(snap);
        }
    }
    let gone: Vec<u32> = seen.players.keys().filter(|id| !here.contains(id)).copied().collect();
    for id in &gone {
        seen.players.remove(id);
    }
    if !list.is_empty() || !gone.is_empty() {
        out.push(Msg::People { list, gone });
    }
    let mut list = vec![];
    let mut looks = vec![];
    let mut here = std::collections::HashSet::new();
    for b in bodies.iter().filter(|b| near(at, b.pos)) {
        here.insert(b.id);
        let known = seen.bodies.get(&b.id).is_some_and(|o| o.rev == b.rev && o.rider == b.rider && o.says == b.says);
        if known && later(b.id, b.pos) {
            continue;
        }
        if seen.revs.get(&b.id) != Some(&b.rev) {
            if let Some(body) = herd.bodies.get(&b.id) {
                looks.push(look_of(body));
            }
            seen.revs.insert(b.id, b.rev);
        }
        let old = seen.bodies.get(&b.id);
        let changed = old.is_none_or(|old| moved(old.pos, b.pos) || (old.yaw - b.yaw).abs() > 0.01 || old.rider != b.rider || old.says != b.says || old.rev != b.rev);
        if changed {
            let mut snap = b.clone();
            if old.is_some() {
                // what it is doesn't change
                snap.creation.clear();
                snap.kind.clear();
            }
            seen.bodies.insert(b.id, b.clone());
            list.push(snap);
        }
    }
    let gone: Vec<u64> = seen.bodies.keys().filter(|id| !here.contains(id)).copied().collect();
    for id in &gone {
        seen.bodies.remove(id);
        seen.revs.remove(id);
    }
    // the look first, so a new creature is never drawn as a blank
    if !looks.is_empty() {
        out.push(Msg::Looks(looks));
    }
    if !list.is_empty() || !gone.is_empty() {
        out.push(Msg::Bodies { list, gone });
    }
    out
}

fn moved(a: [f32; 3], b: [f32; 3]) -> bool {
    (a[0] - b[0]).abs() > 0.01 || (a[1] - b[1]).abs() > 0.01 || (a[2] - b[2]).abs() > 0.01
}

#[allow(clippy::too_many_arguments)]
pub fn client_net(
    mut commands: Commands,
    time: Res<Time>,
    mut net: ResMut<Net>,
    mut voxels: ResMut<Voxels>,
    mut herd: ResMut<Herd>,
    mut lib: ResMut<Library>,
    mut scripts: ResMut<Scripts>,
    mut actions: ResMut<Actions>,
    mut toasts: ResMut<Toasts>,
    mut me: ResMut<Me>,
    mut clock: ResMut<Clock>,
    mut chunks: ResMut<ChunkEntities>,
    mut focus: ResMut<Focus>,
    mut info: ResMut<WorldInfo>,
    mut acts: ResMut<crate::ui::Acts>,
    mut heard: ResMut<crate::sound::Heard>,
) {
    let Role::Client(_) = &net.role else { return };
    let events: Vec<Event> = match &net.role {
        Role::Client(c) => c.events.lock().unwrap().try_iter().filter_map(decode).collect(),
        _ => vec![],
    };
    for ev in events {
        match ev {
            Event::Gone(_) => {
                let (welcomed, addr) = match &net.role {
                    Role::Client(c) => (c.welcomed, c.addr.clone()),
                    _ => (false, String::new()),
                };
                if welcomed {
                    toasts.push("Lost the connection to the world.");
                    acts.0.push(crate::ui::Act::Leave);
                } else {
                    focus.status = format!("Couldn't join {addr}: nobody answered.");
                    net.leave();
                }
                return;
            }
            Event::World(_, bytes) => {
                if let Some(v) = Voxels::decode(voxels.seed, &bytes) {
                    *voxels = v;
                    for (_, slots) in chunks.map.drain() {
                        for e in slots.into_iter().flatten() {
                            commands.entity(e).despawn();
                        }
                    }
                    chunks.meshed = 0;
                    me.respawn(&voxels, None);
                    info.in_world = true;
                    focus.go(Screen::Playing);
                }
            }
            Event::Joined(..) => {}
            Event::Msg(_, m) => match m {
                Msg::Welcome { id, seed, time, host } => {
                    net.my_id = id;
                    voxels.seed = seed;
                    clock.time = time;
                    herd.bodies.clear();
                    lib.running.clear();
                    scripts.vms.clear();
                    if let Role::Client(c) = &mut net.role {
                        c.welcomed = true;
                        c.host_name = host.clone();
                    }
                    toasts.push(format!("Welcome to {host}"));
                }
                Msg::Refused(why) => {
                    focus.status = why;
                    net.leave();
                    return;
                }
                Msg::Creation(p) => {
                    let host = match &net.role { Role::Client(c) => c.host_name.clone(), _ => String::new() };
                    let pid = p.id.clone();
                    // my own creation coming back keeps its folder
                    let mine = lib.list.get(&pid).map(|c| c.origin == Origin::Mine).unwrap_or(false);
                    if mine {
                        if let Some(c) = lib.list.get_mut(&pid) {
                            c.knobs = p.knobs.iter().filter_map(|(k, v)| Some((k.clone(), crate::creations::knob_from_json(v)?))).collect();
                        }
                        if !lib.running.contains(&pid) {
                            lib.running.push(pid);
                        }
                    } else if let Ok(c) = Creation::from_payload(p, Origin::Friend(host)) {
                        let id = c.id.clone();
                        lib.list.insert(id.clone(), c);
                        if !lib.running.contains(&id) {
                            lib.running.push(id);
                        }
                    }
                }
                Msg::Stop(id) => lib.running.retain(|r| *r != id),
                Msg::Blocks(list) => {
                    // what others built and broke (my own come back unchanged)
                    let few = list.len() <= 8;
                    for (x, y, z, b) in list {
                        let p = IVec3::new(x, y, z);
                        if few {
                            block_sound(&mut heard, p, voxels.at(p), b);
                        }
                        voxels.apply(p, b);
                    }
                }
                Msg::People { list, gone } => {
                    let my = net.my_id;
                    if let Role::Client(c) = &mut net.role {
                        for id in gone {
                            c.players.remove(&id);
                        }
                        for p in list.into_iter().filter(|p| p.id != my) {
                            let r = c.players.entry(p.id).or_insert_with(|| Remote { welcomed: true, ..remote(p.id, String::new(), None) });
                            if !p.name.is_empty() {
                                r.name = p.name;
                            }
                            r.pos = Vec3::from_array(p.pos);
                            r.yaw = p.yaw;
                        }
                    }
                }
                Msg::Roster(all) => {
                    if let Role::Client(c) = &mut net.role {
                        c.roster = all;
                    }
                }
                Msg::Go { address, note } => {
                    if !note.is_empty() {
                        toasts.push(note);
                    }
                    if let Role::Client(c) = &mut net.role {
                        // leaving on purpose isn't losing the connection
                        c.welcomed = false;
                    }
                    acts.0.push(crate::ui::Act::JoinAddress(address));
                    return;
                }
                Msg::Looks(list) => {
                    for l in list {
                        let b = herd.bodies.entry(l.id).or_insert_with(|| blank_body(l.id));
                        b.parts = l.parts;
                        b.rev = l.rev;
                        b.half = Vec3::from_array(l.half);
                        b.seat = l.seat;
                        b.rideable = l.rideable;
                    }
                }
                Msg::Bodies { list, gone } => {
                    for id in gone {
                        herd.bodies.remove(&id);
                    }
                    for s in list {
                        let b = herd.bodies.entry(s.id).or_insert_with(|| blank_body(s.id));
                        if !s.creation.is_empty() {
                            b.creation = s.creation;
                            b.kind = s.kind;
                        }
                        b.pos = Vec3::from_array(s.pos);
                        b.yaw = s.yaw;
                        b.phase = s.phase;
                        b.rider = s.rider;
                        b.says = s.says.map(|t| (t, 1.0));
                    }
                }
                Msg::Thrown(v) => throw(&mut me, Vec3::from_array(v)),
                Msg::Toast(t) => toasts.push(t),
                Msg::Focus(c) => me.focus = c,
                Msg::Piece { creation, id, piece } => me.show(crate::script::UiChange::Piece { creation, id, piece }),
                Msg::Panel(p) => me.show(crate::script::UiChange::Panel(p)),
                Msg::Sound(p) => heard.0.push(p),
                _ => {}
            },
        }
    }
    // send what I did
    let changes: Vec<(i32, i32, i32, u8)> = voxels.changes.drain(..).map(|(p, b)| (p.x, p.y, p.z, b)).collect();
    if !changes.is_empty() {
        net.send_host(&Msg::Blocks(changes));
    }
    net.since_send += time.delta_secs();
    let tick = net.since_send >= 0.05;
    for (_, a) in std::mem::take(&mut actions.0) {
        if matches!(a, Action::Ride(..)) && !tick {
            continue;
        }
        net.send_host(&Msg::Act(a));
    }
    if tick {
        net.since_send = 0.0;
        net.send_host(&Msg::Me { pos: me.pos.to_array(), yaw: me.yaw });
    }
    if let Role::Client(c) = &net.role {
        info.line = format!("In {}  ·  {} here", c.host_name, c.roster.len().max(c.players.len() + 1));
    }
}

fn blank_body(id: u64) -> Body {
    Body {
        id,
        creation: String::new(),
        kind: String::new(),
        pos: Vec3::ZERO,
        vel: Vec3::ZERO,
        yaw: 0.0,
        half: Vec3::new(0.4, 1.0, 0.4),
        goal: None,
        wander: false,
        wander_in: 0.0,
        home: Vec3::ZERO,
        speed: 0.0,
        rider: None,
        rideable: false,
        seat: 1.0,
        says: None,
        on_ground: true,
        face: None,
        parts: vec![],
        rev: 0,
        phase: 0.0,
    }
}

// ---------- other players' bodies ----------

#[derive(Component)]
pub struct PlayerView(u32);

/// A person made of boxes, for other players and for yourself seen from
/// behind.
fn spawn_person<'a>(commands: &'a mut Commands, meshes: &mut Assets<Mesh>, mats: &mut Assets<StandardMaterial>, hue: f32, pos: Vec3) -> EntityCommands<'a> {
    let shirt = mats.add(StandardMaterial { base_color: Color::hsl(hue, 0.55, 0.55), perceptual_roughness: 0.9, ..default() });
    let skin = mats.add(StandardMaterial { base_color: Color::srgb(0.86, 0.68, 0.52), perceptual_roughness: 0.9, ..default() });
    let legs = mats.add(StandardMaterial { base_color: Color::srgb(0.2, 0.25, 0.4), perceptual_roughness: 0.9, ..default() });
    let cube = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    let mut e = commands.spawn((Transform::from_translation(pos), Visibility::default()));
    e.with_children(|c| {
        c.spawn((Mesh3d(cube.clone()), MeshMaterial3d(legs.clone()), Transform::from_xyz(0.0, 0.4, 0.0).with_scale(Vec3::new(0.5, 0.8, 0.28))));
        c.spawn((Mesh3d(cube.clone()), MeshMaterial3d(shirt.clone()), Transform::from_xyz(0.0, 1.15, 0.0).with_scale(Vec3::new(0.56, 0.7, 0.3))));
        c.spawn((Mesh3d(cube.clone()), MeshMaterial3d(skin.clone()), Transform::from_xyz(0.0, 1.72, 0.0).with_scale(Vec3::new(0.46, 0.46, 0.46))));
    });
    e
}

pub fn draw_players(
    mut commands: Commands,
    net: Res<Net>,
    mut views: Query<(Entity, &PlayerView, &mut Transform)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
) {
    let players = net.remote_players();
    let mut seen = std::collections::HashSet::new();
    for (e, v, mut t) in &mut views {
        match players.iter().find(|p| p.id == v.0) {
            Some(p) => {
                seen.insert(p.id);
                t.translation = t.translation.lerp(p.pos, 0.4);
                t.rotation = Quat::from_rotation_y(p.yaw);
            }
            None => commands.entity(e).despawn(),
        }
    }
    for p in players {
        if seen.contains(&p.id) {
            continue;
        }
        let hue = (p.id as f32 * 67.0) % 360.0;
        spawn_person(&mut commands, &mut meshes, &mut mats, hue, p.pos).insert(PlayerView(p.id));
    }
}

#[derive(Component)]
pub struct MyView;

/// You, when the camera is behind you.
pub fn draw_me(
    mut commands: Commands,
    me: Res<Me>,
    mut view: Query<(&mut Transform, &mut Visibility), With<MyView>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
) {
    let Ok((mut t, mut vis)) = view.single_mut() else {
        spawn_person(&mut commands, &mut meshes, &mut mats, 200.0, me.pos).insert((MyView, Visibility::Hidden));
        return;
    };
    t.translation = me.pos;
    t.rotation = Quat::from_rotation_y(me.yaw);
    let want = if me.third_person { Visibility::Inherited } else { Visibility::Hidden };
    if *vis != want {
        *vis = want;
    }
}

/// The sound of someone else's block changing: breaking, or placing.
fn block_sound(heard: &mut crate::sound::Heard, p: IVec3, old: u8, new: u8) {
    use crate::sound::material;
    if old == new {
        return;
    }
    let at = p.as_vec3() + Vec3::splat(0.5);
    if new == crate::blocks::AIR {
        if let Some(n) = material(old).breaks() {
            heard.game(n, at, 0.6, 1.0);
        }
    } else {
        heard.game("place", at, 0.5, material(new).knock());
    }
}
