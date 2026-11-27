--!strict
-- Types for the Potluck API, for editors and AI helpers. Written by the game; don't edit.
-- @potluck/world 0.9, @potluck/tweak 0.1, @potluck/game 0.1, @potluck/store 0.1, @potluck/input 0.1, @potluck/ui 0.2, @potluck/sound 0.1, @potluck/net 0.1, @potluck/test 0.1

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
}

export type CreatureDef = {
	name: string,
	body: { Part }?,
	size: { number }?,
	speed: number?,
	count: number?,
	home: (string | Position)?,
	rideable: boolean?,
	seat: number?,
	flies: boolean?,
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
	remove: (self: Creature) -> (),
	on_ground: (self: Creature) -> boolean,
	climb: (self: Creature, speed: number) -> boolean,
	become: (self: Creature, kind: any) -> (),
	[string]: any,
}

export type World = {
	creature: (def: CreatureDef) -> any,
	block: (pos: Position) -> string,
	set_block: (pos: Position, name: string) -> boolean,
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
		& ((event: "block_placed" | "block_broken" | "block_used", fn: (player: Player, at: vector, block: string) -> ()) -> ()),
	-- 0.3
	pockets: (player: Player | number) -> { [string]: number }?,
	has: (player: Player | number, block: string) -> number,
	take: (player: Player | number, block: string, n: number?) -> boolean,
	give: (player: Player | number, block: string, n: number?) -> number,
	-- 0.4
	daytime: () -> number,
	daylight: () -> number,
	find_light: (pos: Position, radius: number) -> vector?,
	hurt: (player: Player | number, amount: number, opts: { down_to: number? }?) -> boolean,
	heal: (player: Player | number, amount: number) -> boolean,
	-- 0.6
	healing: (player: Player | number, rate: number) -> boolean,
	-- 0.8
	held: (player: Player | number) -> string?,
	kind: (block: string) -> "soft" | "hard" | "wood" | "plant" | "glass" | "water" | "air",
	digging: (block: string, speeds: { soft: number?, hard: number?, wood: number?, plant: number?, glass: number? }?) -> (),
}

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

export type Sound = {
	play: (name: string, opts: SoundOptions?) -> boolean,
	play_for: (player: Player | number, name: string, opts: SoundOptions?) -> boolean,
	names: () -> { string },
}

-- In the main file, on the world's host.
export type Net = {
	send: (player: Player | number | nil, topic: string, value: Stored?) -> boolean,
	on: (topic: string, fn: (player: Player, value: Stored) -> ()) -> (),
}

-- In a client half (`client` in potluck.toml), on each player's own game.
export type ClientNet = {
	send: (topic: string, value: Stored?) -> boolean,
	on: (topic: string, fn: (value: Stored) -> ()) -> (),
	me: () -> Player,
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
	place: (player: Player, at: Position, block: string) -> (),
	dig: (player: Player, at: Position) -> (),
	use_block: (player: Player, at: Position) -> (),
	use: (player: Player, creature: Creature | number) -> (),
	hit: (player: Player, creature: Creature | number) -> (),
	ride: (player: Player, creature: Creature | number) -> (),
	get_off: (player: Player) -> (),
	press: (player: Player, action: string) -> (),
	release: (player: Player, action: string) -> (),
	answer: (player: Player, choice: number) -> (),
	-- these only look
	kind: (name: string) -> any,
	creature: (id: number | Creature) -> Creature?,
	health: (player: Player | number) -> number?,
	shown: (player: Player | number, id: string) -> string?,
	asked: (player: Player | number) -> { title: string, text: string, choices: { string } }?,
}

export type Knob = { any, min: number?, max: number?, label: string?, order: number? }
export type Tweak = (spec: { [string]: Knob }) -> { [string]: any }

return {}
