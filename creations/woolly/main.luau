--!strict
-- Woolly: sheep that graze the Green and nibble the tall grass.
-- Hold wheat and they come to you; Interact with one and it follows you for
-- a while; hit one and it runs. All that is `behaviours`, with no code.
-- In survival, Interact shears it too: you get wool (more with shears in
-- hand, world.held), and it's a Shorn woolly (self:become) until its fleece
-- grows back.

local world = require("@potluck/world@0.10")
local tweak = require("@potluck/tweak")

local knobs = tweak {
	wool = { "#efe9dc", label = "Wool colour", order = 1 },
	follow = { 20, min = 5, max = 60, label = "Seconds it follows you", order = 2 },
	flock = { 4, min = 1, max = 8, label = "How many", order = 3 },
	shear = { 1, min = 1, max = 4, label = "Wool from shearing one", order = 4 },
	shears = { 2, min = 0, max = 4, label = "More wool with shears", order = 6 },
	regrow = { 3, min = 0.5, max = 20, label = "Minutes for its wool to grow back", order = 5 },
}

-- what a sheep does by itself, sheared or not
local SHEEP = {
	follow_food = "Wheat",
	follow_when_used = knobs.follow,
	flee_when_hit = 6,
	hop_when_hit = 0.8,
	chatter = { "Baa.", "Baa?", "Meh-eh-eh." },
	sound = "bleat",
}

local Woolly = world.creature {
	name = "Woolly",
	speed = 2,
	count = math.floor(knobs.flock),
	home = "green",
	behaviours = SHEEP,
	body = {
		{ "fleece", size = { 1.0, 0.8, 1.3 }, at = { 0, 0.85, 0.05 }, color = knobs.wool },
		{ "fleece_top", size = { 0.8, 0.2, 1.0 }, at = { 0, 1.32, 0.05 }, color = knobs.wool },
		{ "head", size = { 0.46, 0.5, 0.5 }, at = { 0, 1.05, -0.78 }, color = "#3b3430", head = true },
		{ "ear_left", size = { 0.2, 0.08, 0.14 }, at = { -0.3, 1.18, -0.72 }, color = "#3b3430", head = true },
		{ "ear_right", size = { 0.2, 0.08, 0.14 }, at = { 0.3, 1.18, -0.72 }, color = "#3b3430", head = true },
		{ "eye_left", size = { 0.08, 0.08, 0.04 }, at = { -0.13, 1.12, -1.04 }, color = "#f4f1e8", head = true },
		{ "eye_right", size = { 0.08, 0.08, 0.04 }, at = { 0.13, 1.12, -1.04 }, color = "#f4f1e8", head = true },
		{ "leg_front_left", size = { 0.18, 0.5, 0.18 }, at = { -0.3, 0.25, -0.4 }, color = "#3b3430", leg = true },
		{ "leg_front_right", size = { 0.18, 0.5, 0.18 }, at = { 0.3, 0.25, -0.4 }, color = "#3b3430", leg = true },
		{ "leg_back_left", size = { 0.18, 0.5, 0.18 }, at = { -0.3, 0.25, 0.45 }, color = "#3b3430", leg = true },
		{ "leg_back_right", size = { 0.18, 0.5, 0.18 }, at = { 0.3, 0.25, 0.45 }, color = "#3b3430", leg = true },
		{ "tail", size = { 0.24, 0.22, 0.16 }, at = { 0, 1.0, 0.75 }, color = knobs.wool },
	},
}

-- the same sheep with its fleece clipped short
local Shorn = world.creature {
	name = "Shorn woolly",
	speed = 2.2,
	count = 0,
	home = "green",
	behaviours = SHEEP,
	body = {
		{ "fleece", size = { 0.78, 0.58, 1.12 }, at = { 0, 0.8, 0.05 }, color = knobs.wool },
		{ "head", size = { 0.46, 0.5, 0.5 }, at = { 0, 1.0, -0.7 }, color = "#3b3430", head = true },
		{ "ear_left", size = { 0.2, 0.08, 0.14 }, at = { -0.3, 1.13, -0.64 }, color = "#3b3430", head = true },
		{ "ear_right", size = { 0.2, 0.08, 0.14 }, at = { 0.3, 1.13, -0.64 }, color = "#3b3430", head = true },
		{ "eye_left", size = { 0.08, 0.08, 0.04 }, at = { -0.13, 1.07, -0.96 }, color = "#f4f1e8", head = true },
		{ "eye_right", size = { 0.08, 0.08, 0.04 }, at = { 0.13, 1.07, -0.96 }, color = "#f4f1e8", head = true },
		{ "leg_front_left", size = { 0.16, 0.5, 0.16 }, at = { -0.26, 0.25, -0.36 }, color = "#3b3430", leg = true },
		{ "leg_front_right", size = { 0.16, 0.5, 0.16 }, at = { 0.26, 0.25, -0.36 }, color = "#3b3430", leg = true },
		{ "leg_back_left", size = { 0.16, 0.5, 0.16 }, at = { -0.26, 0.25, 0.4 }, color = "#3b3430", leg = true },
		{ "leg_back_right", size = { 0.16, 0.5, 0.16 }, at = { 0.26, 0.25, 0.4 }, color = "#3b3430", leg = true },
		{ "tail", size = { 0.16, 0.14, 0.12 }, at = { 0, 0.95, 0.64 }, color = knobs.wool },
	},
}

function Woolly:spawned()
	self.nibble = world.random(10, 40)
end

function Woolly:tick(dt: number)
	if self.regrow then
		self.regrow -= dt
		if self.regrow <= 0 then
			self.regrow = nil
			self:become(Woolly)
		end
	end
	-- now and then a mouthful of the tall grass underfoot
	self.nibble -= dt
	if self.nibble <= 0 then
		self.nibble = world.random(20, 50)
		local spot = world.find_block(self.position, 3, "Tall grass")
		if spot then
			world.set_block(spot, "Air")
			self:say("Nom.")
		end
	end
end

function Woolly:used(player)
	-- in survival, a fleecy one gives up its wool
	local wool = math.floor(knobs.shear) + (if world.held(player) == "Shears" then math.floor(knobs.shears) else 0)
	if not self.regrow and world.pockets(player) ~= nil and world.give(player, "Cream wool", wool) > 0 then
		self:become(Shorn)
		self.regrow = knobs.regrow * 60
		self:say("Baa! A bit chilly.")
	end
end

function Woolly:hit(player)
	self:say("Baa!")
end

-- a shorn one is the same sheep
Shorn.spawned = Woolly.spawned
Shorn.tick = Woolly.tick
Shorn.used = Woolly.used
Shorn.hit = Woolly.hit
