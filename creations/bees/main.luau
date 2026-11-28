--!strict
-- Bees: they fly from flower to flower, and home to a beehive with what
-- they gather. A hive the bees have brought enough to is full of honey: use
-- it and you get two jars, and it's empty again. Each hive put down brings
-- two more bees, and the hives (with how full each is) are kept with the
-- world. Bop a bee in survival and it stings.

local world = require("@potluck/world@0.20")
local tweak = require("@potluck/tweak")
local store = require("@potluck/store")
local sound = require("@potluck/sound")

local knobs = tweak {
	swarm = { 4, min = 1, max = 8, label = "Bees on the Green", order = 1 },
	per_hive = { 2, min = 0, max = 4, label = "More bees for each hive", order = 2 },
	trips = { 8, min = 1, max = 40, label = "Trips to fill a hive", order = 3 },
	honey = { 2, min = 1, max = 6, label = "Honey from a full hive", order = 4 },
	sting = { 1, min = 0, max = 6, label = "How much a sting hurts", order = 5 },
}

local HIVE = "Beehive"
local FULL = "Full beehive"
local FLOWERS = { "Red flower", "Blue flower", "White flower", "Violet flower" }
-- how far a bee goes for a flower, and from its hive
local FORAGE = 14
local RANGE = 24
-- room for everything else of this creation's under the 64 a creation may have
local MOST = 40

local YELLOW = "#f2c430"
local DARK = "#2a1c0c"
local WING = "#e4eef6"

local Bee = world.creature {
	name = "Bee",
	speed = 2.6,
	count = math.floor(knobs.swarm),
	home = "green",
	flies = true,
	behaviours = {
		wander = false,
		flee_when_hit = 6,
		chatter = { "Bzz.", "Bzzz." },
		sound = "buzz",
	},
	body = {
		{ "body", size = { 0.2, 0.18, 0.28 }, at = { 0, 0.2, 0.02 }, color = YELLOW },
		{ "stripe_front", size = { 0.21, 0.19, 0.05 }, at = { 0, 0.2, 0.02 }, color = DARK },
		{ "stripe_back", size = { 0.21, 0.19, 0.05 }, at = { 0, 0.2, 0.11 }, color = DARK },
		{ "head", size = { 0.15, 0.15, 0.1 }, at = { 0, 0.21, -0.16 }, color = DARK, head = true },
		{ "wing_left", size = { 0.16, 0.02, 0.12 }, at = { -0.12, 0.31, 0 }, color = WING },
		{ "wing_right", size = { 0.16, 0.02, 0.12 }, at = { 0.12, 0.31, 0 }, color = WING },
	},
}

-- ---- hives ----

type Hive = { at: vector, trips: number }
local hives: { [string]: Hive } = store.get("hives") or {}

local function key(at: vector): string
	return math.floor(at.x) .. "," .. math.floor(at.y) .. "," .. math.floor(at.z)
end

local function save()
	store.set("hives", hives)
end

local function count(): number
	local n = 0
	for _ in hives do
		n += 1
	end
	return n
end

-- forget hives that have gone (broken, or changed by something else)
local function tidy()
	local gone = false
	for k, h in hives do
		local b = world.block(h.at)
		if b ~= HIVE and b ~= FULL then
			hives[k] = nil
			gone = true
		end
	end
	if gone then
		save()
	end
end

-- the nearest hive with room for honey, within reach
local function nearest_hive(pos: vector): string?
	local best, far = nil, RANGE
	for k, h in hives do
		if h.trips < knobs.trips then
			local d = vector.magnitude(h.at - pos)
			if d < far then
				best, far = k, d
			end
		end
	end
	return best
end

-- a flower nearby, one of the near ones at random
local function flower_near(pos: vector): vector?
	local found = {}
	for _, name in FLOWERS do
		local at = world.find_block(pos, FORAGE, name)
		if at then
			table.insert(found, at)
		end
	end
	if #found == 0 then
		return nil
	end
	return found[math.floor(world.random(1, #found + 0.999))]
end

-- more bees for the hives there are, up to the most
local function top_up()
	local want = math.min(MOST, math.floor(knobs.swarm) + count() * math.floor(knobs.per_hive))
	local have = #world.creatures(Bee)
	if have >= want then
		return
	end
	-- by each hive in turn
	local at = {}
	for _, h in hives do
		table.insert(at, h.at + vector.create(0, 1, 0))
	end
	local i = 0
	while have < want and #at > 0 do
		if not pcall(world.spawn, Bee, at[i % #at + 1]) then
			break
		end
		have += 1
		i += 1
	end
end

-- ---- a bee's day ----

-- a bee came to its hive with what it gathered
local function bring(k: string)
	local h = hives[k]
	if not h or world.block(h.at) ~= HIVE then
		return
	end
	h.trips += 1
	if h.trips >= knobs.trips then
		world.set_block(h.at, FULL)
	end
	save()
end

-- where it goes next: home with pollen, or to a flower
local function choose(self)
	self.goal = nil
	self.hive = nil
	if self.pollen then
		local k = nearest_hive(self.position)
		if k then
			self.hive = k
			self.goal = hives[k].at + vector.create(0, 0.6, 0)
			return
		end
		-- nowhere to take it
		self.pollen = nil
	end
	local f = flower_near(self.position)
	if f then
		self.goal = f + vector.create(0, 0.2, 0)
	else
		-- nothing about: a little drift
		self.goal = self.position + vector.create(world.random(-4, 4), world.random(-0.5, 1), world.random(-4, 4))
	end
end

function Bee:spawned()
	self.think = world.random(0, 2)
end

function Bee:tick(dt: number)
	self.think = (self.think or 0) - dt
	local goal: vector? = self.goal
	if goal then
		local d = goal - self.position
		self:climb(math.clamp(d.y * 1.5, -2, 2))
		local flat = vector.magnitude(vector.create(d.x, 0, d.z))
		if flat > 0.4 then
			self:walk_to(goal)
		else
			self:stop()
		end
		if flat < 0.8 and math.abs(d.y) < 0.8 then
			self.goal = nil
			if self.hive then
				bring(self.hive)
				self.pollen = nil
				self.hive = nil
				self.think = world.random(4, 10)
			else
				-- a flower's worth, then a moment on it before going home
				self.pollen = true
				self.think = world.random(1, 3)
			end
		end
	end
	if self.think <= 0 then
		self.think = world.random(6, 12)
		choose(self)
	end
end

function Bee:hit(player)
	self.goal = nil
	self.pollen = nil
	if knobs.sting > 0 and world.hurt(player, knobs.sting) then
		self:say("BZZT!")
	else
		self:say("Bzz!")
	end
end

-- ---- players and hives ----

world.drops(FULL, { gives = HIVE })

world.on("block_placed", function(player, at: vector, block: string)
	if block == HIVE or block == FULL then
		hives[key(at)] = { at = at, trips = if block == FULL then knobs.trips else 0 }
		save()
		top_up()
	end
end)

world.on("block_broken", function(player, at: vector, block: string)
	if block == HIVE or block == FULL then
		hives[key(at)] = nil
		save()
	end
end)

world.on("block_used", function(player, at: vector, block: string)
	if block ~= FULL then
		return
	end
	local n = math.floor(knobs.honey)
	if world.pockets(player) ~= nil and world.give(player, "Honey", n) == 0 then
		return
	end
	world.set_block(at, HIVE)
	local h = hives[key(at)]
	if h then
		h.trips = 0
	else
		hives[key(at)] = { at = at, trips = 0 }
	end
	save()
	sound.play("buzz", { at = at })
end)

-- hives that have gone are forgotten, and bees come for new ones (and
-- after a restart, back for the old)
world.every(20, function()
	tidy()
	top_up()
end)
world.after(0.5, top_up)
