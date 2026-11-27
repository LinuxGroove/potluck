# Building a Potluck creation

This folder is a Potluck creation: plain text that the game runs in a sandbox. Edit any file and save; the game reloads it within a second, keeping the creatures where they are and what they remember. If the code has an error, the last good version keeps running and the game shows the error and its line.

## Files

| File | What |
|---|---|
| `potluck.toml` | Its name, who made it, what kind of thing it is, which file to run (`main`), the API it needs (`[needs] world = "0.2"`, `game = "0.1"` for boards, `store = "0.1"` to keep things) and its licence. A remix also says what it's a remix of (`remix_of`, and `[from]` with the market's code and version): leave those as the game wrote them, so the market can show its family tree and send suggested changes back |
| `main.luau` | The code, in Luau (Lua 5.1 with types). `require("./other")` loads `other.luau` from this folder |
| `types/potluck.d.luau` | The API's types, for editors and AI helpers. Don't edit it: the game writes it |

## The API

Modules are versioned on their own, never by the game's version: `require("@potluck/world")` or, to be exact, `require("@potluck/world@0.2")`. A newer minor version only adds to an older one, so a creation made for `world 0.1` runs with 0.2. The game's `docs/api.md` is the promise that goes with the versions, and lists the limits.

### `@potluck/world` 0.2

Positions are Luau vectors: `vector.create(x, y, z)`, with `+`, `-` and `*` working on them, `.x`, `.y` and `.z` to read them. A table `{ x, y, z }` works too. y is up; one block is 1.

- `world.creature(def)` defines a kind of creature and returns it, to add handlers to. `def` has:
  - `name` (required), `speed` (blocks a second, default 2), `count` (how many the game keeps in the world, default 1), `home` (`"green"`, `"paddock"`, `"creek"`, `"start"` or a position: where they appear and stay near), `rideable`, `seat` (the height a rider sits at)
  - `body`: a list of boxes, each `{ "name", size = { w, h, d }, at = { x, y, z }, color = "#rrggbb" }`, with `leg = true` to swing as it walks, `head = true` to bob, `glow = true` to light up. The creature faces negative z.
  - `size = { w, h, d }`: the box it bumps into things with (worked out from the body if left out)
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
- `world.on(event, fn)`: `"player_joined"` when someone arrives in the world and `"player_left"` when they go, each with the player. Players already here when the creation starts are in `world.players()`.
- A creation's top level runs again when it reloads, and sets its timers and handlers up again. Timers started inside handlers don't carry over a reload.

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
- Each world keeps its own: a server saves it with the world (`store.json`), and a world you play in the game keeps it while it's open.

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

## Rules of the sandbox

- No files, no network, no other creation's memory. `os`, `io` and loading bytecode aren't there.
- Each creation gets 32 MB and a slice of each tick (15 ms for its timers, events and creatures together); a loop that never ends is stopped with an error.
- At most 64 creatures and 16 kinds per creation.
- Creations can't shake, roll or zoom the player's camera.

## Testing

Run the world (alone is fine), stand near your creation, save the file, and watch it change. Look inside it (L, or hold Y on a pad) for its log and any error.
