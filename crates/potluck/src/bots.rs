//! potluck-bots: players that aren't people, for soak tests. Each one joins
//! a server, walks about near the start on top of the ground, now and then
//! builds or breaks a block and pats or rides a creature, and reads all
//! the server sends. Bots that lose the connection join again. Every minute
//! it prints how the server is doing (from its status page) and appends a
//! line of JSON to --report.
//!
//!     potluck-bots --address=ws://127.0.0.1:47126 [--bots=50] [--minutes=60]
//!                  [--build] [--ramp=0.5] [--report=soak.jsonl] [--pid=SERVER_PID]

use std::collections::BTreeMap;
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::time::{Duration, Instant};

use bevy::math::{Vec2, Vec3};
use serde_json::{json, Value};

use potluck::game::Action;
use potluck::net::{self, Event, Msg};
use potluck::noise::Rng;
use potluck::transport::{self, Wire};
use potluck::world::{self, Voxels};

const HELP: &str = "Options:
  --address=ws://127.0.0.1:47126   the server
  --bots=50                        how many
  --minutes=60                     how long (0: until stopped)
  --build                          place and break blocks now and then
  --ramp=0.5                       seconds between bots joining
  --every=60                       seconds between reports
  --report=FILE                    append a line of JSON each report
  --pid=PID                        the server's process, for its memory";

fn arg(args: &[String], key: &str) -> Option<String> {
    args.iter().find_map(|a| a.strip_prefix(&format!("--{key}=")).map(|s| s.to_string()))
}

#[derive(Default)]
struct Counts {
    bytes: u64,
    msgs: u64,
    joins: u64,
    drops: u64,
    refused: u64,
    blocks: u64,
    acts: u64,
}

struct Bot {
    name: String,
    out: Option<Sender<Vec<u8>>>,
    rx: Option<Receiver<Wire>>,
    welcomed: bool,
    pos: Vec3,
    target: Vec3,
    yaw: f32,
    /// creatures in view, by id, and where
    near: BTreeMap<u64, Vec3>,
    riding: Option<(u64, f32)>,
    next_build: f32,
    placed: Option<(i32, i32, i32)>,
    next_act: f32,
    /// standing about, as people do
    wait: f32,
    /// seconds until trying to join again
    retry: f32,
}

struct Ground {
    voxels: Option<Voxels>,
    seed: u32,
}

impl Ground {
    fn feet(&self, x: f32, z: f32) -> f32 {
        match &self.voxels {
            Some(v) => v.top(x.floor() as i32, z.floor() as i32) as f32 + 1.0,
            None => 40.0,
        }
    }
}

fn status_url(address: &str) -> String {
    let a = address.trim().trim_end_matches('/');
    if let Some(r) = a.strip_prefix("wss://") {
        format!("https://{r}/status.json")
    } else if let Some(r) = a.strip_prefix("ws://") {
        format!("http://{r}/status.json")
    } else if a.contains(':') {
        format!("http://{a}/status.json")
    } else {
        format!("http://{a}:{}/status.json", net::PORT)
    }
}

fn rss_mb(pid: &str) -> Option<f64> {
    let s = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    let kb: f64 = s.lines().find_map(|l| l.strip_prefix("VmRSS:"))?.trim().trim_end_matches("kB").trim().parse().ok()?;
    Some((kb / 1024.0 * 10.0).round() / 10.0)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("potluck-bots\n\n{HELP}");
        return;
    }
    let address = arg(&args, "address").unwrap_or_else(|| format!("ws://127.0.0.1:{}", net::PORT));
    let count: usize = arg(&args, "bots").and_then(|v| v.parse().ok()).unwrap_or(50);
    let minutes: f64 = arg(&args, "minutes").and_then(|v| v.parse().ok()).unwrap_or(60.0);
    let ramp: f32 = arg(&args, "ramp").and_then(|v| v.parse().ok()).unwrap_or(0.5);
    let build = args.iter().any(|a| a == "--build");
    let every: f32 = arg(&args, "every").and_then(|v| v.parse().ok()).unwrap_or(60.0);
    let report = arg(&args, "report");
    let pid = arg(&args, "pid");
    let status = status_url(&address);
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(5))).build().into();

    let start = world::START;
    let home = Vec3::new(start.x, 40.0, start.z);
    let mut rng = Rng::new(7);
    let mut bots: Vec<Bot> = (0..count)
        .map(|i| Bot {
            name: format!("Bot {:02}", i + 1),
            out: None,
            rx: None,
            welcomed: false,
            pos: home,
            target: home,
            yaw: 0.0,
            near: BTreeMap::new(),
            riding: None,
            next_build: 10.0 + rng.next() * 20.0,
            placed: None,
            next_act: 5.0 + rng.next() * 15.0,
            wait: 0.0,
            retry: i as f32 * ramp,
        })
        .collect();
    let mut ground = Ground { voxels: None, seed: 0 };
    let mut counts = Counts::default();
    let mut last = Counts::default();
    let began = Instant::now();
    let mut since_report = 0.0f32;
    let step = Duration::from_millis(50);
    let mut was = Instant::now();
    eprintln!("potluck-bots: {count} bots for {address}, reporting from {status}");
    loop {
        let dt = was.elapsed().as_secs_f32().min(0.5);
        was = Instant::now();
        if minutes > 0.0 && began.elapsed().as_secs_f64() > minutes * 60.0 {
            break;
        }
        for bot in bots.iter_mut() {
            tick(bot, dt, &address, &mut ground, &mut counts, &mut rng, build);
        }
        since_report += dt;
        if since_report >= every {
            since_report = 0.0;
            let s: Value = agent.get(&status).call().ok().and_then(|mut r| r.body_mut().read_to_string().ok()).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null);
            let mins = began.elapsed().as_secs_f64() / 60.0;
            let line = json!({
                "minutes": (mins * 10.0).round() / 10.0,
                "connected": bots.iter().filter(|b| b.welcomed).count(),
                "server_players": s["players"].as_array().map(|a| a.len()),
                "creatures": s["creatures"], "creations": s["creations"].as_array().map(|a| a.len()),
                "tick_ms": s["tick_ms"], "tick_ms_max": s["tick_ms_max"],
                "server_bytes_per_second": s["bytes_per_second"], "up_seconds": s["up_seconds"],
                "server_rss_mb": pid.as_deref().and_then(rss_mb),
                "bots_kb_per_second": ((counts.bytes - last.bytes) as f64 / every as f64 / 1024.0 * 10.0).round() / 10.0,
                "msgs_per_second": ((counts.msgs - last.msgs) as f64 / every as f64).round(),
                "joins": counts.joins, "drops": counts.drops, "refused": counts.refused,
                "blocks": counts.blocks - last.blocks, "acts": counts.acts - last.acts,
                "status_ok": !s.is_null(),
            });
            println!("{line}");
            if let Some(path) = &report {
                use std::io::Write;
                if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
                    let _ = writeln!(f, "{line}");
                }
            }
            last = Counts { bytes: counts.bytes, msgs: counts.msgs, blocks: counts.blocks, acts: counts.acts, ..Default::default() };
        }
        std::thread::sleep(step.saturating_sub(was.elapsed()));
    }
    eprintln!("potluck-bots: done after {:.1} minutes: {} joins, {} dropped, {} refused", began.elapsed().as_secs_f64() / 60.0, counts.joins, counts.drops, counts.refused);
}

fn send(bot: &mut Bot, m: &Msg) {
    if let Some(o) = &bot.out {
        if o.send(net::frame_msg(m)).is_err() {
            bot.out = None;
        }
    }
}

fn tick(bot: &mut Bot, dt: f32, address: &str, ground: &mut Ground, counts: &mut Counts, rng: &mut Rng, build: bool) {
    // joining, and joining again
    if bot.out.is_none() {
        bot.retry -= dt;
        if bot.retry > 0.0 {
            return;
        }
        let (tx, rx) = channel();
        match transport::connect(address, net::PORT, tx) {
            Ok(out) => {
                let _ = out.send(net::frame_msg(&Msg::Hello { name: bot.name.clone(), protocol: net::PROTOCOL, pass: None }));
                bot.out = Some(out);
                bot.rx = Some(rx);
            }
            Err(e) => {
                eprintln!("{}: couldn't connect ({e})", bot.name);
                bot.retry = 5.0;
            }
        }
        return;
    }
    // everything the server sent
    let mut gone = false;
    let mut answers = vec![];
    if let Some(rx) = &bot.rx {
        loop {
            match rx.try_recv() {
                Ok(w) => {
                    if let Wire::Data(_, b) = &w {
                        counts.bytes += b.len() as u64;
                        counts.msgs += 1;
                    }
                    match net::decode(w) {
                        Some(Event::Msg(_, Msg::Welcome { seed, .. })) => {
                            bot.welcomed = true;
                            counts.joins += 1;
                            ground.seed = seed;
                            bot.pos.y = ground.feet(bot.pos.x, bot.pos.z);
                        }
                        Some(Event::World(_, bytes)) if ground.voxels.is_none() => ground.voxels = Voxels::decode(ground.seed, &bytes),
                        Some(Event::Msg(_, Msg::Refused(why))) => {
                            eprintln!("{}: refused: {why}", bot.name);
                            counts.refused += 1;
                            gone = true;
                        }
                        Some(Event::Msg(_, Msg::Bodies { list, gone: away })) => {
                            for id in away {
                                bot.near.remove(&id);
                            }
                            for b in list {
                                bot.near.insert(b.id, Vec3::from_array(b.pos));
                            }
                        }
                        Some(Event::Msg(_, Msg::Thrown(_))) => bot.riding = None,
                        Some(Event::Msg(_, Msg::Go { .. })) => gone = true,
                        // a creation's question: a choice, or now and then Close
                        Some(Event::Msg(_, Msg::Panel(Some(p)))) => {
                            let pick = (rng.next() * (p.choices.len() as f32 + 1.0)) as u32;
                            answers.push(Action::Answer { ask: p.ask, choice: (pick < p.choices.len() as u32).then_some(pick) });
                        }
                        Some(Event::Gone(_)) => gone = true,
                        _ => {}
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    gone = true;
                    break;
                }
            }
        }
    }
    for a in answers {
        send(bot, &Msg::Act(a));
    }
    if gone {
        if bot.welcomed {
            counts.drops += 1;
        }
        bot.out = None;
        bot.rx = None;
        bot.welcomed = false;
        bot.near.clear();
        bot.riding = None;
        bot.retry = 5.0;
        return;
    }
    if !bot.welcomed {
        return;
    }

    // riding: steer a little, and get off after a while
    if let Some((id, left)) = bot.riding {
        let left = left - dt;
        if left <= 0.0 || !bot.near.contains_key(&id) {
            bot.riding = None;
            send(bot, &Msg::Act(Action::Dismount));
        } else {
            bot.riding = Some((id, left));
            bot.yaw += (rng.next() - 0.5) * 0.4;
            let yaw = bot.yaw;
            send(bot, &Msg::Act(Action::Ride(Vec2::new(0.0, 1.0), yaw)));
            if let Some(p) = bot.near.get(&id) {
                bot.pos = *p + Vec3::Y;
            }
            return;
        }
    }

    // a stroll round the start, on top of the ground, stopping now and
    // then, and sometimes over to a creature
    let flat = Vec2::new(bot.target.x - bot.pos.x, bot.target.z - bot.pos.z);
    if bot.wait > 0.0 {
        bot.wait -= dt;
    } else if flat.length() < 1.0 {
        if rng.next() < 0.4 {
            bot.wait = 3.0 + rng.next() * 17.0;
        }
        let creature = if rng.next() < 0.3 { bot.near.values().nth((rng.next() * bot.near.len() as f32) as usize).copied() } else { None };
        bot.target = match creature {
            Some(p) => p,
            None => {
                let a = rng.next() * std::f32::consts::TAU;
                let r = 4.0 + rng.next() * 44.0;
                let s = world::START;
                Vec3::new(s.x + a.cos() * r, 0.0, s.z + a.sin() * r)
            }
        };
    } else {
        let d = flat.normalize() * (4.0 * dt).min(flat.length());
        bot.pos.x += d.x;
        bot.pos.z += d.y;
        bot.yaw = (-d.x).atan2(-d.y);
    }
    bot.pos.y = ground.feet(bot.pos.x, bot.pos.z);
    let (pos, yaw) = (bot.pos.to_array(), bot.yaw);
    send(bot, &Msg::Me { pos, yaw });

    // now and then a block, built then taken away again
    bot.next_build -= dt;
    if build && bot.next_build <= 0.0 {
        bot.next_build = 15.0 + rng.next() * 30.0;
        let list = match bot.placed.take() {
            Some((x, y, z)) => vec![(x, y, z, potluck::blocks::AIR)],
            None => {
                let (x, z) = ((bot.pos.x + 2.0).floor() as i32, bot.pos.z.floor() as i32);
                let y = ground.feet(x as f32, z as f32) as i32;
                bot.placed = Some((x, y, z));
                vec![(x, y, z, potluck::blocks::PLANKS)]
            }
        };
        counts.blocks += 1;
        send(bot, &Msg::Blocks(list));
    }

    // and a creature close by: a pat, a ride or a bop
    bot.next_act -= dt;
    if bot.next_act <= 0.0 {
        bot.next_act = 8.0 + rng.next() * 20.0;
        let here = bot.pos;
        let close = bot.near.iter().map(|(id, p)| (*id, p.distance(here))).filter(|(_, d)| *d < 6.0).min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((id, _)) = close {
            let roll = rng.next();
            let act = if roll < 0.4 {
                Action::Use(id)
            } else if roll < 0.7 {
                bot.riding = Some((id, 5.0 + rng.next() * 10.0));
                Action::Use(id)
            } else {
                Action::Hit(id)
            };
            counts.acts += 1;
            send(bot, &Msg::Act(act));
        }
    }
}
