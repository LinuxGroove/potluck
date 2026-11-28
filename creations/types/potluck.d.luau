--!strict
-- Types for the Potluck API, for editors and AI helpers. Written by the game; don't edit.
-- @potluck/world 0.25, @potluck/tweak 0.1, @potluck/game 0.1, @potluck/store 0.1, @potluck/input 0.1, @potluck/ui 0.2, @potluck/sound 0.2, @potluck/net 0.2, @potluck/test 0.8, @potluck/sky 0.1

export type Position = vector | { number }

export type Player = {
	id: number,
	name: string,
	position: vector,
}

export type Part = {
	[number]: string,
	size: { number },
	at: { number },
	color: string?,
	leg: boolean?,
	head: boolean?,
	glow: boolean?,
	-- 0.11: where its picture is in one of the creation's Blockbench models (world.model fills it in)
	texture: { file: string, index: number?, color: string?, stamp: number?, faces: { [string]: { number } }? }?,
}

export type CreatureDef = {
	name: string,
	-- a list of boxes, or (0.11) a Blockbench model in the creation's folder: "hedgehog.bbmodel"
	body: ({ Part } | string)?,
	size: { number }?,
	speed: number?,
	count: number?,
	-- "green", "paddock", "creek", "start", a position, or (0.21) "here":
	-- wherever each one is spawned
	home: (string | Position)?,
	rideable: boolean?,
	seat: number?,
	flies: boolean?,
	-- 0.23: false so a rider doesn't steer it
	steer: boolean?,
	behaviours: Behaviours?,
}

-- What a creature does by itself, with no code (world 0.10).
export type Behaviours = {
	wander: boolean?,
	follow_food: (string | { string })?,
	follow_when_used: (boolean | number)?,
	flee_when_hit: (boolean | number)?,
	hop_when_hit: (boolean | number)?,
	chatter: (string | { string })?,
	sound: string?,
}

export type Creature = {
	id: number,
	kind: string,
	position: vector,
	rider: Player?,
	walk_to: (self: Creature, pos: Position, speed: number?) -> (),
	wander: (self: Creature, speed: number?) -> (),
	stop: (self: Creature) -> (),
	hop: (self: Creature, power: number?) -> (),
	face: (self: Creature, pos: Position) -> (),
	say: (self: Creature, text: string) -> (),
	distance_to: (self: Creature, pos: Position) -> number,
	throw_rider: (self: Creature, force: number?) -> boolean,
	-- 0.21: something thrown from about its eyes, which hits players
	throw: (self: Creature, opts: ThrowOptions, landed: ((at: vector, block: string?, creature: number?, player: Player?) -> ())?) -> number?,
	remove: (self: Creature) -> (),
	on_ground: (self: Creature) -> boolean,
	climb: (self: Creature, speed: number) -> boolean,
	-- 0.23: along a path of points, gently; and how far is left, how fast
	-- it's going and how many points are left
	follow: (self: Creature, points: { Position }?, speed: number?, more: boolean?) -> (),
	following: (self: Creature) -> (number, number, number),
	become: (self: Creature, kind: any) -> (),
	[string]: any,
}

export type World = {
	creature: (def: CreatureDef) -> any,
	-- 0.11: a Blockbench model (.bbmodel in the creation's folder) as a body, 16 pixels to a block times scale
	model: (file: string, scale: number?) -> { Part },
	block: (pos: Position) -> string,
	set_block: (pos: Position, name: string, facing: string?) -> boolean,
	facing: (pos: Position) -> string?,
	top: (x: number, z: number) -> number,
	find_block: (pos: Position, radius: number, name: string) -> vector?,
	players: () -> { Player },
	nearest_player: (pos: Position, radius: number?) -> Player?,
	spawn: (kind: any, pos: Position) -> number,
	creatures: (kind: any?) -> { number },
	spot: (name: string) -> vector?,
	say: (text: string) -> (),
	time: () -> number,
	random: (a: number?, b: number?) -> number,
	-- 0.2
	after: (seconds: number, fn: () -> ()) -> number,
	every: (seconds: number, fn: () -> ()) -> number,
	cancel: (id: number) -> boolean,
	on: ((event: "player_joined" | "player_left", fn: (player: Player) -> ()) -> ())
		& ((event: "block_placed" | "block_broken" | "block_used", fn: (player: Player, at: vector, block: string) -> ()) -> ())
		& ((event: "item_used", fn: (player: Player, item: string?, aim: vector) -> ()) -> ())
		& ((event: "player_fell", fn: (player: Player, at: vector) -> ()) -> ()),
	-- 0.3
	pockets: (player: Player | number) -> { [string]: number }?,
	has: (player: Player | number, block: string) -> number,
	take: (player: Player | number, block: string, n: number?) -> boolean,
	give: (player: Player | number, block: string, n: number?) -> number,
	-- 0.4
	daytime: () -> number,
	daylight: () -> number,
	find_light: (pos: Position, radius: number) -> vector?,
	hurt: (player: Player | number, amount: number, opts: { down_to: number?, guarded: boolean? }?) -> boolean,
	heal: (player: Player | number, amount: number) -> boolean,
	-- 0.6
	healing: (player: Player | number, rate: number) -> boolean,
	-- 0.8
	held: (player: Player | number) -> string?,
	kind: (block: string) -> "soft" | "hard" | "wood" | "plant" | "glass" | "water" | "air",
	digging: (block: string, speeds: { soft: number?, hard: number?, wood: number?, plant: number?, glass: number?, level: number? }?) -> (),
	drops: (block: string, rule: { gives: (string | false)?, count: (number | { number })?, level: number? }?) -> (),
	-- 0.18
	guard: (player: Player | number, share: number) -> boolean,
	-- 0.24: what every game draws them wearing in a slot ("head", "body",
	-- "legs", "feet" or "arm"); nil for nothing
	wear: (player: Player | number, slot: string, thing: string?) -> boolean,
	-- 0.19
	shoot: (player: Player | number, opts: ShotOptions, landed: ((at: vector, block: string?, creature: number?) -> ())?) -> number?,
	aims: (item: string, on: boolean?) -> (),
	-- 0.21
	danger: () -> "peaceful" | "gentle" | "normal" | "hard",
	-- 0.22
	reward: (player: Player | number, xp: number) -> number,
	xp: (player: Player | number) -> (number, number),
	spend_xp: (player: Player | number, xp: number) -> boolean,
	xp_for: (level: number) -> number,
	haste: (player: Player | number, share: number?) -> number,
	might: (player: Player | number, share: number?) -> number,
	charms: (player: Player | number, item: string) -> { string },
	charm: (player: Player | number, item: string, charms: { string }) -> boolean,
	-- 0.25: an item by name, the game's or one a creation made ([items.*]);
	-- nil for a block or nothing. And the names of every item creations made.
	item: (name: string) -> ItemInfo?,
	items: () -> { string },
}

-- world.item's table (0.25). The game's own items have only a name, a stack
-- and `by = "game"`; an item a creation made has its kind and what it wrote.
export type ItemInfo = {
	name: string,
	stack: number,
	by: string,
	kind: ("material" | "food" | "tool" | "weapon" | "armor")?,
	recipe: { [string]: number }?,
	makes: number?,
	food: number?,
	says: string?,
	slot: ("head" | "body" | "legs" | "feet" | "arm")?,
	guard: number?,
	warm: boolean?,
	digs: { [string]: number }?,
	power: number?,
}

-- world.shoot's options (0.19), and the shot a creature's hit hears
export type ShotOptions = { aim: Position, look: ("arrow" | "stone" | "spear" | "ember")?, speed: number?, drop: number?, power: number? }
-- self:throw's (0.21): `aim` the way to throw, or `at` a place to throw at
export type ThrowOptions = { aim: Position?, at: Position?, look: ("arrow" | "stone" | "spear" | "ember")?, speed: number?, drop: number?, power: number? }
export type Shot = { id: number, look: string, power: number }

export type Score = { name: string, value: number }

export type Game = {
	score: (board: string, player: Player | number, value: number, opts: { lower: boolean? }?) -> (),
	board: (board: string, n: number?) -> { Score },
	links: () -> { string },
	send: (player: Player | number, link: string, note: string?) -> (),
}

export type Stored = nil | boolean | number | string | vector | { [any]: Stored }

export type Store = {
	get: (key: string) -> Stored,
	set: (key: string, value: Stored) -> (),
	keys: () -> { string },
	used: () -> number,
}

export type Input = {
	on: (action: string, fn: (player: Player) -> ()) -> (),
	held: (action: string, player: Player | number) -> boolean,
	focus: (player: Player | number, on: boolean?) -> (),
	focused: (player: Player | number) -> boolean,
}

export type Question = {
	title: string?,
	text: string?,
	choices: { string }?,
}

export type UI = {
	show: (player: Player | number | nil, id: string, text: string, opts: { at: ("right" | "left" | "bottom")? }?) -> (),
	hide: (player: Player | number | nil, id: string) -> (),
	-- 0.2
	meter: (player: Player | number | nil, id: string, value: number?, opts: { color: string? }?) -> (),
	ask: (player: Player | number, question: Question, fn: (player: Player, choice: number?) -> ()) -> number,
	close: (player: Player | number) -> boolean,
}

export type SoundOptions = {
	at: (Position | Creature)?,
	volume: number?,
	pitch: number?,
	caption: string?,
}

-- `name` is one of the game's sounds, or one of the creation's own from
-- `[sounds.name]` in potluck.toml (0.2), which come first.
export type Sound = {
	play: (name: string, opts: SoundOptions?) -> boolean,
	play_for: (player: Player | number, name: string, opts: SoundOptions?) -> boolean,
	-- its own first, then the game's
	names: () -> { string },
}

-- In the main file, on the world's host.
export type Net = {
	send: (player: Player | number | nil, topic: string, value: Stored?) -> boolean,
	on: (topic: string, fn: (player: Player, value: Stored) -> ()) -> (),
}

-- In a client half (`client` in potluck.toml), on each player's own game.
-- 0.2: `look` is which way they look, `aim` the middle of the block they aim at.
export type Me = Player & { look: vector, aim: vector? }

export type ClientNet = {
	send: (topic: string, value: Stored?) -> boolean,
	on: (topic: string, fn: (value: Stored) -> ()) -> (),
	me: () -> Me,
}

-- In test.luau, while the creation's tests run (the dev bridge's test, potluck-server --test).
export type TestPlayerOptions = {
	at: Position?,
	survival: boolean?,
	carrying: { [string]: number }?,
}

export type Test = {
	case: (name: string, fn: () -> ()) -> (),
	check: (ok: any, message: string?) -> any,
	same: (got: any, want: any, message: string?) -> (),
	-- these hand over to the game: keep each on a line of its own
	wait: (seconds: number) -> (),
	player: (name: string?, opts: TestPlayerOptions?) -> Player,
	move: (player: Player, at: Position) -> Player,
	place: (player: Player, at: Position, block: string, facing: string?) -> (),
	dig: (player: Player, at: Position) -> boolean,
	use_block: (player: Player, at: Position) -> (),
	use_item: (player: Player, aim: Position?) -> (),
	use: (player: Player, creature: Creature | number) -> (),
	hit: (player: Player, creature: Creature | number) -> (),
	ride: (player: Player, creature: Creature | number) -> (),
	get_off: (player: Player) -> (),
	press: (player: Player, action: string) -> (),
	release: (player: Player, action: string) -> (),
	answer: (player: Player, choice: number) -> (),
	hold: (player: Player, block: string?) -> (),
	from_half: (player: Player, topic: string, value: Stored?) -> (), -- 0.3
	danger: (level: "peaceful" | "gentle" | "normal" | "hard") -> (), -- 0.7: the world's, for this case
	daytime: (time: number) -> (), -- 0.8: 0 midnight, 0.25 sunrise, 0.5 noon, 0.75 sunset; it stays there
	-- these only look
	kind: (name: string) -> any,
	creature: (id: number | Creature) -> Creature?,
	health: (player: Player | number) -> number?,
	shown: (player: Player | number, id: string) -> string?,
	asked: (player: Player | number) -> { title: string, text: string, choices: { string } }?,
	-- 0.2
	heard: (name: string) -> number,
	-- 0.3: the last value sent to that player's client half on a topic, and how many times
	sent: (player: Player | number, topic: string) -> (Stored?, number),
}

-- @potluck/sky 0.1: the weather and the season, the same on every game in
-- the world. The game draws them and plays the rain and thunder.
export type WeatherKind = "clear" | "cloudy" | "rain" | "storm" | "snow" | "fog"
export type SeasonName = "spring" | "summer" | "autumn" | "winter"
export type Weather = {
	kind: WeatherKind,
	-- how heavy, 0 to 1
	strength: number,
	-- blocks a second along x and z (y is 0)
	wind: vector,
}
export type WeatherOptions = {
	-- 0 to 1: 0.6 when not said (0 for clear)
	strength: number?,
	-- blocks a second along x and z, up to 12; the wind stays as it was when not said
	wind: Position?,
	-- seconds it takes to come in, 0 to 120: 20 when not said
	over: number?,
}
export type Sky = {
	weather: () -> Weather,
	-- kind is a WeatherKind
	set_weather: (kind: string, opts: WeatherOptions?) -> (),
	-- the season, and how far through it, 0 to 1
	season: () -> (SeasonName, number),
	-- name is a SeasonName
	set_season: (name: string, progress: number?) -> (),
	-- lightning there: a glow in the sky and thunder on every game, and
	-- `strike` for every creation; false when it's made too many (one a
	-- second, three at once)
	strike: (at: Position) -> boolean,
	on: (event: "strike", fn: (at: vector) -> ()) -> (),
}

export type Knob = { [number]: any, min: number?, max: number?, label: string?, order: number? }
export type Tweak = (spec: { [string]: Knob }) -> { [string]: any }

return {}
