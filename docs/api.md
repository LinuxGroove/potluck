# The creation API and its promise

Every creature, block and mini game in Potluck is a creation: Luau code that asks the game for what it needs through the API's modules. The API is the contract with everyone who makes something, so it's built to outlast any version of the game. This page is that contract: what each module is at now, what the limits are, and what we promise to keep.

`creations/AGENTS.md` is the how-to for each function (and goes into every remix); `creations/types/potluck.d.luau` has the types. This page is about versions and keeping things working.

## Modules

A creation asks for a module by name, and can name the version it was made for:

```luau
local world = require("@potluck/world@0.2")
local store = require("@potluck/store")      -- any version this game has
```

and lists what it needs in `potluck.toml`, which the market checks before sharing:

```toml
[needs]
world = "0.2"
store = "0.1"
```

| Module | Version | What |
|---|---|---|
| `@potluck/world` | 0.2 | Blocks, creatures, players, named spots, messages, time, random numbers, timers and world events |
| `@potluck/tweak` | 0.1 | Knobs anyone can turn in Tweak without touching the code |
| `@potluck/game` | 0.1 | Boards of players' bests, and sending players on to other servers |
| `@potluck/store` | 0.1 | A creation's own saved data in a world, with a quota |
| `@potluck/input` | 0.1 | Actions of its own, on pad buttons and keys players can move, while a player rides its creature or it has their controls |
| `@potluck/ui` | 0.1 | Lines on players' screens, and questions with choices |

Still to come, each as its own module with its own version: `@potluck/sound`, and `@potluck/net` (a creation's client half, for what only one player sees).

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
| Timers waiting at once | 64 |
| Shortest timer | 0.1 s, the length of a tick |
| World event and input handlers (`world.on`, `input.on`) | 32 |
| Actions | 8, each on a pad button (A, X, Y, LB, RB, LT, RT or the d-pad) and a key (Space or a letter but W, A, S and D) |
| Lines on screens (`ui.show`) | 8 at once (different ids), each up to 60 letters |
| Questions (`ui.ask`) | One waiting for each player; 64 waiting over every player; a title of 40 letters, text of 300 and 4 choices of 30. After a player closes one, the creation's questions to them get nil for 10 seconds |
| Boards | 16, of 100 players each |
| Store | 64 KB, counted as JSON |
| Messages | `world.say` 120 letters, a creature's `say` 80 |

A creation over a time limit stops with an error, and the world carries on. After about 30 errors close together it's paused, and Look inside says why.

What a creation can never do, at any version: read or write files, reach the network, see another creation's memory, load code from anything but its own files, shake, roll or zoom the player's camera, or put anything over the game's own menus. Its questions always say whose they are and can always be closed.

## Changes

### `@potluck/world`

- **0.2**: `world.after`, `world.every` and `world.cancel` (timers), and `world.on` for `"player_joined"` and `"player_left"`. A creation with no creatures can now run code every tick.
- **0.1**: the first version.

### `@potluck/input`

- **0.1**: the first version: `[actions]` in `potluck.toml`, and `input.on`, `input.held`, `input.focus` and `input.focused`.

### `@potluck/ui`

- **0.1**: the first version: `ui.show`, `ui.hide`, `ui.ask` and `ui.close`.

### `@potluck/store`

- **0.1**: the first version: `store.get`, `store.set`, `store.keys` and `store.used`.

### `@potluck/game`

- **0.1**: the first version: boards and links to other servers.

### `@potluck/tweak`

- **0.1**: the first version.
