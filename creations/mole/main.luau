--!strict
-- Mole: bumbles about south of the Green leaving molehills, a few at a
-- time: when it has made enough, it flattens its oldest one again.

local world = require("@potluck/world")
local tweak = require("@potluck/tweak")

local knobs = tweak {
	hills = { 4, min = 0, max = 10, label = "Molehills each", order = 1 },
	fur = { "#4b4038", label = "Fur colour", order = 2 },
}

local Mole = world.creature {
	name = "Mole",
	speed = 1.5,
	count = 2,
	home = "start",
	body = {
		{ "body", size = { 0.5, 0.34, 0.66 }, at = { 0, 0.22, 0.04 }, color = knobs.fur },
		{ "head", size = { 0.32, 0.26, 0.26 }, at = { 0, 0.24, -0.38 }, color = knobs.fur, head = true },
		{ "nose", size = { 0.12, 0.1, 0.1 }, at = { 0, 0.24, -0.55 }, color = "#e7a0a8", head = true },
		{ "paw_left", size = { 0.18, 0.08, 0.2 }, at = { -0.3, 0.06, -0.24 }, color = "#e7a0a8", leg = true },
		{ "paw_right", size = { 0.18, 0.08, 0.2 }, at = { 0.3, 0.06, -0.24 }, color = "#e7a0a8", leg = true },
	},
}

function Mole:spawned()
	self.dig_in = world.random(20, 45)
	self.hills = {}
end

function Mole:tick(dt: number)
	self:wander()
	self.dig_in -= dt
	if self.dig_in > 0 or knobs.hills < 1 then
		return
	end
	self.dig_in = world.random(30, 70)
	local p = self.position
	local x, z = math.floor(p.x), math.floor(p.z)
	local y = world.top(x, z)
	local above = vector.create(x, y + 1, z)
	if world.block(vector.create(x, y, z)) ~= "Grass" or world.block(above) ~= "Air" then
		return
	end
	-- one hill in each patch
	if world.find_block(above, 4, "Dirt") then
		return
	end
	world.set_block(above, "Dirt")
	table.insert(self.hills, { x, y + 1, z })
	self:say("*sniff*")
	-- flatten the oldest once there are enough
	while #self.hills > knobs.hills do
		local old = table.remove(self.hills, 1)
		local at = vector.create(old[1], old[2], old[3])
		if world.block(at) == "Dirt" then
			world.set_block(at, "Air")
		end
	end
end

function Mole:hit(player)
	self:hop(0.5)
	self:say("Eep!")
end
