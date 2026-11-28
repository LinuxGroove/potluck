--!strict
-- Buckets and fishing, for survival. Interact on water with something in
-- hand is heard as block_used on "Water" (world 0.17), so:
-- a Bucket used on water fills it into a Water bucket, and a Water bucket
-- used on a block pours the water out on top of it and empties again.
-- A Fishing rod used on water casts its line: keep the rod in hand and
-- stay near, and after a few seconds something bites (CATCHES below says
-- what). Raw fish cooks into cooked fish in a furnace or on a campfire.

local world = require("@potluck/world@0.17")
local tweak = require("@potluck/tweak")
local ui = require("@potluck/ui@0.2")
local sound = require("@potluck/sound@0.2")

local knobs = tweak {
	bite = { 6, min = 1, max = 30, label = "Seconds to a bite, about", order = 1 },
	pour = { true, label = "A water bucket pours water out", order = 2 },
	casts = { 64, min = 8, max = 256, label = "Catches before a rod wears out", order = 3 },
}

local UP = vector.create(0, 1, 0)
local ROD, BUCKET, FULL = "Fishing rod", "Bucket", "Water bucket"

-- What comes up on a line, each by its chance: mostly fish.
type Catch = { thing: string, chance: number }
local CATCHES: { Catch } = {
	{ thing = "Raw fish", chance = 0.84 },
	{ thing = "Planks", chance = 0.06 }, -- driftwood
	{ thing = "Flint", chance = 0.05 },
	{ thing = "Iron ingot", chance = 0.03 }, -- somebody dropped it
	{ thing = "Crystal", chance = 0.02 },
}

-- Blocks other creations use, which a water bucket leaves alone so using
-- them still works: a remix can add its own.
local KEEP_DRY: { [string]: boolean } = {
	Table = true, Furnace = true, ["Lit furnace"] = true, Campfire = true, Anvil = true,
	Crate = true, Barrel = true, ["Old chest"] = true, Kiln = true, Farmland = true,
}

-- one line for a few seconds, for each player
local shown: { [number]: number } = {}
local function tell(player, text: string, seconds: number?)
	ui.show(player, "water", text, { at = "bottom" })
	local mine = (shown[player.id] or 0) + 1
	shown[player.id] = mine
	world.after(seconds or 3, function()
		if shown[player.id] == mine then
			shown[player.id] = nil
			ui.hide(player, "water")
		end
	end)
end

-- swaps one thing in hand for another, or says why not
local function swap(player, from: string, to: string): boolean
	if not world.take(player, from, 1) then
		return false
	end
	if world.give(player, to, 1) == 0 then
		world.give(player, from, 1)
		tell(player, "Your pockets are full.")
		return false
	end
	return true
end

-- Fishing: who's waiting for a bite, where, and how long till it comes.
type Line = { at: vector, left: number }
local lines: { [number]: Line } = {}
local caught: { [number]: number } = {} -- catches on each player's rod

local function catch(): string
	local roll = world.random()
	for _, c in CATCHES do
		roll -= c.chance
		if roll <= 0 then
			return c.thing
		end
	end
	return CATCHES[1].thing
end

world.on("block_used", function(player, at: vector, block: string)
	if world.pockets(player) == nil then
		return
	end
	local held = world.held(player)
	if block == "Water" then
		if held == BUCKET then
			if swap(player, BUCKET, FULL) then
				sound.play("splash", { at = at, volume = 0.6 })
			end
		elseif held == ROD and not lines[player.id] then
			lines[player.id] = { at = at, left = knobs.bite * world.random(0.6, 1.6) }
			sound.play("cast", { at = at, volume = 0.7 })
			tell(player, "Fishing. Keep the rod in hand and wait for a bite.", 4)
		elseif held == ROD then
			-- reeled in before anything bit
			lines[player.id] = nil
			ui.hide(player, "water")
		end
	elseif held == FULL and knobs.pour and not KEEP_DRY[block] and block ~= "Air" then
		local above = at + UP
		if world.block(above) == "Air" and swap(player, FULL, BUCKET) then
			world.set_block(above, "Water")
			sound.play("splash", { at = above })
		end
	end
end)

world.every(0.25, function()
	local here = {}
	for _, p in world.players() do
		here[p.id] = p
	end
	for id, line in lines do
		local p = here[id]
		if not p or world.held(p) ~= ROD or vector.magnitude(p.position - line.at) > 12 then
			lines[id] = nil
			continue
		end
		line.left -= 0.25
		if line.left <= 0 then
			lines[id] = nil
			local thing = catch()
			if world.give(p, thing, 1) == 0 then
				tell(p, "Something bit, but your pockets are full.")
				continue
			end
			sound.play("bite", { at = line.at })
			local said = if thing == "Raw fish" then "a fish" else string.lower(thing)
			tell(p, "You caught " .. said .. ".")
			-- every catch wears the rod a little
			caught[id] = (caught[id] or 0) + 1
			if caught[id] >= math.floor(knobs.casts) then
				caught[id] = 0
				if world.take(p, ROD, 1) then
					tell(p, "You caught " .. said .. ", and your fishing rod broke.")
					sound.play_for(p, "crack", { pitch = 1.3, caption = "A rod breaks" })
				end
			end
		end
	end
end)

world.on("player_left", function(p)
	lines[p.id] = nil
	caught[p.id] = nil
	shown[p.id] = nil
end)
