--!strict
-- Brickworks: blocks of fired clay, made in a kiln. The blocks themselves
-- are in potluck.toml, each drawn in letters ([blocks.red_bricks] and the
-- rest): change a colour or a row there and the world shows it. This file
-- is the kiln: use one (Interact while aiming at it) in survival to fire
-- what you carry into bricks, terracotta, roof tiles, slabs and glass. Make
-- a kiln, and paper lanterns, at a workbench.

local world = require("@potluck/world@0.12")
local tweak = require("@potluck/tweak")
local ui = require("@potluck/ui@0.1")
local sound = require("@potluck/sound@0.2")

local knobs = tweak {
	generous = { 1, min = 1, max = 3, label = "Fires this many times as much", order = 1 },
}

local KILN = "Kiln"

-- A firing: what goes in, what comes out and how many.
type Firing = { want: { [string]: number }, give: string, n: number }

-- Up to eight (a question has eight choices at most).
local FIRINGS: { Firing } = {
	{ want = { Dirt = 4 }, give = "Red bricks", n = 4 },
	{ want = { Gravel = 4 }, give = "Terracotta", n = 4 },
	{ want = { Terracotta = 2 }, give = "Roof tiles", n = 4 },
	{ want = { Terracotta = 1 }, give = "Clay slab", n = 2 },
	{ want = { Sand = 2 }, give = "Glass", n = 1 },
}

local function makes(f: Firing): number
	return math.floor(f.n * knobs.generous)
end

-- what it takes: "2 Terracotta"
local function needs(f: Firing): string
	local parts = {}
	for block, n in f.want do
		table.insert(parts, n .. " " .. block)
	end
	table.sort(parts)
	return table.concat(parts, " and ")
end

-- Fires one lot, or says why not.
local function fire(player, f: Firing): string
	for block, n in f.want do
		if world.has(player, block) < n then
			return "You'll need " .. needs(f) .. " for that."
		end
	end
	for block, n in f.want do
		world.take(player, block, n)
	end
	local given = world.give(player, f.give, makes(f))
	if given == 0 then
		for block, n in f.want do
			world.give(player, block, n)
		end
		return "Your hotbar is full. Make some room first."
	end
	return "Fired " .. given .. " " .. f.give .. "."
end

-- `said` is how the last firing went
local function choose(player, at: vector, said: string?)
	if world.pockets(player) == nil then
		ui.ask(player, {
			title = "Kiln",
			text = "In survival, use a kiln to fire what you carry: dirt into red bricks, gravel into terracotta, terracotta into roof tiles and slabs, and sand into glass.",
			choices = { "OK" },
		}, function() end)
		return
	end
	local choices = {}
	for _, f in FIRINGS do
		table.insert(choices, makes(f) .. " " .. f.give .. " for " .. needs(f))
	end
	local text = (if said then said .. "\n\n" else "") .. "What would you like to fire?"
	ui.ask(player, { title = "Kiln", text = text, choices = choices }, function(who, choice)
		local f = choice and FIRINGS[choice]
		if not f then
			return
		end
		local result = fire(who, f)
		if string.sub(result, 1, 5) == "Fired" then
			sound.play("roar", { at = at })
		end
		-- ask again, so firing several lots is quick
		choose(who, at, result)
	end)
end

world.on("block_used", function(player, at: vector, block: string)
	if block == KILN then
		choose(player, at)
	end
end)
