--!strict
-- Gnome Gardener: keeps the Green in flowers, a few at a time, and tells
-- anyone who asks how things work round here.

local world = require("@potluck/world")
local tweak = require("@potluck/tweak")

local knobs = tweak {
	hat = { "#c0392b", label = "Hat colour", order = 1 },
	coat = { "#2f6f8f", label = "Coat colour", order = 2 },
	plant = { true, label = "Plants flowers", order = 3 },
}

local Gnome = world.creature {
	name = "Gnome Gardener",
	speed = 1.6,
	count = 2,
	home = "green",
	body = {
		{ "boots", size = { 0.44, 0.16, 0.34 }, at = { 0, 0.08, -0.02 }, color = "#4a3424" },
		{ "coat", size = { 0.5, 0.5, 0.4 }, at = { 0, 0.41, 0 }, color = knobs.coat },
		{ "belt", size = { 0.52, 0.06, 0.42 }, at = { 0, 0.42, 0 }, color = "#2b1f16" },
		{ "head", size = { 0.38, 0.34, 0.34 }, at = { 0, 0.83, -0.02 }, color = "#f0c9a6", head = true },
		{ "beard", size = { 0.34, 0.28, 0.1 }, at = { 0, 0.68, -0.2 }, color = "#f4f1e8", head = true },
		{ "nose", size = { 0.1, 0.1, 0.08 }, at = { 0, 0.82, -0.22 }, color = "#e8a888", head = true },
		{ "hat", size = { 0.42, 0.18, 0.4 }, at = { 0, 1.07, 0 }, color = knobs.hat, head = true },
		{ "hat_tip", size = { 0.24, 0.22, 0.24 }, at = { 0, 1.26, 0.04 }, color = knobs.hat, head = true },
		{ "arm_left", size = { 0.12, 0.36, 0.14 }, at = { -0.31, 0.42, 0 }, color = knobs.coat, leg = true },
		{ "arm_right", size = { 0.12, 0.36, 0.14 }, at = { 0.31, 0.42, 0 }, color = knobs.coat, leg = true },
	},
}

local FLOWERS = { "Red flower", "Blue flower", "White flower", "Violet flower" }

local TIPS = {
	"Everything here is a creation someone made. Even me!",
	"Look inside anything: press L, or hold Y on a pad.",
	"Tweak changes a creation's knobs. No code needed.",
	"Remix a creation and it's yours to change.",
	"Share a creation and you get a code your friends can type.",
	"The market lists servers you can join, all over.",
	"Ride a mossback if you dare. They buck!",
	"Try the Rodeo Ram by the paddock. Hold on as long as you can.",
}

function Gnome:spawned()
	self.chore = world.random(5, 20)
	self.tip = math.floor(world.random(1, #TIPS + 1))
end

-- a flower on bare grass near the middle of the Green, if there's room
local function plant_near(centre: vector): vector?
	for _ = 1, 4 do
		local x = math.floor(centre.x + world.random(-10, 10))
		local z = math.floor(centre.z + world.random(-10, 10))
		local y = world.top(x, z)
		local ground = vector.create(x, y, z)
		local above = vector.create(x, y + 1, z)
		if world.block(ground) == "Grass" and world.block(above) == "Air" then
			-- keep it a garden, not a meadow: one flower in each patch
			local crowded = false
			for _, f in FLOWERS do
				if world.find_block(above, 3, f) then
					crowded = true
					break
				end
			end
			if not crowded then
				world.set_block(above, FLOWERS[math.floor(world.random(1, #FLOWERS + 1))])
				return above
			end
		end
	end
	return nil
end

function Gnome:tick(dt: number)
	self.chore -= dt
	if self.chore <= 0 then
		self.chore = world.random(30, 60)
		if knobs.plant then
			local spot = plant_near(world.spot("green") or self.position)
			if spot then
				self:walk_to(spot, 1.6)
				self:say("There we are.")
				return
			end
		end
	end
	local p = world.nearest_player(self.position, 5)
	if p then
		self:stop()
		self:face(p.position)
	else
		self:wander()
	end
end

function Gnome:used(player)
	self:face(player.position)
	self:say(TIPS[self.tip])
	self.tip = self.tip % #TIPS + 1
end

function Gnome:hit(player)
	self:hop(0.6)
	self:say("Mind the petunias!")
end
