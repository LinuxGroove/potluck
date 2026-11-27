--!strict
-- Types for the Potluck API, for editors and AI helpers. Written by the game; don't edit.
-- @potluck/world 0.2, @potluck/tweak 0.1, @potluck/game 0.1, @potluck/store 0.1

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
	on: (event: "player_joined" | "player_left", fn: (player: Player) -> ()) -> (),
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

export type Knob = { any, min: number?, max: number?, label: string?, order: number? }
export type Tweak = (spec: { [string]: Knob }) -> { [string]: any }

return {}
