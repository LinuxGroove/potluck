--!strict
-- Tag Bot: a mini game. A little robot runs from anyone who comes close;
-- catch it and hit it to tag it. Each player's tags go on the board, and
-- the world keeps count (in the store) even after it restarts. A tag shows
-- your count on your screen for a while, and Interact with it shows its
-- board (both from @potluck/ui).

local world = require("@potluck/world@0.2")
local tweak = require("@potluck/tweak")
local game = require("@potluck/game@0.1")
local store = require("@potluck/store@0.1")
local ui = require("@potluck/ui@0.1")

local knobs = tweak {
	paint = { "#d9dde3", label = "Paint", order = 1 },
	light = { "#5ad1ff", label = "Light colour", order = 2 },
	speed = { 4.2, min = 1, max = 8, label = "Running speed", order = 3 },
	wary = { 7, min = 2, max = 16, label = "How close before it runs", order = 4 },
}

local Bot = world.creature {
	name = "Tag Bot",
	speed = 4.5,
	count = 1,
	home = "green",
	body = {
		{ "body", size = { 0.6, 0.6, 0.5 }, at = { 0, 0.6, 0 }, color = knobs.paint },
		{ "head", size = { 0.5, 0.36, 0.44 }, at = { 0, 1.1, 0 }, color = knobs.paint, head = true },
		{ "eyes", size = { 0.36, 0.1, 0.04 }, at = { 0, 1.12, -0.23 }, color = knobs.light, head = true, glow = true },
		{ "antenna", size = { 0.05, 0.24, 0.05 }, at = { 0, 1.4, 0 }, color = "#6b7078", head = true },
		{ "bulb", size = { 0.12, 0.12, 0.12 }, at = { 0, 1.56, 0 }, color = knobs.light, head = true, glow = true },
		{ "wheel_left", size = { 0.12, 0.3, 0.3 }, at = { -0.34, 0.15, 0 }, color = "#2b2f36", leg = true },
		{ "wheel_right", size = { 0.12, 0.3, 0.3 }, at = { 0.34, 0.15, 0 }, color = "#2b2f36", leg = true },
	},
}

-- tags this world has seen, by player name, kept with the world
local tags: { [string]: number } = store.get("tags") or {}

world.on("player_joined", function(player)
	local n = tags[player.name]
	if n then
		world.say("Tag Bot remembers " .. player.name .. ": " .. n .. " tags so far.")
	end
end)

-- your count on your screen, for a few seconds after each tag
local hiding: { [number]: number } = {}
local function show_tags(player, n: number)
	ui.show(player, "tags", "Tag Bot: " .. n .. (if n == 1 then " tag" else " tags"))
	if hiding[player.id] then
		world.cancel(hiding[player.id])
	end
	hiding[player.id] = world.after(8, function()
		ui.hide(player, "tags")
		hiding[player.id] = nil
	end)
end

function Bot:spawned()
	self.dizzy = 0
end

function Bot:tick(dt: number)
	if self.dizzy > 0 then
		self.dizzy -= dt
		self:stop()
		return
	end
	local p = world.nearest_player(self.position, knobs.wary)
	if p then
		-- away from whoever's closest, but never too far from the Green
		local away = self.position + (self.position - p.position) * 1.5
		local green = world.spot("green")
		if green and (away - green).x ^ 2 + (away - green).z ^ 2 > 400 then
			away = green + vector.create(world.random(-6, 6), 0, world.random(-6, 6))
		end
		self:walk_to(away, knobs.speed)
	else
		self:wander()
	end
end

function Bot:hit(player)
	if self.dizzy > 0 then
		return
	end
	local n = (tags[player.name] or 0) + 1
	tags[player.name] = n
	store.set("tags", tags)
	game.score("tags", player, n)
	show_tags(player, n)
	self.dizzy = 1.5
	self:hop(0.8)
	self:say("Tagged! That's " .. n .. " for " .. player.name .. ".")
end

function Bot:used(player)
	local top = game.board("tags", 5)
	if #top == 0 then
		self:say("Beep! Nobody's caught me yet.")
		return
	end
	local lines = {}
	for i, s in top do
		table.insert(lines, i .. ". " .. s.name .. ": " .. math.floor(s.value))
	end
	self:say("Beep! Here's my board.")
	ui.ask(player, { title = "Who's tagged Tag Bot most", text = table.concat(lines, "\n"), choices = { "OK" } }, function() end)
end
