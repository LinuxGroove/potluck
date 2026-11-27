--!strict
-- Mining, for survival. Each ore breaks into what it holds (world.drops),
-- once you hold a pickaxe of its level or better: wooden pickaxes are
-- level 1 and stone ones 2 (the tools creation says so). ORES below says
-- what each gives and needs.
--
-- Old chests are left in old workings underground. Use one and it gives
-- you its prizes and falls apart; the deeper it was, the better they are.
-- PRIZES below says what can be in one.
--
-- Breaking gravel turns up flint now and then, for a flint and steel.

local world = require("@potluck/world@0.14")
local tweak = require("@potluck/tweak")
local ui = require("@potluck/ui@0.2")
local sound = require("@potluck/sound@0.1")

local knobs = tweak {
	levels = { true, label = "Ores need the right pickaxe", order = 1 },
	more = { 1, min = 1, max = 3, label = "How much each ore gives", order = 2 },
	prizes = { 1, min = 0.5, max = 3, label = "How generous old chests are", order = 3 },
	flint = { 0.15, min = 0, max = 1, label = "Chance gravel turns up flint", order = 4 },
}

type Ore = { gives: string, least: number, most: number, level: number }

-- What each ore gives, from least to most of it, and the level of pickaxe
-- it needs.
local ORES: { [string]: Ore } = {
	["Coal ore"] = { gives = "Coal", least = 1, most = 2, level = 1 },
	["Copper ore"] = { gives = "Raw copper", least = 1, most = 3, level = 2 },
	["Iron ore"] = { gives = "Raw iron", least = 1, most = 1, level = 2 },
	["Gold ore"] = { gives = "Raw gold", least = 1, most = 1, level = 2 },
	["Crystal ore"] = { gives = "Crystal", least = 1, most = 2, level = 2 },
	["Moonsilver ore"] = { gives = "Raw moonsilver", least = 1, most = 1, level = 2 },
}

for name, o in ORES do
	world.drops(name, {
		gives = o.gives,
		count = { math.floor(o.least * knobs.more), math.floor(o.most * knobs.more) },
		level = if knobs.levels then o.level else 0,
	})
end

type Prize = { thing: string, least: number, most: number, chance: number, below: number? }

-- What an old chest can hold: each prize is in a chest by its chance,
-- from least to most of it, but only in chests deeper than `below` (a
-- height; the sea is at 19).
local PRIZES: { Prize } = {
	{ thing = "Coal", least = 2, most = 6, chance = 0.6 },
	{ thing = "Lantern", least = 1, most = 2, chance = 0.45 },
	{ thing = "Candle", least = 1, most = 3, chance = 0.3 },
	{ thing = "Carrots", least = 2, most = 5, chance = 0.3 },
	{ thing = "Glowcap", least = 1, most = 3, chance = 0.25 },
	{ thing = "Stone pickaxe", least = 1, most = 1, chance = 0.2 },
	{ thing = "Stone sword", least = 1, most = 1, chance = 0.15 },
	{ thing = "Flint", least = 1, most = 3, chance = 0.3 },
	{ thing = "Bread", least = 1, most = 3, chance = 0.25 },
	{ thing = "Flint and steel", least = 1, most = 1, chance = 0.1 },
	{ thing = "Raw copper", least = 1, most = 4, chance = 0.5, below = 60 },
	{ thing = "Raw iron", least = 1, most = 4, chance = 0.45, below = 20 },
	{ thing = "Iron ingot", least = 1, most = 3, chance = 0.25, below = 0 },
	{ thing = "Raw gold", least = 1, most = 3, chance = 0.35, below = -10 },
	{ thing = "Crystal", least = 1, most = 3, chance = 0.3, below = -40 },
	{ thing = "Raw moonsilver", least = 1, most = 2, chance = 0.15, below = -80 },
}

type Find = { thing: string, n: number }

-- what's still in chests players' pockets were too full for, by place
local left: { [string]: { Find } } = {}

local function key(at: vector): string
	return string.format("%d,%d,%d", math.floor(at.x), math.floor(at.y), math.floor(at.z))
end

-- A chest's prizes at height y: each by its chance, and always something.
local function roll(y: number): { Find }
	local out: { Find } = {}
	for _, p in PRIZES do
		if (p.below == nil or y < p.below) and world.random() < math.min(1, p.chance * knobs.prizes) then
			local n = math.floor(world.random(p.least, p.most + 0.999) * knobs.prizes)
			table.insert(out, { thing = p.thing, n = math.clamp(n, 1, 64) })
		end
	end
	if #out == 0 then
		table.insert(out, { thing = "Coal", n = 3 })
	end
	return out
end

-- "4 coal, 1 lantern and 1 stone pickaxe"
local function say(finds: { Find }): string
	local parts = {}
	for _, f in finds do
		table.insert(parts, f.n .. " " .. string.lower(f.thing))
	end
	if #parts == 1 then
		return parts[1]
	end
	return table.concat(parts, ", ", 1, #parts - 1) .. " and " .. parts[#parts]
end

local shown: { [number]: number } = {}

-- one line above the hotbar for a few seconds, for each player
local function tell(player, text: string)
	ui.show(player, "found", text, { at = "bottom" })
	local mine = (shown[player.id] or 0) + 1
	shown[player.id] = mine
	world.after(4, function()
		if shown[player.id] == mine then
			shown[player.id] = nil
			ui.hide(player, "found")
		end
	end)
end

world.on("player_left", function(p)
	shown[p.id] = nil
end)

-- flint, now and then, as well as the gravel
world.on("block_broken", function(player, _at: vector, block: string)
	if block == "Gravel" and world.pockets(player) ~= nil and world.random() < knobs.flint then
		world.give(player, "Flint", 1)
	end
end)

world.on("block_used", function(player, at: vector, block: string)
	if block ~= "Old chest" then
		return
	end
	if world.pockets(player) == nil then
		tell(player, "Old chests hold prizes for survival.")
		return
	end
	local k = key(at)
	local finds = left[k] or roll(at.y)
	local got: { Find } = {}
	local still: { Find } = {}
	for _, f in finds do
		local n = world.give(player, f.thing, f.n)
		if n > 0 then
			table.insert(got, { thing = f.thing, n = n })
		end
		if n < f.n then
			table.insert(still, { thing = f.thing, n = f.n - n })
		end
	end
	if #still > 0 then
		left[k] = still
		tell(player, if #got > 0 then "You found " .. say(got) .. ". Your pockets are full." else "Your pockets are full.")
		return
	end
	left[k] = nil
	world.set_block(at, "Air")
	sound.play("sparkle", { at = at, caption = "An old chest opens" })
	tell(player, "You found " .. say(got) .. ".")
end)
