# Potluck

A block sandbox where everything is made by someone you know. Build, explore and play in a world of blocks, and look inside anything in it: every creature, block and mini game is a small Luau creation you can tweak, remix and share with a code.

The concept is idea 26 in [game-ideas](https://github.com/LinuxGroove/game-ideas/blob/main/ideas/26-potluck.md). The game is written in Rust on [Bevy](https://bevy.org/), and creations run in [Luau](https://luau.org/) sandboxes through [mlua](https://github.com/mlua-rs/mlua).

This is a test build of milestones M0 to M3 of the plan: a hilly island with the Green at its middle, a dozen creatures and two mini games (all creations, written in Luau), Look inside, Tweak, Remix with hot reload from any editor, building and survival basics, worlds friends can join, the market (creations shared to Everyone or by code, their versions and family trees on the web, and changes suggested from a remix and merged from the game), and `potluck-server`: an always-on world with HTTPS, rules for who may join, a place in the market's server list, boards for its mini games and links to other servers.

## Playing

| | Keyboard and mouse | Controller |
|---|---|---|
| Move, look | W A S D, mouse | Left stick, right stick |
| Jump (twice quickly to fly, outside survival) | Space | A |
| Crouch, get off | Shift | B |
| Sprint | Left Ctrl | Left stick click |
| Break, place | Left and right mouse buttons | Right and left triggers |
| Choose a hotbar slot | 1 to 9, mouse wheel | Bumpers |
| Every block (creative): pick one for your hand; your backpack (survival) | I | Tap Y |
| The block you aim at, into your hand (creative) | Middle mouse button | |
| Interact (ride a Mossback) | F or E | X |
| Look inside what you aim at | L or Tab | Hold Y |
| Share what you aim at or ride | P | D-pad left |
| First or third person | F5 | Right stick click |
| Map | M | View |
| Chat | T or Enter | D-pad up |
| Mark a waypoint where you stand | B | D-pad down |
| Minimap nearer, further or off | N | D-pad right |
| Pause | Esc | Start |

**Play alone** lists your worlds: make a new one or carry on in one you left. Each world saves as you play (every two minutes, when you pause, and when you leave or quit), with everything built in it, what creations keep, what you carry, your health and where you stood. **Host a world for friends** opens one of them to friends the same way. Every world is the same island with the Green at its middle, different in its details.

Walk down the hill from where you start to the Green. The Mossbacks are in the hay paddock east of the well: walk up to one and Interact to climb on. The Rodeo Ram is there too, if you want your longest ride on its board (its clock runs at the bottom of your screen while you hold on), and the Tag Bot on the Green keeps a board of who's caught it most. Woollies, Glowbugs, Jellies, two Gnome Gardeners (Interact with one and pick what you'd like to know) and the villagers Rowan and Ivy, who trade in survival (logs for planks, sand for glass, glass and stone for lanterns), live on the Green, ducks, frogs and Pebble Crabs by the creek north-east of it, and pups, moles and hedgehogs on the hill (hold carrots and the hedgehogs come trotting). In survival, mind the dark: Gloomlings come out at night and nip at your heels, but they can't bear light, so a lantern by your door keeps them off (bop one twice and it pops, or once with a sword). Any table is a workbench in survival: Interact on one to make planks, furniture, lights, fences, farmland, tools and swords from what you carry. Crates and barrels keep what you put in them: use one while holding something to put it in, or to take things out. A workbench makes a kiln from cobblestone: use it to fire dirt into red bricks, gravel into terracotta, terracotta into roof tiles and slabs, and sand into glass. Hold a pickaxe and stone breaks faster, an axe for wood and a shovel for earth and sand; the grey bar beside your health is how much wear the tool has left. Ores need a pickaxe before they break: coal a wooden one, copper, iron and the rest a stone one, and each breaks into what it holds (coal, raw copper, raw iron, raw gold, crystal or raw moonsilver). Old chests left in old workings underground hold prizes: use one and it gives you what's inside, better the deeper it was. Pat a Woolly in survival and you get wool, which grows back in a few minutes. With wool, planks and a lantern a workbench makes a balloon basket: put it down and it fills up into a hot air balloon. Climb in, hold Burner to rise and let go to sink slowly (Vent comes down faster), and steer as you walk; it rises and turns gently and never tips, and the top right of your screen says how high you are and what's below. Land before you climb out, and bop it once it's down to pack it back into a basket. Far from the Green lie three Old Greens in ruins, by the shore, in the woods and on the heights: take a lantern and something from round about (sand, logs or stone) to one's cold hearth and use it to light it again, and it gives you something back. To farm, Interact on grass to dig it into farmland and put wheat or carrots on it: they ripen in a few minutes (faster by day and near water) and give back three for each one you planted. Keep some with you: the amber bar beside your health is food, and you eat what you carry when you get hungry. On an empty belly you stop healing, and it slowly hurts you down to half your health. While you ride one, Pat calms it down a little; creations can have buttons of their own like that, shown at the bottom of the screen, and **Settings, Creations' buttons** moves them to buttons you like better. Every sound is made by the game from numbers (steps, digging, the creatures' voices, and the sounds creations make of their own, like the balloon's burner), and **Settings** has **Sound volume** and **Subtitles for sounds**, which say what you heard and which side it came from. Look inside it to read its code, Tweak its knobs (Buck force is the fun one), or Remix it into your own copy: the game opens its folder, and whatever you save there in any editor runs at once. Share it and your friends load it by its eight letter code. A creature's body can be a model made in [Blockbench](https://www.blockbench.net): the Hedgehog's is `hedgehog.bbmodel`, so Remix it and open that in Blockbench to change its shape and the picture painted on it. Blocks are creations too: Brickworks' bricks, tiles, kiln and paper lanterns are drawn in letters in its `potluck.toml`, so Remix it and change a colour or a row, and the world shows it as you save. In creative, I (or a tap of Y) shows every block, those creations made on a page of their own, to put one in your hand. In survival the same opens your backpack: 27 slots besides the hotbar's nine, up to 99 of a block in each and one tool. Choose a slot to move what's in it, or X (F) to move half, then where it goes: onto the same thing it stacks, onto something else they swap. Tidy the backpack packs it in order. What you gather fills the hotbar first, and when a hotbar slot runs out as you build, the backpack's next lot of it comes to hand.

The minimap in the top right corner shows the land round you as it really is, block by block, with friends, creatures and your waypoints on it; under a roof or underground it shows the level you're on instead, like a plan of the rooms and tunnels. The map (M, or View on a pad) shows the whole island: move it with the mouse or the left stick, zoom with the wheel or the triggers, and mark any spot as a waypoint. Waypoints have names and colours, show over the world with how far they are, and can be shared in chat, where they appear on everyone's map to go to or keep. Outside survival you can go anywhere on the map, to any waypoint or to a friend, at once. In survival you can go back only to your own waypoints at places you've stood, after standing still for three seconds, and once a minute; the map also marks where you last fell. Going somewhere fades the screen out and in rather than moving the camera.

Chat (T, or D-pad up) goes to everyone in the world, through whoever hosts it, so it works on a LAN with no internet. On a pad, A on the line brings up a keyboard on the screen, and the quick lines below it need no typing. Lines are tidied and the worst words hidden; younger players chat with the quick lines, and a parent can turn chat off in **Parent settings**.

The four stone arches on the north side of the Green show what's in this world that someone brought (a friend's creations, or yours) and, in the arches left over, what the market offers: favourites of the last month's jams, entries in this week's, and what's trending. Walk up to one and Interact to bring it into your world, or to look inside one that's here already. Younger players only see creations a moderator checked for all ages.

### Playing with friends

- **On the same network:** one player chooses Host a world for friends; the others choose Join a friend's world, where the world shows up by itself.
- **Over the internet:** run `potluck-server` somewhere friends can reach (below), and everyone joins by its address: `192.168.1.20`, `potluck.example.org:47126` or `https://potluck.example.org`. Servers listed on the market show under **Servers on the market** in Join a friend's world.

Creations the host or anyone else brings into a world are sent to everyone who joins, and Tweak changes show for everyone at once.

## The market

Share something you made from Look inside: **Share to Everyone** puts it on the market at [potluck.linuxgroove.com](https://potluck.linuxgroove.com) once a moderator has looked at it, and **Share by code only** gives it a code for the people you tell. Each creation keeps one code, and sharing it again makes its next version. On the web anyone can read its code, its versions and what changed in each, and its family tree of remixes.

Remix something from the market and change it, and **Suggest my change to the original** sends your change to its maker. They see it under **Changes others suggested**, can try your remix in their world, and merge it into theirs from the game, with your name on the change.

There are no passwords and no emails. The game signs in to the LinuxGroove game server with a random id for your device, and **The market** (in the pause menu and on the title screen) or **Your profile on the market** (in Settings) opens the market signed in as you, or shows a QR code to carry on on your phone: handy on a controller. The profile is a name, a line about you and a block head picked from a few colours, eyes and hats. While the market is new, sharing to Everyone needs an invite code, entered on your page there.

One account works on several devices. In **Settings**, **Link a device** shows a code on a device that's already yours; type it on the new one, on the same screen, and it plays as you, with your name, block head and what you've shared (servers go by it too). The market's **Your devices** page lists them, unlinks them, makes link codes and makes a **recovery code**: write it down, and if you lose every device it brings your account to a new one, once. A device that already shared things of its own can't be swallowed by another account; link the other way.

### Jams

Each week a moderator sets a theme ("Things that float") on the market's **Jams** page. While it runs, make something for it, share it to Everyone and choose **Enter it in this week's jam** under Share (or on its page on the market); a maker can enter three. When it ends, the four entries that got the most stars while it ran are its favourites, and they go on the arches of every new world's Green for a month. The jam running now shows on Explore, in the pause menu and in Creations.

### Younger players and parents

The first time a player does something that reaches other people (sharing, the market, servers, joining a world), the game asks once who's playing: **Under 13** or **13 or older**. Playing alone never asks. Younger players share by code only, for friends; they can't send suggestions to other makers; their market profile has no line about them; and the server list shows only servers that say they're for all ages, while listed servers that don't turn them away (a friend's world always lets them in). **Settings, Parent settings** changes the age and can turn sharing or servers from the market's list off, behind a PIN of 4 to 8 numbers, kept only as a hash. A forgotten PIN can be cleared by deleting the `parent_pin` line from `settings.toml` in Potluck's folder.

On the market, a moderator can mark a creation **Checked for all ages** (when they list it, or later on its page), and Explore has a filter for those. The base game's creations are checked for all ages, and a new version starts without the mark until someone looks again.

The market's service is `potluck-share` ([crates/share-service](crates/share-service)): it keeps every creation as a git repository in [Forgejo](https://forgejo.org/) and its files by hash in a folder or S3, checks every version (size, the API it needs, that the code compiles, words), signs what it serves so the game knows a code brought what the market checked, and serves the web pages. [deploy/market](deploy/market) has its config and a compose file with Forgejo and Caddy; `deploy/market/test.sh` runs its tests against Forgejo in a container. `potluck-share check FOLDER` runs the market's checks on a creation before sharing it.

## potluck-server

A world that runs with nobody at the keyboard. It keeps the world and the creations players bring between restarts, and a web browser pointed at its address shows who's playing.

```sh
sudo snap install potluck-server
sudo potluck-server.info                                      # the addresses friends type
sudo snap set potluck-server domain=potluck.example.org       # HTTPS with a Let's Encrypt certificate (ports 80 and 443)
sudo snap set potluck-server tls.cert=fullchain.pem tls.key=privkey.pem   # or your own, kept in /var/snap/potluck-server/common
sudo snap set potluck-server name="Riverbend" port=47126 seed=26 beacon=false
```

With `domain` set, the snap runs Caddy in front of the world for automatic certificates. With a certificate of your own, the server speaks TLS itself on `tls.port` (443 unless set). Either way the plain port keeps working for players on the local network.

The rest of its settings are in `server.toml` in its data folder (`/var/snap/potluck-server/common/world/server.toml` in the snap), written on the first start with every setting explained ([packaging/server.toml](packaging/server.toml) is the same file):

- **Who may join**: `join = "anyone"` who knows the address, `"signed-in"` players the market vouches for (the game asks the market for a pass naming this server's address, good for five minutes), or `"list"` for only the handles in `allow`; `ban` keeps handles out; `max_players` (50). A listed server is for players 13 and older unless `all_ages = true`.
- **What players may do**: `build` (place and break blocks), `bring` (their own creations: `"anyone"`, `"signed-in"` or `"nobody"`), `teleport` (go to waypoints and friends from the map) and `chat`.
- **Chat**: every line is written to `chat.jsonl` in the data folder, for you to read; put a `chat_words.txt` there (one word a line, `word*` for words starting with it, like [assets/chat_words.txt](assets/chat_words.txt)) to hide your own list of words instead of the game's.
- **Creations it always runs**, by market code: `creations = ["ABCD2345", "WXYZ6789@3"]` (the newest version each start, or the one after `@`).
- **The server list**: make a token on the market's **Your servers** page, then set `list = true`, `address` (what players type) and `market_token`. The server sends a heartbeat every minute; the market lists it once it has reached the server's status page at that address, and shows its creations' boards on their pages.
- **Links to other servers**: `[links] rodeo = "wss://rodeo.example.org"`, which creations send players to with `game.send(player, "rodeo")`.

Outside the snap: `potluck-server --help`, and `potluck-server --check` to read the settings and say what's wrong. Every command line option can also come from the environment (`POTLUCK_CONFIG`, `POTLUCK_NAME`, `POTLUCK_PORT`, `POTLUCK_TLS_PORT`, `POTLUCK_CERT`, `POTLUCK_KEY`, `POTLUCK_SEED`, `POTLUCK_MAX_PLAYERS`, `POTLUCK_HOME`), and wins over `server.toml`.

The status page's `/status.json` has what a soak test needs: players, creatures, the average and slowest tick of the last ten seconds, bytes sent a second and time up. `potluck-bots --address=ws://HOST:47126 --bots=50 --minutes=600 --build --report=soak.jsonl --pid=SERVER_PID` joins that many bot players, who walk about, build, and pat, bop and ride creatures, and writes a line of numbers a minute: with `--pid`, the server's memory, CPU and threads too, and what the bots received by message.

[docs/hosting.md](docs/hosting.md) is the guide to running Potluck for more people: what a world, a player and a creation cost (measured), the minimum machines, a cheap start, deploying worlds and the market, hosted worlds that scale, and the code changes on the way to hundreds of thousands of players.

## Making creations

Remix anything from Look inside, or put a folder in `creations/` of your Potluck folder (`~/snap/potluck/common` for the snap, `~/.local/share/potluck` from source). A creation is a `potluck.toml` and Luau files (a creature can be just a body and `behaviours` from a library: wander, follow food, follow whoever pats it, run off, chatter); [creations/AGENTS.md](creations/AGENTS.md) describes the API, and every remix gets a copy of it and of the API's types, so editors and AI helpers know what's there. A coding helper on the same computer can also work with the running game through the dev bridge (**Settings, Dev bridge for coding helpers on this computer**, or `--bridge`): an MCP server at `http://127.0.0.1:47130/mcp` that lists creations, reloads them, reads their logs, runs a line inside one, turns knobs, looks around the player, takes screenshots, type checks a creation and runs its own tests: a `test.luau` whose cases play in a world of their own (`potluck-server --test=FOLDER` runs them too). As you save, Look inside also lists what Luau's type checker is sure is wrong in your own creations, with the line: a misspelt name, a call given the wrong number of values, text where a number goes. [creations/mossback](creations/mossback) is the worked example, and [creations/rodeo-ram](creations/rodeo-ram) and [creations/balloon](creations/balloon) have client halves: a second file that runs on each player's own game, for what only they see (the balloon's reads the world under you to show how high you are).

The API comes in modules, each with its own version (`@potluck/world` 0.12, `tweak` 0.1, `game` 0.1, `store` 0.1, `input` 0.1, `ui` 0.2, `sound` 0.2, `net` 0.2, `test` 0.3), never tied to the game's version. [docs/api.md](docs/api.md) is the promise that goes with them: what stays working as the API grows, and the sandbox's limits.

## Building from source

Rust 1.97 or newer, and on Linux Bevy's usual dependencies:

```sh
sudo apt install pkg-config libasound2-dev libudev-dev libwayland-dev libxkbcommon-dev
cargo run --bin potluck -- --play          # straight into the world
cargo run --bin potluck-server             # an always-on world on port 47126
cargo test                                 # the world, creations, the sandbox, joining, TLS
```

`POTLUCK_HOME` sets where the game keeps its world, settings and creations. Builds from source share codes only on a local game server (the public server's key is added by CI) and send no launch ping.

### Seeing it without a GPU

`potluck --demo=DIR --size=1280x720` plays through the game on its own (walk to the Green, ride a Mossback, Look inside, Remix, change the code, Tweak, build, nightfall) at a fixed 30 frames a second however slowly the machine draws, and records `DIR/potluck-test-run.mp4` (with ffmpeg) and stills. With Mesa's software Vulkan:

```sh
VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json xvfb-run -a -s "-screen 0 1280x720x24" \
  target/debug/potluck --demo=/tmp/demo --size=1280x720
```

`potluck --shot=out.png --after=8 --at=x,y,z,yaw,pitch --time=0.5 --screen=inside` takes one screenshot, and `--lighting=high` tries a lighting tier without changing your setting. Built with `--features raytracing`, Ultra is ray traced on graphics cards that can (NVIDIA RTX, recent AMD and Intel Arc).

## Releases

Pushing to `main` builds both snaps (`potluck` and `potluck-server`, for amd64 and arm64) and the Windows and macOS zips; publishing the snaps waits on the names being registered, the `STORE_LOGIN` secret and the `PUBLISH_SNAP` variable. Versions are `vYYYY.WW.MINOR` tags, derived by `tools/version.sh` as in the other LinuxGroove games.

## License

The code is MIT; see [LICENSE](LICENSE). The fonts (Bungee and Barlow) are under the SIL Open Font License ([assets/fonts/OFL.txt](assets/fonts/OFL.txt)). Creations carry their own licence in their `potluck.toml`; the base game's are CC-BY-4.0.
