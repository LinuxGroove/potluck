--!strict
-- Stone Sentinels: old stone guardians over the moonsilver deep in the
-- earth. Near moonsilver ore, far down, one stands guard; come within a few
-- blocks of where it stands in survival and it wakes, walks at you (slowly)
-- and slams. It won't follow far from its ore. It's very tough: beat it
-- (swords and shots bop harder) and it leaves a sentinel core, and
-- sometimes raw moonsilver, in the pockets of whoever beat it, and nothing
-- guards that seam again (kept with the world, by where it stood). How
-- hard it slams and how quick it is go by the world's danger
-- (world.danger, world 0.21); none stand guard on Peaceful.

local world = require("@potluck/world@0.21")
local tweak = require("@potluck/tweak")
local store = require("@potluck/store")
local sound = require("@potluck/sound@0.2")

local knobs = tweak {
	on = { true, label = "Sentinels guard moonsilver", order = 1 },
	slam = { 4, min = 0, max = 12, label = "How hard one slams", order = 2 },
	bops = { 16, min = 4, max = 40, label = "Bops to beat one by hand", order = 3 },
	wakes = { 6, min = 2, max = 12, label = "How close to its ore wakes it", order = 4 },
	stone = { "#6e6a66", label = "Stone colour", order = 5 },
	glow = { "#7af0e6", label = "Glow colour", order = 6 },
}

-- How hard it slams and how quick it is, by the world's danger.
local DANGER: { [string]: { hurt: number, speed: number } } = {
	peaceful = { hurt = 0, speed = 0 },
	gentle = { hurt = 0.5, speed = 0.9 },
	normal = { hurt = 1, speed = 1.2 },
	hard = { hurt = 1.5, speed = 1.6 },
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

local ORE = "Moonsilver ore"
local DARK = "#4a4642"
local Sentinel = world.creature {
	name = "Stone Sentinel",
	speed = 1.2,
	count = 0,
	home = "here",
	size = { 0.9, 2.1, 0.9 },
	behaviours = { wander = false },
	body = {
		{ "legs", size = { 0.7, 0.7, 0.5 }, at = { 0, 0.35, 0 }, color = DARK },
		{ "body", size = { 1.0, 0.9, 0.7 }, at = { 0, 1.15, 0 }, color = knobs.stone },
		{ "core", size = { 0.26, 0.26, 0.06 }, at = { 0, 1.2, -0.36 }, color = knobs.glow, glow = true },
		{ "head", size = { 0.56, 0.46, 0.5 }, at = { 0, 1.84, -0.04 }, color = knobs.stone, head = true },
		{ "brow", size = { 0.6, 0.1, 0.12 }, at = { 0, 1.98, -0.3 }, color = DARK, head = true },
		{ "eye_left", size = { 0.12, 0.06, 0.03 }, at = { -0.13, 1.88, -0.3 }, color = knobs.glow, glow = true, head = true },
		{ "eye_right", size = { 0.12, 0.06, 0.03 }, at = { 0.13, 1.88, -0.3 }, color = knobs.glow, glow = true, head = true },
		{ "arm_left", size = { 0.3, 0.95, 0.34 }, at = { -0.66, 1.0, 0 }, color = DARK, leg = true },
		{ "arm_right", size = { 0.3, 0.95, 0.34 }, at = { 0.66, 1.0, 0 }, color = DARK, leg = true },
		{ "fist_left", size = { 0.36, 0.3, 0.38 }, at = { -0.66, 0.42, 0 }, color = knobs.stone, leg = true },
		{ "fist_right", size = { 0.36, 0.3, 0.38 }, at = { 0.66, 0.42, 0 }, color = knobs.stone, leg = true },
		{ "foot_left", size = { 0.3, 0.2, 0.42 }, at = { -0.2, 0.1, -0.04 }, color = DARK, leg = true },
		{ "foot_right", size = { 0.3, 0.2, 0.42 }, at = { 0.2, 0.1, -0.04 }, color = DARK, leg = true },
	},
}

local MOST = 8 -- at once in the whole world
local DEEP = -80 -- only below this
local LEASH = 11 -- it never goes farther than this from where it stood
local SLAM_EVERY = 2.2 -- seconds
local SEAM = 24 -- one guards the seam this far round it

local UP = vector.create(0, 1, 0)

-- where each stands guard
local posts: { [number]: vector } = {}

-- where sentinels were beaten: nothing guards the seam near there again
local beaten: { { number } } = store.get("beaten") or {}
local function was_beaten(at: vector): boolean
	for _, b in beaten do
		if vector.magnitude(vector.create(b[1], b[2], b[3]) - at) < SEAM then
			return true
		end
	end
	return false
end

local function flat(a: vector, b: vector): number
	local dx, dz = a.x - b.x, a.z - b.z
	return math.sqrt(dx * dx + dz * dz)
end

local function solid(at: vector): boolean
	local ok, kind = pcall(world.kind, world.block(at))
	return ok and kind ~= "air" and kind ~= "water" and kind ~= "plant"
end

local function post(self): vector
	return vector.create(self.gx or self.position.x, self.gy or self.position.y, self.gz or self.position.z)
end

local function survivor_near(at: vector, within: number)
	local best, best_d = nil, within
	for _, p in world.players() do
		if world.pockets(p) ~= nil then
			local d = vector.magnitude(p.position - at)
			if d < best_d then
				best, best_d = p, d
			end
		end
	end
	return best
end

function Sentinel:spawned()
	local at = self.position
	self.gx, self.gy, self.gz = at.x, at.y, at.z
	posts[self.id] = at
	self.slam_in = SLAM_EVERY
	self.look_in = 0
end

function Sentinel:tick(dt: number)
	local d = danger()
	if not knobs.on or d.hurt == 0 then
		self:remove()
		return
	end
	local home = post(self)
	posts[self.id] = home
	self.look_in -= dt
	if self.look_in <= 0 then
		self.look_in = world.random(0.4, 0.6)
		if not world.nearest_player(self.position, 48) then
			self:remove()
			return
		end
		local p = survivor_near(home, if self.awake then LEASH else knobs.wakes)
		if p and not self.awake then
			sound.play("wake", { at = self, volume = 0.9 })
			self:say("...")
		end
		self.target = if p then p.id else nil
		self.awake = p ~= nil
	end
	local p = nil
	for _, q in world.players() do
		if q.id == self.target then
			p = q
		end
	end
	if not p then
		-- back to its post, and still
		if flat(self.position, home) > 0.8 then
			self:walk_to(home, d.speed)
		else
			self:stop()
		end
		return
	end
	self.slam_in -= dt
	if self:distance_to(p.position) < 2.1 then
		self:stop()
		self:face(p.position)
		if self.slam_in <= 0 then
			self.slam_in = SLAM_EVERY / d.speed
			self:hop(0.3)
			sound.play("slam", { at = self, volume = 1 })
			if knobs.slam > 0 then
				world.hurt(p, knobs.slam * d.hurt)
			end
		end
	elseif flat(p.position, home) < LEASH then
		self:walk_to(p.position, d.speed)
		if world.random() < dt * 0.6 then
			sound.play("grind", { at = self, volume = 0.5 })
		end
	else
		self.target = nil
	end
end

function Sentinel:hit(player, shot)
	local held = world.held(player)
	local bop = if shot then shot.power else (held and SWORDS[held] or 1)
	self.bopped = (self.bopped or 0) + bop
	if self.bopped < knobs.bops then
		sound.play("grind", { at = self, volume = 0.8, pitch = 1.3, caption = "Stone cracks" })
		-- a bop wakes it, if it can reach whoever did it
		if world.pockets(player) ~= nil and flat(player.position, post(self)) < LEASH then
			self.target, self.awake = player.id, true
		end
		return
	end
	sound.play("crack", { at = self, pitch = 0.6, caption = "A sentinel crumbles" })
	world.give(player, "Sentinel core", 1)
	if world.random() < 0.5 then
		world.give(player, "Raw moonsilver", 1)
	end
	local at = post(self)
	if #beaten < 512 then
		table.insert(beaten, { math.floor(at.x), math.floor(at.y), math.floor(at.z) })
		store.set("beaten", beaten)
	end
	self:remove()
end

-- where one can stand by the ore: room for it, on solid ground
local function stand_by(ore: vector): vector?
	for r = 1, 3 do
		for _ = 1, 8 do
			local a = world.random(0, 2 * math.pi)
			local p = vector.create(math.floor(ore.x + math.cos(a) * r) + 0.5, 0, math.floor(ore.z + math.sin(a) * r) + 0.5)
			for dy = 2, -3, -1 do
				local q = vector.create(p.x, math.floor(ore.y) + dy, p.z)
				if world.block(q) == "Air" and world.block(q + UP) == "Air" and world.block(q + UP * 2) == "Air" and solid(q - UP) then
					return vector.create(q.x, q.y + 0.05, q.z)
				end
			end
		end
	end
	return nil
end

-- Every few seconds: by moonsilver near a player deep down, a sentinel, if
-- nothing guards that stretch of seam yet and none was beaten there.
world.every(5, function()
	if not knobs.on or danger().hurt == 0 then
		return
	end
	-- where each stands guard (gone ones forgotten)
	local live = {}
	for _, id in world.creatures(Sentinel) do
		live[id] = true
	end
	local total = 0
	for id in posts do
		if live[id] then
			total += 1
		else
			posts[id] = nil
		end
	end
	if total >= MOST then
		return
	end
	for _, p in world.players() do
		if p.position.y < DEEP and world.pockets(p) ~= nil then
			local ore = world.find_block(p.position, 16, ORE)
			if ore and not was_beaten(ore) then
				local guarded = false
				for _, at in posts do
					if vector.magnitude(at - ore) < SEAM then
						guarded = true
					end
				end
				local at = if guarded then nil else stand_by(ore)
				if at then
					posts[world.spawn(Sentinel, at)] = at
					return
				end
			end
		end
	end
end)
