--!strict
-- Frog: hops about the creek bank. Sings when nobody's close, goes quiet
-- when someone is, and leaps if bopped. Turn up Hop to see them fly.

local world = require("@potluck/world")
local tweak = require("@potluck/tweak")
local sound = require("@potluck/sound@0.1")

local knobs = tweak {
	skin = { "#5a9e3a", label = "Skin colour", order = 1 },
	hop = { 0.9, min = 0.2, max = 3, label = "Hop", order = 2 },
}

local Frog = world.creature {
	name = "Frog",
	speed = 3,
	count = 4,
	home = "creek",
	body = {
		{ "body", size = { 0.44, 0.24, 0.48 }, at = { 0, 0.2, 0.02 }, color = knobs.skin },
		{ "belly", size = { 0.36, 0.06, 0.4 }, at = { 0, 0.07, 0 }, color = "#d9e8a2" },
		{ "head", size = { 0.4, 0.16, 0.24 }, at = { 0, 0.3, -0.24 }, color = knobs.skin, head = true },
		{ "eye_left", size = { 0.12, 0.12, 0.12 }, at = { -0.14, 0.42, -0.26 }, color = "#f2e14b", head = true },
		{ "eye_right", size = { 0.12, 0.12, 0.12 }, at = { 0.14, 0.42, -0.26 }, color = "#f2e14b", head = true },
		{ "leg_back_left", size = { 0.14, 0.12, 0.3 }, at = { -0.28, 0.07, 0.16 }, color = knobs.skin, leg = true },
		{ "leg_back_right", size = { 0.14, 0.12, 0.3 }, at = { 0.28, 0.07, 0.16 }, color = knobs.skin, leg = true },
	},
}

local SONGS = { "Ribbit.", "Rrribbit!", "Croak.", "Ribbit ribbit." }

function Frog:spawned()
	self.hop_in = world.random(1, 4)
	self.sing_in = world.random(5, 25)
end

function Frog:tick(dt: number)
	self.hop_in -= dt
	if self.hop_in <= 0 then
		self.hop_in = world.random(1.5, 5)
		local here = self.position
		self:walk_to(here + vector.create(world.random(-3, 3), 0, world.random(-3, 3)), 3)
		self:hop(knobs.hop)
	end
	self.sing_in -= dt
	if self.sing_in <= 0 then
		self.sing_in = world.random(10, 30)
		if not world.nearest_player(self.position, 4) then
			self:say(SONGS[math.floor(world.random(1, #SONGS + 1))])
			sound.play("croak", { at = self })
		end
	end
end

function Frog:hit(player)
	self:hop(knobs.hop * 2)
	self:say("Brrp!")
	sound.play("croak", { at = self, pitch = 1.3 })
end
