--!strict
-- Mossback: a slow, mossy friend from the paddock on the Green.
-- Walk up and press Interact to ride one. It doesn't love being ridden, so
-- now and then it bucks. Turn Buck force up to see how far you fly, or
-- Pat it while you ride and it calms down a little.

local world = require("@potluck/world")
local tweak = require("@potluck/tweak")
local input = require("@potluck/input")

-- Knobs show up in Tweak, so anyone can change them without code.
local knobs = tweak {
	speed = { 2.2, min = 0.5, max = 8, label = "Walking speed", order = 1 },
	buck = { 6, min = 0, max = 25, label = "Buck force", order = 2 },
	shell = { "#4f7a3a", label = "Shell colour", order = 3 },
}

-- A body is a list of boxes: a name, a size and where its middle sits
-- (x right, y up, z back: the head is at the front, at negative z).
local Mossback = world.creature {
	name = "Mossback",
	speed = knobs.speed,
	rideable = true,
	seat = 1.6,
	count = 3,
	home = "paddock",
	body = {
		{ "belly", size = { 1.2, 0.5, 1.5 }, at = { 0, 0.6, 0 }, color = "#8a7a52" },
		{ "shell", size = { 1.5, 0.6, 1.8 }, at = { 0, 1.05, 0.05 }, color = knobs.shell },
		{ "moss", size = { 1.1, 0.22, 1.3 }, at = { 0, 1.42, 0 }, color = "#6fa64a" },
		{ "tuft", size = { 0.4, 0.25, 0.4 }, at = { 0.25, 1.62, 0.2 }, color = "#86bb48" },
		{ "head", size = { 0.62, 0.55, 0.6 }, at = { 0, 0.95, -1.15 }, color = "#9c8a5c", head = true },
		{ "eye_left", size = { 0.12, 0.14, 0.06 }, at = { -0.17, 1.05, -1.46 }, color = "#1b1b1b", head = true },
		{ "eye_right", size = { 0.12, 0.14, 0.06 }, at = { 0.17, 1.05, -1.46 }, color = "#1b1b1b", head = true },
		{ "leg_front_left", size = { 0.34, 0.5, 0.34 }, at = { -0.5, 0.25, -0.55 }, color = "#7d6c47", leg = true },
		{ "leg_front_right", size = { 0.34, 0.5, 0.34 }, at = { 0.5, 0.25, -0.55 }, color = "#7d6c47", leg = true },
		{ "leg_back_right", size = { 0.34, 0.5, 0.34 }, at = { 0.5, 0.25, 0.6 }, color = "#7d6c47", leg = true },
		{ "leg_back_left", size = { 0.34, 0.5, 0.34 }, at = { -0.5, 0.25, 0.6 }, color = "#7d6c47", leg = true },
		{ "tail", size = { 0.2, 0.2, 0.3 }, at = { 0, 0.75, 1.05 }, color = "#7d6c47" },
	},
}

local GRUMBLES = { "Hrrmf!", "Mmmf.", "Off you get!", "Blrrp." }

-- Pat is an action in potluck.toml. While you ride a mossback its button
-- pats it instead of getting you off (B or Shift still do).
local patted: { [number]: boolean } = {}
input.on("pat", function(player)
	patted[player.id] = true
end)

function Mossback:spawned()
	self.patience = 5
	self.stroll = 0
end

function Mossback:tick(dt: number)
	if self.rider then
		-- a pat buys a little more patience
		if patted[self.rider.id] then
			patted[self.rider.id] = nil
			self.patience += 1.5
			self:say("Mmm.")
		end
		-- now and then a mossback has had enough
		self.patience -= dt
		if self.patience <= 0 then
			self.patience = world.random(2, 6)
			self:hop(0.7)
			if world.random() < 0.5 then
				self:throw_rider(knobs.buck)
				self:say(GRUMBLES[math.floor(world.random(1, #GRUMBLES + 1))])
			end
		end
		return
	end

	-- wander the paddock, and come over to say hello to anyone close by
	local friend = world.nearest_player(self.position, 6)
	if friend and self:distance_to(friend.position) > 2.5 then
		self:walk_to(friend.position, knobs.speed * 0.6)
	else
		self:wander()
	end

	-- mossbacks leave flowers where they've been
	self.stroll += dt
	if self.stroll > 25 then
		self.stroll = 0
		local here = self.position
		if world.block(here) == "Air" and world.block(here - vector.create(0, 1, 0)) == "Grass" then
			world.set_block(here, "Red flower")
		end
	end
end

-- Interact with one that's free and you climb on; this runs when you do.
function Mossback:ridden(player)
	self.patience = world.random(3, 7)
	self:say("Mmmf?")
end

-- Hit one and it complains.
function Mossback:hit(player)
	self:hop(1)
	self:say("Oi!")
end
