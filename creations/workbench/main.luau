--!strict
-- Workbench: crafting for survival. Use any table (Interact while aiming at
-- it) and pick something to make; it takes what that needs from your
-- hotbar and gives you what you made (world.has, world.take, world.give).
-- Every recipe is in RECIPES below, so a remix can add its own, change the
-- prices, or craft at a different block.

local world = require("@potluck/world@0.8")
local tweak = require("@potluck/tweak")
local ui = require("@potluck/ui@0.1")
local sound = require("@potluck/sound@0.1")

local knobs = tweak {
	generous = { 1, min = 1, max = 3, label = "Makes this many times as much", order = 1 },
	hint = { true, label = "Say so near a table", order = 2 },
}

local BENCH = "Table" -- the block you craft at

-- A recipe: what it takes, what it makes and how many.
type Recipe = { want: { [string]: number }, give: string, n: number }

-- Kinds of thing, each with its recipes. A question has eight choices at
-- most, so a longer list comes in pages, seven to a page and More. Tools,
-- swords and the anvil do their work through the Tools creation, hoes
-- through Farming, armor through the Armor creation, bows and the rest
-- through Bows, slings and spears, and a basket fills up into a balloon
-- through the Hot air balloon creation.
local KINDS = { "Building", "Furniture", "Lights", "Garden and farm", "Tools", "Weapons and armor", "Travel", "Circuits" }
local RECIPES: { [string]: { Recipe } } = {
	Building = {
		{ want = { ["Oak log"] = 1 }, give = "Planks", n = 4 },
		{ want = { ["Birch log"] = 1 }, give = "Planks", n = 4 },
		{ want = { Sand = 4 }, give = "Sandstone", n = 1 },
		{ want = { Stone = 4 }, give = "Mossy brick", n = 4 },
		-- Cave Crawlers' silk, spun
		{ want = { Silk = 4 }, give = "Cream wool", n = 1 },
		-- the Brickworks creation's, when it's in the world
		{ want = { Cobblestone = 8 }, give = "Kiln", n = 1 },
		-- the furnace creation's
		{ want = { Cobblestone = 8 }, give = "Furnace", n = 1 },
		-- the circuits creation's: it opens when powered
		{ want = { ["Copper ingot"] = 2 }, give = "Copper gate", n = 1 },
		-- the tools creation's: it makes iron and moonsilver tools and
		-- mends worn ones
		{ want = { ["Iron ingot"] = 5, Stone = 2 }, give = "Anvil", n = 1 },
	},
	Furniture = {
		{ want = { Planks = 6 }, give = "Table", n = 1 },
		{ want = { Planks = 2 }, give = "Stool", n = 1 },
		{ want = { Planks = 6 }, give = "Barrel", n = 1 },
		{ want = { Planks = 4 }, give = "Crate", n = 1 },
		{ want = { Planks = 6 }, give = "Bookshelf", n = 1 },
		-- cooks pies and stews over a fire (Furnaces and campfires)
		{ want = { ["Iron ingot"] = 3 }, give = "Cooking pot", n = 1 },
	},
	Lights = {
		{ want = { Coal = 1, Planks = 1 }, give = "Torch", n = 4 },
		{ want = { Hay = 1 }, give = "Candle", n = 2 },
		{ want = { Glass = 1, Cobblestone = 2 }, give = "Lantern", n = 1 },
		{ want = { Cobblestone = 4, ["Oak log"] = 1 }, give = "Brazier", n = 1 },
		{ want = { Candle = 1, ["Cream wool"] = 1 }, give = "Paper lantern", n = 2 },
		{ want = { Planks = 3, Coal = 1 }, give = "Campfire", n = 1 },
	},
	["Garden and farm"] = {
		{ want = { Planks = 3 }, give = "Fence", n = 2 },
		{ want = { Dirt = 3 }, give = "Clay pot", n = 1 },
		{ want = { Wheat = 4 }, give = "Hay", n = 1 },
		{ want = { Dirt = 4 }, give = "Farmland", n = 4 },
		-- tills a patch at a time (the farming creation)
		{ want = { ["Copper ingot"] = 2, Planks = 2 }, give = "Copper hoe", n = 1 },
		-- waters crops, and reaps them (farming too)
		{ want = { ["Copper ingot"] = 3 }, give = "Watering can", n = 1 },
		{ want = { ["Copper ingot"] = 2, Planks = 1 }, give = "Sickle", n = 1 },
		-- bees fill it with honey (the Bees creation)
		{ want = { Planks = 4, Hay = 2 }, give = "Beehive", n = 1 },
	},
	Tools = {
		{ want = { Planks = 3 }, give = "Wooden pickaxe", n = 1 },
		{ want = { Planks = 3 }, give = "Wooden axe", n = 1 },
		{ want = { Planks = 2 }, give = "Wooden shovel", n = 1 },
		{ want = { Cobblestone = 3, Planks = 2 }, give = "Stone pickaxe", n = 1 },
		{ want = { Cobblestone = 3, Planks = 2 }, give = "Stone axe", n = 1 },
		{ want = { Cobblestone = 1, Planks = 2 }, give = "Stone shovel", n = 1 },
		-- copper ones here; iron and moonsilver at an anvil
		{ want = { ["Copper ingot"] = 3, Planks = 2 }, give = "Copper pickaxe", n = 1 },
		{ want = { ["Copper ingot"] = 3, Planks = 2 }, give = "Copper axe", n = 1 },
		{ want = { ["Copper ingot"] = 1, Planks = 2 }, give = "Copper shovel", n = 1 },
		-- shears and a hammer (the Tools creation), a bucket and a rod
		-- (Buckets and fishing)
		{ want = { ["Iron ingot"] = 2 }, give = "Shears", n = 1 },
		{ want = { ["Iron ingot"] = 3, Planks = 2 }, give = "Hammer", n = 1 },
		{ want = { ["Iron ingot"] = 3 }, give = "Bucket", n = 1 },
		{ want = { Planks = 3, ["Cream wool"] = 2 }, give = "Fishing rod", n = 1 },
		{ want = { Planks = 3, Silk = 2 }, give = "Fishing rod", n = 1 },
		-- lights fires (the fire creation)
		{ want = { Flint = 1, ["Iron ingot"] = 1 }, give = "Flint and steel", n = 1 },
	},
	["Weapons and armor"] = {
		{ want = { Planks = 2 }, give = "Wooden sword", n = 1 },
		{ want = { Cobblestone = 2, Planks = 1 }, give = "Stone sword", n = 1 },
		{ want = { ["Copper ingot"] = 2, Planks = 1 }, give = "Copper sword", n = 1 },
		-- shot and thrown (the Bows, slings and spears creation)
		{ want = { Planks = 3, ["Cream wool"] = 2 }, give = "Bow", n = 1 },
		-- silk strings a bow better than wool (Cave Crawlers leave it)
		{ want = { Planks = 3, Silk = 2 }, give = "Bow", n = 1 },
		{ want = { Flint = 1, Planks = 1 }, give = "Arrow", n = 4 },
		{ want = { Flint = 1, Planks = 1, Feather = 1 }, give = "Arrow", n = 8 },
		-- boar bristles fletch them as well as a feather
		{ want = { Flint = 1, Planks = 1, Bristles = 1 }, give = "Arrow", n = 8 },
		{ want = { ["Cream wool"] = 3 }, give = "Sling", n = 1 },
		{ want = { Flint = 1, Planks = 3 }, give = "Spear", n = 1 },
		{ want = { Planks = 6, ["Iron ingot"] = 1 }, give = "Shield", n = 1 },
		-- wool and copper armor here; iron and moonsilver at an anvil
		{ want = { ["Cream wool"] = 5 }, give = "Wool cap", n = 1 },
		{ want = { ["Cream wool"] = 8 }, give = "Wool coat", n = 1 },
		{ want = { ["Cream wool"] = 7 }, give = "Wool leggings", n = 1 },
		{ want = { ["Cream wool"] = 4 }, give = "Wool boots", n = 1 },
		-- leather from Bramble Boars
		{ want = { Leather = 5 }, give = "Leather cap", n = 1 },
		{ want = { Leather = 8 }, give = "Leather coat", n = 1 },
		{ want = { Leather = 7 }, give = "Leather leggings", n = 1 },
		{ want = { Leather = 4 }, give = "Leather boots", n = 1 },
		{ want = { ["Copper ingot"] = 5 }, give = "Copper helmet", n = 1 },
		{ want = { ["Copper ingot"] = 8 }, give = "Copper coat", n = 1 },
		{ want = { ["Copper ingot"] = 7 }, give = "Copper leggings", n = 1 },
		{ want = { ["Copper ingot"] = 4 }, give = "Copper boots", n = 1 },
	},
	-- put down, a basket fills up into a hot air balloon (the balloon creation)
	Travel = {
		{ want = { ["Cream wool"] = 6, Planks = 4, Lantern = 1 }, give = "Balloon basket", n = 1 },
	},
	-- the circuits creation's parts
	Circuits = {
		{ want = { ["Copper ingot"] = 1 }, give = "Copper wire", n = 8 },
		{ want = { Cobblestone = 1, Planks = 1 }, give = "Lever", n = 1 },
		{ want = { Stone = 1 }, give = "Button", n = 1 },
		{ want = { Planks = 2 }, give = "Pressure plate", n = 1 },
		{ want = { Glass = 1, Crystal = 1, Planks = 2 }, give = "Daylight sensor", n = 1 },
		{ want = { ["Copper wire"] = 1, Torch = 1, Stone = 1 }, give = "Inverter", n = 1 },
		{ want = { ["Copper wire"] = 1, Torch = 2, Stone = 2 }, give = "Delay", n = 1 },
		{ want = { ["Copper ingot"] = 1, Glass = 1, Coal = 1 }, give = "Copper lamp", n = 1 },
	},
}

local function makes(r: Recipe): number
	return math.floor(r.n * knobs.generous)
end

-- what it takes: "1 Glass and 2 Cobblestone"
local function needs(r: Recipe): string
	local parts = {}
	for block, n in r.want do
		table.insert(parts, n .. " " .. block)
	end
	table.sort(parts)
	return table.concat(parts, " and ")
end

-- "4 Planks for 1 Oak log", or just "1 Lantern" when that's too long for a
-- choice (30 letters), with what it takes said in the question instead
local function label(r: Recipe): (string, string?)
	local full = makes(r) .. " " .. r.give .. " for " .. needs(r)
	if #full <= 30 then
		return full, nil
	end
	return makes(r) .. " " .. r.give, makes(r) .. " " .. r.give .. " takes " .. needs(r) .. "."
end

-- Makes one, or says why not.
local function craft(player, r: Recipe): string
	for block, n in r.want do
		if world.has(player, block) < n then
			return "You'll need " .. needs(r) .. " for that."
		end
	end
	for block, n in r.want do
		world.take(player, block, n)
	end
	local given = world.give(player, r.give, makes(r))
	if given == 0 then
		-- no room: give it all back
		for block, n in r.want do
			world.give(player, block, n)
		end
		return "Your hotbar is full. Make some room first."
	end
	return "Made " .. given .. " " .. r.give .. "."
end

local choose

-- Whether a block is in this world: some come from other creations (the
-- kiln and paper lanterns from Brickworks).
local function exists(name: string): boolean
	return (pcall(world.kind, name))
end

-- One page of a long list: what's on it, which page it is and how many
-- there are. Eight fit on one; past that, seven to a page and More.
local PAGE = 7
local function page_of<T>(list: { T }, page: number): ({ T }, number, number)
	if #list <= 8 then
		return list, 1, 1
	end
	local pages = math.ceil(#list / PAGE)
	page = (page - 1) % pages + 1
	return table.move(list, (page - 1) * PAGE + 1, math.min(page * PAGE, #list), 1, {}), page, pages
end

-- the recipes of one kind, a page of them; `said` is how the last one went
local function recipes(player, at: vector, kind: string, said: string?, page: number?)
	local list = {}
	for _, r in RECIPES[kind] do
		if exists(r.give) then
			table.insert(list, r)
		end
	end
	local shown, this, pages = page_of(list, page or 1)
	local choices, more = {}, {}
	for _, r in shown do
		local short, long = label(r)
		table.insert(choices, short)
		if long then
			table.insert(more, long)
		end
	end
	if pages > 1 then
		table.insert(choices, "More")
	end
	local text = (if said then said .. "\n\n" else "") .. "What would you like to make?"
	if #more > 0 then
		text ..= "\n\n" .. table.concat(more, "\n")
	end
	local title = "Workbench: " .. kind .. (if pages > 1 then " (" .. this .. " of " .. pages .. ")" else "")
	ui.ask(player, { title = title, text = text, choices = choices }, function(who, choice)
		if pages > 1 and choice == #shown + 1 then
			recipes(who, at, kind, nil, this + 1)
			return
		end
		local r = choice and shown[choice]
		if not r then
			return
		end
		local result = craft(who, r)
		if string.sub(result, 1, 4) == "Made" then
			sound.play("click", { at = at, caption = "Something's made" })
		end
		-- ask again, so making several is quick
		recipes(who, at, kind, result, this)
	end)
end

function choose(player, at: vector, page: number?)
	if world.pockets(player) == nil then
		ui.ask(player, {
			title = "Workbench",
			text = "In creative you've every block already. In survival, use a table to make things from what you've gathered.",
			choices = { "OK" },
		}, function() end)
		return
	end
	-- only kinds with something to make here (circuits need their creation)
	local kinds = {}
	for _, kind in KINDS do
		for _, r in RECIPES[kind] do
			if exists(r.give) then
				table.insert(kinds, kind)
				break
			end
		end
	end
	local shown, this, pages = page_of(kinds, page or 1)
	local choices = table.clone(shown)
	if pages > 1 then
		table.insert(choices, "More")
	end
	ui.ask(player, { title = "Workbench", text = "What would you like to make?", choices = choices }, function(who, choice)
		if pages > 1 and choice == #shown + 1 then
			choose(who, at, this + 1)
			return
		end
		local kind = choice and shown[choice]
		if kind then
			recipes(who, at, kind)
		end
	end)
end

world.on("block_used", function(player, at: vector, block: string)
	if block == BENCH then
		sound.play("chop", { at = at, volume = 0.3, caption = "Workbench" })
		choose(player, at)
	end
end)

-- a word to survival players who come near a table, once each time
local near: { [number]: boolean } = {}
world.every(1, function()
	if not knobs.hint then
		return
	end
	for _, p in world.players() do
		local close = world.pockets(p) ~= nil and world.find_block(p.position, 3, BENCH) ~= nil
		if close and not near[p.id] then
			ui.show(p, "bench", "Use a table to make things", { at = "left" })
			world.after(6, function()
				ui.hide(p, "bench")
			end)
		end
		near[p.id] = close
	end
end)

world.on("player_left", function(player)
	near[player.id] = nil
end)
