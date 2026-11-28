--!strict
-- Hens: they peck about the Green and lay eggs on the grass now and then.
-- Hold seeds or wheat and they come to you (behaviours, no code). In
-- survival a pat gives a feather now and then. Feed two hens and they make
-- a chick, which grows into a hen: the chicks and the hens they grow into
-- are kept with the world (store), since the game only brings back as many
-- as `count` after a restart.

local world = require("@potluck/world@0.20")
local tweak = require("@potluck/tweak")
local store = require("@potluck/store")

local knobs = tweak {
	flock = { 4, min = 1, max = 8, label = "How many", order = 1 },
	lay = { 4, min = 0.5, max = 20, label = "Minutes between a hen's eggs", order = 2 },
	eggs = { 12, min = 1, max = 32, label = "Most eggs lying about", order = 3 },
	feather = { 2, min = 0.2, max = 10, label = "Minutes between a hen's feathers", order = 4 },
	grow = { 5, min = 0.5, max = 30, label = "Minutes for a chick to grow up", order = 5 },
	rest = { 5, min = 0.5, max = 30, label = "Minutes before a hen makes another chick", order = 6 },
	colour = { "#b0673a", label = "Feather colour", order = 7 },
}

local EGG = "Egg"
local FEATHER = "Feather"
-- what they come for, and what makes two of them a chick
local FOOD = { "Seeds", "Wheat" }
-- room for everything else of this creation's under the 64 a creation may have
local MOST = 48

local function likes(name: string?): boolean
	return name ~= nil and table.find(FOOD, name) ~= nil
end

local HEN = {
	follow_food = FOOD,
	flee_when_hit = 5,
	hop_when_hit = 0.5,
	chatter = { "Cluck.", "Bawk?", "Buk-buk-buk." },
	sound = "cluck",
}

local CHICK = {
	follow_food = FOOD,
	flee_when_hit = 4,
	hop_when_hit = 0.3,
	chatter = { "Peep.", "Peep peep!" },
	sound = "peep",
}

local LEGS = "#e0a028"
local BEAK = "#e8a020"
local COMB = "#c8282c"
local EYE = "#1c1814"

local Hen = world.creature {
	name = "Hen",
	speed = 1.8,
	count = math.floor(knobs.flock),
	home = "green",
	behaviours = HEN,
	body = {
		{ "body", size = { 0.46, 0.4, 0.58 }, at = { 0, 0.46, 0.02 }, color = knobs.colour },
		{ "tail", size = { 0.3, 0.3, 0.14 }, at = { 0, 0.66, 0.34 }, color = knobs.colour },
		{ "wing_left", size = { 0.06, 0.26, 0.4 }, at = { -0.25, 0.48, 0.04 }, color = knobs.colour },
		{ "wing_right", size = { 0.06, 0.26, 0.4 }, at = { 0.25, 0.48, 0.04 }, color = knobs.colour },
		{ "head", size = { 0.26, 0.28, 0.26 }, at = { 0, 0.78, -0.3 }, color = knobs.colour, head = true },
		{ "beak", size = { 0.1, 0.07, 0.12 }, at = { 0, 0.77, -0.48 }, color = BEAK, head = true },
		{ "comb", size = { 0.06, 0.1, 0.18 }, at = { 0, 0.96, -0.3 }, color = COMB, head = true },
		{ "wattle", size = { 0.06, 0.08, 0.05 }, at = { 0, 0.69, -0.42 }, color = COMB, head = true },
		{ "eye_left", size = { 0.04, 0.05, 0.05 }, at = { -0.13, 0.82, -0.36 }, color = EYE, head = true },
		{ "eye_right", size = { 0.04, 0.05, 0.05 }, at = { 0.13, 0.82, -0.36 }, color = EYE, head = true },
		{ "leg_left", size = { 0.06, 0.26, 0.06 }, at = { -0.1, 0.13, 0 }, color = LEGS, leg = true },
		{ "leg_right", size = { 0.06, 0.26, 0.06 }, at = { 0.1, 0.13, 0 }, color = LEGS, leg = true },
	},
}

local Chick = world.creature {
	name = "Chick",
	speed = 1.6,
	count = 0,
	home = "green",
	behaviours = CHICK,
	body = {
		{ "body", size = { 0.26, 0.24, 0.3 }, at = { 0, 0.22, 0 }, color = "#f2d24a" },
		{ "head", size = { 0.2, 0.2, 0.2 }, at = { 0, 0.4, -0.12 }, color = "#f2d24a", head = true },
		{ "beak", size = { 0.06, 0.04, 0.06 }, at = { 0, 0.39, -0.25 }, color = BEAK, head = true },
		{ "eye_left", size = { 0.03, 0.04, 0.03 }, at = { -0.1, 0.43, -0.2 }, color = EYE, head = true },
		{ "eye_right", size = { 0.03, 0.04, 0.03 }, at = { 0.1, 0.43, -0.2 }, color = EYE, head = true },
		{ "leg_left", size = { 0.04, 0.12, 0.04 }, at = { -0.06, 0.06, 0 }, color = LEGS, leg = true },
		{ "leg_right", size = { 0.04, 0.12, 0.04 }, at = { 0.06, 0.06, 0 }, color = LEGS, leg = true },
	},
}

-- ---- chicks: two fed hens make one ----

-- who's keen and where, each saying every tick (an old word is someone gone)
type Keen = { at: vector, t: number }
local keen: { [number]: Keen } = {}
-- a hen whose partner made the chick: it rests too
local done: { [number]: boolean } = {}
-- how grown a chick spawned from here is to be, by its id, for its `spawned`
local born: { [number]: number } = {}

-- every hen and chick that hatched here, each saying every tick, kept with
-- the world so they're back after a restart
type Seen = { at: vector, grow: number, t: number }
local seen: { [number]: Seen } = {}
type Kept = { at: vector, grow: number }

local function chick_at(at: vector, grow: number): boolean
	if #world.creatures() >= MOST then
		return false
	end
	local ok, id = pcall(world.spawn, Chick, at)
	if ok then
		born[id] = grow
	end
	return ok
end

-- the nearest other keen hen within ten blocks, and how far
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

-- what a fed hen does: go to another fed hen, and make a chick between them
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
	if far > 1.4 then
		self:walk_to(keen[other].at)
	elseif self.id < other then
		-- the one of the two that goes first makes it, and tells the other
		local mid = (self.position + keen[other].at) * 0.5
		if chick_at(mid, knobs.grow * 60) then
			done[other] = true
			rested(self)
			self:say("Buk-buk-BAWK!")
		end
	end
end

-- a fed hen gets keen; a fed chick grows a little
local function feed(self, player, young: boolean)
	local food = world.held(player) :: string
	if young then
		if world.take(player, food, 1) then
			self.grow = math.max(0, (self.grow or 0) - 60)
			self:say("Peep!")
		end
	elseif self.love then
		self:say("Cluck!")
	elseif self.rest then
		self:say("Bawk. Not now.")
	elseif world.take(player, food, 1) then
		self.love = 30
		self:say("Buk-buk!")
	end
end

-- ---- eggs and feathers ----

local laid: { vector } = store.get("eggs") or {}

-- the eggs laid here that are still lying where they were laid
local function lying(): number
	local still = {}
	for _, at in laid do
		if world.block(at) == EGG then
			table.insert(still, at)
		end
	end
	laid = still
	return #laid
end

local function lay(self)
	local at = self.position
	local below = at - vector.create(0, 1, 0)
	if world.block(at) ~= "Air" or world.block(below) == "Air" or world.kind(world.block(below)) == "water" then
		return
	end
	if world.find_block(at, 2, EGG) or lying() >= knobs.eggs then
		return
	end
	world.set_block(at, EGG)
	table.insert(laid, at)
	store.set("eggs", laid)
	self:say("Bawk! Bawk!")
end

function Hen:spawned()
	self.egg = world.random(30, knobs.lay * 60)
end

function Hen:tick(dt: number)
	court(self, dt)
	self.egg = (self.egg or knobs.lay * 60) - dt
	if self.egg <= 0 then
		self.egg = knobs.lay * 60 * world.random(0.8, 1.2)
		lay(self)
	end
	if self.plucked then
		self.plucked -= dt
		if self.plucked <= 0 then
			self.plucked = nil
		end
	end
	if self.bred then
		seen[self.id] = { at = self.position, grow = 0, t = world.time() }
	end
end

function Hen:used(player)
	if likes(world.held(player)) then
		feed(self, player, false)
	elseif not self.plucked and world.pockets(player) ~= nil and world.give(player, FEATHER, 1) > 0 then
		-- in survival, a pat now and then gives a feather
		self.plucked = knobs.feather * 60
		self:say("Bawk! That was mine.")
	end
end

function Hen:hit(player)
	self:say("BAWK!")
end

function Chick:spawned()
	self.bred = true
	self.grow = born[self.id] or knobs.grow * 60
	born[self.id] = nil
end

function Chick:tick(dt: number)
	self.grow = (self.grow or 0) - dt
	if self.grow <= 0 then
		self.grow = nil
		self.egg = world.random(30, knobs.lay * 60)
		self:become(Hen)
		return
	end
	seen[self.id] = { at = self.position, grow = self.grow, t = world.time() }
end

function Chick:used(player)
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
	store.set("flock", list)
end)

-- after a restart, the hens hatched here come back (a reload keeps them, and
-- they've said where they are by now)
world.after(0.5, function()
	if next(seen) ~= nil then
		return
	end
	local kept: { Kept } = store.get("flock") or {}
	for _, s in kept do
		if not chick_at(s.at, s.grow) then
			break
		end
	end
end)
