--!strict
-- Rodeo Ram: a mini game. Climb on (Interact) and hold on as long as you
-- can: it bucks harder the longer you stay. Your longest ride goes on the
-- board, and a new record is announced to everyone. Hit it to hear the
-- best rides so far.

local world = require("@potluck/world")
local tweak = require("@potluck/tweak")
local game = require("@potluck/game@0.1")
local sound = require("@potluck/sound@0.1")

local knobs = tweak {
	fleece = { "#d8cfbf", label = "Fleece colour", order = 1 },
	horns = { "#8c7b62", label = "Horn colour", order = 2 },
	buck = { 9, min = 2, max = 25, label = "Buck force", order = 3 },
	temper = { 1, min = 0.3, max = 3, label = "Temper", order = 4 },
}

local Ram = world.creature {
	name = "Rodeo Ram",
	speed = 2.6,
	rideable = true,
	seat = 1.55,
	count = 1,
	home = "paddock",
	body = {
		{ "fleece", size = { 1.2, 0.9, 1.6 }, at = { 0, 1.0, 0.05 }, color = knobs.fleece },
		{ "saddle", size = { 0.8, 0.12, 0.6 }, at = { 0, 1.5, 0.1 }, color = "#7a2e22" },
		{ "head", size = { 0.5, 0.5, 0.56 }, at = { 0, 1.2, -0.98 }, color = "#4a4038", head = true },
		{ "horn_left", size = { 0.24, 0.3, 0.3 }, at = { -0.34, 1.42, -0.9 }, color = knobs.horns, head = true },
		{ "horn_right", size = { 0.24, 0.3, 0.3 }, at = { 0.34, 1.42, -0.9 }, color = knobs.horns, head = true },
		{ "eye_left", size = { 0.08, 0.08, 0.04 }, at = { -0.14, 1.28, -1.27 }, color = "#e8d36a", head = true },
		{ "eye_right", size = { 0.08, 0.08, 0.04 }, at = { 0.14, 1.28, -1.27 }, color = "#e8d36a", head = true },
		{ "leg_front_left", size = { 0.22, 0.6, 0.22 }, at = { -0.38, 0.3, -0.5 }, color = "#4a4038", leg = true },
		{ "leg_front_right", size = { 0.22, 0.6, 0.22 }, at = { 0.38, 0.3, -0.5 }, color = "#4a4038", leg = true },
		{ "leg_back_left", size = { 0.22, 0.6, 0.22 }, at = { -0.38, 0.3, 0.55 }, color = "#4a4038", leg = true },
		{ "leg_back_right", size = { 0.22, 0.6, 0.22 }, at = { 0.38, 0.3, 0.55 }, color = "#4a4038", leg = true },
	},
}

local SNORTS = { "Hrrnk!", "Get OFF!", "Baaah!", "Not today!" }

local function seconds(t: number): string
	return string.format("%.1f s", t)
end

function Ram:spawned()
	self.rode = nil
	self.started = 0
	self.buck_in = 2
end

-- the ride's over, however it ended: on the board it goes
local function finish(ram)
	local who = ram.rode
	ram.rode = nil
	if not who then
		return
	end
	local t = math.floor((world.time() - ram.started) * 10) / 10
	if t < 0.5 then
		return
	end
	local best = game.board("longest ride", 1)[1]
	game.score("longest ride", who.id, t)
	if not best or t > best.value then
		world.say(who.name .. " rode the Rodeo Ram for " .. seconds(t) .. ". A new record!")
	else
		ram:say(seconds(t) .. "! Best is " .. seconds(best.value) .. ".")
		sound.play("ding", { at = ram })
	end
end

function Ram:tick(dt: number)
	if self.rode and not self.rider then
		finish(self)
	end
	if not self.rider then
		self:wander()
		return
	end
	-- it bucks more often, and harder to stay on, the longer you ride
	local riding = world.time() - self.started
	self.buck_in -= dt * knobs.temper
	if self.buck_in <= 0 then
		self.buck_in = world.random(0.8, 2.2)
		self:hop(0.9)
		local chance = 0.15 + riding * 0.05
		if world.random() < chance then
			self:throw_rider(knobs.buck)
			self:say(SNORTS[math.floor(world.random(1, #SNORTS + 1))])
			sound.play("bleat", { at = self, pitch = 0.8 })
			finish(self)
		end
	end
end

function Ram:ridden(player)
	self.rode = { id = player.id, name = player.name }
	self.started = world.time()
	self.buck_in = 1.5
	self:say("Hold on!")
	sound.play("bleat", { at = self })
end

function Ram:hit(player)
	local top = game.board("longest ride", 3)
	if #top == 0 then
		self:say("Nobody's lasted yet. You?")
		return
	end
	local parts = {}
	for i, s in top do
		table.insert(parts, i .. ". " .. s.name .. " " .. seconds(s.value))
	end
	self:say(table.concat(parts, "  "))
end
