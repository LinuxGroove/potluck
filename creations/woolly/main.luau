--!strict
-- Woolly: sheep that graze the Green and nibble the tall grass.
-- Hold wheat and they come to you; Interact with one and it follows you for
-- a while; hit one and it runs. All that is `behaviours`, with no code.
-- In survival, Interact shears it too: you get wool (more with shears in
-- hand, world.held), and it's a Shorn woolly (self:become) until its fleece
-- grows back. Feed two of them wheat and they have a lamb, which grows into
-- a woolly: the lambs and the woollies they grow into are kept with the
-- world (store), since the game only brings back as many as `count` after a
-- restart.

local world = require("@potluck/world@0.10")
local tweak = require("@potluck/tweak")
local store = require("@potluck/store")

local knobs = tweak {
	wool = { "#efe9dc", label = "Wool colour", order = 1 },
	follow = { 20, min = 5, max = 60, label = "Seconds it follows you", order = 2 },
	flock = { 4, min = 1, max = 8, label = "How many", order = 3 },
	shear = { 1, min = 1, max = 4, label = "Wool from shearing one", order = 4 },
	shears = { 2, min = 0, max = 4, label = "More wool with shears", order = 6 },
	regrow = { 3, min = 0.5, max = 20, label = "Minutes for its wool to grow back", order = 5 },
	grow = { 5, min = 0.5, max = 30, label = "Minutes for a lamb to grow up", order = 7 },
	rest = { 5, min = 0.5, max = 30, label = "Minutes before a woolly has another lamb", order = 8 },
}

local FOOD = "Wheat"
-- room for everything else of this creation's under the 64 a creation may have
local MOST = 48

-- what a sheep does by itself, sheared or not
local SHEEP = {
	follow_food = FOOD,
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

-- a young one, small and woolly, too young to shear
local Lamb = world.creature {
	name = "Lamb",
	speed = 1.8,
	count = 0,
	home = "green",
	behaviours = {
		follow_food = FOOD,
		flee_when_hit = 5,
		hop_when_hit = 0.6,
		chatter = { "Meh!", "Baa?" },
		sound = "bleat",
	},
	body = {
		{ "fleece", size = { 0.56, 0.46, 0.72 }, at = { 0, 0.55, 0.03 }, color = knobs.wool },
		{ "head", size = { 0.3, 0.32, 0.32 }, at = { 0, 0.72, -0.44 }, color = "#3b3430", head = true },
		{ "ear_left", size = { 0.13, 0.05, 0.09 }, at = { -0.2, 0.8, -0.4 }, color = "#3b3430", head = true },
		{ "ear_right", size = { 0.13, 0.05, 0.09 }, at = { 0.2, 0.8, -0.4 }, color = "#3b3430", head = true },
		{ "eye_left", size = { 0.05, 0.05, 0.03 }, at = { -0.08, 0.77, -0.61 }, color = "#f4f1e8", head = true },
		{ "eye_right", size = { 0.05, 0.05, 0.03 }, at = { 0.08, 0.77, -0.61 }, color = "#f4f1e8", head = true },
		{ "leg_front_left", size = { 0.11, 0.34, 0.11 }, at = { -0.16, 0.17, -0.22 }, color = "#3b3430", leg = true },
		{ "leg_front_right", size = { 0.11, 0.34, 0.11 }, at = { 0.16, 0.17, -0.22 }, color = "#3b3430", leg = true },
		{ "leg_back_left", size = { 0.11, 0.34, 0.11 }, at = { -0.16, 0.17, 0.26 }, color = "#3b3430", leg = true },
		{ "leg_back_right", size = { 0.11, 0.34, 0.11 }, at = { 0.16, 0.17, 0.26 }, color = "#3b3430", leg = true },
	},
}

-- ---- lambs: two fed woollies have one ----

-- who's keen and where, each saying every tick (an old word is someone gone)
type Keen = { at: vector, t: number }
local keen: { [number]: Keen } = {}
-- a woolly whose partner had the lamb: it rests too
local done: { [number]: boolean } = {}
-- how grown a lamb spawned from here is to be, by its id, for its `spawned`
local born: { [number]: number } = {}

-- every lamb born here and the woollies they grow into, each saying every
-- tick, kept with the world so they're back after a restart
type Seen = { at: vector, grow: number, t: number }
local seen: { [number]: Seen } = {}
type Kept = { at: vector, grow: number }

local function lamb_at(at: vector, grow: number): boolean
	if #world.creatures() >= MOST then
		return false
	end
	local ok, id = pcall(world.spawn, Lamb, at)
	if ok then
		born[id] = grow
	end
	return ok
end

-- the nearest other keen woolly within ten blocks, and how far
local function partner(self): (number?, number)
	local best, far = nil, 10
	local now = world.time()
	for id, k in keen do
		if id ~= self.id and now - k.t < 1 then
			local d = self:distance_to(k.at)
			if d < far then
				best, far = id, d
			end
		end
	end
	return best, far
end

local function rested(self)
	self.love = nil
	self.rest = knobs.rest * 60
	keen[self.id] = nil
end

-- what a fed woolly does: go to another fed one, and have a lamb between them
local function court(self, dt: number)
	if done[self.id] then
		done[self.id] = nil
		rested(self)
		return
	end
	if self.rest then
		self.rest -= dt
		if self.rest <= 0 then
			self.rest = nil
		end
	end
	if not self.love then
		return
	end
	self.love -= dt
	if self.love <= 0 then
		self.love = nil
		keen[self.id] = nil
		return
	end
	keen[self.id] = { at = self.position, t = world.time() }
	local other, far = partner(self)
	if not other then
		return
	end
	if far > 1.8 then
		self:walk_to(keen[other].at)
	elseif self.id < other then
		-- the one of the two that goes first has it, and tells the other
		local mid = (self.position + keen[other].at) * 0.5
		if lamb_at(mid, knobs.grow * 60) then
			done[other] = true
			rested(self)
			self:say("Baa-aa!")
		end
	end
end

-- a fed woolly gets keen; a fed lamb grows a little
local function feed(self, player, young: boolean)
	if young then
		if world.take(player, FOOD, 1) then
			self.grow = math.max(0, (self.grow or 0) - 60)
			self:say("Meh!")
		end
	elseif self.love then
		self:say("Baa.")
	elseif self.rest then
		self:say("Baa. Not now.")
	elseif world.take(player, FOOD, 1) then
		self.love = 30
		self:say("Nom nom.")
	end
end

function Woolly:spawned()
	self.nibble = world.random(10, 40)
end

function Woolly:tick(dt: number)
	court(self, dt)
	if self.bred then
		seen[self.id] = { at = self.position, grow = 0, t = world.time() }
	end
	if self.regrow then
		self.regrow -= dt
		if self.regrow <= 0 then
			self.regrow = nil
			self:become(Woolly)
		end
	end
	-- now and then a mouthful of the tall grass underfoot
	self.nibble = (self.nibble or 30) - dt
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
	if world.held(player) == FOOD then
		feed(self, player, false)
		return
	end
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

function Lamb:spawned()
	self.bred = true
	self.grow = born[self.id] or knobs.grow * 60
	born[self.id] = nil
end

function Lamb:tick(dt: number)
	self.grow = (self.grow or 0) - dt
	if self.grow <= 0 then
		self.grow = nil
		self.nibble = world.random(10, 40)
		self:become(Woolly)
		return
	end
	seen[self.id] = { at = self.position, grow = self.grow, t = world.time() }
end

function Lamb:used(player)
	if world.held(player) == FOOD then
		feed(self, player, true)
	end
end

-- ---- kept with the world ----

world.every(15, function()
	local list: { Kept } = {}
	local now = world.time()
	for id, s in seen do
		if now - s.t > 2 then
			seen[id] = nil
		else
			table.insert(list, { at = s.at, grow = s.grow })
		end
	end
	store.set("flock", list)
end)

-- after a restart, the woollies born here come back (a reload keeps them,
-- and they've said where they are by now)
world.after(0.5, function()
	if next(seen) ~= nil then
		return
	end
	local kept: { Kept } = store.get("flock") or {}
	for _, s in kept do
		if not lamb_at(s.at, s.grow) then
			break
		end
	end
end)
