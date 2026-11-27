# Building a Potluck creation

This folder is a Potluck creation: plain text that the game runs in a sandbox. Edit any file and save; the game reloads it within a second, keeping the creatures where they are and what they remember. If the code has an error, the last good version keeps running and the game shows the error and its line.

## Files

| File | What |
|---|---|
| `potluck.toml` | Its name, who made it, what kind of thing it is, which file to run (`main`), the API it needs (`[needs] world = "0.3"`, `game = "0.1"` for boards, `store = "0.1"` to keep things, `input = "0.1"` for buttons of its own, `ui = "0.1"` for lines on the screen and questions, `sound = "0.2"` for sounds (and `[sounds]` of its own), `net = "0.1"` for a client half), its `client` file if it has one, its `[actions]` if it has any, its `[blocks]` if it makes any (world 0.12), and its licence. A remix also says what it's a remix of (`remix_of`, and `[from]` with the market's code and version): leave those as the game wrote them, so the market can show its family tree and send suggested changes back |
| `main.luau` | The code, in Luau (Lua 5.1 with types). `require("./other")` loads `other.luau` from this folder. It runs on the world's host, the only place the world changes |
| `client.luau` | Only if it has a client half (`client = "client.luau"` in potluck.toml): code that runs on each player's own game, for what only they see. See `@potluck/net` |
| `*.bbmodel` | Blockbench models, for creatures' bodies (`world.model`). Make them in Blockbench (any format; save the project, File > Save Model, with the texture kept inside it) |
| `test.luau` | Only if it has tests of its own: cases that play in a world of their own, to check it does what it should. See `@potluck/test` |
| `types/potluck.d.luau` | The API's types, for editors and AI helpers. Don't edit it: the game writes it |

## The API

Modules are versioned on their own, never by the game's version: `require("@potluck/world")` or, to be exact, `require("@potluck/world@0.12")`. A newer minor version only adds to an older one, so a creation made for `world 0.1` runs with 0.12. The game's `docs/api.md` is the promise that goes with the versions, and lists the limits.

### `@potluck/world` 0.12

Positions are Luau vectors: `vector.create(x, y, z)`, with `+`, `-` and `*` working on them, `.x`, `.y` and `.z` to read them. A table `{ x, y, z }` works too. y is up; one block is 1.

- `world.creature(def)` defines a kind of creature and returns it, to add handlers to. `def` has:
  - `name` (required), `speed` (blocks a second, default 2), `count` (how many the game keeps in the world, default 1; one that has `become` another kind still counts as the kind it came as), `home` (`"green"`, `"paddock"`, `"creek"`, `"start"` or a position: where they appear and stay near), `rideable`, `seat` (the height a rider sits at), `flies` (new in 0.9: no gravity; it rises and sinks as `self:climb` says)
  - `body`: a list of boxes, each `{ "name", size = { w, h, d }, at = { x, y, z }, color = "#rrggbb" }`, with `leg = true` to swing as it walks, `head = true` to bob, `glow = true` to light up. The creature faces negative z. Or (new in 0.11) the name of a Blockbench model in the creation's folder, `body = "hedgehog.bbmodel"`: see `world.model`.
  - `size = { w, h, d }`: the box it bumps into things with (worked out from the body if left out)
  - `behaviours` (new in 0.10): what it does by itself, with no code. See below.
- `world.model(file, scale?)` (new in 0.11) reads a Blockbench model (a `.bbmodel` file in the creation's folder, saved from Blockbench) into the list of boxes a `body` is, so code can change it first (`part.color = knobs.fur`). Each cube is a part: 16 pixels to a block (`scale` 0.1 to 4 changes that), drawn with its faces' pictures from the model's texture, as Blockbench shows them. Its `color` is the colour most of its faces show (or its marker colour if it has no texture): change it and the picture takes on the new colour, keeping its detail. Its `texture` says where its picture is in the model; set it to nil to paint the part in its colour instead, like a part you write yourself. `head = true` if its name or a group it's in says "head", `leg = true` for "leg" and `glow = true` for "glow". The front is Blockbench's north (negative z). Bodies are upright boxes, so a turned cube is drawn upright and meshes are left out; the log says so. 64 cubes at most.
- `world.block(pos)` is the name of the block there (`"Grass"`, `"Air"`, ...). `world.set_block(pos, name)` changes it.
- `world.top(x, z)` is the height of the highest solid block. `world.find_block(pos, radius, name)` is the nearest block of that name within `radius` (up to 16), or nil.
- `world.players()` is a list of players; `world.nearest_player(pos, radius?)` is the nearest one, or nil. A player is `{ id, name, position }`.
- `world.spawn(kind, pos)` adds a creature; `world.creatures(kind?)` lists this creation's creatures by id.
- `world.spot(name)` is a named place: `"green"`, `"paddock"`, `"creek"` (north-east of the Green), `"start"`.
- `world.say(text)` shows a message to everyone. `world.time()` is seconds since the world started. `world.random()` is 0 to 1; `world.random(a, b)` is between a and b.
- `print(...)` writes to the creation's log, shown in Look inside.

Timers and world events (new in 0.2), for code that isn't one creature's:

```luau
local round = world.every(60, function()
	world.say("A new round!")
end)
world.after(5, function() world.say("Five seconds in") end)
world.cancel(round)                       -- stops a timer; true if it was waiting

world.on("player_joined", function(player) world.say("Hello, " .. player.name) end)
world.on("player_left", function(player) print(player.name .. " went home") end)
```

- `world.after(seconds, fn)` calls `fn` once, `seconds` from now; `world.every(seconds, fn)` calls it every `seconds`. Both return an id for `world.cancel(id)`. Timers run on the world's ticks, ten a second, so 0.1 is the shortest; at most 64 wait at once.
- `world.on(event, fn)`: `"player_joined"` when someone arrives in the world and `"player_left"` when they go, each with the player. Players already here when the creation starts are in `world.players()`. Block events are below (0.5).
- A creation's top level runs again when it reloads, and sets its timers and handlers up again. Timers started inside handlers don't carry over a reload.

The blocks players carry (new in 0.3), for trading, crafting and prizes:

```luau
if world.has(player, "Oak log") >= 2 and world.take(player, "Oak log", 2) then
	world.give(player, "Planks", 8)
end
local carried = world.pockets(player)     -- { ["Oak log"] = 3, Sand = 2 }, or nil in creative
```

- In survival a player carries what's in their hotbar, nine kinds of block at most. In creative they have every block, and nothing is counted.
- `world.pockets(player)` is what they carry, by block name, or nil for a player in creative.
- `world.has(player, block)` is how many of that block they carry (`math.huge` in creative).
- `world.take(player, block, n?)` takes `n` (1 if left out) if they carry that many, and returns whether it did. In creative it returns true and takes nothing.
- `world.give(player, block, n?)` puts `n` in their hotbar and returns how many it gave: 0 if they've nine other kinds already. In creative it returns `n` and gives nothing.
- `n` is a whole number from 1 to 64. Players can't carry air, water or bedrock.
- A player's blocks live on their own game, so what a creation sees is what their game last said, a moment ago. A player nobody's heard from yet carries nothing.

Night and day, lights, and players' health (new in 0.4), for things that come out after dark:

```luau
if world.daylight() < 0.15 and not world.find_light(self.position, 6) then
	world.hurt(player, 1)                 -- true if they're in survival
end
world.heal(player, 4)
print(world.daytime())                    -- 0.5 is noon
```

- `world.daytime()` is the time of day, 0 to 1: 0 is midnight, 0.25 sunrise, 0.5 noon and 0.75 sunset. A day is 20 minutes.
- `world.daylight()` is how light it is, 0 in the dead of night to 1 in full day. It's 1 through most of the day and 0 through most of the night, and moves between them in the minute or so around sunrise and sunset.
- `world.find_light(pos, radius)` is the middle of the nearest block that gives light (a lantern, brazier or candle) within `radius` (up to 16), or nil.
- `world.hurt(player, amount)` takes `amount` from a player's health and `world.heal(player, amount)` gives it back. Health is 0 to 20; at 0 the player goes back to the hill. Only players in survival have health, so both return false, and do nothing, for anyone in creative. `amount` is 0 to 20.

What players do to blocks (new in 0.5), for crafting tables, farms and anything a player builds:

```luau
world.on("block_used", function(player, at, block)
	if block == "Table" then
		print(player.name .. " wants to make something")
	end
end)
world.on("block_placed", function(player, at, block) end)
world.on("block_broken", function(player, at, block) end)
```

- `world.on("block_placed", fn)` and `world.on("block_broken", fn)` hear a player put a block down or break one; `"block_used"` hears a player Interact on a block (aiming at it, not at a creature). Each calls `fn(player, at, block)`, with the middle of the block and its name (for `block_broken`, what it was).
- They're only for what players do: blocks creations change with `world.set_block`, and water flowing, aren't heard.
- At most 64 are heard a tick; more wait their turn, and past a few hundred waiting the rest are dropped.

How fast players heal (new in 0.6), for hunger, rest and healing springs:

```luau
world.healing(player, 0)                  -- an empty belly: no healing by themselves
world.healing(player, 2)                  -- well fed: twice as fast
world.healing(player, 1)                  -- as usual again
```

- Survival players heal by themselves, one health every 4 seconds. `world.healing(player, rate)` changes how fast for that player: 1 as usual, 0 not at all, up to 4. It stays until the creation changes it again, stops, or the player leaves, and returns false for a player whose game hasn't said hello yet.
- When several creations set it, their rates multiply, so any creation's 0 means no healing.

Hurting only so far (new in 0.7), for hunger, thorns and anything that should sting but not send a player back to the hill:

```luau
world.hurt(player, 1, { down_to = 10 })   -- never below half health
```

- `down_to` is the least health the hurt leaves, 0 to 20 (0, or leaving it out, is no limit). A player already at or below it isn't hurt at all.
- Only the player's own game knows their health, so it works out how much to take when the hurt arrives.

What players hold, and tools (new in 0.8), for pickaxes, swords, wands and anything else that does something in hand:

```luau
if world.held(player) == "Stone sword" then hits = 3 end     -- what's in their hand, or nil
world.digging("Wooden pickaxe", { hard = 2.5 })               -- stone and brick break 2.5 times as fast
world.digging("Wooden pickaxe", nil)                          -- as usual again
print(world.kind("Oak log"))                                  -- "wood"
```

- Some blocks are things to hold, not to build with: the pickaxes, axes, shovels and swords. They're carried, given and taken like any block, but never put down, and `world.set_block` with one is an error.
- `world.held(player)` is the name of what's in the chosen slot of their hotbar, or nil for an empty hand (an empty slot in survival).
- `world.kind(block)` is what a block is made of: `"soft"` (earth, sand, gravel, wool), `"hard"` (stone, brick, ore), `"wood"`, `"plant"`, `"glass"`, `"water"` or `"air"`.
- `world.digging(block, speeds)` makes a survival player holding `block` break each kind of block faster: speeds by kind (soft, hard, wood, plant or glass), from 0 to 10, 1 as usual and 0 not at all. nil puts it back. When several creations set speeds for the same thing, they multiply; they last until the creation changes them or stops. Breaking happens on the player's own game, so it's told the speeds.
- Nothing wears out by itself: count `block_broken` while `world.held` is your tool, and `world.take` it when it's done.

A creature's handlers, written as `function Kind:handler(...)`:

- `spawned()` when it first appears
- `tick(dt)` ten times a second; `dt` is the seconds since the last
- `used(player)` when a player interacts with it (if it isn't rideable)
- `ridden(player)` when a player climbs on (rideable ones climb on by themselves)
- `hit(player)` when a player hits it

Inside a handler, `self` is that creature. Fields you set on it (`self.mood = "happy"`) are kept across reloads. It has:

- `self.position`, `self.id`, `self.kind`, `self.rider` (the player riding it, or nil)
- `self:walk_to(pos, speed?)`, `self:wander(speed?)`, `self:stop()`, `self:hop(power?)`, `self:face(pos)`
- `self:say(text)` (a speech bubble), `self:distance_to(pos)`, `self:throw_rider(force?)`, `self:remove()`
- `self:on_ground()`, and (new in 0.9) `self:climb(speed)` and `self:become(kind)`, below

Fliers and changing kind (new in 0.9), for balloons, birds, hatching eggs and anything with more than one look:

```luau
local Balloon = world.creature { name = "Balloon", flies = true, rideable = true, body = { ... } }
function Balloon:tick(dt)
	if self.rider and input.held("burner", self.rider) then self:climb(1.5) else self:climb(-0.6) end
end
function Egg:hit(player) self:become(Chick) end   -- same creature, the Chick's body and handlers
```

- A creature with `flies = true` has no gravity. `self:climb(speed)` sets how fast it rises (or sinks, below 0), from -4 to 4 blocks a second, and it eases into that speed over a second or so, so whoever rides it is never jolted. It stays at what it was last told; 0 hovers. It returns false for a creature that doesn't fly. Fliers float on water, and slow to a stop 24 blocks above the world's highest blocks.
- A rider steers a flier with their movement while it's in the air; it gathers and loses speed gently. On the ground it stays put.
- `self:on_ground()` is whether it's standing on something.
- `self:become(kind)` turns it into another kind of this creation's: the new kind's body, size, speed, `rideable`, `seat` and `flies`, and its handlers from then on. Its fields and its rider stay (unless the new kind isn't rideable). `spawned` isn't called again, and it still counts toward the `count` of the kind it came as, so a sheared sheep doesn't bring another.
- Creatures aren't kept when a server restarts (they come back by `count`); keep anything that matters, such as where a balloon is, in `@potluck/store`.

Behaviours (new in 0.10): a creature can do the usual things with no code at all, and code is for what they can't do.

```luau
local Woolly = world.creature {
	name = "Woolly",
	home = "green",
	behaviours = {
		follow_food = "Wheat",       -- comes to a player holding wheat
		follow_when_used = 20,       -- follows whoever pats it, for 20 s
		flee_when_hit = 6,           -- runs 6 blocks from whoever bops it
		hop_when_hit = 0.8,
		chatter = { "Baa.", "Baa?" }, -- says one now and then, and when patted
		sound = "bleat",             -- with a sound, and when patted or bopped
	},
	body = { ... },
}
```

- `wander` (true unless it's false): wanders about its home when it has nothing else to do.
- `follow_food`: a block's name, or a list of them. It comes to the nearest player within 10 blocks with one in their hand, stops a couple of blocks off and looks at them, and goes back to what it was doing when they put it away.
- `follow_when_used`: seconds it follows whoever used it (true for 20, up to 300). Being hit stops it.
- `flee_when_hit`: how far it runs from whoever hit it, straight away from them, at nearly twice its speed (true for 6, up to 16).
- `hop_when_hit`: how high it hops when hit, as `self:hop` has it (true for 0.6, up to 3).
- `chatter`: lines it says now and then (every 10 to 35 seconds) and when it's used, one at random, 80 letters each.
- `sound`: one of the game's sounds or the creation's own (`sound.names()`), with its chatter, and when it's used or hit. It needn't `require` `@potluck/sound`.
- Behaviours act first, each tick and when it's used or hit, and then its own handlers run, so its code has the last word: a `tick` that calls `self:walk_to` takes it somewhere else, and a `hit` that calls `self:say` says something else. A creature being ridden doesn't follow anyone. Kinds that `become` one another can share one table of behaviours, or have their own.

#### Blocks of its own (0.12)

A creation makes blocks in `potluck.toml`, one table each, drawn in letters: each letter in a picture is one pixel, coloured by `colors`, and `.` is see-through. Players and code use them by name like the game's own (`world.set_block(at, "Red bricks")`, `world.give`, `follow_food`), and they stay in the world as long as it lasts, even if the creation goes. Require `@potluck/world@0.12` so an older game says it needs updating instead of running the code without its blocks.

```toml
[blocks.red_bricks]
name = "Red bricks"   # what players see it called (default: the key, "Red bricks")
kind = "hard"         # how it sounds and which tools dig it: soft, hard, wood, plant or glass (default: by its name)
hardness = 1.5        # seconds to break by hand, 0 to 20 (default 1)
colors = { m = "#cdc3b3", a = "#a4432f", b = "#b24f37" }
picture = """
aaaaaaam
aaaaaaam
mmmmmmmm
aaamaaaa
aaamaaaa
mmmmmmmm
aaaaaaam
mmmmmmmm
"""
```

- A picture is 8, 16, 32 or 64 pixels square: a string of lines, or a list of strings. `picture` is every side; `top`, `side` and `bottom` give one side each.
- `shape`: `"solid"` (the default), `"see-through"` (glass and leaves: `.` pixels show what's behind) or `"plant"` (two crossed pictures, like flowers, that bodies walk through).
- `boxes` makes a block of boxes instead: up to 16 of `[x, y, z, x, y, z]` in sixteenths of a block, from 0 to 16 (a slab is `[[0, 0, 0, 16, 8, 16]]`). Each side of a box shows the part of the picture it covers, so draw the side as the block looks from the side. `solid = false` lets bodies through.
- `light = true` lights up what's round it, and the letters in `glow` (`glow = "yo"`) shine; a light with no `glow` shines in its brightest pixels.
- Up to 16 blocks, 24 colours each. A block's name in `potluck.toml` is lowercase letters, digits and `_`; what players see it called can't be one of the game's blocks. Names are shared by every creation in a world, so pick one nobody else has.
- Change a colour or a row and save, and the world shows it. Each world gives a creation's blocks their ids the first time it runs there and keeps them.

### `@potluck/tweak` 0.1

```luau
local tweak = require("@potluck/tweak")
local knobs = tweak {
	speed = { 2.2, min = 0.5, max = 8, label = "Walking speed" },
	shell = { "#4f7a3a", label = "Shell colour" },
	friendly = { true, label = "Says hello" },
}
-- knobs.speed, knobs.shell and knobs.friendly hold what the player chose
```

A knob is a number (a slider from `min` to `max`), a colour (`"#rrggbb"`) or true or false (a switch). `order` sorts them. Anyone can change knobs in Tweak without touching the code; the creation reloads with the new values.

### `@potluck/store` 0.1

What a creation keeps in a world: it lasts across reloads and, on a server, across restarts.

```luau
local store = require("@potluck/store")
local tags = store.get("tags") or {}       -- what was kept, or nil
tags[player.name] = (tags[player.name] or 0) + 1
store.set("tags", tags)                    -- keep it (a copy: change it, then set it again)
store.set("old", nil)                      -- forget a key
for _, key in store.keys() do print(key) end
print(store.used() .. " bytes of 65536")
```

- `store.get(key)` is the value kept under `key`, or nil. `store.set(key, value)` keeps it; nil forgets it. Keys are 1 to 64 letters. Values are numbers, strings, true or false, vectors and tables of those (six deep); functions and creatures aren't kept.
- `store.keys()` lists the keys; `store.used()` is the bytes they take as JSON. Each creation has 64 KB in each world, and `store.set` fails with an error past it.
- Each world keeps its own: a server saves it with the world (`store.json`), and so does each world a player keeps in the game (Play alone's list). A friend's world keeps it on the friend's game.

### `@potluck/input` 0.1

Buttons of a creation's own. Name them in `potluck.toml`, each with a pad button and a key:

```toml
[needs]
input = "0.1"

[actions.pat]
name = "Pat"        # what players see, on the HUD and in Settings
pad = "X"           # A, X, Y, LB, RB, LT, RT, Up, Down, Left or Right
keys = "F"          # Space, or a letter but W, A, S and D
```

```luau
local input = require("@potluck/input")
input.on("pat", function(player) print(player.name .. " patted it") end)

-- a mini game takes a player's controls while they play it
world.on("player_joined", function(player) input.focus(player) end)
world.every(0.1, function()
	for _, p in world.players() do
		if input.held("pat", p) then print(p.name .. " is holding Pat") end
	end
end)
```

- A creation's actions are live for a player while they ride one of its creatures, or while it has their controls. Then its buttons do its actions instead of the game's (B and Shift still get a rider off, and Menu or Esc still pauses), and the HUD shows them.
- `input.on(action, fn)` calls `fn(player)` when a player presses it. Only actions in `potluck.toml` can have handlers; `world.on` and `input.on` handlers together are at most 32.
- `input.held(action, player)` is true while they hold it.
- `input.focus(player)` takes a player's controls, so its actions are live wherever they are; `input.focus(player, false)` gives them back. One creation has a player's controls at a time: the last to ask. `input.focused(player)` says whether this one has them.
- Players can move any action to other buttons (Settings, Creations' buttons), so say what an action does by its name, never by a button. A creation has at most 8 actions. One with a pad button for every action is **pad friendly** on the market.

### `@potluck/ui` 0.2

Lines on a player's screen, meters beside the health bar (new in 0.2), and questions with choices.

```luau
local ui = require("@potluck/ui")
ui.show(player, "score", "Tags: 3")                     -- top right, for one player
ui.show(nil, "round", "Round 2 of 5", { at = "left" })  -- nil: everyone, and whoever comes later
ui.hide(player, "score")

ui.ask(player, {
	title = "Rodeo",
	text = "Ride again?",
	choices = { "Yes", "Not now" },
}, function(player, choice)
	if choice == 1 then print(player.name .. " wants another go") end
	-- choice is the number of the one they chose, or nil if they closed it
end)
```

- `ui.show(player, id, text, { at = ... }?)` shows one line, or changes the one with that id. `at` is `"right"` (top right, the default), `"left"` (top left) or `"bottom"` (above the hotbar). `player` nil shows it to everyone in the world, now and when they arrive. A line is up to 60 letters, on one line; a creation shows at most 8 at once (different ids).
- `ui.hide(player, id)` takes it off; `ui.hide(nil, id)` takes it off every screen, including lines shown to one player with that id.
- `ui.meter(player, id, value, { color = "#rrggbb" }?)` shows a bar beside the health bar, `value` full from 0 to 1, amber if no colour is given (food, air, a mini game's clock). nil for `value` takes it away, as does `ui.hide`. They share the row above the hotbar with the health bar (which shows only in survival); a screen shows 3 at most, from every creation together. Meters count among a creation's 8 pieces. Give each a colour of its own and say once, with a line, what it means.
- `ui.ask(player, { title?, text?, choices? }, fn)` asks one player a question. `title` is up to 40 letters (the creation's name if left out), `text` up to 300 (`"\n"` starts a new line), and up to 8 `choices` of 30 letters (`{ "OK" }` if left out). `fn(player, choice)` is called once, on a later tick, with the number of their choice, or nil if they closed it, the creation asked them something else first, or it couldn't be shown. It returns an id.
- `ui.close(player)` takes this creation's question back off their screen, and `fn` isn't called.
- The panel opens when the player isn't in a menu, and the game always adds who it's from and a Close button. One question from each creation waits for a player at a time. After a player closes one, the creation's questions to them get nil at once for 10 seconds, so ask again only when they come back to it (using one of its creatures, or a block it hears `block_used` for, lets it ask at once).
- Lines and questions go when the creation stops, and a reload takes its questions away (its lines stay until it shows them again). A player who leaves takes theirs with them, and their `fn` isn't called.

### `@potluck/sound` 0.2

The game's sounds by name, and sounds of the creation's own. Every one is made from numbers on each player's game, so there are no sound files.

```luau
local sound = require("@potluck/sound@0.2")
sound.play("quack", { at = self })                         -- from a creature, for anyone near
sound.play("chime", { at = world.spot("green"), volume = 0.5 })
sound.play("fanfare")                                      -- nowhere in particular: everyone hears it
sound.play_for(player, "ding", { pitch = 1.5 })            -- only this player
for _, name in sound.names() do print(name) end
```

- The sounds: `pop`, `chime`, `ding`, `boing`, `beep`, `quack`, `croak`, `bleat`, `yip`, `chirp`, `grumble`, `splash`, `thud`, `whoosh`, `fanfare`, `buzz`, `sparkle`, `click`, and the game's own `step`, `step_hard`, `step_wood`, `step_plant`, `dig`, `crack`, `chop`, `rustle`, `shatter` and `place`. `sound.names()` lists them.
- Options: `at` (a position or a creature; left out, it's the same loudness everywhere), `volume` (0 to 1), `pitch` (0.5 to 2; 2 is an octave up) and `caption`, what subtitles say instead of the sound's own word (30 letters).
- A sound somewhere reaches players within 40 blocks, fading with distance. `sound.play_for(player, name, opts?)` plays it to one player only.
- A creation plays at most 10 sounds a second; past that, `play` skips the sound and returns false.
- Players who turn on **Subtitles for sounds** read each sound's caption, and which side it came from, so pick a caption that says what made it (`"Mossback hums"`).

#### Sounds of its own (0.2)

A creation makes its own sounds in `potluck.toml`, one table each, and plays them by name like the game's. Its own come first, so `[sounds.quack]` plays instead of the game's quack for this creation only.

```toml
[sounds.moo]
caption = "Moo"            # what subtitles say (left out: the name, "Moo")
wave = "saw"               # sine, triangle, saw, square or noise
pitch = [220, 160]         # Hz, sliding from the first to the second (or one number)
length = 0.9               # seconds, up to 3
attack = 0.08              # seconds to full loudness
decay = 0.35               # seconds for it to fade to a third (left out: a third of its length)
wobble = [5, 12]           # vibrato: times a second, and Hz up and down
pulse = [7, 0.3]           # tremolo: times a second, and how much quieter (0 to 1)
lowpass = 1600             # takes the edge off: only what's below this many Hz
# highpass = 200           # only what's above this many Hz

[[sounds.moo.layers]]      # up to 3 more layers, mixed in
wave = "noise"             # for noise, pitch is how bright it is
pitch = 900
length = 0.3
at = 0.05                  # seconds after the start
gain = 0.3                 # how loud against the rest (0 to 2)

[sounds.tada]
wave = "square"
notes = ["C5", "E5", "G5", "-", "C6"]   # one after another; "-" is a rest, and numbers are Hz
each = 0.12                # seconds a note (0.15 if left out); the length is notes times each
lowpass = 3000
```

```luau
sound.play("moo", { at = self })
sound.play_for(player, "tada")
```

- Names are small letters, numbers and `_`, up to 24. A creation has 16 sounds of its own at most, each up to 3 seconds, with up to 3 layers and 32 notes a layer. Numbers out of range are brought into it.
- Every sound is made as loud as the game's own, so set how loud it plays with `volume`. A sound is made once on each game and again only when its table changes, so tweak it, save, and play it again to hear the difference.
- A table that can't be read (`piitch`, a wave that isn't one) stops the creation loading, and Look inside says which and why. The market checks them too.

### `@potluck/net` 0.2

A creation can have a **client half**: a second file, named by `client` in potluck.toml, that runs on each player's own game while the main file runs on the world's host. Use it for what only that player sees and that would otherwise mean a message ten times a second: a clock ticking down, a line that follows what they do, a sound only for them. The half never changes the world. It hears what its creation sends it and answers the same way, and from net 0.2 it reads that player's game's copy of the world: the blocks, the ground and the players near enough to see. The balloon's half shows its rider how high they are.

```luau
-- main.luau, on the host
local net = require("@potluck/net")
net.send(player, "round", { seconds = 30 })   -- to one player's half
net.send(nil, "over", { winner = "Ada" })      -- to every player here now
net.on("ready", function(player, value)      -- from a player's half
	print(player.name .. " is ready")
end)

-- client.luau, on each player's game
local net = require("@potluck/net")
local ui = require("@potluck/ui")
local world = require("@potluck/world")
net.on("round", function(value)
	local ends = world.time() + value.seconds
	world.every(0.1, function()
		ui.show(nil, "clock", string.format("%.1f s left", math.max(0, ends - world.time())), { at = "bottom" })
	end)
end)
net.send("ready", { name = net.me().name })
```

- In the main file: `net.send(player, topic, value?)` sends a message to that player's half, and `net.send(nil, topic, value?)` to every player in the world now (not to those who come later). `net.on(topic, fn(player, value))` is called on the next tick for each message a player's half sends.
- In the client half: `net.send(topic, value?)` sends a message to the main file, `net.on(topic, fn(value))` is called for each message the main file sends this player, and `net.me()` is this player (`id`, `name` and where they stand, and from 0.2 `look`, which way they look, and `aim`, the middle of the block they aim at, or nil).
- A topic is 1 to 32 letters. A value is what the store keeps (numbers, strings, true or false, vectors, and tables of those), at most 4 KB as JSON.
- The main file sends at most 30 messages a second, and each player's half 10; past that, `send` returns false and the message isn't sent. The host drops what a half sends beyond that too.
- What a client half has: `@potluck/net`; `@potluck/ui`'s `show` and `hide`, for this player only (`player` is nil, or `net.me()`), at most 8 lines; `@potluck/sound`'s `play`, heard only by this player; and from `@potluck/world` its timers (`after`, `every`, `cancel`), `time` (seconds since this half started, so send it lengths of time rather than the host's times), `random`, `spot`, `daytime` and `daylight` (this game's clock). The rest of the API is the main file's, and says so if a half calls it.
- From net 0.2 a client half reads too: `world.block`, `world.top`, `world.find_block` and `world.kind` from this game's copy of the world, and `world.players` and `world.nearest_player` for the players it knows of (this player first, then those near enough to see). A guest's copy is the host's as it last heard, so it can be a moment behind; anything that matters (who won, what's in a chest) stays the main file's to decide and send.
- A half starts when its player arrives or the creation loads, and starts again, fresh, whenever any of the creation's files change. If it needs to know what's going on, have it say hello (`net.send("hello")`) and answer that from the main file.
- Its lines share ids with the main file's lines for that player, so the last to show one wins. Its lines go when it stops.
- A half has 16 MB and 4 ms of each frame. One that fails or runs too long stops, and Look inside shows why (its prints show there too, marked `client:`).

### `@potluck/game` 0.1

For mini games: boards and other servers.

```luau
local game = require("@potluck/game")
game.score("longest ride", player, 12.5)                   -- higher is better
game.score("fastest lap", player, 31.2, { lower = true })   -- lower is better
local top = game.board("longest ride", 5)                   -- { { name, value }, ... }, best first
if table.find(game.links(), "rodeo") then
	game.send(player, "rodeo", "Off to the rodeo!")
end
```

- `game.score(board, player, value, { lower = true }?)` puts a player's score on one of this creation's boards. A board keeps each player's best (highest, or lowest with `lower`), up to 100 players, and at most 16 boards a creation. On a server in the market's list, boards also show on the creation's page.
- `game.board(board, n?)` is the best `n` (10 by default) as `{ name, value }`, best first.
- `game.links()` is the names of the other servers this one can send players to (its `[links]`, set by whoever runs it). Alone or hosting from the game it's empty.
- `game.send(player, link, note?)` sends a player to one of those servers, showing them `note`. The player at the keyboard of a hosted world stays: they are the world.

### `@potluck/test` 0.3

A creation's own tests, in `test.luau` beside `main.luau`. They run only when asked for: the dev bridge's `test` tool, or `potluck-server --test=balloon` (an id or a folder, or `all` for every creation that has tests; it ends with 1 if any failed, for scripts). Each case plays in a world of its own: a copy of the world with only this creation in it, its knobs as its code sets them, and players made for the test. Nothing a test does reaches the world anyone plays in. It needn't be in `[needs]`. The base game's balloon and woolly have tests to read.

```luau
local test = require("@potluck/test@0.3")
local world = require("@potluck/world@0.12")

test.case("a basket put down fills up into a balloon", function()
	local p = test.player("Ada")                    -- on the Green
	local x, z = p.position.x + 3, p.position.z
	local at = vector.create(x, world.top(x, z) + 1, z)
	test.place(p, at, "Balloon basket")             -- as if Ada put it down
	test.wait(4)                                     -- four seconds of play
	local ids = world.creatures(test.kind("Hot air balloon"))
	test.same(#ids, 1, "balloons")
	local b = test.creature(ids[1])
	test.ride(p, b)
	test.press(p, "burner")                          -- one of its [actions]
	test.wait(4)
	test.check(b.position.y > at.y + 2, "it rose")
end)
```

- `test.case(name, fn)` adds a case (64 at most). It passes if `fn` returns, and fails at the first error, saying the line. It fails too if the creation's own code goes wrong while it plays.
- `test.check(ok, message?)` fails the case unless `ok`; `test.same(got, want, message?)` unless the two are equal, and says both.
- `test.wait(seconds)` lets the world play on: creatures move, and the creation's ticks, timers and events run, as in play. A case may play 600 s at most, and take 30 s of real time.
- `test.player(name?, { at = pos, survival = true, carrying = { ["Cream wool"] = 2 } }?)` brings a player in, on the Green unless `at` says where. In creative they have every block, as in play. `test.move(player, pos)` moves them, and the table you hold with them.
- What players do: `test.place(player, pos, block)` and `test.dig(player, pos)` (heard as `block_placed` and `block_broken`; a survival player puts down only what they carry, and keeps what they dig), `test.use_block(player, pos)`, `test.use(player, creature)` and `test.hit(player, creature)`, `test.ride(player, creature)` and `test.get_off(player)`, `test.press(player, action)` and `test.release(player, action)` for its `[actions]`, `test.answer(player, choice)` for the question in front of them (choices count from 1), `test.hold(player, block)` for what's in their hand (nil for nothing), and `test.from_half(player, topic, value?)`, a message from that player's client half as `net.send` sends it there (0.3). Placing, digging, using a block, pressing, answering and messages from a half pass a tenth of a second so the creation hears them; using, hitting and riding a creature are heard at once.
- Looking: `test.kind(name)` is one of its kinds, for `world.creatures` and `world.spawn`. `test.creature(id)` is a creature's table (`position`, `kind`, `rider` and the fields its code keeps), or nil once it's gone; it's the same table from one moment to the next. `test.health(player)` is their health (20 is full), `test.shown(player, id)` the text of one of its pieces on their screen, or nil, `test.asked(player)` the question in front of them (`title`, `text`, `choices`), or nil, and `test.heard(name)` how many times the creation has played that sound (its own or the game's, by the name it plays) since the case began (0.2). `test.sent(player, topic)` is the last value the creation sent that player's client half on that topic (true for a message with no value), and how many times it has since the case began: `nil, 0` if it hasn't (0.3). The half itself doesn't run in a case.
- The rest of the API works in a case as in `main.luau`: `world.block`, `world.pockets`, `store.get` and so on. `main.luau`'s locals aren't reachable: use `test.kind` and `test.creature`.
- `wait`, `player`, `move` and what players do hand over to the game, so give each a line of its own rather than putting one inside a call to `world` or a creature: `local p = test.player()` and then `world.spawn(kind, p.position)`, not `world.spawn(kind, test.player().position)`.

## Rules of the sandbox

- No files, no network, no other creation's memory. `os`, `io` and loading bytecode aren't there.
- Each creation gets 32 MB and a slice of each tick (15 ms for its timers, events and creatures together, counting only while its code runs, not while the computer is busy with something else); a loop that never ends is stopped with an error. A client half gets 16 MB and 4 ms a frame on its player's game.
- At most 64 creatures and 16 kinds per creation.
- Creations can't shake, roll or zoom the player's camera, and can't cover the game's own menus: their lines and questions sit under them.

## Testing

Run the world (alone is fine), stand near your creation, save the file, and watch it change. Look inside it (L, or hold Y on a pad) for its log and any error, and for how it's doing against its budgets: how long its ticks take of their 15 ms, on average and at most lately, and its memory of 32 MB.

To check it keeps doing what it should as you change it, give it tests of its own in `test.luau` (see `@potluck/test`) and run them with the dev bridge's `test` tool or `potluck-server --test=<its folder>`.

### The type checker

Each time you save one of your own creations, the game runs Luau's type checker over its files with the API's types (`types/potluck.d.luau`), and Look inside lists what it found under **The type checker spotted**, with the file and line. It speaks up only when it's sure something is wrong:

- a name a module doesn't have (`world.spwan`, with the name it probably should be);
- a global that isn't one (`pritn`);
- a call given too many or too few values;
- a plain value of the wrong kind: text where a number goes, or a word an option doesn't take (`at = "middle"`).

Everything it can only guess at stays quiet, so no hints doesn't mean the code is right; tests do that. `--!strict` at the top changes nothing: it checks every file as strictly and still says only what it's sure of. `--!nocheck` on a file's first line leaves that file out, if it's ever wrong. The dev bridge's `check` tool and `potluck-server --typecheck=<its folder>` (or an id, or `all`; it ends with 1 if it found anything, for scripts) run the same checks.

### With a coding helper: the dev bridge

A coding helper on the same computer can work with the game while you play: turn on **Settings, Dev bridge for coding helpers on this computer** (or start the game with `--bridge`) and add `http://127.0.0.1:47130/mcp` to the helper as an MCP server over HTTP (in Claude Code, `claude mcp add --transport http potluck http://127.0.0.1:47130/mcp`). It answers only programs on this computer, and it's off until you turn it on. Its tools:

- `creations`: every creation in the game, which are running, the folder each of yours is in, their errors, knobs, actions, creatures, memory and load time.
- `guide`: this file (or, with `types`, the API's types).
- `reload`: reads a creation's folder again and starts or restarts it, keeping its creatures; an error says which file and line, and the last good version keeps running. A new folder in your creations folder is a new creation: give it a `potluck.toml` and `main.luau` and reload it by the folder's name (it gets this file and the types).
- `log`: what it printed, its last error, and its client half's lines.
- `run`: a few lines of Luau inside a running creation, as if they were part of its code (`return #require("@potluck/world").creatures()`), with what they printed and returned. Its files' locals aren't reachable, and it has one tick's time.
- `test`: runs its own tests (`test.luau`, as saved in its folder) in a copy of the world, and says which cases passed and why the others failed, with the line and what it printed. Nothing it does reaches the world.
- `check`: runs the type checker over its files as saved, and lists what it's sure is wrong, with the file and line (see The type checker above).
- `tweak`: turns one of its knobs, as Tweak does.
- `look_around`: where you are and what's near you: the block you look at, creatures with their creation, kind, rider and the fields their code keeps, other players, the blocks about and the time of day.
- `screenshot`: the screen as it is, as a picture.
