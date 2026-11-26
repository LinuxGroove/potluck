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
pub const PROTOCOL: u32 = 1;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PlayerSnap {
    pub id: u32,
    pub name: String,
    pub pos: [f32; 3],
    pub yaw: f32,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BodySnap {
    pub id: u64,
    pub creation: String,
    pub kind: String,
    pub pos: [f32; 3],
    pub yaw: f32,
    pub phase: f32,
    pub rider: Option<u32>,
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
    Hello { name: String, protocol: u32 },
    Welcome { id: u32, seed: u32, time: f32, host: String },
    Refused(String),
    Creation(Payload),
    Stop(String),
    Blocks(Vec<(i32, i32, i32, u8)>),
    Me { pos: [f32; 3], yaw: f32 },
    Players(Vec<PlayerSnap>),
    Herd(Vec<BodySnap>),
    Looks(Vec<Look>),
    Act(Action),
    Thrown([f32; 3]),
    Toast(String),
    Tweak { id: String, knobs: BTreeMap<String, serde_json::Value> },
}

pub(crate) enum Event {
    Msg(u32, Msg),
    World(u32, Vec<u8>),
    Joined(u32, Sender<Vec<u8>>),
    Gone(u32),
}

pub(crate) fn decode(w: Wire) -> Option<Event> {
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

pub(crate) fn frame_msg(m: &Msg) -> Vec<u8> {
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
}

pub struct Remote {
    pub id: u32,
    pub name: String,
    pub pos: Vec3,
    pub yaw: f32,
    out: Option<Sender<Vec<u8>>>,
    welcomed: bool,
}

pub struct HostNet {
    events: Mutex<Receiver<Wire>>,
    status: Arc<Mutex<String>>,
    name: String,
    since_status: f32,
    players: BTreeMap<u32, Remote>,
    sent_revs: HashMap<u64, u32>,
    stop_beacon: Arc<Mutex<bool>>,
}

pub struct ClientNet {
    events: Mutex<Receiver<Wire>>,
    out: Sender<Vec<u8>>,
    pub host_name: String,
    players: BTreeMap<u32, Remote>,
    welcomed: bool,
    addr: String,
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
        self.host_with(HostOptions { name: name.to_string(), port: PORT, tls: None, beacon: true })
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
        self.role = Role::Host(HostNet { events: Mutex::new(rx), status, name: o.name, since_status: 9.0, players: BTreeMap::new(), sent_revs: HashMap::new(), stop_beacon: stop });
        self.my_id = 1;
        Ok(format!("{}:{}", local_ip(), o.port))
    }

    pub fn join(&mut self, addr: &str, name: &str) -> Result<(), String> {
        self.leave();
        let (tx, rx) = channel();
        let out = transport::connect(addr, PORT, tx)?;
        let _ = out.send(frame_msg(&Msg::Hello { name: name.to_string(), protocol: PROTOCOL }));
        self.role = Role::Client(ClientNet { events: Mutex::new(rx), out, host_name: String::new(), players: BTreeMap::new(), welcomed: false, addr: addr.to_string() });
        Ok(())
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
                    if tx.send((format!("{}'s world", parts[1]), format!("{}:{}", from.ip(), parts[2]))).is_err() {
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

    fn broadcast(&self, m: &Msg) {
        if let Role::Host(h) = &self.role {
            let f = frame_msg(m);
            for p in h.players.values().filter(|p| p.welcomed) {
                if let Some(o) = &p.out {
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
            pos: b.pos.to_array(),
            yaw: b.yaw,
            phase: b.phase,
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
    for ev in events {
        match ev {
            Event::Joined(id, out) => {
                if let Role::Host(h) = &mut net.role {
                    h.players.insert(id, Remote { id, name: format!("guest {id}"), pos: Vec3::ZERO, yaw: 0.0, out: Some(out), welcomed: false });
                }
            }
            Event::Gone(id) => {
                let name = match &mut net.role {
                    Role::Host(h) => h.players.remove(&id).map(|p| p.name),
                    _ => None,
                };
                if let Some(n) = name {
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
                Msg::Hello { name, protocol } => {
                    let Role::Host(h) = &mut net.role else { continue };
                    let Some(p) = h.players.get_mut(&id) else { continue };
                    let Some(out) = p.out.clone() else { continue };
                    if protocol != PROTOCOL {
                        let _ = out.send(frame_msg(&Msg::Refused("This world runs a different version of Potluck.".into())));
                        continue;
                    }
                    p.name = name.chars().take(20).collect();
                    // the world, then everything in it
                    let _ = out.send(frame_msg(&Msg::Welcome { id, seed: voxels.seed, time: clock.time, host: settings.name.clone() }));
                    let _ = out.send(frame_world(&voxels.encode()));
                    for cid in &lib.running {
                        if let Some(c) = lib.list.get(cid) {
                            let _ = out.send(frame_msg(&Msg::Creation(c.payload())));
                        }
                    }
                    let _ = out.send(frame_msg(&Msg::Looks(herd.bodies.values().map(look_of).collect())));
                    p.welcomed = true;
                    toasts.push(format!("{} joined", p.name));
                }
                Msg::Blocks(list) => {
                    for (x, y, z, b) in list {
                        voxels.set(IVec3::new(x, y, z), b);
                    }
                }
                Msg::Me { pos, yaw } => {
                    if let Role::Host(h) = &mut net.role {
                        if let Some(p) = h.players.get_mut(&id) {
                            p.pos = Vec3::from_array(pos);
                            p.yaw = yaw;
                        }
                    }
                }
                Msg::Act(a) => actions.0.push((id, a)),
                Msg::Creation(p) => {
                    let who = match &net.role {
                        Role::Host(h) => h.players.get(&id).map(|p| p.name.clone()).unwrap_or_default(),
                        _ => String::new(),
                    };
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
                    if let Some(c) = lib.list.get_mut(&cid) {
                        c.knobs = knobs.iter().filter_map(|(k, v)| Some((k.clone(), crate::creations::knob_from_json(v)?))).collect();
                        crate::game::bring(&cid, &mut lib, &mut scripts, &mut herd, &mut net, &mut toasts);
                    }
                }
                _ => {}
            },
        }
    }
    // twenty times a second: blocks, players and creatures
    net.since_send += time.delta_secs();
    let changes: Vec<(i32, i32, i32, u8)> = voxels.changes.drain(..).map(|(p, b)| (p.x, p.y, p.z, b)).collect();
    if !changes.is_empty() {
        net.broadcast(&Msg::Blocks(changes));
    }
    if net.since_send >= 0.05 {
        net.since_send = 0.0;
        let mut players = vec![];
        if let Some(me) = &me {
            players.push(PlayerSnap { id: 1, name: settings.name.clone(), pos: me.pos.to_array(), yaw: me.yaw });
        }
        players.extend(net.remote_players().iter().map(|p| PlayerSnap { id: p.id, name: p.name.clone(), pos: p.pos.to_array(), yaw: p.yaw }));
        net.broadcast(&Msg::Players(players));
        let looks: Vec<Look> = match &net.role {
            Role::Host(h) => herd.bodies.values().filter(|b| h.sent_revs.get(&b.id) != Some(&b.rev)).map(look_of).collect(),
            _ => vec![],
        };
        if !looks.is_empty() {
            if let Role::Host(h) = &mut net.role {
                for l in &looks {
                    h.sent_revs.insert(l.id, l.rev);
                }
            }
            net.broadcast(&Msg::Looks(looks));
        }
        net.broadcast(&Msg::Herd(snap_bodies(&herd)));
    }
    let n = net.remote_players().len();
    info.line = format!("Hosting at {}  ·  {} {}", local_ip(), n, if n == 1 { "friend" } else { "friends" });
    let names: Vec<String> = net.remote_players().iter().map(|p| p.name.clone()).collect();
    let creations: Vec<String> = lib.running.iter().filter_map(|id| lib.list.get(id)).map(|c| format!("{} by {}", c.manifest.name, c.manifest.by)).collect();
    if let Role::Host(h) = &mut net.role {
        h.since_status += time.delta_secs();
        if h.since_status > 1.0 {
            h.since_status = 0.0;
            let mut all = names;
            if me.is_some() {
                all.insert(0, settings.name.clone());
            }
            *h.status.lock().unwrap() = serde_json::json!({ "name": h.name, "players": all, "creations": creations, "version": crate::share::version() }).to_string();
        }
    }
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
                    toasts.push("Lost the connection to your friend's world.");
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
                    toasts.push(format!("Welcome to {host}'s world"));
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
                    for (x, y, z, b) in list {
                        voxels.apply(IVec3::new(x, y, z), b);
                    }
                }
                Msg::Players(list) => {
                    let my = net.my_id;
                    if let Role::Client(c) = &mut net.role {
                        c.players = list
                            .into_iter()
                            .filter(|p| p.id != my)
                            .map(|p| (p.id, Remote { id: p.id, name: p.name, pos: Vec3::from_array(p.pos), yaw: p.yaw, out: None, welcomed: true }))
                            .collect();
                    }
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
                Msg::Herd(list) => {
                    let ids: std::collections::HashSet<u64> = list.iter().map(|b| b.id).collect();
                    herd.bodies.retain(|id, _| ids.contains(id));
                    for s in list {
                        let b = herd.bodies.entry(s.id).or_insert_with(|| blank_body(s.id));
                        b.creation = s.creation;
                        b.kind = s.kind;
                        b.pos = Vec3::from_array(s.pos);
                        b.yaw = s.yaw;
                        b.phase = s.phase;
                        b.rider = s.rider;
                        b.says = s.says.map(|t| (t, 1.0));
                    }
                }
                Msg::Thrown(v) => throw(&mut me, Vec3::from_array(v)),
                Msg::Toast(t) => toasts.push(t),
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
        info.line = format!("In {}'s world  ·  {} here", c.host_name, c.players.len() + 1);
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
