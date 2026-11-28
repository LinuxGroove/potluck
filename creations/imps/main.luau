--!strict
-- Ember Imps: little fiery things that flit about the deep caverns (more
-- than 40 blocks down, and well under the ground over them). They fly
-- (flies), keep a few blocks off anyone in survival, and throw embers at
-- them (self:throw, world 0.21): an ember stings, and where it comes down
-- it may start a small fire that soon goes out. Bop one twice to pop it,
-- harder with a sword or a shot, and it leaves an ember in the pockets of
-- whoever popped it. How many come, how often they throw and how hard
-- embers sting go by the world's danger (world.danger): none on Peaceful.

local world = require("@potluck/world@0.21")
local tweak = require("@potluck/tweak")
local store = require("@potluck/store")
local sound = require("@potluck/sound@0.2")

local knobs = tweak {
	how_many = { 2, min = 0, max = 6, label = "How many flit about you deep down (0 for none)", order = 1 },
	burn = { 2, min = 0, max = 8, label = "How hard an ember stings", order = 2 },
	every = { 3.5, min = 1, max = 12, label = "Seconds between embers", order = 3 },
	fires = { true, label = "Embers start small fires", order = 4 },
	bops = { 2, min = 1, max = 6, label = "Bops to pop one by hand", order = 5 },
}

-- How many come, how often they throw and how hard, by the world's danger.
local DANGER: { [string]: { many: number, hurt: number, pace: number } } = {
	peaceful = { many = 0, hurt = 0, pace = 1 },
	gentle = { many = 0.5, hurt = 0.5, pace = 1.4 },
	normal = { many = 1, hurt = 1, pace = 1 },
	hard = { many = 1.5, hurt = 1.5, pace = 0.7 },
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

local SKIN = "#c8401c"
local HORN = "#3a1408"
local Imp = world.creature {
	name = "Ember Imp",
	speed = 3,
	count = 0,
	home = "here",
	flies = true,
	size = { 0.5, 0.7, 0.5 },
	behaviours = { wander = false },
	body = {
		{ "body", size = { 0.34, 0.36, 0.26 }, at = { 0, 0.32, 0 }, color = SKIN },
		{ "head", size = { 0.36, 0.32, 0.32 }, at = { 0, 0.66, -0.02 }, color = SKIN, head = true },
		{ "horn_left", size = { 0.06, 0.14, 0.06 }, at = { -0.13, 0.88, 0 }, color = HORN, head = true },
		{ "horn_right", size = { 0.06, 0.14, 0.06 }, at = { 0.13, 0.88, 0 }, color = HORN, head = true },
		{ "eye_left", size = { 0.08, 0.06, 0.03 }, at = { -0.08, 0.7, -0.19 }, color = "#ffe060", glow = true, head = true },
		{ "eye_right", size = { 0.08, 0.06, 0.03 }, at = { 0.08, 0.7, -0.19 }, color = "#ffe060", glow = true, head = true },
		{ "grin", size = { 0.16, 0.03, 0.03 }, at = { 0, 0.58, -0.19 }, color = "#ffb030", glow = true, head = true },
		{ "flame", size = { 0.16, 0.18, 0.16 }, at = { 0, 0.96, 0.06 }, color = "#ffa020", glow = true, head = true },
		{ "wing_left", size = { 0.32, 0.22, 0.03 }, at = { -0.3, 0.44, 0.15 }, color = "#7a2010", leg = true },
		{ "wing_right", size = { 0.32, 0.22, 0.03 }, at = { 0.3, 0.44, 0.15 }, color = "#7a2010", leg = true },
		{ "tail", size = { 0.05, 0.05, 0.3 }, at = { 0, 0.2, 0.26 }, color = SKIN },
		{ "tail_tip", size = { 0.1, 0.1, 0.08 }, at = { 0, 0.2, 0.42 }, color = "#ffa020", glow = true },
		{ "leg_left", size = { 0.08, 0.16, 0.08 }, at = { -0.09, 0.08, 0 }, color = SKIN },
		{ "leg_right", size = { 0.08, 0.16, 0.08 }, at = { 0.09, 0.08, 0 }, color = SKIN },
	},
}

local MOST = 24 -- at once, however many players are down there
local DEEP = -40 -- only below this
local NEAR, FAR = 8, 16 -- how far from a player they turn up
local CHASE = 22 -- they go after players this close
local LONELY = 40 -- and go with nobody this close
local KEEP = 7 -- how far off they like to stay
local REACH = 14 -- the farthest they throw
local FIRE = "Fire"
local BURNS = 8 -- seconds a fire an ember started burns

local UP = vector.create(0, 1, 0)
local SIDES = { UP, vector.create(1, 0, 0), vector.create(-1, 0, 0), vector.create(0, 0, 1), vector.create(0, 0, -1), vector.create(0, -1, 0) }

local where: { [number]: vector } = {}

local function flat(a: vector, b: vector): number
	local dx, dz = a.x - b.x, a.z - b.z
	return math.sqrt(dx * dx + dz * dz)
end

local function deep(at: vector): boolean
	return at.y < DEEP and at.y < world.top(at.x, at.z) - 6
end

local function prey()
	local list = {}
	for _, p in world.players() do
		if world.pockets(p) ~= nil and deep(p.position) then
			table.insert(list, p)
		end
	end
	return list
end

local function gone(self)
	where[self.id] = nil
	self:remove()
end

-- ---- the little fires embers start, which go out by themselves ----

type Lit = { at: vector, out: number }
local lit: { Lit } = {}
do
	local now = world.time()
	for _, f in (store.get("fires") or {}) :: { { number } } do
		table.insert(lit, { at = vector.create(f[1], f[2], f[3]), out = now + BURNS })
	end
end

local function keep()
	local list = {}
	for _, f in lit do
		table.insert(list, { f.at.x, f.at.y, f.at.z })
	end
	store.set("fires", list)
end

local function burnable(at: vector): boolean
	if world.block(at) ~= "Air" then
		return false
	end
	for _, d in SIDES do
		if world.block(at + d) == "Water" then
			return false
		end
	end
	local ok, kind = pcall(world.kind, world.block(at - UP))
	return ok and kind ~= "air" and kind ~= "water"
end

-- a small fire at or beside where an ember came down, if fire is in this
-- world (the Fire creation's block) and there's room
local function kindle(at: vector)
	if not knobs.fires or #lit >= 32 or not pcall(world.kind, FIRE) then
		return
	end
	local here = vector.create(math.floor(at.x), math.floor(at.y), math.floor(at.z))
	for _, d in SIDES do
		local p = here + d
		if burnable(p) then
			world.set_block(p, FIRE)
			table.insert(lit, { at = p, out = world.time() + BURNS * world.random(0.8, 1.3) })
			keep()
			return
		end
	end
end

world.every(1, function()
	if #lit == 0 then
		return
	end
	local now, left = world.time(), {}
	for _, f in lit do
		if now >= f.out then
			if world.block(f.at) == FIRE then
				world.set_block(f.at, "Air")
			end
		elseif world.block(f.at) == FIRE then
			table.insert(left, f)
		end
	end
	if #left ~= #lit then
		lit = left
		keep()
	end
end)

-- ---- imps ----

function Imp:spawned()
	self.throw_in = world.random(1.5, 3)
	self.side = world.random(0, 2 * math.pi)
	self.look_in = 0
	sound.play("cackle", { at = self, volume = 0.6 })
end

local function sting(at: vector, block: string?, creature: number?, player)
	local d = danger()
	if player then
		if knobs.burn > 0 and world.hurt(player, knobs.burn * d.hurt) then
			sound.play_for(player, "fling", { caption = "An ember stings you", volume = 0.8 })
		end
		if world.random() < 0.3 then
			kindle(player.position)
		end
	elseif block then
		kindle(at)
	end
end

function Imp:tick(dt: number)
	where[self.id] = self.position
	local d = danger()
	if d.many == 0 or knobs.how_many < 1 then
		gone(self)
		return
	end
	if (self.back_off or 0) > 0 then
		self.back_off -= dt
		return
	end
	self.look_in -= dt
	if self.look_in <= 0 then
		self.look_in = world.random(0.5, 0.8)
		if not world.nearest_player(self.position, LONELY) then
			gone(self)
			return
		end
		local best, best_d = nil, CHASE
		for _, p in prey() do
			local dist = vector.magnitude(p.position - self.position)
			if dist < best_d then
				best, best_d = p, dist
			end
		end
		self.prey = if best then best.id else nil
	end
	local p = nil
	for _, q in prey() do
		if q.id == self.prey then
			p = q
		end
	end
	if not p then
		self:climb(0)
		self:wander(1.5)
		return
	end
	-- hover a little above them, and circle at a distance
	local want_y = p.position.y + 2.2
	self:climb(math.clamp((want_y - self.position.y) * 1.2, -2.5, 2.5))
	self.side += dt * 0.4
	local ring = p.position + vector.create(math.cos(self.side), 0, math.sin(self.side)) * KEEP
	self:walk_to(vector.create(ring.x, self.position.y, ring.z), if flat(self.position, p.position) > KEEP + 4 then 3.4 else 2.2)
	self:face(p.position)
	self.throw_in -= dt
	if self.throw_in <= 0 then
		self.throw_in = knobs.every * d.pace * world.random(0.8, 1.25)
		if vector.magnitude(p.position - self.position) < REACH then
			local ok = pcall(function()
				self:throw({ at = p.position + UP * 1.1, look = "ember", speed = 13, drop = 0.3, power = 1 }, sting)
			end)
			if ok then
				sound.play("fling", { at = self, volume = 0.7 })
			end
		end
	end
end

function Imp:hit(player, shot)
	local held = world.held(player)
	local bop = if shot then shot.power else (held and SWORDS[held] or 1)
	self.bopped = (self.bopped or 0) + bop
	if self.bopped < knobs.bops then
		local away = self.position - player.position
		self:walk_to(self.position + vector.normalize(vector.create(away.x, 0, away.z) + vector.create(0.01, 0, 0)) * 4, 4)
		self:climb(1.5)
		self.back_off = 1
		sound.play("cackle", { at = self, volume = 0.7, pitch = 1.4 })
		return
	end
	sound.play("pop", { at = self, pitch = 1.3, caption = "An ember imp pops" })
	world.give(player, "Ember", math.floor(world.random(1, 2.999)))
	gone(self)
end

-- somewhere an imp can come out: a cavern's open air, three blocks high
local function open_air(x: number, y: number, z: number): vector?
	local at = vector.create(math.floor(x) + 0.5, y, math.floor(z) + 0.5)
	for dy = 4, -2, -1 do
		local p = at + UP * dy
		if world.block(p) == "Air" and world.block(p + UP) == "Air" and world.block(p - UP) == "Air" and deep(p) then
			return vector.create(p.x, math.floor(p.y) + 0.2, p.z)
		end
	end
	return nil
end

-- Every few seconds: one more near each player in survival deep down,
-- with fewer near them than the knob and the danger say.
world.every(4, function()
	local d = danger()
	local want = math.floor(knobs.how_many * d.many + 0.5)
	if want < 1 then
		return
	end
	local live = {}
	for _, id in world.creatures(Imp) do
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
		if total < MOST and near < want then
			for _ = 1, 4 do
				local a = world.random(0, 2 * math.pi)
				local r = world.random(NEAR, FAR)
				local at = open_air(p.position.x + math.cos(a) * r, math.floor(p.position.y), p.position.z + math.sin(a) * r)
				if at then
					where[world.spawn(Imp, at)] = at
					total += 1
					break
				end
			end
		end
	end
end)
