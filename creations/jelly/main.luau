--!strict
-- Jelly: wobbly jellies that bounce round the Green. Bop one and it
-- splits into two jellylets, who bounce about for a bit and then pop.

local world = require("@potluck/world")
local tweak = require("@potluck/tweak")
local sound = require("@potluck/sound@0.1")

local knobs = tweak {
	colour = { "#6fd6b8", label = "Jelly colour", order = 1 },
	bounce = { 1.1, min = 0.3, max = 3, label = "Bounce", order = 2 },
	last = { 40, min = 5, max = 120, label = "Seconds a jellylet lasts", order = 3 },
}

local Jelly = world.creature {
	name = "Jelly",
	speed = 1.8,
	count = 2,
	home = "green",
	body = {
		{ "jelly", size = { 0.9, 0.7, 0.9 }, at = { 0, 0.35, 0 }, color = knobs.colour },
		{ "top", size = { 0.6, 0.16, 0.6 }, at = { 0, 0.78, 0 }, color = knobs.colour },
		{ "eye_left", size = { 0.12, 0.16, 0.04 }, at = { -0.18, 0.5, -0.46 }, color = "#1b1b1b", head = true },
		{ "eye_right", size = { 0.12, 0.16, 0.04 }, at = { 0.18, 0.5, -0.46 }, color = "#1b1b1b", head = true },
	},
}

local Jellylet = world.creature {
	name = "Jellylet",
	speed = 2.4,
	count = 0,
	home = "green",
	body = {
		{ "jelly", size = { 0.44, 0.36, 0.44 }, at = { 0, 0.18, 0 }, color = knobs.colour },
		{ "eye_left", size = { 0.07, 0.09, 0.03 }, at = { -0.09, 0.25, -0.23 }, color = "#1b1b1b", head = true },
		{ "eye_right", size = { 0.07, 0.09, 0.03 }, at = { 0.09, 0.25, -0.23 }, color = "#1b1b1b", head = true },
	},
}

local MOST_LETS = 12

local function bounce(j, power: number, dt: number)
	j.bounce_in -= dt
	if j.bounce_in <= 0 then
		j.bounce_in = world.random(0.9, 1.8)
		j:hop(power)
	end
end

function Jelly:spawned()
	self.bounce_in = world.random(0, 1)
end

function Jelly:tick(dt: number)
	self:wander()
	bounce(self, knobs.bounce, dt)
end

function Jelly:hit(player)
	if #world.creatures(Jellylet) + 2 > MOST_LETS then
		self:hop(knobs.bounce * 1.5)
		self:say("Blorp!")
		sound.play("boing", { at = self })
		return
	end
	local here = self.position
	world.spawn(Jellylet, here + vector.create(-0.6, 0.5, 0))
	world.spawn(Jellylet, here + vector.create(0.6, 0.5, 0))
	self:remove()
end

function Jellylet:spawned()
	self.bounce_in = 0
	self.left = knobs.last
end

function Jellylet:tick(dt: number)
	self:wander(2.4)
	bounce(self, knobs.bounce * 0.8, dt)
	self.left -= dt
	if self.left <= 0 then
		self:remove()
	end
end

function Jellylet:hit(player)
	self:say("Pop!")
	sound.play("pop", { at = self })
	self:remove()
end
