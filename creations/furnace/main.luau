--!strict
-- Furnaces and campfires, for survival. Use a furnace (Interact while aiming
-- at it) to smelt what you carry: raw metal into ingots, logs into charcoal,
-- sand into glass and cobblestone back into stone. It cooks food too. It
-- takes its fuel from your pockets (coal, charcoal, logs or planks), lights
-- up and works through the lot a few seconds at a time, and what it makes
-- waits inside until you, or anyone, use it again.
--
-- A campfire cooks food with no fuel, and its light keeps Gloomlings back.
--
-- A cooking pot set over a campfire, a fire or a lit furnace (under it or
-- beside it) cooks dishes of several things at once: pies, stews, porridge
-- and honey cake (DISHES below). Use it and pick one.
--
-- SMELTS, COOKS, DISHES and FUELS below say what goes in, what comes out
-- and how far fuel goes, so a remix can add its own. The blocks are in potluck.toml;
-- the lit furnace and the campfire flicker through pictures of their own.

local world = require("@potluck/world@0.21")
local tweak = require("@potluck/tweak")
local store = require("@potluck/store")
local ui = require("@potluck/ui@0.2")
local sound = require("@potluck/sound@0.2")

local knobs = tweak {
	seconds = { 5, min = 1, max = 20, label = "Seconds to smelt or cook one", order = 1 },
	fuel = { 1, min = 0.5, max = 4, label = "How far fuel goes", order = 2 },
	most = { 16, min = 1, max = 64, label = "Most it takes in at once", order = 3 },
}

local FURNACE, LIT, CAMPFIRE = "Furnace", "Lit furnace", "Campfire"

-- A recipe: `n` of `from` make `count` of `gives`.
type Recipe = { from: string, n: number, gives: string, count: number }

-- Only a furnace is hot enough for these.
local SMELTS: { Recipe } = {
	{ from = "Raw copper", n = 1, gives = "Copper ingot", count = 1 },
	{ from = "Raw iron", n = 1, gives = "Iron ingot", count = 1 },
	{ from = "Raw gold", n = 1, gives = "Gold ingot", count = 1 },
	{ from = "Raw moonsilver", n = 1, gives = "Moonsilver ingot", count = 1 },
	{ from = "Oak log", n = 1, gives = "Charcoal", count = 1 },
	{ from = "Birch log", n = 1, gives = "Charcoal", count = 1 },
	{ from = "Sand", n = 1, gives = "Glass", count = 1 },
	{ from = "Cobblestone", n = 1, gives = "Stone", count = 1 },
}

-- A furnace or a campfire cooks these.
local COOKS: { Recipe } = {
	{ from = "Carrots", n = 1, gives = "Roasted carrots", count = 1 },
	{ from = "Raw fish", n = 1, gives = "Cooked fish", count = 1 },
	{ from = "Wheat", n = 3, gives = "Bread", count = 1 },
	{ from = "Potatoes", n = 1, gives = "Baked potato", count = 1 },
	{ from = "Egg", n = 1, gives = "Fried egg", count = 1 },
	{ from = "Raw meat", n = 1, gives = "Cooked meat", count = 1 },
}

-- How many lots each fuel smelts, best first: the furnace burns the first
-- it finds in your pockets.
local FUELS: { { name: string, lots: number } } = {
	{ name = "Coal", lots = 8 },
	{ name = "Charcoal", lots = 8 },
	-- an Ember Imp's ember burns long and hot
	{ name = "Ember", lots = 6 },
	{ name = "Oak log", lots = 2 },
	{ name = "Birch log", lots = 2 },
	{ name = "Planks", lots = 1 },
}

-- What a furnace or campfire has in it: the recipe it's working through,
-- how many lots are left, what's made and waiting, how much fuel is left
-- (in lots) and when the next lot is done.
type Cooker = { at: vector, recipe: number?, smelt: boolean?, left: number, made: string?, ready: number, fuel: number, next: number }
local cookers: { [string]: Cooker } = {}
local changed = false

local function key(at: vector): string
	return math.floor(at.x) .. "," .. math.floor(at.y) .. "," .. math.floor(at.z)
end

local function cooker(at: vector): Cooker
	local k = key(at)
	local c = cookers[k]
	if not c then
		c = { at = at, left = 0, ready = 0, fuel = 0, next = 0 }
		cookers[k] = c
	end
	return c
end

local function recipe_of(c: Cooker): Recipe?
	if not c.recipe then
		return nil
	end
	return if c.smelt then SMELTS[c.recipe] else COOKS[c.recipe]
end

-- Whether a block or item is in this world: eggs, potatoes and the like
-- come from other creations.
local function exists(name: string): boolean
	return (pcall(world.kind, name))
end

-- "3 Raw iron"
local function some(n: number, name: string): string
	return n .. " " .. name
end

-- Gives a player what's waiting, as much as fits.
local function collect(player, c: Cooker): string?
	if not c.made or c.ready == 0 then
		return nil
	end
	local given = world.give(player, c.made, c.ready)
	local made = c.made
	c.ready -= given
	if c.ready == 0 and c.left == 0 then
		c.made = nil
	end
	changed = true
	if given == 0 then
		return "Your pockets are full, so the " .. made .. " waits here."
	end
	return "You took " .. some(given, made) .. "."
end

-- Burns fuel from a player's pockets until there's enough for `lots`, and
-- says how many lots it can do.
local function stoke(player, c: Cooker, lots: number): number
	local per = function(f: { name: string, lots: number }): number
		return f.lots * knobs.fuel
	end
	for _, f in FUELS do
		while c.fuel < lots and world.has(player, f.name) >= 1 do
			world.take(player, f.name, 1)
			c.fuel += per(f)
		end
	end
	return math.min(lots, math.floor(c.fuel))
end

-- Puts a lot of something in, if the cooker can take it.
local function load(player, c: Cooker, list: { Recipe }, i: number, furnace: boolean): string
	local r = list[i]
	local smelt = list == SMELTS
	if c.left > 0 and (c.recipe ~= i or c.smelt ~= smelt) then
		return "It's busy with " .. (recipe_of(c) :: Recipe).from .. ". Come back when it's done."
	end
	if c.made and c.made ~= r.gives and c.ready > 0 then
		return "Take out the " .. c.made .. " first."
	end
	local lots = math.min(math.floor(world.has(player, r.from) / r.n), math.max(1, math.floor(knobs.most / r.n)))
	if lots < 1 then
		return "You'll need " .. some(r.n, r.from) .. " for that."
	end
	-- in first, so the furnace doesn't burn what it's to smelt
	world.take(player, r.from, lots * r.n)
	if furnace then
		local fueled = stoke(player, c, lots)
		if fueled < lots then
			world.give(player, r.from, (lots - fueled) * r.n)
		end
		lots = fueled
		if lots < 1 then
			return "It needs fuel: coal, charcoal, logs or planks."
		end
	end
	if c.left == 0 then
		c.next = world.time() + knobs.seconds
	end
	c.recipe, c.smelt, c.made = i, smelt, r.gives
	c.left += lots
	changed = true
	if furnace and world.block(c.at) == FURNACE then
		world.set_block(c.at, LIT)
	end
	sound.play(if furnace then "roar" else "crackle", { at = c.at, volume = 0.6 })
	return (if furnace and smelt then "Smelting " else "Cooking ") .. some(lots * r.n, r.from) .. "."
end

-- What it's doing, in a line or two.
local function doing(c: Cooker): string
	local r = recipe_of(c)
	local lines = {}
	if r and c.left > 0 then
		table.insert(lines, some(c.left * r.n, r.from) .. " to go, " .. math.ceil(c.left * knobs.seconds) .. " seconds.")
	end
	if c.made and c.ready > 0 then
		table.insert(lines, some(c.ready, c.made) .. " ready.")
	end
	return table.concat(lines, " ")
end

local choose

-- `said` is how the last thing went
function choose(player, at: vector, block: string, said: string?)
	local furnace = block ~= CAMPFIRE
	local title = if furnace then "Furnace" else "Campfire"
	if world.pockets(player) == nil then
		ui.ask(player, {
			title = title,
			text = if furnace
				then "In survival, a furnace smelts raw metal into ingots, logs into charcoal and sand into glass, and cooks food, burning coal, charcoal, logs or planks."
				else "In survival, a campfire cooks food with no fuel. Its light keeps Gloomlings away.",
			choices = { "OK" },
		}, function() end)
		return
	end
	local c = cooker(at)
	local took = collect(player, c)
	-- what they carry that it can take: smelting first, then cooking
	local options: { { list: { Recipe }, i: number } } = {}
	local choices = {}
	local lists = if furnace then { SMELTS, COOKS } else { COOKS }
	for _, list in lists do
		for i, r in list do
			if exists(r.from) and world.has(player, r.from) >= r.n and #choices < 8 then
				table.insert(options, { list = list, i = i })
				local verb = if list == SMELTS then "Smelt " else "Cook "
				table.insert(choices, verb .. r.from)
			end
		end
	end
	local parts = {}
	for _, s in { said, took, doing(c) } do
		if s and s ~= "" then
			table.insert(parts, s)
		end
	end
	if #choices == 0 then
		table.insert(parts, if furnace then "Bring raw metal, logs, sand, cobblestone, or food to cook: carrots, potatoes, eggs, fish or wheat." else "Bring food to cook: carrots, potatoes, eggs, fish or wheat.")
	else
		table.insert(parts, "What would you like to put in?")
	end
	ui.ask(player, { title = title, text = table.concat(parts, "\n\n"), choices = if #choices > 0 then choices else { "OK" } }, function(who, choice)
		local o = choice and options[choice]
		if not o then
			return
		end
		-- what's in it now: the one asked about may have been tidied away
		choose(who, at, world.block(at), load(who, cooker(at), o.list, o.i, furnace))
	end)
end

-- ---- the cooking pot ----

local POT = "Cooking pot"
-- what's hot enough to cook over, under the pot or beside it
local HEAT: { [string]: boolean } = { [CAMPFIRE] = true, [LIT] = true, Fire = true }
local NEAR = { vector.create(0, -1, 0), vector.create(1, 0, 0), vector.create(-1, 0, 0), vector.create(0, 0, 1), vector.create(0, 0, -1) }

-- A dish: what it takes, what it makes and how many, and what comes back
-- (the bucket the milk was in). Eggs come from the Hens creation, honey
-- from Bees and the rest from Farming, and a dish shows only when all it
-- takes is in the world.
type Dish = { want: { [string]: number }, give: string, n: number, back: string? }
local DISHES: { Dish } = {
	{ want = { Pumpkin = 1, Egg = 1, Wheat = 2 }, give = "Pumpkin pie", n = 2 },
	{ want = { Berries = 4, Egg = 1, Wheat = 2 }, give = "Berry pie", n = 2 },
	{ want = { Beans = 2, Potatoes = 1, Carrots = 1 }, give = "Bean stew", n = 2 },
	{ want = { ["Raw fish"] = 1, Potatoes = 1, ["Milk bucket"] = 1 }, give = "Fish stew", n = 2, back = "Bucket" },
	{ want = { Wheat = 2, ["Milk bucket"] = 1 }, give = "Porridge", n = 2, back = "Bucket" },
	{ want = { Wheat = 3, Egg = 1, Honey = 1, ["Milk bucket"] = 1 }, give = "Honey cake", n = 2, back = "Bucket" },
}

local function hot(at: vector): boolean
	for _, d in NEAR do
		if HEAT[world.block(at + d)] then
			return true
		end
	end
	return false
end

-- what a dish takes: "1 Egg and 1 Pumpkin and 2 Wheat"
local function takes(d: Dish): string
	local parts = {}
	for name, n in d.want do
		table.insert(parts, n .. " " .. name)
	end
	table.sort(parts)
	return table.concat(parts, " and ")
end

-- Cooks one, or says why not.
local function cook(player, d: Dish): (boolean, string)
	for name, n in d.want do
		if world.has(player, name) < n then
			return false, "You'll need " .. takes(d) .. " for that."
		end
	end
	for name, n in d.want do
		world.take(player, name, n)
	end
	local given = world.give(player, d.give, d.n)
	if given == 0 then
		for name, n in d.want do
			world.give(player, name, n)
		end
		return false, "Your pockets are full. Make some room first."
	end
	if d.back then
		world.give(player, d.back, 1)
	end
	return true, "You cooked " .. some(given, d.give) .. "."
end

local function stir(player, at: vector, said: string?)
	if world.pockets(player) == nil then
		ui.ask(player, { title = "Cooking pot", text = "In survival, a cooking pot over a campfire cooks pies, stews, porridge and honey cake.", choices = { "OK" } }, function() end)
		return
	end
	if not hot(at) then
		ui.ask(player, { title = "Cooking pot", text = "It's cold. Set it over a campfire, a fire or a lit furnace, or right beside one.", choices = { "OK" } }, function() end)
		return
	end
	local list, choices, lines = {}, {}, {}
	for _, d in DISHES do
		local ok = exists(d.give)
		for name in d.want do
			ok = ok and exists(name)
		end
		if ok and #list < 8 then
			table.insert(list, d)
			table.insert(choices, some(d.n, d.give))
			table.insert(lines, d.give .. ": " .. takes(d))
		end
	end
	if #list == 0 then
		ui.ask(player, { title = "Cooking pot", text = "There's nothing to cook in this world yet.", choices = { "OK" } }, function() end)
		return
	end
	local text = (if said then said .. "\n\n" else "") .. "What would you like to cook?\n\n" .. table.concat(lines, "\n")
	ui.ask(player, { title = "Cooking pot", text = text, choices = choices }, function(who, choice)
		local d = choice and list[choice]
		if not d then
			return
		end
		local cooked, said = cook(who, d)
		if cooked then
			sound.play("crackle", { at = at, volume = 0.5, caption = "Something's cooking" })
		end
		-- ask again, so cooking several is quick
		stir(who, at, said)
	end)
end

world.on("block_used", function(player, at: vector, block: string)
	if block == FURNACE or block == LIT or block == CAMPFIRE then
		choose(player, at, block)
	elseif block == POT then
		stir(player, at)
	end
end)

-- Breaking one gives back what was in it, as much as fits.
world.on("block_broken", function(player, at: vector, block: string)
	if block ~= FURNACE and block ~= LIT and block ~= CAMPFIRE then
		return
	end
	local k = key(at)
	local c = cookers[k]
	if not c then
		return
	end
	local r = recipe_of(c)
	if r and c.left > 0 then
		world.give(player, r.from, c.left * r.n)
	end
	if c.made and c.ready > 0 then
		world.give(player, c.made, c.ready)
	end
	cookers[k] = nil
	changed = true
end)

-- Works through each lot, and puts the furnace out when it's done.
world.every(0.5, function()
	local now = world.time()
	for k, c in cookers do
		local here = world.block(c.at)
		if here ~= FURNACE and here ~= LIT and here ~= CAMPFIRE then
			cookers[k] = nil
			changed = true
			continue
		end
		local r = recipe_of(c)
		while r and c.left > 0 and now >= c.next do
			c.left -= 1
			if here ~= CAMPFIRE then
				c.fuel = math.max(0, c.fuel - 1)
			end
			c.ready += r.count
			c.next += knobs.seconds
			changed = true
		end
		if c.left == 0 and here == LIT then
			world.set_block(c.at, FURNACE)
			-- a tiny bit of fuel left over stays for next time
		end
		if c.left == 0 and c.ready == 0 and c.fuel < 1 then
			cookers[k] = nil
			changed = true
		end
	end
end)

-- Keeps what's in them in the world's store, so it's there after a restart.
world.every(2, function()
	if not changed then
		return
	end
	changed = false
	local now = world.time()
	local kept = {}
	for _, c in cookers do
		table.insert(kept, {
			x = c.at.x,
			y = c.at.y,
			z = c.at.z,
			recipe = c.recipe,
			smelt = c.smelt,
			left = c.left,
			made = c.made,
			ready = c.ready,
			fuel = c.fuel,
			wait = math.max(0, c.next - now),
		})
	end
	store.set("cookers", kept)
end)

do
	local now = world.time()
	for _, s in (store.get("cookers") or {}) :: { any } do
		local at = vector.create(s.x, s.y, s.z)
		cookers[key(at)] = { at = at, recipe = s.recipe, smelt = s.smelt, left = s.left, made = s.made, ready = s.ready, fuel = s.fuel, next = now + s.wait }
	end
end

-- A lit furnace breaks into a furnace.
world.drops(LIT, { gives = FURNACE })
