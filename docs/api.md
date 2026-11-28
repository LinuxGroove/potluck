# The creation API and its promise

Every creature, block and mini game in Potluck is a creation: Luau code that asks the game for what it needs through the API's modules. The API is the contract with everyone who makes something, so it's built to outlast any version of the game. This page is that contract: what each module is at now, what the limits are, and what we promise to keep.

`creations/AGENTS.md` is the how-to for each function (and goes into every remix); `creations/types/potluck.d.luau` has the types. This page is about versions and keeping things working.

## Modules

A creation asks for a module by name, and can name the version it was made for:

```luau
local world = require("@potluck/world@0.23")
local store = require("@potluck/store")      -- any version this game has
```

and lists what it needs in `potluck.toml`, which the market checks before sharing:

```toml
[needs]
world = "0.23"
store = "0.1"
```

| Module | Version | What |
|---|---|---|
| `@potluck/world` | 0.23 | Blocks (the game's and the creation's own, drawn in `potluck.toml`, still or moving, turning to face the way they're put down, with boxes that join their neighbours) and lights, creatures (walking or flying, changing kind, behaviours that need no code, and bodies from Blockbench models), players, their health (and how fast it comes back), the blocks they carry and what's in their hand, tools that dig faster and how hard a block each reaches, what breaking a block gives, what they build and use (and use in hand on nothing), how much of a hurt reaches them, their XP and charms, how fast they dig and how hard they hit, things shot and thrown, named spots, messages, the time of day, random numbers, timers and world events |
| `@potluck/tweak` | 0.1 | Knobs anyone can turn in Tweak without touching the code |
| `@potluck/game` | 0.1 | Boards of players' bests, and sending players on to other servers |
| `@potluck/store` | 0.1 | A creation's own saved data in a world, with a quota |
| `@potluck/input` | 0.1 | Actions of its own, on pad buttons and keys players can move, while a player rides its creature or it has their controls |
| `@potluck/ui` | 0.2 | Lines on players' screens, meters beside the health bar, and questions with choices |
| `@potluck/sound` | 0.2 | The game's sounds and the creation's own (made from numbers in `potluck.toml`), somewhere in the world or for one player, with subtitles |
| `@potluck/net` | 0.2 | Messages between a creation and its client half, a second file that runs on each player's own game for what only they see, reading that game's copy of the world |
| `@potluck/test` | 0.7 | A creation's own tests, in `test.luau`: cases that play in a world of their own, with players made for the test, run by the dev bridge or `potluck-server --test` |

Every module planned for the API is here, and client halves read the world as well as hear their creation.

A creation's actions are part of `potluck.toml`, so the market and the game know them before it runs:

```toml
[actions.pat]
name = "Pat"
pad = "X"
keys = "F"
```

Versions belong to each module, never to the game. The game is `vYYYY.WW.MINOR` like every LinuxGroove game; a creation never names it.

## The promise

From a module's 1.0 on:

1. **A minor version only adds.** A creation made for `world 1.2` works with every `world 1.x` after it: nothing it uses is taken away, renamed or made to mean something else.
2. **Every major stays.** When `world 2.0` arrives, `world 1.x` keeps working as a layer over it, so a creation made in the first month still loads years later. A creation says which major it wants, and gets it.
3. **Deprecated isn't gone.** Something we'd rather creations stopped using keeps working. Its maker sees a note in Look inside, with what to use instead; players never see it.
4. **The limits are part of the API.** The numbers below only become more generous within a major. A creation that fits today fits tomorrow.
5. **Beta has no promise.** A beta module (`world 2.1-beta`) may change or go. Creations that need one can be shared by code or with friends, but not to Everyone, until it's stable.
6. **Bugs get fixed.** If a creation relies on something this page or AGENTS.md doesn't describe, a fix may change it. If a fix would break creations people play, we keep the old behaviour for the creations that need the older version.

### Before 1.0

Every module is at 0.x while the API settles in the open beta. In 0.x we keep to the same rules as best we can (minor versions have only added so far), but a 0.x minor may still change something when it has to, and AGENTS.md says what changed. When a module reaches 1.0, creations made for its 0.x keep loading: 0 counts as a major of its own and stays, as rule 2 says.

## Limits

Each creation runs in its own sandbox. These are its limits, per creation, per world:

| Limit | Now |
|---|---|
| Memory | 32 MB |
| Running its top level when it loads | 300 ms |
| Each tick (ten a second): its timers, world events and every creature's handlers together | 15 ms |
| Creatures at once | 64 |
| Kinds of creature | 16 |
| A body | 64 boxes (a Blockbench model's cubes past 64 are left out), each up to 6 blocks a side and within 8 blocks of the creature's middle |
| Timers waiting at once | 64 |
| Shortest timer | 0.1 s, the length of a tick |
| World event, input and message handlers (`world.on`, `input.on`, `net.on`) | 32 |
| Actions | 8, each on a pad button (A, X, Y, LB, RB, LT, RT or the d-pad) and a key (Space or a letter but W, A, S and D) |
| Lines on screens (`ui.show`) | 8 at once (different ids, meters among them), each up to 60 letters |
| Meters (`ui.meter`) | 3 on a screen, from every creation together |
| Questions (`ui.ask`) | One waiting for each player; 64 waiting over every player; a title of 40 letters, text of 300 and 8 choices of 30. After a player closes one, the creation's questions to them get nil for 10 seconds, until they use one of its creatures or a block it hears `block_used` for |
| Sounds | 10 a second; heard within 40 blocks |
| Blocks of its own (`[blocks]`) | 32 (16 before 0.23), each with up to 24 colours, pictures 8 to 64 pixels square and up to 16 boxes; 128 from every creation in a world together (a block that turns takes four), and as many pictures as the block atlas holds |
| Sounds of its own (`[sounds]`) | 16, each up to 3 seconds, with up to 3 layers besides the first and 32 notes a layer |
| Things given or taken (`world.give`, `world.take`) | 64 at a time; a player's pockets hold 36 slots, of up to 99 of a block or one tool each |
| Hurting or healing (`world.hurt`, `world.heal`) | 20 at a time, a whole health bar, and only players in survival |
| Flying (`flies`, `creature:climb`) | -4 to 4 blocks a second, eased into; no higher than 24 blocks over the world's highest blocks |
| Digging speeds (`world.digging`) | 0 to 10 times as fast for each kind of block, every creation's multiplied and kept to that |
| Looking for blocks (`world.find_block`, `world.find_light`) | 16 blocks away |
| Block events (`block_placed`, `block_broken`, `block_used`) | 64 heard a tick; past 512 waiting, the rest are dropped |
| Messages to client halves (`net.send`) | 30 a second from a creation, 10 a second from each player's half; a topic of 32 letters and a value of 4 KB as JSON |
| A client half | 16 MB, 300 ms to load, and 4 ms of each frame on its player's game |
| Boards | 16, of 100 players each |
| Store | 64 KB, counted as JSON |
| XP (`world.reward`) | Up to 100 at once, to players in survival; each creation gives each player at most 120 at once and 4 a second after that |
| Haste and might (`world.haste`, `world.might`) | 0.25 to 4 times as usual from each creation, every creation's multiplied and kept to that |
| Charms (`world.charm`) | 4 on a thing, each named in 1 to 16 letters, on up to 64 kinds of thing a player |
| Things said | `world.say` 120 letters, a creature's `say` 80 |

A creation over a time limit stops with an error, and the world carries on. After about 30 errors close together it's paused, and Look inside says why.

What a creation can never do, at any version: read or write files, reach the network, see another creation's memory, load code from anything but its own files, shake, roll or zoom the player's camera, or put anything over the game's own menus. Its questions always say whose they are and can always be closed.

## Changes

### `@potluck/world`

- **0.23**: `self:follow(points, speed?, more?)` sends a creature along a path of points with no gravity, gathering and losing speed gently and slowing by itself to stop at the end, and `self:following()` says how far it has left, how fast it's going and how many points are left; a kind's `steer = false` stops its rider steering it. `world.spawn` puts a creature where it says when that isn't in a solid block. A creation can make 32 blocks (16 before). The base game gains Rails and carts (rails that join round corners and up slopes by themselves, carts that roll along them, powered rails), pistons and note blocks in Circuits, and dispensers and chutes in Storage.
- **0.22**: players gain XP: `world.reward(player, xp)` gives a player in survival some (rate-limited for each creation), `world.xp(player)` is how much they have and their level, `world.spend_xp` spends it and `world.xp_for(level)` says what a level takes; the game shows it in a bar over the hotbar and says when they go up a level. `world.haste(player, share)` makes a player dig faster or slower and `world.might(player, share)` says how much harder they hit, for creations to read (each creation's share multiplies). `world.charm(player, item, charms)` and `world.charms(player, item)` keep named charms on a player's things, kept with the world. The base game gains Charms (a charm table that charms tools and weapons with XP and gloom dust), XP from mining, smelting, cooking, farming, raising animals, beating wild creatures and relighting Old Greens, and mending with XP at the anvil.
- **0.21**: `world.danger()` is the world's danger, Peaceful, Gentle (the default), Normal or Hard, which players set for each world and servers in `server.toml`; `world.on("player_fell", fn)` hears a player in survival hurt too badly, with where they fell. `self:throw(shot, landed?)` sends something flying from a creature, at a place or the way it aims, and its shots hit players (`landed` gets the player); shots can look like an `"ember"`. A kind's `home = "here"` makes each one's home where it's spawned. The game's items gain leather armor and what wild creatures leave: silk, leather, bristles, raw and cooked meat, embers, sentinel cores and gloom dust. The base game gains Cave Crawlers, Bramble Boars, Ember Imps, Stone Sentinels and Lost packs.
- **0.20**: the game's items gain seeds, berries, apples, baked potatoes, fried eggs, pumpkin and berry pies, bean and fish stews, porridge, honey, honey cake, a milk bucket and feathers. Farming grows potatoes, beans, pumpkins and berry bushes too, Hunger eats them all, Furnaces and campfires gains a cooking pot, and the new Hens, Goats and Bees creations give eggs and feathers, milk and honey. Woollies, hens and goats have young when two are fed.
- **0.19**: `world.shoot(player, shot, landed?)` sends an arrow, a stone or a spear flying the way a player aims; a creature it hits hears `hit(player, shot)` with it, and the creation hears where it landed. `world.aims(item)` makes Interact with a thing in hand always `item_used`, even at a block or a creature. The game's items gain a bow, arrows, a sling and a spear; the workbench makes them, and the new Bows, slings and spears creation shoots them. Gloomlings take a shot's power.
- **0.18**: `world.on("item_used", fn)` hears a player Interact with nothing in reach, with what's in their hand and the way they look; `world.guard(player, share)` is how much of every `world.hurt` reaches a player (creations' shares multiply), and `world.hurt`'s `guarded = false` is a hurt it doesn't touch. The game's items gain caps, helmets, coats, leggings and boots in wool, copper, iron and moonsilver, and a shield. The game's workbench makes wool and copper armor and shields, the anvil makes iron and moonsilver armor, and the new Armor creation puts them on.
- **0.17**: with an item in hand, Interact on water is heard as `block_used` with `"Water"` (it used to go through to the block behind, or nothing). The game's items gain shears, a bucket and a water bucket, a fishing rod, a watering can, a sickle, a hammer, raw fish and cooked fish. The game's workbench makes them, and the new Buckets and fishing creation fills buckets, pours them out and fishes.
- **0.16**: the game's items gain copper, iron and moonsilver pickaxes, axes, shovels and swords, and copper, iron and moonsilver hoes. The game's workbench makes copper tools and an anvil, and the anvil (the Tools creation's) makes iron and moonsilver ones and mends worn tools.
- **0.15**: blocks of a creation's own can turn (`turns = true`: they face north, east, south or west, the way the player looked when they put them down, with the top picture and boxes turned to match) and have boxes that join (`{ box = [...], joins = "east" }`: shown only when the block on that side is one of the same creation's). `world.facing(pos)` says which way one faces, and `world.set_block` takes the way to face; changing one turning block into another keeps it. The game's workbench gains Circuits.
- **Every version**, from 27 November 2026: the world goes on past the island in every direction (to 30,000 blocks out) and down to bedrock at y = -128, where it stopped at the island's edge and y = 0. Land is made as players come near it, so far from everyone `world.block` says `"Air"` and `world.top` says where the ground will be; `world.set_block` there makes the land first. The island, 0 to 384 each way, is always there.
- **0.14**: moving blocks: a block of a creation's own can give `frames`, more pictures it shows in turn (up to 8 in all), and `speed`, how many a second (0.5 to 8). Players with the new Reduce flashing setting on see only the first. The game's items gain charcoal, flint, copper, iron, gold and moonsilver ingots, a flint and steel, bread and roasted carrots.
- **0.13**: `world.drops`, what breaking a block gives in survival (another thing, a number of them or a range, or nothing) and the level of tool it needs before it breaks; and `level` for `world.digging`, how hard a block a thing in hand reaches among the kinds it digs faster (0 to 9; creations' levels take the highest). A block that needs a better tool doesn't break, and the player is told what would.
- **0.12**: blocks of a creation's own, `[blocks.name]` in `potluck.toml`: pictures drawn in letters with a colour for each, a shape (solid, see-through, a plant, or boxes in sixteenths), how hard it is to break, its kind, and light and glow. Every block function takes their names; a world keeps their ids for as long as it lasts, and sends them to everyone who joins.
- **0.11**: `world.model`, a Blockbench model (a `.bbmodel` in the creation's folder) as the list of boxes a body is, 16 pixels to a block, drawn with the model's texture (a part's `texture`; its `color` tints it), with `head`, `leg` and `glow` from its cubes' and groups' names; and `body = "file.bbmodel"` for `world.creature` as short for it. Creations may carry `.bbmodel` files.
- **0.10**: `behaviours` for `world.creature`, what a creature does with no code: `wander`, `follow_food`, `follow_when_used`, `flee_when_hit`, `hop_when_hit`, `chatter` and `sound`. They act before its own handlers, which have the last word.
- **0.9**: `flies` for `world.creature` and `creature:climb` (fliers with no gravity, easing into how fast they rise or sink, steered by riders in the air), `creature:on_ground` and `creature:become` (turn into another of the creation's kinds, keeping its fields, and still counting toward the `count` of the kind it came as). `world.spawn` puts a creature wider than a block that's set down beside a wall just clear of it.
- **0.8**: `world.held` (what's in a player's hand), `world.kind` (what a block is made of) and `world.digging` (a thing held breaks some kinds of block faster, 0 to 10 times; creations' speeds multiply). Tools and swords are things to hold, never put down: `world.set_block` with one is an error.
- **0.7**: `down_to` for `world.hurt`, the least health a hurt leaves (0 to 20).
- **0.6**: `world.healing`, how fast a player heals by themselves (0 to 4, 1 as usual; creations' rates multiply).
- **0.5**: `world.on` for `"block_placed"`, `"block_broken"` and `"block_used"`: a player put down, broke or used (Interact on) a block, with the player, the middle of the block and its name.
- **0.4**: `world.daytime` and `world.daylight` (the time of day), `world.find_light` (the nearest lantern, brazier, candle or other block that gives light), and `world.hurt` and `world.heal` for survival players' health. A client half has `daytime` and `daylight` too.
- **0.3**: `world.pockets`, `world.has`, `world.take` and `world.give`, for the blocks players carry in survival. Since the backpack came, players carry 36 slots rather than nine kinds, and `world.give` can give fewer than asked when they're nearly full. Its answer was always how many it gave.
- **0.2**: `world.after`, `world.every` and `world.cancel` (timers), and `world.on` for `"player_joined"` and `"player_left"`. A creation with no creatures can now run code every tick.
- **0.1**: the first version.

### `@potluck/input`

- **0.1**: the first version: `[actions]` in `potluck.toml`, and `input.on`, `input.held`, `input.focus` and `input.focused`.

### `@potluck/net`

- **0.1**: the first version: `client` in `potluck.toml` for a client half, and `net.send` and `net.on` in both halves, with `net.me` in the client half. A client half has `ui.show` and `ui.hide`, `sound.play` and `play_for`, and `world`'s timers, `time`, `random` and `spot` (and from world 0.4, `daytime` and `daylight`).
- **0.2**: a client half reads its player's game's copy of the world: `world.block`, `top`, `find_block` and `kind`, and `players` and `nearest_player` for the players that game knows of. `net.me()` adds `look` and `aim`.

### `@potluck/sound`

- **0.1**: the first version: `sound.play`, `sound.play_for` and `sound.names`, with the game's own sounds.
- **0.2**: sounds of the creation's own, made from numbers in `potluck.toml` (`[sounds.moo]`: a wave, a pitch or notes, a length, how it rises and fades, wobble, pulse, filters, and up to 3 layers). `play`, `play_for`, `names` and behaviours' `sound` find them first. Client halves play them too.

### `@potluck/ui`

- **0.2**: `ui.meter`, a bar beside the health bar (0 to 1, in a colour of the creation's own). A client half has it too.
- **0.1**: the first version: `ui.show`, `ui.hide`, `ui.ask` and `ui.close`.

### `@potluck/store`

- **0.1**: the first version: `store.get`, `store.set`, `store.keys` and `store.used`.

### `@potluck/game`

- **0.1**: the first version: boards and links to other servers.

### `@potluck/tweak`

- **0.1**: the first version.

### `@potluck/test`

- **0.1**: the first version: `test.case`, `check` and `same`; `wait`; `player` and `move`; what players do (`place`, `dig`, `use_block`, `use`, `hit`, `ride`, `get_off`, `press`, `release`, `answer`, `hold`); and looking (`kind`, `creature`, `health`, `shown`, `asked`).
- **0.2**: `test.heard(name)`, how many times the creation played a sound since the case began.
- **0.3**: `test.sent(player, topic)`, what the creation last sent that player's client half on a topic and how many times, and `test.from_half(player, topic, value?)`, a message to the creation as that half would send it.
- **0.4**: `test.dig` breaks with what the player holds, as the game does: a survival player gets what `world.drops` says, and it returns false (breaking nothing) when the block needs a better tool.
- **0.5**: `test.place(player, pos, block, facing?)` puts down a block that turns facing that way.
- **0.6**: `test.use_item(player, aim?)`, Interact with nothing in reach, heard as `item_used`.
- **0.7**: `test.danger(level)`, the world's danger for the case; a survival player whose health runs out is heard as `player_fell`.
