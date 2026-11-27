# Hosting Potluck

What it takes to run Potluck online: one world for friends, the market, and worlds we host for everyone, from one cheap machine to hundreds of thousands of players. Every number here says whether it was **measured** (and how) or is an **estimate** (and from what). The commands to measure again are at the end.

## The pieces

| Piece | What it does | Where it lives | How it grows |
|---|---|---|---|
| **World server** (`potluck-server`) | One world: its blocks, creatures and running creations, and the players in it. Host-authoritative over WebSocket (`net.rs`), 30 ticks a second, snapshots 20 times a second. | `crates/potluck/src/server.rs`, the `potluck-server` snap (`packaging/potluck-server`) | One process per world. More worlds, more processes: they share nothing but the market. |
| **The market** (`potluck-share`, Forgejo, a file store) | Shares, versions, remixes, suggestions, jams, the server list and passes, boards, the web pages. Creations' files by SHA-256. | `crates/share-service`, `deploy/market` | One service in front of SQLite, Forgejo and a folder or S3 bucket. Read-heavy; files go to a CDN. |
| **The relay** (`iroh-relay`) | Worlds shared from home: introduces two players' games so they can reach each other through home routers (hole punching), and carries the connections that can't. The market says which keys may use it. | `deploy/relay`, `crates/potluck/src/p2p.rs` | Bandwidth only for the connections it carries; more relays in more places when one fills. |
| **The game server** (Nakama) | Device sign-in for the market, the older share codes, the launch ping. Shared by every LinuxGroove game. | `LinuxGroove/game-server` (its `docs/operations.md`) | Already running at `play.linuxgroove.com`; Potluck adds a sign-in per session. |

Players' games do the expensive work: drawing, meshing, their own movement, and client halves of creations. A world server does no drawing and no meshing; it runs creations' Luau, moves creatures, and tells each player what changed near them.

## What it costs, measured

### One world

**Measured** on the cloud container (4 vCPUs, the server pinned to two of them and `potluck-bots` to the other two), release build of `87ed870` plus the bots' new report fields, 17 base creations running, a fresh world each time, four minutes of bots after a minute empty. The bots walk within 48 blocks of the start, build and break a block every half minute and pat or ride creatures: everyone is in view of everyone, so this is the crowded case, not the average one.

| Players | Memory | CPU (share of one core) | Threads | Tick: average (worst) | Sent | Per player |
|---|---|---|---|---|---|---|
| 0 | 75 MB | 3% | 6 | 1.0 ms (18 ms) | nothing | |
| 10 | 87 MB | 5% | 16 | 1.2 ms (20 ms) | 0.20 MB/s (2 Mbit/s) | 19 KB/s |
| 50 | 122 MB | 10% | 56 | 2.1 ms (21 ms) | 1.4 MB/s (11 Mbit/s) | 27 KB/s |
| 100 | 161 MB | 21% | 106 | 4.7 ms (63 ms) | 4.6 MB/s (36 Mbit/s) | 45 KB/s |
| 200 | 265 MB | 56% | 206 | 13.6 ms (60 ms) | 14.5 MB/s (116 Mbit/s) | 71 KB/s |

A tick has 33 ms (30 a second). No bot was dropped or refused in any run. CPU is a share of one of the cloud container's vCPUs; amd-halo's faster cores ticked about 40% quicker (1.3 ms at 50 against 2.1 here).

- **Empty world:** 75 MB resident, 3% of one core (17 creations and 40 creatures keep running), 6 threads. A world loaded from its save is 86 MB.
- **Start:** 0.25 s from launch to accepting players, fresh or from a save. That makes sleeping worlds cheap (below).
- **Disk:** `world.potluck` is 655 KB (run-length coded), plus a few KB of `boards.json`, `store.json` (at most 64 KB a creation) and `world-creations.json`. It barely grows: after 506 minutes of 50 bots building, run 1's world was 715 KB.
- **Memory over time** (**measured**, amd-halo, run 1 of the M3 soak, 50 bots for 506 minutes): 147 MB after a minute, 168 MB after 45 minutes, 173 MB at the end, with no drops, a median tick of 1.3 ms and a 95th percentile of 1.8 ms. Run 2 (started 14:37 UTC on 27 November, for 48 hours) showed night: Gloomlings doubled the creatures from 41 to 82, the tick went from 1.5 to 4.2 ms and bytes sent rose to 2.6 MB/s.

### What a player costs, and why

Each player adds about 0.9 MB of memory and one thread. CPU and bandwidth per player grow with the crowd, because everyone hears about everyone near them: a player costs 19 KB/s among 10 and 71 KB/s among 200. The tick time fits 1.2 ms + 0.0003 ms x players² across the four runs (**estimate** from that fit), which uses up the 33 ms a tick has at about 320 players all in one place. Real players spread out over the island and see fewer of each other, so these are the worst case.

What each player received, by message (**measured**, KB/s per player, about 40 creatures in the world by day):

| Message | 10 players | 50 | 100 | 200 |
|---|---|---|---|---|
| `Bodies` (creatures moving) | 15.4 | 14.7 | 15.7 | 15.8 |
| `People` (other players moving) | 3.0 | 12.9 | 27.0 | 52.7 |
| `Roster` (everyone's names) | 0.05 | 0.23 | 0.44 | 0.93 |
| Everything else (`Blocks`, `Sound`, `Looks`, ...) | 0.1 | 0.1 | 0.2 | 0.4 |

- **Creatures (`Bodies`) are the biggest share.** Every creature within `NEAR` (144 blocks, which is most of a 384-block island) is sent to every player, 20 times a second within 32 blocks and 5 times beyond, as JSON with two-decimal floats, whenever it moved 1 cm. With 40 creatures that's about 15 KB/s a player whether there are 10 players or 200.
- **Other players (`People`) grow with the square of a crowd.** Each player hears about every other one in view, so 10 players cost 3 KB/s each, 50 cost 13 KB/s each and 200 cost 53 KB/s each, three quarters of the bill.
- **Joining costs 1.2 MB.** The world is 655 KB, and the creations are 557 KB, of which 471 KB is the same two files (`AGENTS.md` and the API's types) sent again with each of the 17 base creations. Every client already has the base creations built in.
- **`Roster`** (every name, every 2 s) is small at 50 and grows with the square too.
- **Threads:** one per connection, each waking every 8 ms to look for something to send or read (`transport.rs`). **Measured:** 200 connections that only sit there cost 20% of a core and 1,000 cost a whole core (and 150 KB of memory each), so at 200 players over a third of the server's CPU is threads waking to find nothing to do.

For comparison, hosting companies size a Minecraft Java server at 4 to 16 GB for 10 to 100 players, about 150 to 200 MB of memory per player, with the game's simulation on one main thread ([DatHost](https://dathost.net/blog/minecraft-server-performance-benchmarks-2026) and [MineStrator](https://minestrator.com/en/blog/article/minecraft-server-ram-2026-vanilla-paper-modded-gb), 2026; vendor figures, not benchmarks). A Potluck world of 50 measured 122 to 173 MB in all, about 1 MB a player, and 200 bots in one crowd ticked in 13.6 ms, under half the time a tick has. Memory and CPU are already far below Minecraft's; **bandwidth and the thread per connection are what to work on**, and the changes in [Lighter players](#lighter-players-the-network-work) bring the bandwidth down several times.

### The market

**Measured** (cloud container and amd-halo's test market):

- **A creation's files:** 2 to 7 KB of Luau and TOML each in the base game (the market caps a creation at 256 KB). Files are stored once by hash, so remixes that keep a file cost nothing for it.
- **Its git repository** in Forgejo: 72 KB on disk for a first version with an empty hook template (git's directories each take a 4 KB block; the files themselves are 2 to 4 KB compressed), and about 12 KB more per version until git packs it. Forgejo adds its hooks and a few database rows. **Estimate:** 100 to 150 KB per creation all told, so a million creations is 100 to 150 GB, almost all of it git's directory overhead.
- **The index** (`market.db`): 1.26 KB per creation with one version, **measured** with 100,000 made-up creations (below; real ones add a row to the search index too). amd-halo's test market, with the 17 base creations and a few test shares and accounts, is 151 KB. The `plays` table gains a row per creation, player and day (about 50 bytes); nothing prunes it yet, so at 100,000 players loading five creations a day it would grow by about 25 MB a day.
- **The service:** **measured** with 100,000 made-up creations from 20,000 makers put straight into its index (no Forgejo, so reads only): 13 MB resident when idle and 16 MB under load. It loaded creations by code 1,570 times a second on two cores (median 4.8 ms, 99th percentile 14 ms), each load also writing a play. The index came to 1.26 KB per creation with one version. `/api/green` is the weak spot: it sorts every listed creation on each call, 70 ms each at 100,000, and since the index has one connection, 13 calls a second fill a core and hold up every other request (median 660 ms under load). It needs a cache before the market has tens of thousands of creations; every game asks on entering a world and every half hour.

### Worlds shared from home

A world shared from home (Share this world) runs on the host's own game, and friends connect to it peer to peer (`p2p.rs`: iroh, QUIC dialled by the host's key). What we pay for:

- **The market:** one heartbeat a minute per shared world (a few hundred bytes of JSON and one row in `worlds`), one request per friend joining, and a row per key in `relay_keys` (the host's for five minutes after its last heartbeat, a joiner's for eight hours). **Estimate:** nothing next to loading creations.
- **The relay, introducing:** every shared world and every friend joining holds a connection open to the relay while it plays, and the two games exchange their addresses through it to punch through their routers. **Estimate** (iroh's own figures): a few KB per connection, and pings every few seconds.
- **The relay, carrying:** the connections that can't go direct, about **one in ten** by n0's account of their public relays (two strict NATs, some phone networks and campus networks). A relayed friend costs what a player costs a world server, about **20 KB/s from the host to the friend** (measured on world servers, above), so **72 MB an hour**. The relay passes it on, so it's counted in and out.
- **Always connect through LinuxGroove** (a setting, on by default for players under 13) sends every connection of that player's through the relay, so their address never reaches anyone. If most younger players' parents leave it on, younger players' play is all relay traffic.

Caps in `deploy/relay/relay.toml`: each game may send **192 KB/s** through the relay (about ten friends' worth, with a 4 MB burst for a friend's first download of the world), and a world from home takes the host and 8 friends at once (`game::HOME_PLAYERS`), each about 20 KB/s of the host's upload. **Estimate** of a month: if 2,000 players a day share or join for two hours and one in ten (plus the younger players who always go through) is relayed, 400 relayed hours a day at 72 MB is 29 GB a day, under 1 TB a month. A cheap VPS with 20 TB of traffic covers about twenty times that. Watch `relayserver` `bytes_sent` on the relay's metrics (`127.0.0.1:9090`): alarm at half the plan's traffic, and if it gets there, lower `bytes_per_second` or set `relay = ""` in share.toml for a while (friends on the same network still connect; others can't).

### The game server

Potluck's use is small: a device sign-in when a player first needs the market, a session the market checks against Nakama once per five minutes per player (`identity.rs` keeps it), and one launch ping per start. **Estimate:** a few KB per account in Nakama's PostgreSQL. The game server's own operations guide covers its sizing; a single Nakama handles tens of thousands of players online at once.

## Minimum requirements

From the measurements above, with headroom for night (creatures double) and players bringing creations (each creation may use up to 32 MB and 15 ms a tick; `script.rs`).

| Running | CPU | Memory | Disk | Network up (today's netcode) |
|---|---|---|---|---|
| A world for a few friends (up to 10) | 1 vCPU | 512 MB | 1 GB | 2 Mbit/s (measured 0.2 MB/s at 10) |
| A public world of 50 | 1 vCPU (2 for headroom) | 1 GB | 2 GB, plus backups | 11 to 21 Mbit/s (measured 1.4 MB/s by day, 2.6 MB/s at night on amd-halo), 3.5 to 7 TB a month if always full |
| The market, starting out | 2 vCPU | 2 GB (Forgejo is the biggest part) | 20 GB, plus backups | small: files go to a CDN once it's busy |
| Everything to start (market and three or four official worlds) | 4 vCPU | 8 GB | 80 GB | a plan with unmetered or 20 TB+ traffic |

Any 64-bit Linux, amd64 or arm64 (both snaps build for both). The world needs no GPU. Pick a provider whose plans include traffic: at today's 27 KB/s a player, a full world of 50 sends about 3.6 TB a month by day, which is free on most VPS plans and costs about $300 a month at the big clouds' egress prices (**estimate**, about $0.09/GB). That alone is a reason to do the network work before moving anything to a big cloud.

## A cheap start

One VPS with 4 vCPUs, 8 GB and unmetered or 20 TB+ traffic (about $10 to $15 a month in October 2026 at OVHcloud's VPS range; check prices again before buying), next to the game server's existing VPS:

- the market from `deploy/market/compose.yaml` (Forgejo, `potluck-share`, Caddy) at `potluck.linuxgroove.com`, files in a folder to begin with;
- three or four official worlds as systemd instances of `potluck-server` behind the same Caddy (`wss://w1.worlds.potluck.linuxgroove.com`, ...), each listed on the market and `join = "signed-in"`;
- backups every hour to storage at another provider (restic to Backblaze B2 or similar): the worlds' folders, the market's `market.db` (with `.backup`), its `signing.key` and files, and Forgejo's dump.

Four full worlds of 50 use under half a core and under 1 GB, so the machine has room for many more worlds than that; with today's netcode its traffic runs out first (four full worlds send 45 to 85 Mbit/s, 15 to 28 TB a month if full around the clock), so check the plan's port speed and fair use terms. Steps for each part are below.

## Deploying, step by step

### A world server

The snap is the easy way for one world on one machine:

```sh
sudo snap install potluck-server
sudo snap set potluck-server name="Riverbend" domain=riverbend.example.org   # HTTPS on 443, Let's Encrypt
sudo potluck-server.info                                                       # the addresses players type
```

Open ports 80 and 443 (Caddy, for `domain`) and 47126 (plain WebSocket, for the LAN or players typing `host:47126`). Everything else is in `/var/snap/potluck-server/common/world/server.toml`; restart after editing it (`sudo snap restart potluck-server`). For a public server, set `join = "signed-in"` so every player has a market handle you can `ban`, `bring = "signed-in"` so creations come from accounts, and list it on the market (`list`, `address`, `market_token`).

What to keep, all in `/var/snap/potluck-server/common/world/` (or `POTLUCK_HOME` outside the snap):

| File | What | Written |
|---|---|---|
| `world.potluck` | Every block (the world's seed and its blocks, run-length coded) | every minute and on stop, to a temporary file then renamed, so a copy taken at any moment is whole |
| `world-creations.json` | Creations players brought, which come back at the next start | with the world |
| `boards.json` | Creations' boards | with the world |
| `store.json` | What creations keep with `@potluck/store` (64 KB each at most) | with the world |
| `server.toml` | The settings | by you |

Back them up with anything that copies files: `restic backup /var/snap/potluck-server/common/world` from a root cron job every hour, to storage at another provider, is enough. To restore, stop the service, put the files back and start it.

Updates: `snap refresh` brings new versions. Players with an older game get "This world runs a different version of Potluck" when `PROTOCOL` changes, so refresh servers soon after a game release that changes it.

Logs: `sudo snap logs -n 100 potluck-server`. The status page (`https://<domain>/` or `http://<host>:47126/`) and `/status.json` show players, creations, creatures, the tick time and bytes sent per second: point a monitor (Uptime Kuma, Prometheus' blackbox or JSON exporter) at `/status.json`.

### Several worlds on one machine

The snap runs one world. For more on one machine, run the binary from systemd, one data folder and port each, behind one Caddy:

```ini
# /etc/systemd/system/potluck-world@.service: one world per instance name
[Unit]
Description=Potluck world %i
After=network-online.target

[Service]
User=potluck
Environment=POTLUCK_HOME=/var/lib/potluck/worlds/%i
ExecStart=/usr/local/bin/potluck-server --no-beacon
Restart=on-failure
RestartSec=5
# it saves on SIGTERM before it stops
TimeoutStopSec=30
MemoryMax=1G
CPUQuota=100%

[Install]
WantedBy=multi-user.target
```

Each world's `server.toml` sets its own `port` (47201, 47202, ...) and its `address`. Give each world a name under one wildcard DNS record (`*.worlds.example.org` pointing at the machine) and let Caddy fetch a certificate for each:

```
w1.worlds.example.org {
	reverse_proxy 127.0.0.1:47201
}
w2.worlds.example.org {
	reverse_proxy 127.0.0.1:47202
}
```

The game would also join a path (`wss://worlds.example.org/w/1` reaches the server, which ignores the path), but the market's server list only takes addresses without one (`status_url` in `servers.rs`), so names are the way to list several worlds on one machine until that changes.

`MemoryMax` and `CPUQuota` keep one busy world from starving the rest; a world's creations already have their own limits (32 MB and 15 ms a tick each). Build the binary with `cargo build --release --bin potluck-server` (it needs `libasound2` and `libudev1` at run time, as the snap stages).

### The market

The market is `deploy/market/compose.yaml`: Forgejo (git for every creation, never public), `potluck-share` (the service and the web pages) and Caddy. It needs the game server registered for Potluck first (game-server#11, share kind `creation`), since every signed-in request is checked against it.

First run, on a machine with Docker or Podman and ports 80 and 443 open:

```sh
cd deploy/market
# share.toml: public_url, nakama (the game server) and, later, [store]
docker compose up -d forgejo
docker compose exec -u git forgejo forgejo admin user create --admin --username market \
  --email market@users.potluck.linuxgroove.com --password "$(head -c 18 /dev/urandom | base64)" --must-change-password=false
docker compose exec -u git forgejo forgejo admin user generate-access-token --username market \
  --token-name market --scopes all --raw > forgejo.token && chmod 600 forgejo.token
FORGEJO_TOKEN=$(cat forgejo.token) docker compose up -d
```

The base game's creations go up on their own once Forgejo answers. Then open the market from the game once (so your account exists) and make yourself a moderator, and make invites while sharing to Everyone is invite-only:

```sh
docker compose exec market potluck-share --config /etc/potluck-share/share.toml moderator <your handle>
docker compose exec market potluck-share --config /etc/potluck-share/share.toml invite 20
```

What to keep:

| Where | What | How |
|---|---|---|
| `market` volume, `signing.key` | The key every creation's signature is made with. Release builds of the game carry its public half (`POTLUCK_MARKET_KEY`), so **losing it breaks every share for every player until a new game release**. Keep two copies off the machine. | copy once |
| `market` volume, `market.db` | Accounts, creations, versions, stars, plays, servers, jams | `sqlite3 market.db ".backup /backups/market.db"` every hour (WAL, so don't copy the file while it runs) |
| `market` volume, `files/` (or the bucket) | Every creation's files by hash, written once | `restic` or the bucket's own replication |
| `forgejo` volume | One git repository per creation, and Forgejo's own SQLite | `docker compose exec -u git forgejo forgejo dump` nightly, or the volume with restic while Forgejo is stopped |
| `caddy` volume | Certificates | none needed; Caddy fetches them again |

Files: with `[store] kind = "s3"` and `public_url` (a bucket with a CDN in front), the game fetches creations' files from the CDN and the market only sends small JSON. Files are named by their SHA-256 and never change, so they cache forever (`/f/<sha>` already says `immutable`).

### The relay

The relay is `deploy/relay/compose.yaml`: iroh-relay built from crates.io at the version the game uses, on a machine of its own (the smallest VPS will do: it's bandwidth, not CPU) with TCP 80 and 443 and UDP 7842 open, and `relay.potluck.linuxgroove.com` pointing at it. It gets its own certificate from LetsEncrypt, which is why it can't sit behind the market's Caddy: QUIC address discovery needs its own TLS on UDP.

```sh
# one token, kept by both: the relay sends it, the market checks it
head -c 24 /dev/urandom | base64 > relay.token && chmod 600 relay.token
cd deploy/relay
# relay.toml: set contact (LetsEncrypt's email) first
RELAY_TOKEN=$(cat ../../relay.token) docker compose up -d
# then the market, with the same token and share.toml's relay set
cd ../market && RELAY_TOKEN=$(cat ../../relay.token) FORGEJO_TOKEN=$(cat forgejo.token) docker compose up -d
```

Start the relay first: a market that names a relay that isn't there leaves friends on different networks unable to find each other. Nothing on it needs keeping (certificates come again); the market's `relay_keys` rows run out by themselves.

### The game server

Potluck signs players in through the shared LinuxGroove game server (Nakama, `LinuxGroove/game-server`), the same one every other game uses: device sign-in, the older share codes and the launch ping. It's already running at `play.linuxgroove.com`; its own `docs/operations.md` covers backups, moving the database off and what comes after one machine. Potluck adds little to it: one sign-in each time the game starts and needs it, and the market's check of a session, which the market keeps for five minutes.

## The scaling plan

The rule of thumb for turning players into machines (**estimate**): at the busiest hour, about 3% of a month's players are online at once, and a world fills to about 80% before players are sent to another. So 300,000 players a month is about 10,000 online at the peak.

| Stage | Players a month (peak online) | What runs | What to build first |
|---|---|---|---|
| 0. Start | up to 5,000 (150) | One VPS: market and a few official worlds. Game server as now. | This guide; hourly backups; a monitor on `/status.json` and the market. |
| 1. Busy | 5,000 to 30,000 (150 to 1,000) | Market on its own VPS. Worlds on 2 to 4 VPSs. Files in an S3 bucket with a CDN in front. | The join and creature changes below (a quarter of the join, and about a third less traffic in play, for little work); placement (`/api/play`); worlds that sleep. |
| 2. Popular | 30,000 to 150,000 (1,000 to 5,000) | A fleet agent on each world machine (or Nomad); worlds sleep and wake on demand; a second region. Market: two `potluck-share` behind a load balancer on PostgreSQL, Forgejo on its own with PostgreSQL. Game server: Potluck's own Nakama per game-server's plan. | Binary snapshots and interest limits; the market on PostgreSQL with a connection pool; per-creation git locks. |
| 3. Big | 150,000 to 500,000+ (5,000 to 15,000+) | Kubernetes with Agones for worlds in two or three regions; CloudNativePG for the market and Nakama; object storage for every world's save. | Bigger worlds in chunks; one world across several servers if players want crowds of thousands. |

**Estimate** for stage 3 at 15,000 online, from the measured world of 50 (122 to 173 MB, 10% of a core, 11 to 21 Mbit/s): with today's netcode, 300 worlds need about 31 cores, 50 GB of memory and 3 to 6 Gbit/s at the peak. At the big clouds' egress prices that traffic alone is tens of thousands of dollars a month; at hosts with included traffic it's a dozen or so machines with 1 Gbit/s ports. After the network work below (about 4 KB/s a player, no thread per connection), the same players need about 0.5 Gbit/s and half the CPU, and fit on four to six machines of 8 vCPUs and 32 GB per region.

None of the stages needs a rewrite: the same `potluck-server` runs a friend's world and every official one, and each stage adds a service around it.

## Potluck Online: worlds we host

Players should be able to press **Play online** and land in a busy world without anyone running a server. Potluck's world is one process (`potluck-server`), so hosted online play is a fleet of those processes plus a little service that decides where each player goes. That scales sideways without limit, and it can start on the same machine as the market.

### Stage 1: a fleet of worlds

- **A world is a cell.** Every official world is a `potluck-server` with its own data folder, seed, creations and `max_players`. They're all the same binary players can run themselves, so nothing official is special.
- **Placement.** A new endpoint on the market, `POST /api/play`, answers "which world, and a pass for it". It sends a player to the world their friend is in when they follow someone, else to the fullest world that still has room below a soft cap (say 80% of `max_players`), else wakes or starts a world. Worlds already send a heartbeat with their player count every minute (`/api/servers/heartbeat`); placement needs it every few seconds, or a count it keeps itself from the passes it hands out.
- **Sleeping worlds.** A world with nobody in it for ten minutes saves and stops; its files go to object storage. The next player who needs it starts it again from its save. Running worlds cost memory and CPU; sleeping ones cost about a megabyte of storage. That's what lets every player have a world of their own (like Realms) without paying for it to run: only busy worlds run.
- **A fleet agent** on each world machine starts and stops worlds (systemd instances, or containers) and reports what fits. Kubernetes with Agones does the same job once there are many machines; it isn't needed before then.
- **Kinds of official worlds.** Public islands that fill up and persist; one island per player or group that sleeps when they're away; a hub (a Green with arches) that sends players on with `game.send`; mini game worlds that run one creation (Rodeo Ram, Tag Bot) with their boards. All of them are the same server with different `server.toml`s.
- **Regions.** The same fleet in each region (US East first, then Europe), with placement choosing by the player's region. Worlds don't talk to each other, so regions need nothing shared but the market.

A fleet scales with machines: ten thousand players online is two hundred worlds of fifty, or forty of two hundred and fifty once the network work below is done.

### Stage 2: one big world across several servers

A single world bigger than one process can hold (thousands of players on one continuous map) needs the world split into regions, each run by its own process, with players handed from one to the next as they walk:

- **A bigger world in chunks.** Today a world is one 384 x 128 x 384 array (18 MB) saved and sent whole. A big world keeps chunks (the 32³ the client already meshes) in a region store, loads the ones near players and streams them to clients as they walk instead of sending everything at the start.
- **Regions with borders.** Each region server owns a square of chunks. Near a border it also tells its neighbour about players and creatures within `NEAR` of it, so players see across. Crossing the border hands the player to the next server with a transfer token; `Msg::Go` already moves a player to another server, and the handoff makes it seamless instead of a reconnect.
- **Creations across borders.** A creature that wanders across is handed over with its fields as JSON, as hot reload already does; a creation runs one VM per region it has things in, and shared state (boards, `store`) goes through the market or a small shared store.

This is a big piece of engine work and the fleet covers most of the need (most players play with a few friends in one place), so it waits until a world full of people is the thing players ask for. The portal model (hubs, `game.send`, `[links]`) gives a connected world of many servers long before then.

## Lighter players: the network work

The aim is far more players per server than Minecraft manages. Memory and CPU per player are already small (about 1 MB of memory and a fifth of a percent of a core each in a world of 50); what each player costs is almost all bandwidth, and in a crowd it grows with the square of the crowd. These changes, in order of what they buy for the work, would make a player's cost flat and small. The savings are **estimates** from the measured message sizes; measure again with `potluck-bots` after each.

1. **Don't send what every game already has.** Base creations are built into every game: send their id and knobs, not their files, and never send `AGENTS.md` or the types file in a `Creation` (the game adds them for Look inside). A join goes from 1.2 MB to about 0.75 MB. Then compress the world with zstd (measured: 715 KB to 216 KB at level 9) and encode it once per change instead of once per join. Better still, since the island comes from its seed, send only the blocks that differ from it: most worlds would be a few KB. *Small change, `net.rs` and `creations.rs`; join from 1.2 MB to well under 0.3 MB.*
2. **Binary snapshots.** `People`, `Bodies` and `Looks` are JSON with two-decimal floats (about 50 bytes a player, 70 a creature). A binary form with positions as 16-bit fixed point (a 384-block island at 1/64 of a block fits), yaw as one byte, ids as varints and no animation phase (the client can work it out from how far the creature moved, as `creatures.rs` does) is about 8 to 12 bytes. One frame per player per tick instead of several. *A new message encoding behind `PROTOCOL`; about 5 times less bandwidth for movement.*
3. **Send creatures like players.** A creature standing still sends nothing today unless it moves 1 cm, but walking creatures go 20 times a second to everyone within 32 blocks. With interpolation on the client (it already smooths), 10 a second up close, 4 within 64 blocks and 1 beyond is enough, and creatures nobody is near can tick slower on the server too. *About 3 times less for `Bodies`, and less CPU.*
4. **Cap who each player hears about.** Instead of everyone within 144 blocks, the nearest 32 players and 48 creatures at full rate and the rest in a slow round-robin, with a spatial grid instead of checking every pair (`news_for` is O(players x things) per player). This is what makes a player's cost the same in a crowd of 50 or 500. *Flat cost per player; the change that lets one world hold hundreds.*
5. **Blocks and the roster by distance and by change.** `Blocks` goes to everyone today and `Roster` sends every name every 2 seconds: send block changes to players within view of them, and the roster as joins and leaves. *Small now, quadratic in big worlds.*
6. **No thread per connection.** Each connection has a thread that wakes every 8 ms: **measured** at a tenth of a percent of a core per connection doing nothing, a whole core for 1,000. Moving connections to an async runtime (tokio, which `potluck-share` already uses) or one `mio` loop removes the idle wake-ups and the thread stacks, and lets one process serve thousands of connections. *Needed before worlds of a few hundred, and for many worlds per machine.*
7. **A byte budget per player.** With the above, give each player a budget (say 4 KB/s) and fill it by importance (near, moving, being ridden). A crowd then degrades into slightly less frequent far updates instead of more bandwidth.

**Estimate** of where that ends up: about 3 to 5 KB/s per player in a crowd (from 28), flat as the crowd grows, and a join of a few hundred KB. A 4-vCPU, 16 GB machine would then carry about 2,500 players in 50 worlds of 50 before its CPU runs out, and 1 Gbit/s of traffic about 30,000.

## Other code changes the plan needs

**Worlds we host**

- `POST /api/play` on the market: pick a world (friends first, fullest with room, else wake one) and hand back its address and a pass. Worlds report their count every few seconds, or placement counts the passes it gives.
- Sleep and wake: a world with nobody for ten minutes saves and exits; the fleet agent starts it again on demand (0.25 s measured). Saves go to object storage, not only the local folder.
- A fleet token: an account may list five servers today (`MAX_SERVERS` in `servers.rs`); official worlds need a fleet that lists as many as it runs, under one owner, marked official.
- Limits per world for creations players bring: how many may run at once (nothing limits it today), a total tick budget across them (each has 15 ms a tick; ten slow ones would stall the world) and how many each player may bring.
- Metrics: the status page's numbers as Prometheus metrics (`/metrics`), and per-message byte counts like the bots now report.
- The world's save in a form that can grow: chunks with their own version, so saves can be partial and worlds can be bigger than 384 x 384.
- Addresses with a path in the server list: `status_url` in `servers.rs` refuses `wss://host/w/1`, so today each listed world needs its own name or port.
- A package for fleets: the snap runs one world; a container image or a release tarball of `potluck-server` would replace building it by hand for the systemd template.

**The market**

- SQLite has one connection behind a mutex (`market.rs`), so every request waits for the one before. A small pool of read connections (WAL allows it) first; PostgreSQL when it needs two machines.
- Git writes take one lock for the whole market (`writing` in `market.rs`); a lock per creation lets shares of different creations go at once.
- `/api/green` sorts every listed creation on each call (`jams.rs`); cache it for a minute (twice: for all players and for younger ones). Every game asks on entering a world and every half hour.
- Prune `plays` (keep the counter on `creations`, drop rows older than the trending window) and expired `web_sessions` and `tickets`.
- The session check (`identity.rs`) is per process; with two or more `potluck-share`s, keep it longer or share it (or have the game server sign a short token the market checks itself, with no call to Nakama).

**The game server**

- Register Potluck (game-server#11, share kind `creation`) and deploy it before the market goes public.

## How to measure again

`potluck-bots --pid=<server pid>` now reports the server's memory, its CPU as a share of one core, its threads and what the bots received by message, each interval:

```sh
POTLUCK_HOME=/tmp/w taskset -c 0,1 target/release/potluck-server &        # a world, on two cores
taskset -c 2,3 target/release/potluck-bots --address=ws://127.0.0.1:47126 \
  --bots=50 --minutes=5 --ramp=0.1 --build --every=30 --pid=$! --report=/tmp/50.jsonl
```

Each line has `server_rss_mb`, `server_cpu_percent`, `server_threads`, `tick_ms`, `server_bytes_per_second` and `kb_per_second_by_kind`. Run the server and the bots on different cores, or the bots' own work shows up as the server's. Bots stay near the start, so they measure the crowded case; real players spread out and cost less. For days of running, see the soak in `README.txt` on amd-halo (`~/potluck-soak`).
