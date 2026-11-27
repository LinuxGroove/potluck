# Potluck

A block sandbox where every creature, block and mini game is a Luau creation players can read, tweak, remix and share by code, in Rust on Bevy 0.20 with Luau through mlua. The concept is idea 26 in [game-ideas](https://github.com/LinuxGroove/game-ideas/blob/main/ideas/26-potluck.md). Like Cinderkirk it is **not Godot** and doesn't use the shared add-on; it talks to the shared [game server](https://github.com/LinuxGroove/game-server) over its REST API (signing in, the codes made before the market, the launch ping), and its own market (`crates/share-service`) keeps and lists creations.

## Commands

```sh
cargo build --bins                         # the game (potluck), the server (potluck-server) and the market (potluck-share)
cargo test                                 # world, creations, sandbox, a friend joining, wss and https, the market's checks
deploy/market/test.sh [docker|podman]      # the market end to end against Forgejo in a container: sharing, reviews, a remix of a remix, a suggestion merged from the game
cargo run -p potluck-share -- --config deploy/market/share.toml   # the market (FORGEJO_TOKEN from the environment); potluck-share check FOLDER runs its checks
cargo run --bin potluck -- --play          # straight into the world
cargo run --bin potluck --features raytracing -- --play --lighting=ultra   # Ultra ray traced (Bevy's Solari), on cards with ray queries
cargo run --bin potluck-server -- --port=47126 [--tls-port=47443 --cert=fullchain.pem --key=privkey.pem]
VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json xvfb-run -a -s "-screen 0 1280x720x24" target/debug/potluck --demo=/tmp/demo --size=1280x720   # scripted play-through, recorded (about 1.5 s a frame on 4 CPUs)
VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json xvfb-run -a -s "-screen 0 1600x900x24" target/debug/potluck --shot=/tmp/a.png --after=8 [--at=x,y,z,yaw,pitch] [--time=0.8] [--screen=play|inside|tweak|pause|title] [--lighting=low|medium|high|ultra] [--no-vsync]   # logs the frame rate too
tools/tiers.sh [out=/tmp/potluck-tiers] [size=2560x1440] [binary]   # every lighting tier from four places, with frame rates (on a real graphics card)
tools/server-snap.sh build/potluck-server && (cd build/potluck-server && snapcraft pack)   # the server snap
CYCLES_DEVICE=OPTIX python tools/blocks.py -- assets/blocks 256 [only=grass_top,dirt]   # bake block textures (bpy wheel in a venv; drop CYCLES_DEVICE without a GPU)
```

Run the tests before every commit. Set `POTLUCK_HOME` to a scratch folder for runs that shouldn't touch your own world and settings. Without a GPU, launches take a minute or more under lavapipe (most of it Bevy's pipelines compiling); `libxkbcommon-x11-0` and `mesa-vulkan-drivers` must be installed. A local `.cargo/config.toml` with clang and mold speeds up linking; it isn't committed.

## Layout

| Path | What |
|---|---|
| `crates/potluck/src/lib.rs` | `sim()`: everything a world needs without drawing, shared by the game and the server |
| `src/main.rs`, `src/server.rs` | The game (window, drawing, input, `--play`, `--shot`, `--demo`) and `potluck-server` (MinimalPlugins at 30 Hz, autosave, saving on stop) |
| `src/world.rs`, `blocks.rs`, `noise.rs` | The 384 x 128 x 384 island from a seed, the Green, blocks, the save format (`encode`/`decode`) |
| `src/mesh.rs`, `textures.rs`, `render.rs` | 32³ chunk meshing in three layers, the generated block atlas (glowing tiles kept apart), sky, sun, lanterns, the lighting tiers (`Tier`: Low, Medium, High, Ultra) |
| `src/player.rs`, `input.rs` | Walking, flying, building, survival; controls read once a frame into `Controls` |
| `src/script.rs` | Luau VMs (one per creation, sandboxed, memory and time limits), the API, knobs, events, hot reload keeping creatures' state |
| `src/creatures.rs` | Bodies made of boxes, walking, riding, drawing |
| `src/creations.rs` | Creations on disk and embedded (`BASE`), the library, Remix, watching folders |
| `src/net.rs`, `transport.rs` | Host-authoritative worlds: messages (`Msg`, bump `PROTOCOL` when they change), WebSocket over TCP or TLS, the status page, the LAN beacon |
| `src/share.rs` | The market's client (sharing, loading signed creations, suggestions, merging, sign-in links for the QR code), signing in to the game server and its older codes, the launch ping |
| `src/ui.rs`, `game.rs` | Menus (`Screen`, `Act`), HUD, what menu choices do, interacting |
| `src/demo.rs` | The scripted, recorded play-through |
| `src/tests.rs` | Tests that need no window |
| `src/textures/art.rs` | The blocks' 64 px pixel art, painted from code (palettes, cells, heights for the normal map, roughness per texel); it wins over the bakes. `POTLUCK_DUMP_TEX=<dir> cargo test dump_textures` writes every drawn texture out |
| `tools/blocks.py`, `assets/blocks/` | Block textures baked in Blender (colour, normal, roughness, see-through); `build.rs` builds in whatever is in `assets/blocks`, and fills the gaps the drawn textures leave |
| `creations/` | The base game's creations (embedded at build time), `AGENTS.md` (the API, copied into every remix) and the API's types |
| `crates/share-service/` | `potluck-share`, the market: `market.rs` (sharing, versions, remixes, suggestions, moderation, sign-in links), `forgejo.rs` (repositories, commits, tags, pull requests; remixes copy history with git, since Forgejo allows one fork per person), `checks.rs`, `store.rs` (files by hash, folder or S3), `db.rs` (the SQLite index), `api.rs` (the game's API) and `web.rs` (the pages, in maud), `avatar.rs` (block heads), `tests.rs` |
| `deploy/market/` | The market's config, word list, Dockerfile, compose file (Forgejo, the service, Caddy) and `test.sh` |
| `snap/`, `packaging/potluck-server/snap/` | The game's snap and the server's (with Caddy for `domain`); `tools/server-snap.sh` makes the server's project folder |

## How the game is built

- **One world, many views.** `sim()` holds the voxels, the library, the running creations and the herd. Alone or hosting, `game::host_step` runs creations and creatures; a client only mirrors what the host sends. The server is `sim()` with no window and no `Me`.
- **Creations are the game.** Base content is creations too (`creations/mossback`). A creation's VM never touches Bevy: calls go through a scoped API that reads and changes the world and herd, and anything a creation can do must be safe for a stranger's code: no files, no network, no `loadstring`, a memory cap and an interrupt deadline. `AGENTS.md` and `types/potluck.d.luau` describe the API; keep them in step with `script.rs`, and version API modules on their own (`@potluck/world@0.1`).
- **Hot reload keeps state.** Restarting a VM carries each creature's fields over as JSON and restyles bodies; tests check it.
- **Networking** is WebSocket binary messages: tag 0 is a JSON `Msg`, tag 1 the world's bytes. The same port answers plain HTTP with the status page and `/status.json`. TLS comes from a certificate (`--cert`, `--key`) or a proxy in front (the snap's Caddy).
- **The market** (`share.rs` in the game, `crates/share-service`): the game signs in to the game server with its device id and gives the market that session, which the market checks with the game server; no emails or passwords. The market keeps one code per creation that loads its newest version anyone may load, signs each version's manifest (Ed25519; release builds carry the key as `POTLUCK_MARKET_KEY`, others keep the first one seen) and serves files by SHA-256, which the game checks and caches. Codes made before the market still load from the game server, and sharing by code falls back to it when the market is down. A remix's `potluck.toml` carries `[from]` (code and version, or a base creation's id), and `market.json` in the data folder keeps each of the player's creations' code and version there.
- **Lighting tiers** (`render::Tier`): Low for integrated graphics and the Steam Deck, Medium the default, High and Ultra for desktop cards. With the `raytracing` feature, Ultra on a card with ray queries lights the world with Solari instead of shadow maps and fog (`render::Raytraced`): meshes then carry only position, normal, UV and tangent (no vertex colours), water and plants (their own chunk layer) stay out of the ray traced scene, and everything ray traced counts as opaque, so leaves shade as solid blocks. Judge looks on a real GPU (lavapipe is slow and renders differently), and bake final block textures on one too (`CYCLES_DEVICE=OPTIX`); quick cloud bakes are previews, not for committing.
- **Detail by distance** (`mesh::Detail`): chunks near the player get every tuft, leaf fin and the wind; further out fewer, wider tufts that keep still, and past 176 blocks no tufts or fins. Only near ground cover casts shadows. `POTLUCK_OFF=tufts,fins,lips,wind,lod` and `POTLUCK_HIGH_OFF=ssao,contact,haze,taa,shadowmap` leave things out for measuring what each costs; hold every tier to 60 fps at 1440p on the p16g3 (Low to 40 on a Steam Deck).
- **Comfort:** no head bob or camera shake; the field of view is a setting.
- **Controller first:** everything works on a pad, and the keyboard and mouse always work too.
- **The game server key** is `defaultkey` in git; CI writes the real one to `server.key` (read by `build.rs`), never committed. The version comes from `tools/version.sh` through `POTLUCK_VERSION` or a `VERSION` file.
- Player-facing text is plain and short, in the game's words (creations, Look inside, Tweak, Remix, the Green, Mossbacks).

## Releases

Pushing to `main` builds the `potluck` and `potluck-server` snaps for amd64 and arm64 (`snap.yml`) and the Windows and macOS zips (`desktop.yml`). Publishing snaps waits on both names being registered, the `STORE_LOGIN` secret and the `PUBLISH_SNAP` variable. Versions are `vYYYY.WW.MINOR` tags from `tools/version.sh`, as in the other games. Signing in and the older share codes need `potluck` registered on game-server (share kind `creation`); the market is deployed on its own from `deploy/market`.
