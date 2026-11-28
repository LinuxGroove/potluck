--!strict
-- Cave Crawlers: many-legged things in the dark of caves. Underground (well
-- below the ground over them) and away from any light, they come at anyone
-- in survival, climb after them (a hop up whatever's in the way) and nip.
-- A torch, lantern or candle keeps them off (world.find_light). Bop one
-- three times to pop it, harder with a sword or a shot, and it leaves silk
-- in the pockets of whoever popped it. How many come, and how hard they
-- nip, go by the world's danger (world.danger, world 0.21): none on
-- Peaceful, half on Gentle, more on Hard. Set how many to 0 for none at all.

local world = require("@potluck/world@0.25")
local tweak = require("@potluck/tweak")
local sound = require("@potluck/sound@0.2")

local knobs = tweak {
	how_many = { 3, min = 0, max = 8, label = "How many come at you in the dark (0 for none)", order = 1 },
	bite = { 1.5, min = 0, max = 6, label = "How hard they nip", order = 2 },
	bops = { 3, min = 1, max = 8, label = "Bops to pop one by hand", order = 3 },
	shy = { 5, min = 2, max = 12, label = "How far they keep from a light", order = 4 },
	colour = { "#3a3430", label = "Crawler colour", order = 5 },
	eyes = { "#c8f070", label = "Eye colour", order = 6 },
}

-- How many come and how hard they nip, by the world's danger.
local DANGER: { [string]: { many: number, hurt: number } } = {
	peaceful = { many = 0, hurt = 0 },
	gentle = { many = 0.5, hurt = 0.5 },
	normal = { many = 1, hurt = 1 },
	hard = { many = 1.5, hurt = 1.5 },
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

-- What's in hand bops this hard: a sword, a weapon another creation made
-- ([items.*] with kind = "weapon", world.item) by its power, or a hand.
local function bops(held: string?): number
	if not held then
		return 1
	end
	if SWORDS[held] then
		return SWORDS[held]
	end
	local it = world.item(held)
	return if it and it.kind == "weapon" then it.power or 1 else 1
end

-- What one leaves, from least to most.
local SILK = { thing = "Silk", least = 1, most = 2 }

local LEG = "#2a2420"
local Crawler = world.creature {
	name = "Cave Crawler",
	speed = 2.8,
	count = 0,
	home = "here",
	size = { 0.8, 0.45, 0.8 },
	behaviours = { wander = false },
	body = {
		{ "body", size = { 0.5, 0.3, 0.62 }, at = { 0, 0.34, 0.14 }, color = knobs.colour },
		{ "back", size = { 0.34, 0.12, 0.42 }, at = { 0, 0.52, 0.18 }, color = knobs.colour },
		{ "head", size = { 0.36, 0.24, 0.28 }, at = { 0, 0.32, -0.3 }, color = knobs.colour, head = true },
		{ "eye_1", size = { 0.07, 0.07, 0.03 }, at = { -0.1, 0.38, -0.45 }, color = knobs.eyes, glow = true, head = true },
		{ "eye_2", size = { 0.07, 0.07, 0.03 }, at = { 0.1, 0.38, -0.45 }, color = knobs.eyes, glow = true, head = true },
		{ "eye_3", size = { 0.05, 0.05, 0.03 }, at = { -0.06, 0.29, -0.45 }, color = knobs.eyes, glow = true, head = true },
		{ "eye_4", size = { 0.05, 0.05, 0.03 }, at = { 0.06, 0.29, -0.45 }, color = knobs.eyes, glow = true, head = true },
		{ "fang_left", size = { 0.04, 0.1, 0.04 }, at = { -0.07, 0.17, -0.43 }, color = "#d8d0b8", head = true },
		{ "fang_right", size = { 0.04, 0.1, 0.04 }, at = { 0.07, 0.17, -0.43 }, color = "#d8d0b8", head = true },
		{ "leg_front_left", size = { 0.34, 0.06, 0.06 }, at = { -0.4, 0.22, -0.16 }, color = LEG, leg = true },
		{ "leg_front_right", size = { 0.34, 0.06, 0.06 }, at = { 0.4, 0.22, -0.16 }, color = LEG, leg = true },
		{ "leg_mid_left", size = { 0.38, 0.06, 0.06 }, at = { -0.42, 0.22, 0.08 }, color = LEG, leg = true },
		{ "leg_mid_right", size = { 0.38, 0.06, 0.06 }, at = { 0.42, 0.22, 0.08 }, color = LEG, leg = true },
		{ "leg_back_left", size = { 0.34, 0.06, 0.06 }, at = { -0.4, 0.22, 0.32 }, color = LEG, leg = true },
		{ "leg_back_right", size = { 0.34, 0.06, 0.06 }, at = { 0.4, 0.22, 0.32 }, color = LEG, leg = true },
		{ "foot_1", size = { 0.06, 0.2, 0.06 }, at = { -0.58, 0.1, -0.16 }, color = LEG, leg = true },
		{ "foot_2", size = { 0.06, 0.2, 0.06 }, at = { 0.58, 0.1, -0.16 }, color = LEG, leg = true },
		{ "foot_3", size = { 0.06, 0.2, 0.06 }, at = { -0.62, 0.1, 0.08 }, color = LEG, leg = true },
		{ "foot_4", size = { 0.06, 0.2, 0.06 }, at = { 0.62, 0.1, 0.08 }, color = LEG, leg = true },
		{ "foot_5", size = { 0.06, 0.2, 0.06 }, at = { -0.58, 0.1, 0.32 }, color = LEG, leg = true },
		{ "foot_6", size = { 0.06, 0.2, 0.06 }, at = { 0.58, 0.1, 0.32 }, color = LEG, leg = true },
	},
}

local MOST = 40 -- at once, however many players are down there
local NEAR, FAR = 7, 14 -- how far from a player they turn up
local CHASE = 18 -- they follow players this close
local LONELY = 40 -- and go back into the dark with nobody this close
local NIP_EVERY = 1.2 -- seconds
local DEEP = 4 -- blocks under the ground over them that counts as a cave

local UP = vector.create(0, 1, 0)

-- where each one is, for counting how many are near a player
local where: { [number]: vector } = {}

local function flat(a: vector, b: vector): number
	local dx, dz = a.x - b.x, a.z - b.z
	return math.sqrt(dx * dx + dz * dz)
end

local function solid(at: vector): boolean
	local ok, kind = pcall(world.kind, world.block(at))
	return ok and kind ~= "air" and kind ~= "water" and kind ~= "plant"
end

-- underground: well below the highest ground over it
local function below_ground(at: vector): boolean
	return at.y < world.top(at.x, at.z) - DEEP
end

-- the players in survival down in a cave this tick
local seen_at, seen = -1, {}
local function prey()
	local now = world.time()
	if now ~= seen_at then
		seen_at, seen = now, {}
		for _, p in world.players() do
			if world.pockets(p) ~= nil and below_ground(p.position) then
				table.insert(seen, p)
			end
		end
	end
	return seen
end

local function nearest(pos: vector, within: number)
	local best, best_d = nil, within
	for _, p in prey() do
		local d = pos - p.position
		local dist = vector.magnitude(d)
		if dist < best_d then
			best, best_d = p, dist
		end
	end
	return best
end

local function away_from(self, from: vector, far: number, speed: number)
	local d = math.max(flat(self.position, from), 0.1)
	local away = vector.create((self.position.x - from.x) / d, 0, (self.position.z - from.z) / d)
	self:walk_to(self.position + away * far, speed)
end

function Crawler:spawned()
	self.nip_in = NIP_EVERY
	self.look_in = 0
	self.stuck = 0
	self.was = self.position
	sound.play("skitter", { at = self, volume = 0.5, caption = "Something skitters in the dark" })
end

local function gone(self)
	where[self.id] = nil
	self:remove()
end

function Crawler:tick(dt: number)
	where[self.id] = self.position
	local d = danger()
	if d.many == 0 or knobs.how_many < 1 then
		gone(self)
		return
	end
	-- just bopped: keep running off a moment
	if (self.back_off or 0) > 0 then
		self.back_off -= dt
		return
	end
	self.look_in -= dt
	if self.look_in <= 0 then
		self.look_in = world.random(0.7, 1.1)
		if not world.nearest_player(self.position, LONELY) then
			gone(self)
			return
		end
		self.light = world.find_light(self.position, knobs.shy)
		local p = nearest(self.position, CHASE)
		self.prey = if p then p.id else nil
		self.safe = if p then world.find_light(p.position, knobs.shy) else nil
		-- stuck against something: climb it
		if self.chasing and vector.magnitude(self.position - self.was) < 0.3 and self:on_ground() then
			self:hop(1.25)
		end
		self.was = self.position
		if world.random() < 0.15 then
			sound.play("skitter", { at = self, volume = 0.35 })
		end
	end
	if self.light then
		self.chasing = false
		away_from(self, self.light, 4, 3.4)
		return
	end
	local p = nil
	for _, q in prey() do
		if q.id == self.prey then
			p = q
		end
	end
	if not p then
		self.chasing = false
		self:wander(1.4)
		return
	end
	-- they're in the light: wait just out of it
	if self.safe then
		self.chasing = false
		if flat(self.position, self.safe) < knobs.shy + 1.5 then
			away_from(self, self.safe, 2, 2)
		else
			self:stop()
			self:face(p.position)
		end
		return
	end
	self.nip_in -= dt
	local close = flat(self.position, p.position)
	if close < 1.4 and math.abs(self.position.y - p.position.y) < 1.6 then
		self.chasing = false
		self:face(p.position)
		if self.nip_in <= 0 then
			self.nip_in = NIP_EVERY
			if knobs.bite > 0 and world.hurt(p, knobs.bite * d.hurt) then
				sound.play("hiss", { at = self, volume = 0.7 })
			end
		end
	else
		self.chasing = true
		self:walk_to(p.position, 2.8 + (if d.hurt > 1 then 0.6 else 0))
		-- they're above: up after them
		if p.position.y > self.position.y + 0.8 and close < 2.5 and self:on_ground() then
			self:hop(1.3)
		end
	end
end

function Crawler:hit(player, shot)
	local held = world.held(player)
	-- a Keen sword hits harder (world.might); a shot's power says already
	local bop = if shot then shot.power else bops(held) * world.might(player)
	self.bopped = (self.bopped or 0) + bop
	if self.bopped < knobs.bops then
		away_from(self, player.position, 3, 4)
		self:hop(0.5)
		self.back_off = 1.2
		self.nip_in = 1.5
		sound.play("hiss", { at = self, volume = 0.6, pitch = 1.3 })
		return
	end
	sound.play("pop", { at = self, pitch = 0.6, caption = "A cave crawler pops" })
	local n = math.floor(world.random(SILK.least, SILK.most + 0.999))
	world.give(player, SILK.thing, n)
	world.reward(player, 3)
	gone(self)
end

-- somewhere a crawler can come out: two blocks of air on the ground, in a
-- cave, out of the light
local function hole(x: number, y: number, z: number): vector?
	local at = vector.create(math.floor(x) + 0.5, y, math.floor(z) + 0.5)
	for dy = 3, -3, -1 do
		local p = at + UP * dy
		if world.block(p) == "Air" and world.block(p + UP) == "Air" and solid(p - UP) and below_ground(p) then
			if not world.find_light(p, knobs.shy + 1) then
				return vector.create(p.x, math.floor(p.y) + 0.05, p.z)
			end
			return nil
		end
	end
	return nil
end

-- Every few seconds: one more near each player in survival down a cave and
-- out of the light, with fewer near them than the knob and the danger say.
world.every(3, function()
	local d = danger()
	local want = math.floor(knobs.how_many * d.many + 0.5)
	if want < 1 then
		return
	end
	-- where each one is (gone ones forgotten)
	local live = {}
	for _, id in world.creatures(Crawler) do
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
	for _, p in prey() do
		local near = 0
		for _, at in where do
			if vector.magnitude(at - p.position) < CHASE + 4 then
				near += 1
			end
		end
		if total < MOST and near < want and not world.find_light(p.position, knobs.shy) then
			for _ = 1, 4 do
				local a = world.random(0, 2 * math.pi)
				local r = world.random(NEAR, FAR)
				local at = hole(p.position.x + math.cos(a) * r, math.floor(p.position.y), p.position.z + math.sin(a) * r)
				if at then
					where[world.spawn(Crawler, at)] = at
					total += 1
					break
				end
			end
		end
	end
end)
