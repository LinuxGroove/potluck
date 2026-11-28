--!strict
-- Bramble Boars: bristly wild boars that root about in the woods by day,
-- near players on the surface, where there are trees. Come too close in
-- survival and one snorts, stamps, and charges; it hits once and goes back
-- to rooting for a while. Bop one and it charges whoever did. They're
-- tough: bop one enough (a sword or a shot bops harder) and it leaves
-- leather, bristles and raw meat in the pockets of whoever beat it. The
-- world's danger (world.danger, world 0.21) says how hard they hit and how
-- soon they charge again; on Peaceful they never do, and run off when hit.

local world = require("@potluck/world@0.22")
local tweak = require("@potluck/tweak")
local sound = require("@potluck/sound@0.2")

local knobs = tweak {
	how_many = { 2, min = 0, max = 6, label = "How many root about near you in the woods (0 for none)", order = 1 },
	charge = { 5, min = 0, max = 10, label = "How close you can come before one charges (0 never)", order = 2 },
	hits = { 3, min = 0, max = 10, label = "How hard a charge hits", order = 3 },
	bops = { 8, min = 2, max = 20, label = "Bops to beat one by hand", order = 4 },
	bristles = { "#4a3226", label = "Bristle colour", order = 5 },
}

-- How hard they hit and how long they cool down after, by the world's
-- danger (0 never charges).
local DANGER: { [string]: { hurt: number, rest: number } } = {
	peaceful = { hurt = 0, rest = 0 },
	gentle = { hurt = 0.5, rest = 14 },
	normal = { hurt = 1, rest = 10 },
	hard = { hurt = 1.5, rest = 6 },
}
local function danger()
	return DANGER[world.danger()] or DANGER.gentle
end

-- How hard each sword bops (a hand bops 1): a remix can add its own.
local SWORDS: { [string]: number } = {
	["Wooden sword"] = 2,
	["Stone sword"] = 3,
	["Copper sword"] = 3,
	["Iron sword"] = 4,
	["Moonsilver sword"] = 5,
}

-- What one leaves: each thing, from least to most.
local LEAVES = {
	{ thing = "Leather", least = 1, most = 2 },
	{ thing = "Bristles", least = 1, most = 2 },
	{ thing = "Raw meat", least = 1, most = 3 },
}

-- Trees: what makes the woods.
local TREES = { "Oak log", "Birch log" }

local SNOUT = "#8a6a58"
local TUSK = "#ece2c8"
local Boar = world.creature {
	name = "Bramble Boar",
	speed = 1.6,
	count = 0,
	home = "here",
	behaviours = {
		chatter = { "Hrnk.", "Snrrf.", "Hrm-hrm." },
		sound = "grunt",
	},
	body = {
		{ "body", size = { 0.62, 0.56, 1.04 }, at = { 0, 0.7, 0.06 }, color = knobs.bristles },
		{ "mane", size = { 0.2, 0.16, 0.8 }, at = { 0, 1.02, 0.0 }, color = "#2e1e16" },
		{ "tail", size = { 0.06, 0.18, 0.06 }, at = { 0, 0.82, 0.6 }, color = knobs.bristles },
		{ "head", size = { 0.46, 0.42, 0.42 }, at = { 0, 0.72, -0.62 }, color = knobs.bristles, head = true },
		{ "snout", size = { 0.24, 0.18, 0.16 }, at = { 0, 0.62, -0.88 }, color = SNOUT, head = true },
		{ "tusk_left", size = { 0.05, 0.14, 0.05 }, at = { -0.15, 0.66, -0.88 }, color = TUSK, head = true },
		{ "tusk_right", size = { 0.05, 0.14, 0.05 }, at = { 0.15, 0.66, -0.88 }, color = TUSK, head = true },
		{ "ear_left", size = { 0.1, 0.14, 0.06 }, at = { -0.18, 0.98, -0.58 }, color = "#2e1e16", head = true },
		{ "ear_right", size = { 0.1, 0.14, 0.06 }, at = { 0.18, 0.98, -0.58 }, color = "#2e1e16", head = true },
		{ "eye_left", size = { 0.05, 0.05, 0.03 }, at = { -0.15, 0.82, -0.84 }, color = "#140c08", head = true },
		{ "eye_right", size = { 0.05, 0.05, 0.03 }, at = { 0.15, 0.82, -0.84 }, color = "#140c08", head = true },
		{ "leg_front_left", size = { 0.14, 0.42, 0.14 }, at = { -0.2, 0.21, -0.32 }, color = "#2e1e16", leg = true },
		{ "leg_front_right", size = { 0.14, 0.42, 0.14 }, at = { 0.2, 0.21, -0.32 }, color = "#2e1e16", leg = true },
		{ "leg_back_left", size = { 0.14, 0.42, 0.14 }, at = { -0.2, 0.21, 0.4 }, color = "#2e1e16", leg = true },
		{ "leg_back_right", size = { 0.14, 0.42, 0.14 }, at = { 0.2, 0.21, 0.4 }, color = "#2e1e16", leg = true },
	},
}

local MOST = 24 -- at once, however many players are out
local NEAR, FAR = 14, 26 -- how far from a player they turn up
local LONELY = 56 -- they wander off with nobody this close
local STAMP = 0.8 -- seconds between the snort and the charge
local CHARGING = 3 -- seconds a charge lasts, at most
local GREEN = 34 -- never this near the Green

local UP = vector.create(0, 1, 0)

local where: { [number]: vector } = {}

local function flat(a: vector, b: vector): number
	local dx, dz = a.x - b.x, a.z - b.z
	return math.sqrt(dx * dx + dz * dz)
end

local function survivor(id: number?)
	for _, p in world.players() do
		if p.id == id and world.pockets(p) ~= nil then
			return p
		end
	end
	return nil
end

-- the nearest player in survival within `within`
local function nearest(pos: vector, within: number)
	local best, best_d = nil, within
	for _, p in world.players() do
		if world.pockets(p) ~= nil then
			local d = vector.magnitude(p.position - pos)
			if d < best_d then
				best, best_d = p, d
			end
		end
	end
	return best
end

local function gone(self)
	where[self.id] = nil
	self:remove()
end

function Boar:spawned()
	self.rest = world.random(2, 6)
	self.look_in = 0
end

function Boar:tick(dt: number)
	where[self.id] = self.position
	local d = danger()
	if knobs.how_many < 1 then
		gone(self)
		return
	end
	self.rest = math.max(0, (self.rest or 0) - dt)
	-- running off after a bop on Peaceful
	if (self.back_off or 0) > 0 then
		self.back_off -= dt
		return
	end
	-- winding up: snort, stamp, then go
	if self.stamp then
		self.stamp -= dt
		local p = survivor(self.target)
		if not p then
			self.stamp, self.target = nil, nil
		elseif self.stamp <= 0 then
			self.stamp = nil
			self.charging = CHARGING
		else
			self:stop()
			self:face(p.position)
		end
		return
	end
	if self.charging then
		self.charging -= dt
		local p = survivor(self.target)
		if not p or self.charging <= 0 then
			self.charging, self.target = nil, nil
			self.rest = d.rest
			self:wander(1.2)
			return
		end
		self:walk_to(p.position, 5.5)
		if self:distance_to(p.position) < 1.5 then
			self.charging, self.target = nil, nil
			self.rest = d.rest
			self:hop(0.4)
			if knobs.hits > 0 and world.hurt(p, knobs.hits * d.hurt) then
				sound.play("grunt", { at = self, volume = 0.9, pitch = 0.8, caption = "A boar hits you" })
			end
			-- and off it goes
			local away = self.position - p.position
			self:walk_to(self.position + vector.normalize(vector.create(away.x, 0, away.z) + vector.create(0.01, 0, 0)) * 6, 2.5)
		end
		return
	end
	self.look_in -= dt
	if self.look_in > 0 then
		return
	end
	self.look_in = world.random(0.4, 0.7)
	if not world.nearest_player(self.position, LONELY) then
		gone(self)
		return
	end
	if d.hurt > 0 and knobs.charge > 0 and self.rest <= 0 then
		local p = nearest(self.position, knobs.charge)
		if p and math.abs(p.position.y - self.position.y) < 3 then
			self.target = p.id
			self.stamp = STAMP
			sound.play("snort", { at = self, volume = 0.8 })
			self:say("SNRRF!")
			return
		end
	end
	self:wander(1.2)
end

function Boar:hit(player, shot)
	local held = world.held(player)
	-- a Keen sword hits harder (world.might); a shot's power says already
	local bop = if shot then shot.power else (held and SWORDS[held] or 1) * world.might(player)
	self.bopped = (self.bopped or 0) + bop
	if self.bopped >= knobs.bops then
		sound.play("squeal", { at = self, pitch = 0.7, caption = "A boar falls" })
		for _, l in LEAVES do
			local n = math.floor(world.random(l.least, l.most + 0.999))
			world.give(player, l.thing, n)
		end
		world.reward(player, 5)
		gone(self)
		return
	end
	self:hop(0.4)
	local d = danger()
	if d.hurt > 0 and world.pockets(player) ~= nil then
		-- it goes for whoever bopped it, straight away
		self.target = player.id
		self.stamp = STAMP * 0.5
	else
		local away = self.position - player.position
		self:walk_to(self.position + vector.normalize(vector.create(away.x, 0, away.z) + vector.create(0.01, 0, 0)) * 8, 4)
		self.back_off = 2
	end
end

-- somewhere in the woods: grass under it, room for it, and a tree near
local function in_woods(x: number, z: number): vector?
	local y = world.top(x, z)
	local ground = vector.create(math.floor(x) + 0.5, y, math.floor(z) + 0.5)
	if world.block(ground) ~= "Grass" or world.block(ground + UP) ~= "Air" or world.block(ground + UP * 2) ~= "Air" then
		return nil
	end
	for _, t in TREES do
		if world.find_block(ground, 7, t) then
			return ground + UP * 1.05
		end
	end
	return nil
end

-- Every few seconds by day: one more near each player on the surface,
-- in the woods, with fewer near them than the knob says.
world.every(4, function()
	local want = math.floor(knobs.how_many)
	if want < 1 or world.daylight() < 0.5 then
		return
	end
	local live = {}
	for _, id in world.creatures(Boar) do
		live[id] = true
	end
	local total = 0
	for id in where do
		if live[id] then
			total += 1
		else
			where[id] = nil
		end
	end
	local green = world.spot("green")
	for _, p in world.players() do
		local on_top = p.position.y > world.top(p.position.x, p.position.z) - 3
		local near = 0
		for _, at in where do
			if flat(at, p.position) < FAR + 8 then
				near += 1
			end
		end
		if on_top and total < MOST and near < want then
			for _ = 1, 3 do
				local a = world.random(0, 2 * math.pi)
				local r = world.random(NEAR, FAR)
				local at = in_woods(p.position.x + math.cos(a) * r, p.position.z + math.sin(a) * r)
				if at and flat(at, green) > GREEN and math.abs(at.y - p.position.y) < 12 then
					where[world.spawn(Boar, at)] = at
					total += 1
					break
				end
			end
		end
	end
end)
