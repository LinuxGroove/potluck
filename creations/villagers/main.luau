--!strict
-- Villagers: Rowan the carpenter and Ivy the mason, who live on the Green
-- and trade. Interact with one and pick a trade: in survival they take
-- what you've gathered from your hotbar and give you something made from
-- it (world.has, world.take and world.give). Every trade is in TRADES
-- below, so a remix can add its own or change the prices.

local world = require("@potluck/world@0.3")
local tweak = require("@potluck/tweak")
local ui = require("@potluck/ui@0.1")
local sound = require("@potluck/sound@0.1")

local knobs = tweak {
	rowan = { "#8a5a2b", label = "Rowan's apron", order = 1 },
	ivy = { "#5b6f8a", label = "Ivy's apron", order = 2 },
	generous = { 1, min = 1, max = 3, label = "Gives this many times as much", order = 3 },
}

-- A trade: what the villager wants for what they make. At most four each.
type Trade = { want: { [string]: number }, give: string, n: number }

local TRADES: { [string]: { Trade } } = {
	Carpenter = {
		{ want = { ["Oak log"] = 1 }, give = "Planks", n = 4 },
		{ want = { Planks = 4 }, give = "Crate", n = 1 },
		{ want = { Planks = 3 }, give = "Fence", n = 2 },
		{ want = { Planks = 6 }, give = "Table", n = 1 },
	},
	Mason = {
		{ want = { Sand = 3 }, give = "Glass", n = 2 },
		{ want = { Sandstone = 4 }, give = "Sandstone bricks", n = 4 },
		{ want = { Glass = 1, Stone = 2 }, give = "Lantern", n = 1 },
		{ want = { Cobblestone = 4 }, give = "Brazier", n = 1 },
	},
}

local GREETINGS = {
	Carpenter = { "Mind the sawdust.", "Fresh planks today!", "Bring me a log or two." },
	Mason = { "Sand into glass, stone into light.", "Lovely day for a lantern.", "Got any sand?" },
}

local function person(name: string, apron: string, hair: string)
	return world.creature {
		name = name,
		speed = 1.4,
		count = 1,
		home = "green",
		body = {
			{ "leg_left", size = { 0.2, 0.62, 0.22 }, at = { -0.12, 0.31, 0 }, color = "#4a4a5a", leg = true },
			{ "leg_right", size = { 0.2, 0.62, 0.22 }, at = { 0.12, 0.31, 0 }, color = "#4a4a5a", leg = true },
			{ "body", size = { 0.5, 0.66, 0.3 }, at = { 0, 0.95, 0 }, color = "#e9e2cf" },
			{ "apron", size = { 0.42, 0.56, 0.04 }, at = { 0, 0.9, -0.17 }, color = apron },
			{ "arm_left", size = { 0.14, 0.58, 0.16 }, at = { -0.33, 0.98, 0 }, color = "#e9e2cf", leg = true },
			{ "arm_right", size = { 0.14, 0.58, 0.16 }, at = { 0.33, 0.98, 0 }, color = "#e9e2cf", leg = true },
			{ "head", size = { 0.42, 0.42, 0.4 }, at = { 0, 1.5, 0 }, color = "#e3b48f", head = true },
			{ "hair", size = { 0.46, 0.14, 0.44 }, at = { 0, 1.75, 0.02 }, color = hair, head = true },
			{ "fringe", size = { 0.46, 0.08, 0.06 }, at = { 0, 1.66, -0.2 }, color = hair, head = true },
			{ "eye_left", size = { 0.06, 0.06, 0.02 }, at = { -0.09, 1.53, -0.21 }, color = "#2b2b33", head = true },
			{ "eye_right", size = { 0.06, 0.06, 0.02 }, at = { 0.09, 1.53, -0.21 }, color = "#2b2b33", head = true },
		},
	}
end

local Carpenter = person("Carpenter", knobs.rowan, "#6b4226")
local Mason = person("Mason", knobs.ivy, "#2f2f3a")
local NAMES = { Carpenter = "Rowan", Mason = "Ivy" }

-- how many a trade gives, with the Tweak knob
local function makes(t: Trade): number
	return math.floor(t.n * knobs.generous)
end

-- "4 Planks for 1 Oak log"
local function label(t: Trade): string
	local parts = {}
	for block, n in t.want do
		table.insert(parts, n .. " " .. block)
	end
	table.sort(parts)
	return makes(t) .. " " .. t.give .. " for " .. table.concat(parts, " and ")
end

local function trade(villager, player, t: Trade)
	for block, n in t.want do
		if world.has(player, block) < n then
			villager:say("You'll need " .. n .. " " .. block .. " for that.")
			return
		end
	end
	for block, n in t.want do
		world.take(player, block, n)
	end
	local given = world.give(player, t.give, makes(t))
	if given == 0 then
		-- no room: give it all back
		for block, n in t.want do
			world.give(player, block, n)
		end
		villager:say("Your hands are full! Make some room.")
		return
	end
	villager:say("There you go: " .. given .. " " .. t.give .. ".")
	sound.play("pop", { at = villager })
end

local function offer(villager, player)
	local kind = villager.kind
	local name = NAMES[kind]
	if world.pockets(player) == nil then
		ui.ask(player, {
			title = name .. " the " .. string.lower(kind),
			text = "In creative you've every block already! Turn on survival in Settings, gather what you can, and come and trade.",
			choices = { "I will" },
		}, function() end)
		return
	end
	local trades = TRADES[kind]
	local choices = {}
	for _, t in trades do
		table.insert(choices, label(t))
	end
	ui.ask(player, { title = name .. " the " .. string.lower(kind), text = "What can I make you?", choices = choices }, function(who, choice)
		local t = choice and trades[choice]
		if t then
			trade(villager, who, t)
		else
			villager:say("Come back any time.")
		end
	end)
end

local function spawned(self)
	self.chat = world.random(4, 12)
end

local function tick(self, dt: number)
	local p = world.nearest_player(self.position, 4)
	if p then
		self:stop()
		self:face(p.position)
		self.chat -= dt
		if self.chat <= 0 then
			self.chat = world.random(20, 40)
			local lines = GREETINGS[self.kind]
			self:say(lines[math.floor(world.random(1, #lines + 1))])
		end
	else
		self:wander(1.2)
	end
end

local function used(self, player)
	self:face(player.position)
	sound.play("grumble", { at = self, pitch = 1.6, volume = 0.5, caption = NAMES[self.kind] .. " hums" })
	offer(self, player)
end

local function hit(self)
	self:hop(0.4)
	self:say("Oi!")
end

for _, kind in { Carpenter, Mason } do
	kind.spawned = spawned
	kind.tick = tick
	kind.used = used
	kind.hit = hit
end
