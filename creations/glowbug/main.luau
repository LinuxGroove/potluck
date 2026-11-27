--!strict
-- Glowbug: soft little lights that bob about the Green. They drift over
-- to anyone nearby, and scatter when bopped. Change the glow in Tweak.

local world = require("@potluck/world")
local tweak = require("@potluck/tweak")
local sound = require("@potluck/sound@0.1")

local knobs = tweak {
	glow = { "#ffd76a", label = "Glow colour", order = 1 },
	bob = { 0.5, min = 0, max = 1.5, label = "Bob height", order = 2 },
	swarm = { 8, min = 1, max = 16, label = "How many", order = 3 },
}

local Glowbug = world.creature {
	name = "Glowbug",
	speed = 1.4,
	count = math.floor(knobs.swarm),
	home = "green",
	body = {
		{ "glow", size = { 0.26, 0.22, 0.32 }, at = { 0, 0.3, 0.04 }, color = knobs.glow, glow = true },
		{ "head", size = { 0.16, 0.14, 0.14 }, at = { 0, 0.32, -0.18 }, color = "#2d2a24", head = true },
		{ "wing_left", size = { 0.24, 0.03, 0.16 }, at = { -0.16, 0.44, 0 }, color = "#e9f2ff", leg = true },
		{ "wing_right", size = { 0.24, 0.03, 0.16 }, at = { 0.16, 0.44, 0 }, color = "#e9f2ff", leg = true },
	},
}

function Glowbug:spawned()
	self.bob_in = world.random(0, 2)
end

function Glowbug:tick(dt: number)
	-- a little hop now and then keeps it bobbing in the air
	self.bob_in -= dt
	if self.bob_in <= 0 then
		self.bob_in = world.random(0.8, 2.2)
		if knobs.bob > 0 then
			self:hop(knobs.bob)
		end
	end
	local p = world.nearest_player(self.position, 8)
	if p and self:distance_to(p.position) > 2 then
		self:walk_to(p.position + vector.create(world.random(-1, 1), 0, world.random(-1, 1)), 1.2)
	else
		self:wander(1)
	end
end

function Glowbug:hit(player)
	local away = self.position + (self.position - player.position) * 3
	self:walk_to(away, 4)
	self:hop(1.2)
	sound.play("sparkle", { at = self, volume = 0.7 })
end
