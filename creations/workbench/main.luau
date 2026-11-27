--!strict
-- Workbench: crafting for survival. Use any table (Interact while aiming at
-- it) and pick something to make; it takes what that needs from your
-- hotbar and gives you what you made (world.has, world.take, world.give).
-- Every recipe is in RECIPES below, so a remix can add its own, change the
-- prices, or craft at a different block.

local world = require("@potluck/world@0.5")
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

-- Up to eight kinds of thing, each with up to eight recipes (a question has
-- eight choices at most). Tools and swords do their work through the Tools
-- creation; without it they're only things to hold.
local KINDS = { "Building", "Furniture", "Lights", "Garden and farm", "Tools", "Weapons" }
local RECIPES: { [string]: { Recipe } } = {
	Building = {
		{ want = { ["Oak log"] = 1 }, give = "Planks", n = 4 },
		{ want = { ["Birch log"] = 1 }, give = "Planks", n = 4 },
		{ want = { Sand = 4 }, give = "Sandstone", n = 1 },
		{ want = { Stone = 4 }, give = "Mossy brick", n = 4 },
	},
	Furniture = {
		{ want = { Planks = 6 }, give = "Table", n = 1 },
		{ want = { Planks = 2 }, give = "Stool", n = 1 },
		{ want = { Planks = 6 }, give = "Barrel", n = 1 },
		{ want = { Planks = 6 }, give = "Bookshelf", n = 1 },
	},
	Lights = {
		{ want = { Hay = 1 }, give = "Candle", n = 2 },
		{ want = { Glass = 1, Cobblestone = 2 }, give = "Lantern", n = 1 },
		{ want = { Cobblestone = 4, ["Oak log"] = 1 }, give = "Brazier", n = 1 },
	},
	["Garden and farm"] = {
		{ want = { Planks = 3 }, give = "Fence", n = 2 },
		{ want = { Dirt = 3 }, give = "Clay pot", n = 1 },
		{ want = { Wheat = 4 }, give = "Hay", n = 1 },
		{ want = { Dirt = 4 }, give = "Farmland", n = 4 },
	},
	Tools = {
		{ want = { Planks = 3 }, give = "Wooden pickaxe", n = 1 },
		{ want = { Planks = 3 }, give = "Wooden axe", n = 1 },
		{ want = { Planks = 2 }, give = "Wooden shovel", n = 1 },
		{ want = { Cobblestone = 3, Planks = 2 }, give = "Stone pickaxe", n = 1 },
		{ want = { Cobblestone = 3, Planks = 2 }, give = "Stone axe", n = 1 },
		{ want = { Cobblestone = 1, Planks = 2 }, give = "Stone shovel", n = 1 },
	},
	Weapons = {
		{ want = { Planks = 2 }, give = "Wooden sword", n = 1 },
		{ want = { Cobblestone = 2, Planks = 1 }, give = "Stone sword", n = 1 },
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

-- the recipes of one kind; `said` is how the last one went
local function recipes(player, at: vector, kind: string, said: string?)
	local list = RECIPES[kind]
	local choices, more = {}, {}
	for _, r in list do
		local short, long = label(r)
		table.insert(choices, short)
		if long then
			table.insert(more, long)
		end
	end
	local text = (if said then said .. "\n\n" else "") .. "What would you like to make?"
	if #more > 0 then
		text ..= "\n\n" .. table.concat(more, "\n")
	end
	ui.ask(player, { title = "Workbench: " .. kind, text = text, choices = choices }, function(who, choice)
		local r = choice and list[choice]
		if not r then
			return
		end
		local result = craft(who, r)
		if string.sub(result, 1, 4) == "Made" then
			sound.play("click", { at = at, caption = "Something's made" })
		end
		-- ask again, so making several is quick
		recipes(who, at, kind, result)
	end)
end

function choose(player, at: vector)
	if world.pockets(player) == nil then
		ui.ask(player, {
			title = "Workbench",
			text = "In creative you've every block already. In survival, use a table to make things from what you've gathered.",
			choices = { "OK" },
		}, function() end)
		return
	end
	ui.ask(player, { title = "Workbench", text = "What would you like to make?", choices = KINDS }, function(who, choice)
		local kind = choice and KINDS[choice]
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
