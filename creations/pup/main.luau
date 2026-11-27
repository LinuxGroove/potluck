--!strict
-- Pup: friendly pups on the hill where everyone starts. They trot after
-- whoever's close. Interact and they run off to fetch, then come back.

local world = require("@potluck/world")
local tweak = require("@potluck/tweak")

local knobs = tweak {
	coat = { "#c58b4e", label = "Coat colour", order = 1 },
	patch = { "#f4ede2", label = "Patch colour", order = 2 },
	fetch = { 10, min = 3, max = 24, label = "How far it fetches", order = 3 },
}

local Pup = world.creature {
	name = "Pup",
	speed = 3.4,
	count = 2,
	home = "start",
	body = {
		{ "body", size = { 0.46, 0.44, 0.86 }, at = { 0, 0.56, 0.04 }, color = knobs.coat },
		{ "patch", size = { 0.48, 0.2, 0.3 }, at = { 0, 0.5, -0.22 }, color = knobs.patch },
		{ "head", size = { 0.4, 0.38, 0.4 }, at = { 0, 0.88, -0.5 }, color = knobs.coat, head = true },
		{ "snout", size = { 0.2, 0.16, 0.18 }, at = { 0, 0.8, -0.76 }, color = knobs.patch, head = true },
		{ "nose", size = { 0.08, 0.06, 0.04 }, at = { 0, 0.86, -0.86 }, color = "#1b1b1b", head = true },
		{ "ear_left", size = { 0.1, 0.2, 0.12 }, at = { -0.17, 1.1, -0.44 }, color = "#8a5a32", head = true },
		{ "ear_right", size = { 0.1, 0.2, 0.12 }, at = { 0.17, 1.1, -0.44 }, color = "#8a5a32", head = true },
		{ "eye_left", size = { 0.06, 0.06, 0.04 }, at = { -0.1, 0.94, -0.71 }, color = "#1b1b1b", head = true },
		{ "eye_right", size = { 0.06, 0.06, 0.04 }, at = { 0.1, 0.94, -0.71 }, color = "#1b1b1b", head = true },
		{ "leg_front_left", size = { 0.12, 0.36, 0.12 }, at = { -0.14, 0.18, -0.28 }, color = knobs.coat, leg = true },
		{ "leg_front_right", size = { 0.12, 0.36, 0.12 }, at = { 0.14, 0.18, -0.28 }, color = knobs.coat, leg = true },
		{ "leg_back_left", size = { 0.12, 0.36, 0.12 }, at = { -0.14, 0.18, 0.34 }, color = knobs.coat, leg = true },
		{ "leg_back_right", size = { 0.12, 0.36, 0.12 }, at = { 0.14, 0.18, 0.34 }, color = knobs.coat, leg = true },
		{ "tail", size = { 0.08, 0.08, 0.26 }, at = { 0, 0.78, 0.56 }, color = knobs.coat },
	},
}

function Pup:spawned()
	self.fetching = nil
	self.back_to = nil
	self.sit = 0
end

function Pup:tick(dt: number)
	if self.sit > 0 then
		self.sit -= dt
		self:stop()
		return
	end
	if self.fetching then
		if self:distance_to(self.fetching) < 1.2 then
			self.fetching = nil
			self:say("Woof!")
		else
			return
		end
	end
	if self.back_to then
		local p = self.back_to
		if self:distance_to(p) < 2 then
			self.back_to = nil
			self:hop(0.5)
		else
			self:walk_to(p, 4)
		end
		return
	end
	local friend = world.nearest_player(self.position, 10)
	if friend and self:distance_to(friend.position) > 3 then
		self:walk_to(friend.position, 3.4)
	elseif friend then
		self:stop()
		self:face(friend.position)
	else
		self:wander()
	end
end

function Pup:used(player)
	-- off it goes, the way you're facing it, then back to you
	local away = self.position - player.position
	local len = math.max(0.1, math.sqrt(away.x * away.x + away.z * away.z))
	local dir = vector.create(away.x / len, 0, away.z / len)
	self.fetching = self.position + dir * knobs.fetch
	self.back_to = player.position
	self:walk_to(self.fetching, 5)
	self:say("Arf!")
end

function Pup:hit(player)
	self.sit = 3
	self.fetching = nil
	self:say("Whimper.")
end
