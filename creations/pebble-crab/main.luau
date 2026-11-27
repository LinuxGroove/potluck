--!strict
-- Pebble Crab: little crabs from the creek north-east of the Green.
-- Walk up too fast and they scuttle off sideways. Interact and they dance.

local world = require("@potluck/world")
local tweak = require("@potluck/tweak")
local sound = require("@potluck/sound@0.1")

local knobs = tweak {
	shell = { "#c8553d", label = "Shell colour", order = 1 },
	shy = { 3, min = 0, max = 8, label = "How close is too close", order = 2 },
}

local Crab = world.creature {
	name = "Pebble Crab",
	speed = 2.6,
	count = 5,
	home = "creek",
	body = {
		{ "shell", size = { 0.7, 0.26, 0.5 }, at = { 0, 0.3, 0 }, color = knobs.shell },
		{ "shell_top", size = { 0.5, 0.1, 0.36 }, at = { 0, 0.47, 0 }, color = knobs.shell },
		{ "claw_left", size = { 0.2, 0.16, 0.24 }, at = { -0.42, 0.32, -0.28 }, color = knobs.shell, head = true },
		{ "claw_right", size = { 0.2, 0.16, 0.24 }, at = { 0.42, 0.32, -0.28 }, color = knobs.shell, head = true },
		{ "eye_left", size = { 0.06, 0.14, 0.06 }, at = { -0.1, 0.56, -0.2 }, color = "#1b1b1b", head = true },
		{ "eye_right", size = { 0.06, 0.14, 0.06 }, at = { 0.1, 0.56, -0.2 }, color = "#1b1b1b", head = true },
		{ "legs_left", size = { 0.12, 0.2, 0.4 }, at = { -0.38, 0.1, 0.05 }, color = "#a8432f", leg = true },
		{ "legs_right", size = { 0.12, 0.2, 0.4 }, at = { 0.38, 0.1, 0.05 }, color = "#a8432f", leg = true },
	},
}

function Crab:spawned()
	self.dance = 0
	self.calm = 0
end

function Crab:tick(dt: number)
	if self.dance > 0 then
		self.dance -= dt
		if math.floor(self.dance * 3) % 2 == 0 then
			self:hop(0.35)
		end
		return
	end
	self.calm -= dt
	local p = world.nearest_player(self.position, knobs.shy)
	if p and self.calm <= 0 then
		-- crabs go sideways: across the line to whoever came close
		local d = self.position - p.position
		local side = vector.create(-d.z, 0, d.x)
		self:walk_to(self.position + side * 1.5 + d * 0.5, 3.6)
		self:say("Click!")
		sound.play("click", { at = self })
		self.calm = 2
		return
	end
	if self.calm <= 0 then
		self:wander()
	end
end

function Crab:used(player)
	self.dance = 2.5
	self:face(player.position)
	self:say("Click click!")
	sound.play("click", { at = self, pitch = 1.2 })
end

function Crab:hit(player)
	self:hop(0.9)
	self:say("Snip!")
	sound.play("click", { at = self, pitch = 0.7 })
	self.calm = 0
end
