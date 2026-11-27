--!strict
-- Duck: a mother duck and her ducklings along the creek. The ducklings
-- follow her in a line. Two kinds of creature from one creation: they
-- share this file, so the ducklings can always see where she is.

local world = require("@potluck/world")
local tweak = require("@potluck/tweak")
local sound = require("@potluck/sound@0.1")

local knobs = tweak {
	ducklings = { 3, min = 0, max = 6, label = "Ducklings", order = 1 },
	feathers = { "#8a6a4a", label = "Mother's feathers", order = 2 },
	quack = { true, label = "Quacks", order = 3 },
}

local Duck = world.creature {
	name = "Duck",
	speed = 1.8,
	count = 1,
	home = "creek",
	body = {
		{ "body", size = { 0.5, 0.36, 0.7 }, at = { 0, 0.34, 0.04 }, color = knobs.feathers },
		{ "tail", size = { 0.26, 0.16, 0.16 }, at = { 0, 0.5, 0.42 }, color = knobs.feathers },
		{ "neck", size = { 0.18, 0.26, 0.16 }, at = { 0, 0.6, -0.26 }, color = knobs.feathers, head = true },
		{ "head", size = { 0.26, 0.24, 0.28 }, at = { 0, 0.82, -0.3 }, color = "#6b5038", head = true },
		{ "bill", size = { 0.16, 0.08, 0.18 }, at = { 0, 0.78, -0.5 }, color = "#e8a13a", head = true },
		{ "eye_left", size = { 0.04, 0.06, 0.06 }, at = { -0.13, 0.86, -0.36 }, color = "#1b1b1b", head = true },
		{ "eye_right", size = { 0.04, 0.06, 0.06 }, at = { 0.13, 0.86, -0.36 }, color = "#1b1b1b", head = true },
		{ "foot_left", size = { 0.12, 0.16, 0.14 }, at = { -0.12, 0.08, 0 }, color = "#e8a13a", leg = true },
		{ "foot_right", size = { 0.12, 0.16, 0.14 }, at = { 0.12, 0.08, 0 }, color = "#e8a13a", leg = true },
	},
}

local Duckling = world.creature {
	name = "Duckling",
	speed = 2.2,
	count = math.floor(knobs.ducklings),
	home = "creek",
	body = {
		{ "body", size = { 0.28, 0.22, 0.34 }, at = { 0, 0.2, 0 }, color = "#f2d14b" },
		{ "head", size = { 0.18, 0.18, 0.18 }, at = { 0, 0.4, -0.14 }, color = "#f2d14b", head = true },
		{ "bill", size = { 0.1, 0.05, 0.1 }, at = { 0, 0.38, -0.27 }, color = "#e8a13a", head = true },
		{ "foot_left", size = { 0.07, 0.1, 0.08 }, at = { -0.07, 0.05, 0 }, color = "#e8a13a", leg = true },
		{ "foot_right", size = { 0.07, 0.1, 0.08 }, at = { 0.07, 0.05, 0 }, color = "#e8a13a", leg = true },
	},
}

-- where she is and which way she went, for the ducklings to follow
local mum: vector? = nil
local trail: { vector } = {}

function Duck:spawned()
	self.quack_in = world.random(8, 20)
end

function Duck:tick(dt: number)
	self:wander()
	mum = self.position
	local last = trail[1]
	if not last or self:distance_to(last) > 0.8 then
		table.insert(trail, 1, self.position)
		if #trail > 12 then
			table.remove(trail)
		end
	end
	self.quack_in -= dt
	if self.quack_in <= 0 then
		self.quack_in = world.random(10, 30)
		if knobs.quack then
			self:say("Quack!")
			sound.play("quack", { at = self })
		end
	end
end

function Duck:hit(player)
	self:hop(0.6)
	self:say("QUACK!")
	sound.play("quack", { at = self, pitch = 1.15 })
end

function Duckling:spawned()
	-- each one knows its place in the line
	self.place = #world.creatures(Duckling)
end

function Duckling:tick(dt: number)
	if not mum then
		self:wander()
		return
	end
	local spot = trail[math.min(#trail, 2 + self.place * 2)] or mum
	if self:distance_to(spot) > 0.6 then
		self:walk_to(spot, 2.4)
	else
		self:stop()
	end
end

function Duckling:used(player)
	self:hop(0.4)
	self:say("Peep!")
	sound.play("chirp", { at = self, volume = 0.6 })
end
