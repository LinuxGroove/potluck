--!strict
-- Goats: they graze the hill and jump about for the fun of it. Hold wheat
-- or carrots and they come to you (behaviours, no code). In survival, use
-- one with a bucket in hand to milk it. Feed two goats and they have a kid,
-- which grows into a goat: the kids and the goats they grow into are kept
-- with the world (store), since the game only brings back as many as
-- `count` after a restart.

local world = require("@potluck/world@0.20")
local tweak = require("@potluck/tweak")
local store = require("@potluck/store")

local knobs = tweak {
	herd = { 3, min = 1, max = 8, label = "How many", order = 1 },
	milk = { 2, min = 0.2, max = 20, label = "Minutes before a goat gives milk again", order = 2 },
	grow = { 5, min = 0.5, max = 30, label = "Minutes for a kid to grow up", order = 3 },
	rest = { 5, min = 0.5, max = 30, label = "Minutes before a goat has another kid", order = 4 },
	jump = { true, label = "Jump about", order = 5 },
	coat = { "#ddd6c8", label = "Coat colour", order = 6 },
}

-- what they come for, and what makes two of them a kid
local FOOD = { "Wheat", "Carrots" }
-- room for everything else of this creation's under the 64 a creation may have
local MOST = 48

local function likes(name: string?): boolean
	return name ~= nil and table.find(FOOD, name) ~= nil
end

local GOAT = {
	follow_food = FOOD,
	flee_when_hit = 7,
	hop_when_hit = 1,
	chatter = { "Meh-eh-eh!", "Baa-aah.", "Mlem." },
	sound = "bleat",
}

local KID = {
	follow_food = FOOD,
	flee_when_hit = 5,
	hop_when_hit = 0.8,
	chatter = { "Meh!", "Meh-eh?" },
	sound = "bleat",
}

local HORN = "#6b5a48"
local LEG = "#a89f90"
local EYE = "#2a2018"

local Goat = world.creature {
	name = "Goat",
	speed = 2.2,
	count = math.floor(knobs.herd),
	home = "start",
	behaviours = GOAT,
	body = {
		{ "body", size = { 0.6, 0.58, 1.0 }, at = { 0, 0.85, 0.05 }, color = knobs.coat },
		{ "udder", size = { 0.24, 0.1, 0.2 }, at = { 0, 0.51, 0.25 }, color = "#e8b8b0" },
		{ "tail", size = { 0.1, 0.18, 0.08 }, at = { 0, 1.12, 0.58 }, color = knobs.coat },
		{ "head", size = { 0.36, 0.4, 0.46 }, at = { 0, 1.25, -0.62 }, color = knobs.coat, head = true },
		{ "horn_left", size = { 0.08, 0.28, 0.08 }, at = { -0.11, 1.56, -0.52 }, color = HORN, head = true },
		{ "horn_right", size = { 0.08, 0.28, 0.08 }, at = { 0.11, 1.56, -0.52 }, color = HORN, head = true },
		{ "beard", size = { 0.1, 0.2, 0.08 }, at = { 0, 0.98, -0.8 }, color = "#bfb6a6", head = true },
		{ "ear_left", size = { 0.18, 0.06, 0.1 }, at = { -0.26, 1.34, -0.58 }, color = knobs.coat, head = true },
		{ "ear_right", size = { 0.18, 0.06, 0.1 }, at = { 0.26, 1.34, -0.58 }, color = knobs.coat, head = true },
		{ "eye_left", size = { 0.06, 0.06, 0.04 }, at = { -0.11, 1.33, -0.86 }, color = EYE, head = true },
		{ "eye_right", size = { 0.06, 0.06, 0.04 }, at = { 0.11, 1.33, -0.86 }, color = EYE, head = true },
		{ "leg_front_left", size = { 0.14, 0.56, 0.14 }, at = { -0.2, 0.28, -0.35 }, color = LEG, leg = true },
		{ "leg_front_right", size = { 0.14, 0.56, 0.14 }, at = { 0.2, 0.28, -0.35 }, color = LEG, leg = true },
		{ "leg_back_left", size = { 0.14, 0.56, 0.14 }, at = { -0.2, 0.28, 0.4 }, color = LEG, leg = true },
		{ "leg_back_right", size = { 0.14, 0.56, 0.14 }, at = { 0.2, 0.28, 0.4 }, color = LEG, leg = true },
	},
}

local Kid = world.creature {
	name = "Kid",
	speed = 2,
	count = 0,
	home = "start",
	behaviours = KID,
	body = {
		{ "body", size = { 0.36, 0.34, 0.6 }, at = { 0, 0.52, 0.03 }, color = knobs.coat },
		{ "tail", size = { 0.06, 0.12, 0.05 }, at = { 0, 0.7, 0.35 }, color = knobs.coat },
		{ "head", size = { 0.26, 0.28, 0.3 }, at = { 0, 0.8, -0.38 }, color = knobs.coat, head = true },
		{ "ear_left", size = { 0.12, 0.05, 0.07 }, at = { -0.18, 0.86, -0.36 }, color = knobs.coat, head = true },
		{ "ear_right", size = { 0.12, 0.05, 0.07 }, at = { 0.18, 0.86, -0.36 }, color = knobs.coat, head = true },
		{ "eye_left", size = { 0.05, 0.05, 0.03 }, at = { -0.08, 0.86, -0.54 }, color = EYE, head = true },
		{ "eye_right", size = { 0.05, 0.05, 0.03 }, at = { 0.08, 0.86, -0.54 }, color = EYE, head = true },
		{ "leg_front_left", size = { 0.09, 0.36, 0.09 }, at = { -0.12, 0.18, -0.2 }, color = LEG, leg = true },
		{ "leg_front_right", size = { 0.09, 0.36, 0.09 }, at = { 0.12, 0.18, -0.2 }, color = LEG, leg = true },
		{ "leg_back_left", size = { 0.09, 0.36, 0.09 }, at = { -0.12, 0.18, 0.24 }, color = LEG, leg = true },
		{ "leg_back_right", size = { 0.09, 0.36, 0.09 }, at = { 0.12, 0.18, 0.24 }, color = LEG, leg = true },
	},
}

-- ---- kids: two fed goats have one ----

-- who's keen and where, each saying every tick (an old word is someone gone)
type Keen = { at: vector, t: number }
local keen: { [number]: Keen } = {}
-- a goat whose partner had the kid: it rests too
local done: { [number]: boolean } = {}
-- how grown a kid spawned from here is to be, by its id, for its `spawned`
local born: { [number]: number } = {}

-- every kid born here and the goats they grow into, each saying every
-- tick, kept with the world so they're back after a restart
type Seen = { at: vector, grow: number, t: number }
local seen: { [number]: Seen } = {}
type Kept = { at: vector, grow: number }

local function kid_at(at: vector, grow: number): boolean
	if #world.creatures() >= MOST then
		return false
	end
	local ok, id = pcall(world.spawn, Kid, at)
	if ok then
		born[id] = grow
	end
	return ok
end

-- the nearest other keen goat within ten blocks, and how far
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

-- what a fed goat does: go to another fed goat, and have a kid between them
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
		if kid_at(mid, knobs.grow * 60) then
			done[other] = true
			rested(self)
			self:say("Meh-eh-eh!")
		end
	end
end

-- a fed goat gets keen; a fed kid grows a little
local function feed(self, player, young: boolean)
	local food = world.held(player) :: string
	if young then
		if world.take(player, food, 1) then
			self.grow = math.max(0, (self.grow or 0) - 60)
			self:say("Mlem!")
		end
	elseif self.love then
		self:say("Mlem.")
	elseif self.rest then
		self:say("Meh. Not now.")
	elseif world.take(player, food, 1) then
		self.love = 30
		self:say("Mlem mlem!")
	end
end

-- ---- milk, and jumping about ----

function Goat:spawned()
	self.boing = world.random(5, 30)
end

local function jumpy(self, dt: number, power: number)
	if not knobs.jump then
		return
	end
	self.boing = (self.boing or 10) - dt
	if self.boing <= 0 then
		self.boing = world.random(8, 30)
		if self:on_ground() then
			self:hop(power)
		end
	end
end

function Goat:tick(dt: number)
	court(self, dt)
	jumpy(self, dt, 1.1)
	if self.milked then
		self.milked -= dt
		if self.milked <= 0 then
			self.milked = nil
		end
	end
	if self.bred then
		seen[self.id] = { at = self.position, grow = 0, t = world.time() }
	end
end

function Goat:used(player)
	local held = world.held(player)
	if likes(held) then
		feed(self, player, false)
	elseif held == "Bucket" and world.pockets(player) ~= nil then
		if self.milked then
			self:say("Meh. Not yet.")
		elseif world.take(player, "Bucket", 1) then
			if world.give(player, "Milk bucket", 1) > 0 then
				self.milked = knobs.milk * 60
				self:say("Meh-eh.")
			else
				world.give(player, "Bucket", 1)
			end
		end
	end
end

function Goat:hit(player)
	self:say("MEH!")
end

function Kid:spawned()
	self.bred = true
	self.grow = born[self.id] or knobs.grow * 60
	born[self.id] = nil
	self.boing = world.random(3, 15)
end

function Kid:tick(dt: number)
	jumpy(self, dt, 0.8)
	self.grow = (self.grow or 0) - dt
	if self.grow <= 0 then
		self.grow = nil
		self:become(Goat)
		return
	end
	seen[self.id] = { at = self.position, grow = self.grow, t = world.time() }
end

function Kid:used(player)
	if likes(world.held(player)) then
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
	store.set("herd", list)
end)

-- after a restart, the goats born here come back (a reload keeps them, and
-- they've said where they are by now)
world.after(0.5, function()
	if next(seen) ~= nil then
		return
	end
	local kept: { Kept } = store.get("herd") or {}
	for _, s in kept do
		if not kid_at(s.at, s.grow) then
			break
		end
	end
end)
