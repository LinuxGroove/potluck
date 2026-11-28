--!strict
-- Charms, for survival. Use a charm table (Interact while aiming at it)
-- with a tool or a weapon in your hand, and it puts a charm on it for XP
-- (world.spend_xp) and a little of what wild creatures and the deep give:
--   Swift: a tool digs faster (world.haste, while it's in your hand).
--   Sturdy: a tool wears half as fast (the Tools creation reads it).
--   Keen: a sword, bow, sling or spear hits harder (world.might, which
--   wild creatures and shots read).
-- Charms go on that kind of thing (world.charm), so every iron pickaxe
-- you carry is Swift once one is, and they're kept with the world. A tool
-- that breaks takes its charms with it, unless you've another.
-- CHARMS below says what each fits, costs and does, so a remix can add its
-- own. The table is in potluck.toml, and the workbench makes it.

local world = require("@potluck/world@0.22")
local tweak = require("@potluck/tweak")
local ui = require("@potluck/ui@0.2")
local sound = require("@potluck/sound@0.2")

local knobs = tweak {
	swift = { 1.5, min = 1, max = 4, label = "How much faster Swift digs", order = 1 },
	keen = { 1.5, min = 1, max = 4, label = "How much harder Keen hits", order = 2 },
	cost = { 1, min = 0, max = 3, label = "How much XP charms take", order = 3 },
}

local TABLE = "Charm table"

-- What digs and what hits, by name: a name that starts with a space is the
-- end of one ("Iron pickaxe"), any other the whole of it.
local DIGS = { " pickaxe", " axe", " shovel", " hoe", "Shears", "Sickle", "Hammer" }
local HITS = { " sword", "Bow", "Sling", "Spear" }

-- A charm: its name, what it does, what it fits, the XP it takes and what
-- else.
type Charm = { name: string, does: string, fits: { string }, xp: number, wants: { [string]: number } }
local CHARMS: { Charm } = {
	{ name = "Swift", does = "digs faster", fits = DIGS, xp = 40, wants = { ["Gloom dust"] = 2, Crystal = 1 } },
	{ name = "Sturdy", does = "wears half as fast", fits = DIGS, xp = 30, wants = { ["Gloom dust"] = 2, ["Iron ingot"] = 2 } },
	{ name = "Keen", does = "hits harder", fits = HITS, xp = 50, wants = { ["Gloom dust"] = 3, ["Gold ingot"] = 1 } },
}

local function fits(name: string, list: { string }): boolean
	for _, e in list do
		if name == e or (string.sub(e, 1, 1) == " " and string.sub(name, -#e) == e) then
			return true
		end
	end
	return false
end

local function cost(c: Charm): number
	return math.floor(c.xp * knobs.cost)
end

-- "2 Gloom dust and 1 Crystal"
local function needs(c: Charm): string
	local parts = {}
	for name, n in c.wants do
		table.insert(parts, n .. " " .. name)
	end
	table.sort(parts)
	return table.concat(parts, " and ")
end

-- "Swift", "Swift and Keen", "Swift, Sturdy and Keen"
local function list(names: { string }): string
	if #names == 1 then
		return names[1]
	end
	return table.concat(names, ", ", 1, #names - 1) .. " and " .. names[#names]
end

-- What a player's charms make them, kept so it's only said when it changes:
-- what's in their hand and its charms.
local was: { [number]: string } = {}

-- Digging and hitting by the charms on what's in a player's hand now.
local function apply(p)
	local held = world.held(p)
	local on: { string } = if held and world.pockets(p) ~= nil then world.charms(p, held) else {}
	local now = (held or "") .. ":" .. table.concat(on, ",")
	if was[p.id] == now then
		return
	end
	was[p.id] = now
	local swift = held ~= nil and table.find(on, "Swift") ~= nil and fits(held, DIGS)
	local keen = held ~= nil and table.find(on, "Keen") ~= nil and fits(held, HITS)
	world.haste(p, if swift then knobs.swift else 1)
	world.might(p, if keen then knobs.keen else 1)
	-- and a word of what it carries, as it comes into their hand
	if held and #on > 0 then
		ui.show(p, "charmed", held .. ": " .. list(on), { at = "bottom" })
		world.after(2.5, function()
			if was[p.id] == now then
				ui.hide(p, "charmed")
			end
		end)
	else
		ui.hide(p, "charmed")
	end
end

world.every(0.25, function()
	for _, p in world.players() do
		apply(p)
	end
end)

world.on("player_left", function(p)
	was[p.id] = nil
end)

local function close() end

-- The table's question: the charms that fit what's in their hand.
local function offer(player, at: vector, said: string?)
	if world.pockets(player) == nil then
		ui.ask(player, {
			title = TABLE,
			text = "In survival, a charm table puts charms on tools and weapons, for XP.",
			choices = { "OK" },
		}, close)
		return
	end
	local held = world.held(player)
	local xp, level = world.xp(player)
	local text = (if said then said .. "\n\n" else "") .. "You have " .. xp .. " XP, level " .. level .. "."
	local on: { string } = if held then world.charms(player, held) else {}
	local can: { Charm } = {}
	local any = false
	for _, c in CHARMS do
		if held and fits(held, c.fits) then
			any = true
			if not table.find(on, c.name) then
				table.insert(can, c)
			end
		end
	end
	if not held or not any then
		ui.ask(player, { title = TABLE, text = text .. " Hold a tool or a weapon to charm it.", choices = { "Close" } }, close)
		return
	end
	local name = string.lower(held)
	if #on > 0 then
		text ..= " Your " .. name .. " is " .. list(on) .. "."
	end
	if #can == 0 then
		ui.ask(player, { title = TABLE, text = text .. " It has every charm it can take.", choices = { "Close" } }, close)
		return
	end
	local choices = {}
	for _, c in can do
		text ..= "\n" .. c.name .. " " .. c.does .. ": " .. cost(c) .. " XP, " .. needs(c) .. "."
		table.insert(choices, c.name)
	end
	table.insert(choices, "Close")
	ui.ask(player, { title = TABLE .. ": " .. held, text = text, choices = choices }, function(who, choice)
		local c = choice and can[choice]
		if not c then
			return
		end
		-- they may have changed what's in hand since
		if world.held(who) ~= held then
			offer(who, at, "Hold your " .. name .. " to charm it.")
			return
		end
		for thing, n in c.wants do
			if world.has(who, thing) < n then
				offer(who, at, "You'll need " .. needs(c) .. " for that.")
				return
			end
		end
		if not world.spend_xp(who, cost(c)) then
			offer(who, at, "You'll need " .. cost(c) .. " XP for that.")
			return
		end
		for thing, n in c.wants do
			world.take(who, thing, n)
		end
		local now = world.charms(who, held)
		table.insert(now, c.name)
		world.charm(who, held, now)
		sound.play("sparkle", { at = at, caption = "A charm takes" })
		apply(who)
		offer(who, at, "Your " .. name .. " is " .. c.name .. " now.")
	end)
end

world.on("block_used", function(player, at: vector, block: string)
	if block == TABLE then
		offer(player, at)
	end
end)
