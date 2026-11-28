--!strict
-- Tools, for survival. Holding a pickaxe, stone and brick break faster; an
-- axe, wood; a shovel, earth, sand and gravel; a hoe, plants (world.digging,
-- by the kind of block). A tool's level is how hard a block it reaches:
-- ores need a pickaxe of their level or better (the mining creation says
-- which). Wood is level 1, stone 2, copper 3, iron 4 and moonsilver 5.
-- Every block a tool breaks wears it a little (a hoe, every patch it
-- tills), a bar beside your health shows how much it has left, and a worn
-- out tool breaks.
-- Shears cut leaves and plants fast (and get more wool from a Woolly), a
-- sickle cuts plants (and harvests a square of ripe crops for the farming
-- creation), and a hammer breaks stone three by three, leaving ores for a
-- pickaxe (HAMMERED below says what it breaks).
-- The anvil (made at a workbench from iron) makes iron and moonsilver
-- tools and armor (for the Armor creation), and mends a worn tool with one
-- more of what it's made of.
-- TOOLS below says what each does, so a remix can change them. Swords are
-- for Gloomlings, who say what a sword does to them, and hoes till for the
-- farming creation.

local world = require("@potluck/world@0.17")
local tweak = require("@potluck/tweak")
local ui = require("@potluck/ui@0.2")
local sound = require("@potluck/sound@0.2")

local knobs = tweak {
	faster = { 1, min = 0.5, max = 3, label = "How much faster tools dig", order = 1 },
	wear = { true, label = "Tools wear out", order = 2 },
	last = { 1, min = 0.5, max = 4, label = "How long tools last", order = 3 },
	color = { "#a8b0b8", label = "Wear bar", order = 4 },
	mend = { 1, min = 1, max = 3, label = "What mending takes", order = 5 },
	hammer = { true, label = "A hammer breaks three by three", order = 6 },
}

type Tool = { digs: string, speed: number, level: number, uses: number, made: string }

local ANVIL = "Anvil"

-- What each tool digs faster (soft, hard, wood, plant or glass), how much
-- faster, its level, how many blocks it breaks before it wears out, and
-- what it's made of (which mends it).
local TOOLS: { [string]: Tool } = {}

-- what each tool needs to dig faster
local KINDS: { [string]: string } = { pickaxe = "hard", axe = "wood", shovel = "soft", hoe = "plant" }

-- each material: its name in a tool's, what mends it, speed, level and uses
local MATERIALS = {
	{ name = "Wooden", made = "Planks", speed = 2.5, level = 1, uses = 60 },
	{ name = "Stone", made = "Cobblestone", speed = 4, level = 2, uses = 130 },
	{ name = "Copper", made = "Copper ingot", speed = 5, level = 3, uses = 200 },
	{ name = "Iron", made = "Iron ingot", speed = 6.5, level = 4, uses = 320 },
	{ name = "Moonsilver", made = "Moonsilver ingot", speed = 8.5, level = 5, uses = 640 },
}

for _, m in MATERIALS do
	for kind, digs in KINDS do
		-- there are no wooden or stone hoes: a hand tills a block as well
		if kind ~= "hoe" or m.level >= 3 then
			TOOLS[m.name .. " " .. kind] = { digs = digs, speed = m.speed, level = m.level, uses = m.uses, made = m.made }
		end
	end
end

-- and the tools that aren't in every material
TOOLS.Shears = { digs = "plant", speed = 5, level = 1, uses = 240, made = "Iron ingot" }
TOOLS.Sickle = { digs = "plant", speed = 4, level = 1, uses = 200, made = "Copper ingot" }
TOOLS.Hammer = { digs = "hard", speed = 2.5, level = 2, uses = 250, made = "Iron ingot" }

-- What a hammer breaks round the block it hits (ores and what creations
-- make are left alone).
local HAMMERED: { [string]: boolean } = {
	Stone = true, Cobblestone = true, Slate = true, Sandstone = true, ["Mossy brick"] = true,
}

for name, t in TOOLS do
	world.digging(name, { [t.digs] = math.min(t.speed * knobs.faster, 10), level = t.level })
end

-- What the anvil makes, for each of its metals: tools, and armor for the
-- Armor creation, and what each takes.
type Recipe = { tool: string, metal: number, planks: number }
local ANVIL_MAKES: { [string]: { Recipe } } = {
	tools = {
		{ tool = "pickaxe", metal = 3, planks = 2 },
		{ tool = "axe", metal = 3, planks = 2 },
		{ tool = "shovel", metal = 1, planks = 2 },
		{ tool = "hoe", metal = 2, planks = 2 },
		{ tool = "sword", metal = 2, planks = 1 },
	},
	armor = {
		{ tool = "helmet", metal = 5, planks = 0 },
		{ tool = "coat", metal = 8, planks = 0 },
		{ tool = "leggings", metal = 7, planks = 0 },
		{ tool = "boots", metal = 4, planks = 0 },
	},
}
local ANVIL_METALS = { "Iron", "Moonsilver" }
local ANVIL_KINDS = { "tools", "armor" }

local function uses(t: Tool): number
	return math.max(1, math.floor(t.uses * knobs.last))
end

-- how many blocks each player's tools have broken, by tool
local worn: { [number]: { [string]: number } } = {}

-- One more use of what's in a survival player's hand, if it's a tool:
-- it breaks when it's worn out.
local function wear(player)
	if not knobs.wear or world.pockets(player) == nil then
		return
	end
	local held = world.held(player)
	local t = held and TOOLS[held]
	if not held or not t then
		return
	end
	local mine = worn[player.id] or {}
	worn[player.id] = mine
	mine[held] = (mine[held] or 0) + 1
	if mine[held] >= uses(t) then
		mine[held] = 0
		if world.take(player, held, 1) then
			ui.show(player, "broke", "Your " .. string.lower(held) .. " broke.", { at = "bottom" })
			world.after(3, function()
				ui.hide(player, "broke")
			end)
			sound.play_for(player, "crack", { pitch = 1.3, caption = "A tool breaks" })
		end
	end
end

-- A hammer's blow: the eight blocks round the one it broke, across the
-- way the player looks at it, that HAMMERED says it breaks.
local function blow(player, at: vector)
	local d = at - (player.position + vector.create(0, 1.6, 0))
	local ax, ay, az = math.abs(d.x), math.abs(d.y), math.abs(d.z)
	for i = -1, 1 do
		for j = -1, 1 do
			if i ~= 0 or j ~= 0 then
				local off = if ax >= ay and ax >= az
					then vector.create(0, i, j)
					elseif ay >= az then vector.create(i, 0, j)
					else vector.create(i, j, 0)
				local q = at + off
				local b = world.block(q)
				if HAMMERED[b] then
					world.set_block(q, "Air")
					world.give(player, b, 1)
				end
			end
		end
	end
end

world.on("block_broken", function(player, at: vector, block: string)
	-- a hoe wears as it tills, not as it breaks things
	local held = world.held(player)
	if held and string.sub(held, -4) == " hoe" then
		return
	end
	if held == "Hammer" and knobs.hammer and HAMMERED[block] and world.pockets(player) ~= nil then
		blow(player, at)
	end
	wear(player)
end)

-- a hoe wears a little each time it tills (the farming creation tills
-- grass or dirt with air over it)
world.on("block_used", function(player, at: vector, block: string)
	local held = world.held(player)
	if held and string.sub(held, -4) == " hoe" and (block == "Grass" or block == "Dirt") and world.block(at + vector.create(0, 1, 0)) == "Air" then
		wear(player)
	end
end)

-- who's been told what the bar means
local told: { [number]: boolean } = {}

-- the wear bar, while a tool is in hand
world.every(0.5, function()
	for _, p in world.players() do
		local held = world.held(p)
		local t = held and TOOLS[held]
		if knobs.wear and held and t and world.pockets(p) ~= nil then
			local used = (worn[p.id] or {})[held] or 0
			-- in twentieths, so it isn't sent for every block
			local left = math.ceil((1 - used / uses(t)) * 20) / 20
			ui.meter(p, "wear", left, { color = knobs.color })
			if not told[p.id] then
				told[p.id] = true
				ui.show(p, "wear_says", "The new bar is your tool's wear. It breaks when empty.", { at = "left" })
				world.after(6, function()
					ui.hide(p, "wear_says")
				end)
			end
		else
			ui.meter(p, "wear", nil)
		end
	end
end)

-- The anvil.

-- "3 Iron ingot and 2 Planks"
local function needs(metal: string, r: Recipe): string
	return r.metal .. " " .. metal .. " ingot" .. (if r.planks > 0 then " and " .. r.planks .. " Planks" else "")
end

local function title(s: string): string
	return string.upper(string.sub(s, 1, 1)) .. string.sub(s, 2)
end

local anvil

-- The tools or armor of one metal: pick one to make.
local function make(player, at: vector, metal: string, kind: string, said: string?)
	local list = ANVIL_MAKES[kind]
	local choices = {}
	local lines = {}
	for _, r in list do
		table.insert(choices, title(r.tool))
		table.insert(lines, title(r.tool) .. ": " .. needs(metal, r))
	end
	table.insert(choices, "Back")
	local text = (if said then said .. "\n\n" else "") .. table.concat(lines, "\n")
	ui.ask(player, { title = "Anvil: " .. metal .. " " .. kind, text = text, choices = choices }, function(who, choice)
		local r = choice and list[choice]
		if choice == #list + 1 then
			anvil(who, at)
			return
		end
		if not r then
			return
		end
		local ingot = metal .. " ingot"
		if world.has(who, ingot) < r.metal or world.has(who, "Planks") < r.planks then
			make(who, at, metal, kind, "You'll need " .. needs(metal, r) .. " for that.")
			return
		end
		local tool = metal .. " " .. r.tool
		world.take(who, ingot, r.metal)
		if r.planks > 0 then
			world.take(who, "Planks", r.planks)
		end
		if world.give(who, tool, 1) == 0 then
			world.give(who, ingot, r.metal)
			if r.planks > 0 then
				world.give(who, "Planks", r.planks)
			end
			make(who, at, metal, kind, "Your pockets are full. Make some room first.")
			return
		end
		sound.play("clink", { at = at })
		-- "Made an iron pickaxe."
		local a = if string.find(string.lower(tool), "^[aeiou]") then "an " else "a "
		local made = if r.tool == "leggings" or r.tool == "boots" then "Made " .. string.lower(tool) .. "." else "Made " .. a .. string.lower(tool) .. "."
		make(who, at, metal, kind, made)
	end)
end

-- Mends what's in their hand, or says why not.
local function mend(player): string
	local held = world.held(player)
	local t = held and TOOLS[held]
	if not held or not t then
		return "Hold the tool you'd like mended."
	end
	local used = (worn[player.id] or {})[held] or 0
	if used == 0 then
		return "Your " .. string.lower(held) .. " is as good as new."
	end
	local n = math.floor(knobs.mend)
	if world.has(player, t.made) < n then
		return "Mending your " .. string.lower(held) .. " takes " .. n .. " " .. t.made .. "."
	end
	world.take(player, t.made, n)
	worn[player.id][held] = 0
	return "Mended your " .. string.lower(held) .. "."
end

function anvil(player, at: vector, said: string?)
	if world.pockets(player) == nil then
		ui.ask(player, {
			title = "Anvil",
			text = "In survival, an anvil makes iron and moonsilver tools and armor, and mends worn tools.",
			choices = { "OK" },
		}, function() end)
		return
	end
	local held = world.held(player)
	local t = held and TOOLS[held]
	local choices = {}
	local text = if said then said .. "\n\n" else ""
	if held and t then
		local left = uses(t) - ((worn[player.id] or {})[held] or 0)
		text ..= "Your " .. string.lower(held) .. " has " .. left .. " of " .. uses(t) .. " uses left. Mending it takes " .. math.floor(knobs.mend) .. " " .. t.made .. "."
		table.insert(choices, "Mend your " .. string.lower(held))
	else
		text ..= "Hold a worn tool to mend it, or make a new one."
	end
	local makes = {}
	for _, metal in ANVIL_METALS do
		for _, kind in ANVIL_KINDS do
			table.insert(choices, "Make " .. string.lower(metal) .. " " .. kind)
			table.insert(makes, { metal, kind })
		end
	end
	ui.ask(player, { title = "Anvil", text = text, choices = choices }, function(who, choice)
		if not choice then
			return
		end
		if held and t and choice == 1 then
			local result = mend(who)
			if string.sub(result, 1, 6) == "Mended" then
				sound.play("clink", { at = at })
			end
			anvil(who, at, result)
			return
		end
		local m = makes[choice - (if held and t then 1 else 0)]
		if m then
			make(who, at, m[1], m[2])
		end
	end)
end

world.on("block_used", function(player, at: vector, block: string)
	if block == ANVIL then
		anvil(player, at)
	end
end)

world.on("player_left", function(p)
	worn[p.id] = nil
	told[p.id] = nil
end)
