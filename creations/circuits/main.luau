--!strict
-- Circuits: copper wire carries power from levers, buttons, pressure plates
-- and daylight sensors, through inverters and delays, to copper lamps and
-- copper gates. Power is on or off, nothing in between.
--
-- Wire joins wire beside it, above or below it, and a step up or down, so
-- a line of wire can climb stairs. Anything that gives power (a lever
-- pulled, a button pressed, someone on a plate, a sensor in daylight, an
-- inverter or delay giving out) powers the wire, lamps and gates it
-- touches. Inverters and delays face the way you put them down: they take
-- power from behind (the side nearest you as you put it down) and give it
-- out in front. An inverter gives power when none comes in; a delay passes
-- on what comes in a moment later (use it to set how long). Gates touching
-- each other open together.
--
-- The circuit is worked out ten times a second, so every inverter and
-- delay takes at least a tenth of a second: a loop of them makes a clock.
-- Everything is saved in the world's store. The blocks are in potluck.toml;
-- what each is called is in BLOCKS below.

local world = require("@potluck/world@0.15")
local tweak = require("@potluck/tweak")
local store = require("@potluck/store")
local ui = require("@potluck/ui@0.2")
local sound = require("@potluck/sound@0.2")

local knobs = tweak {
	daylight = { 0.5, min = 0.05, max = 1, label = "How light it is when a sensor turns on", order = 1 },
	button = { 1, min = 0.2, max = 5, label = "Seconds a button stays pressed", order = 2 },
	blink = { 0.25, min = 0.1, max = 1, label = "Fastest a lamp changes, in seconds", order = 3 },
	sounds = { true, label = "Levers and gates make sounds", order = 4 },
}

-- What each part is, by its blocks' names: [off, on].
local BLOCKS: { [string]: { string } } = {
	wire = { "Copper wire", "Live wire" },
	lever = { "Lever", "Pulled lever" },
	button = { "Button" },
	plate = { "Pressure plate" },
	sensor = { "Daylight sensor" },
	inverter = { "Inverter", "Lit inverter" },
	delay = { "Delay", "Lit delay" },
	lamp = { "Copper lamp", "Lit copper lamp" },
	gate = { "Copper gate", "Open copper gate" },
}
-- Which kind each block is, and whether it's its on look.
local KIND: { [string]: { kind: string, on: boolean } } = {}
for kind, names in BLOCKS do
	for i, name in names do
		KIND[name] = { kind = kind, on = i == 2 }
	end
end

local SOURCES = { lever = true, button = true, plate = true, sensor = true }
local GATES = { inverter = true, delay = true }

-- The way each facing points.
local AHEAD: { [string]: vector } = {
	north = vector.create(0, 0, -1),
	east = vector.create(1, 0, 0),
	south = vector.create(0, 0, 1),
	west = vector.create(-1, 0, 0),
}

local SIDES = {
	vector.create(1, 0, 0),
	vector.create(-1, 0, 0),
	vector.create(0, 1, 0),
	vector.create(0, -1, 0),
	vector.create(0, 0, 1),
	vector.create(0, 0, -1),
}
-- Wire also joins wire a step up or down.
local STEPS = {}
for _, d in { vector.create(1, 0, 0), vector.create(-1, 0, 0), vector.create(0, 0, 1), vector.create(0, 0, -1) } do
	table.insert(STEPS, d + vector.create(0, 1, 0))
	table.insert(STEPS, d - vector.create(0, 1, 0))
end

-- One part of a circuit: what it is, where, which way it faces (inverters
-- and delays), whether it's on, and for a delay its tenths of a second and
-- what's coming through it.
type Part = {
	at: vector,
	kind: string,
	facing: string?,
	on: boolean,
	-- what it shows: a lamp only changes its look every so often
	shown: boolean,
	shown_at: number,
	tenths: number,
	ends: number,
	queue: { { at: number, on: boolean } },
	heard: boolean,
}
local parts: { [string]: Part } = {}
-- Wire networks: their wire, what touches them that could power them, and
-- whether they're live (nil until first worked out, so they show it).
type Net = { wires: { string }, feeders: { { part: Part, to: vector } }, on: boolean? }
local net_of: { [string]: number } = {} -- each wire's network
local nets: { Net } = {}
local shaped = false -- the networks need working out again
local changed = false -- something to keep in the store

local function key(at: vector): string
	return math.floor(at.x) .. "," .. math.floor(at.y) .. "," .. math.floor(at.z)
end

local function middle(at: vector): vector
	return vector.create(math.floor(at.x) + 0.5, math.floor(at.y) + 0.5, math.floor(at.z) + 0.5)
end

local function add(at: vector, block: string): Part?
	local k = KIND[block]
	if not k then
		return nil
	end
	local p: Part = {
		at = middle(at),
		kind = k.kind,
		facing = if GATES[k.kind] then world.facing(at) or "north" else nil,
		on = k.on,
		shown = k.on,
		shown_at = -1,
		tenths = 1,
		ends = 0,
		queue = {},
		heard = k.on,
	}
	parts[key(at)] = p
	shaped, changed = true, true
	return p
end

-- Wire into networks: everything one piece of wire can reach through others.
local function shape()
	shaped = false
	net_of, nets = {}, {}
	for k, p in parts do
		if p.kind ~= "wire" or net_of[k] then
			continue
		end
		local n = #nets + 1
		local net: Net = { wires = {}, feeders = {}, on = nil }
		nets[n] = net
		local todo = { p.at }
		net_of[k] = n
		while #todo > 0 do
			local at = table.remove(todo) :: vector
			table.insert(net.wires, key(at))
			for _, d in SIDES do
				local q = parts[key(at + d)]
				if q and q.kind ~= "wire" then
					table.insert(net.feeders, { part = q, to = at })
				end
			end
			for _, list in { SIDES, STEPS } do
				for _, d in list do
					local q = at + d
					local qk = key(q)
					local other = parts[qk]
					if other and other.kind == "wire" and not net_of[qk] then
						net_of[qk] = n
						table.insert(todo, other.at)
					end
				end
			end
		end
	end
end

-- Whether a part gives power out to `to`, one block away: a source that's
-- on gives it every way, an inverter or delay only in front.
local function gives_to(p: Part, to: vector): boolean
	if not p.on then
		return false
	end
	if SOURCES[p.kind] then
		return true
	end
	if GATES[p.kind] then
		return key(p.at + AHEAD[p.facing or "north"]) == key(to)
	end
	return false
end

-- Whether power reaches `at` from the part or wire at `from`.
local function powered_from(from: vector, at: vector): boolean
	local q = parts[key(from)]
	if not q then
		return false
	end
	if q.kind == "wire" then
		local n = net_of[key(from)]
		return n ~= nil and nets[n].on == true
	end
	return gives_to(q, at)
end

-- Whether anything beside `at` powers it.
local function powered(at: vector): boolean
	for _, d in SIDES do
		if powered_from(at + d, at) then
			return true
		end
	end
	return false
end

-- Shows a part's state with its own block, keeping the way it faces.
local function show(p: Part, on: boolean)
	local names = BLOCKS[p.kind]
	local want = names[if on and #names > 1 then 2 else 1]
	if world.block(p.at) ~= want then
		world.set_block(p.at, want)
	end
end

-- Some players on a plate.
local function pressed(p: Part, players: { any }): boolean
	for _, who in players do
		if key(who.position) == key(p.at) then
			return true
		end
	end
	return false
end

-- Works the circuit out once: sources, then the wire they power, then
-- what that wire powers. Inverters and delays look at what came in last
-- time, so each takes a tenth of a second and a loop of them ticks.
local function step()
	if shaped then
		shape()
	end
	local now = world.time()
	local players = world.players()
	local light = world.daylight()
	for _, p in parts do
		if p.kind == "button" then
			p.on = now < p.ends
		elseif p.kind == "plate" then
			p.on = pressed(p, players)
		elseif p.kind == "sensor" then
			p.on = light >= knobs.daylight
		end
	end
	-- what reaches each wire network
	for _, net in nets do
		local on = false
		for _, f in net.feeders do
			if gives_to(f.part, f.to) then
				on = true
				break
			end
		end
		if on ~= net.on then
			net.on = on
			for _, wk in net.wires do
				show(parts[wk], on)
			end
		end
	end
	-- what comes into each inverter and delay, from behind
	local coming: { [Part]: boolean } = {}
	for _, p in parts do
		if GATES[p.kind] then
			coming[p] = powered_from(p.at - AHEAD[p.facing or "north"], p.at)
		end
	end
	for p, input in coming do
		local was = p.on
		if p.kind == "inverter" then
			p.on = not input
		else
			local last = if #p.queue > 0 then p.queue[#p.queue].on else p.heard
			if input ~= last and #p.queue < 40 then
				table.insert(p.queue, { at = now + p.tenths / 10 - 0.05, on = input })
			end
			while #p.queue > 0 and now >= p.queue[1].at do
				p.heard = table.remove(p.queue, 1).on
				p.on = p.heard
			end
		end
		if p.on ~= was then
			show(p, p.on)
		end
	end
	-- lamps, and gates with every gate they touch
	local seen: { [string]: boolean } = {}
	for k, p in parts do
		if p.kind == "lamp" then
			p.on = powered(p.at)
			if p.on ~= p.shown and now - p.shown_at >= knobs.blink then
				p.shown, p.shown_at = p.on, now
				show(p, p.on)
			end
		elseif p.kind == "gate" and not seen[k] then
			local group, todo, open = {}, { p }, false
			seen[k] = true
			while #todo > 0 do
				local g = table.remove(todo) :: Part
				table.insert(group, g)
				open = open or powered(g.at)
				for _, d in SIDES do
					local q = parts[key(g.at + d)]
					if q and q.kind == "gate" and not seen[key(q.at)] then
						seen[key(q.at)] = true
						table.insert(todo, q)
					end
				end
			end
			for _, g in group do
				if g.on ~= open then
					g.on = open
					show(g, open)
				end
			end
			if open ~= p.shown then
				p.shown = open
				if knobs.sounds then
					sound.play("thud", { at = p.at, volume = 0.4, caption = if open then "A gate opens" else "A gate shuts" })
				end
			end
		end
	end
end

world.every(0.1, step)

world.on("block_placed", function(_player, at: vector, block: string)
	local here = parts[key(at)]
	if here then
		parts[key(at)] = nil
		shaped, changed = true, true
	end
	add(at, block)
end)

world.on("block_broken", function(_player, at: vector, _block: string)
	if parts[key(at)] then
		parts[key(at)] = nil
		shaped, changed = true, true
	end
end)

world.on("block_used", function(player, at: vector, block: string)
	local p = parts[key(at)]
	if not p then
		-- one put down before this creation was here
		if not KIND[block] then
			return
		end
		p = add(at, block)
	end
	if not p then
		return
	end
	if p.kind == "lever" then
		p.on = not p.on
		show(p, p.on)
		changed = true
		if knobs.sounds then
			sound.play("click", { at = p.at, pitch = if p.on then 1.2 else 0.9, caption = if p.on then "Lever on" else "Lever off" })
		end
	elseif p.kind == "button" then
		p.ends = world.time() + knobs.button
		if knobs.sounds then
			sound.play("click", { at = p.at, pitch = 1.4, caption = "Button pressed" })
		end
	elseif p.kind == "delay" then
		-- 1, 2, 3, 5 then 10 tenths of a second, and round again
		local next = { [1] = 2, [2] = 3, [3] = 5, [5] = 10, [10] = 1 }
		p.tenths = next[p.tenths] or 1
		changed = true
		ui.show(player, "delay", string.format("Delay: %.1f seconds", p.tenths / 10), { at = "bottom" })
		world.after(2, function()
			ui.hide(player, "delay")
		end)
		if knobs.sounds then
			sound.play("click", { at = p.at, pitch = 1.6, caption = "Delay set" })
		end
	end
end)

-- On looks break into what they are: a live wire into copper wire.
for _, names in BLOCKS do
	if #names > 1 then
		world.drops(names[2], { gives = names[1] })
	end
end

-- Now and then, forget parts that aren't there any more (taken away by
-- another creation, or the world changed under them), a few at a time.
local checking: { string } = {}
world.every(0.5, function()
	if #checking == 0 then
		for k in parts do
			table.insert(checking, k)
		end
		return
	end
	for _ = 1, 32 do
		local k = table.remove(checking)
		if not k then
			break
		end
		local p = parts[k]
		local k2 = p and KIND[world.block(p.at)]
		if p and not (k2 and k2.kind == p.kind) then
			parts[k] = nil
			shaped, changed = true, true
		end
	end
end)

-- Keeps the circuits in the world's store, so they're there after a restart.
world.every(2, function()
	if not changed then
		return
	end
	changed = false
	local kept = {}
	for _, p in parts do
		table.insert(kept, { p.at.x, p.at.y, p.at.z, p.kind, p.facing or "", if p.kind == "lever" and p.on then 1 else 0, p.tenths })
	end
	store.set("parts", kept)
end)

do
	for _, s in (store.get("parts") or {}) :: { { any } } do
		local at = vector.create(s[1], s[2], s[3])
		local k = KIND[world.block(at)]
		if k and k.kind == s[4] then
			local p = add(at, world.block(at)) :: Part
			p.on = p.on or s[6] == 1
			p.tenths = s[7] or 1
		end
	end
	changed = false
end
