--!strict
-- Gloomlings: little shadows that come out after dark and follow anyone
-- out in survival, nipping at their heels. They can't bear light: they
-- keep away from lanterns, braziers and candles (world.find_light), lurk
-- at the edge of a lit place, and fade away in the morning. Bop one twice
-- and it pops, or once with a sword (world.held); an arrow, a stone or a
-- spear bops it as hard as the shot's power says (world.shoot). A popped
-- one leaves gloom dust. How many come and how hard they nip go by the
-- world's danger (world.danger, world 0.21): none on Peaceful, fewer on
-- Gentle. Players in creative have nothing to fear, and nothing comes near
-- the Green.

local world = require("@potluck/world@0.21")
local tweak = require("@potluck/tweak")
local ui = require("@potluck/ui@0.1")
local sound = require("@potluck/sound@0.1")

local knobs = tweak {
	colour = { "#221d36", label = "Gloomling colour", order = 1 },
	eyes = { "#ffe27a", label = "Eye colour", order = 2 },
	how_many = { 4, min = 0, max = 8, label = "How many follow you at night (0 for none)", order = 3 },
	bite = { 1, min = 0, max = 4, label = "How hard they nip", order = 4 },
	shy = { 6, min = 2, max = 12, label = "How far they keep from a light", order = 5 },
	bops = { 2, min = 1, max = 5, label = "Bops to pop one by hand", order = 6 },
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

local Gloomling = world.creature {
	name = "Gloomling",
	speed = 2.6,
	count = 0,
	home = "here",
	body = {
		{ "body", size = { 0.62, 0.5, 0.62 }, at = { 0, 0.32, 0 }, color = knobs.colour },
		{ "tuft", size = { 0.3, 0.14, 0.3 }, at = { 0, 0.63, 0.04 }, color = knobs.colour, head = true },
		{ "eye_left", size = { 0.13, 0.1, 0.04 }, at = { -0.14, 0.42, -0.32 }, color = knobs.eyes, glow = true, head = true },
		{ "eye_right", size = { 0.13, 0.1, 0.04 }, at = { 0.14, 0.42, -0.32 }, color = knobs.eyes, glow = true, head = true },
		{ "foot_left", size = { 0.16, 0.1, 0.22 }, at = { -0.16, 0.05, 0 }, color = knobs.colour, leg = true },
		{ "foot_right", size = { 0.16, 0.1, 0.22 }, at = { 0.16, 0.05, 0 }, color = knobs.colour, leg = true },
	},
}

local MOST = 40 -- at once, however many players are out
local DUSK, DAWN = 0.15, 0.3 -- daylight they come out under, and go in
local NEAR, FAR = 12, 18 -- how far from a player they turn up
local CHASE = 20 -- they follow players this close
local LONELY = 40 -- and fade away with nobody this close
local NIP_EVERY = 1.5 -- seconds

-- where each one is, for counting how many follow a player
local where: { [number]: vector } = {}

local function flat(a: vector, b: vector): number
	local dx, dz = a.x - b.x, a.z - b.z
	return math.sqrt(dx * dx + dz * dz)
end

-- the players out in survival this tick (creative ones have no pockets)
local seen_at, seen = -1, {}
local function prey()
	local now = world.time()
	if now ~= seen_at then
		seen_at, seen = now, {}
		for _, p in world.players() do
			if world.pockets(p) ~= nil then
				table.insert(seen, p)
			end
		end
	end
	return seen
end

local function nearest(pos: vector, within: number)
	local best, best_d = nil, within
	for _, p in prey() do
		local d = flat(p.position, pos)
		if d < best_d then
			best, best_d = p, d
		end
	end
	return best
end

local function fade(g, why: string?)
	where[g.id] = nil
	if why then
		sound.play(why, { at = g, volume = 0.3, pitch = 0.6, caption = "A gloomling fades" })
	end
	g:remove()
end

function Gloomling:spawned()
	where[self.id] = self.position
	self.nip_in = NIP_EVERY
	self.look_in = 0
	self.side = world.random(0, 2 * math.pi) -- which side of you it comes at
	sound.play("grumble", { at = self, volume = 0.35, pitch = 1.7, caption = "A gloomling stirs" })
end

function Gloomling:tick(dt: number)
	where[self.id] = self.position
	-- none on a Peaceful world
	if danger().many == 0 then
		fade(self, "whoosh")
		return
	end
	-- gone by morning, a few at a time
	if world.daylight() > DAWN then
		if world.random() < dt then
			fade(self, "whoosh")
		end
		return
	end
	-- just bopped: keep running off a moment
	if (self.back_off or 0) > 0 then
		self.back_off -= dt
		return
	end
	-- once a second: who to follow, and whether a light is near
	self.look_in -= dt
	if self.look_in <= 0 then
		self.look_in = world.random(0.8, 1.2)
		local p = nearest(self.position, LONELY)
		if not p then
			fade(self)
			return
		end
		self.light = world.find_light(self.position, knobs.shy)
		self.prey = if flat(p.position, self.position) < CHASE then p.id else nil
		self.safe = if self.prey then world.find_light(p.position, knobs.shy) else nil
	end
	-- a light near: back off, quick
	if self.light then
		local d = math.max(flat(self.position, self.light), 0.1)
		local away = vector.create((self.position.x - self.light.x) / d, 0, (self.position.z - self.light.z) / d)
		self:walk_to(self.position + away * 4, 3.4)
		return
	end
	local p = nil
	for _, q in prey() do
		if q.id == self.prey then
			p = q
		end
	end
	if not p then
		self:wander(1.2)
		return
	end
	-- they're in the light: wait at its edge
	if self.safe then
		local d = math.max(flat(self.position, self.safe), 0.1)
		if d < knobs.shy + 2 then
			local away = vector.create((self.position.x - self.safe.x) / d, 0, (self.position.z - self.safe.z) / d)
			self:walk_to(self.position + away * 2, 2)
		else
			self:stop()
			self:face(p.position)
		end
		return
	end
	self.nip_in -= dt
	if self:distance_to(p.position) < 1.7 then
		self:face(p.position)
		if self.nip_in <= 0 then
			self.nip_in = NIP_EVERY
			self:hop(0.5)
			if knobs.bite > 0 and world.hurt(p, knobs.bite * danger().hurt) then
				sound.play("grumble", { at = self, volume = 0.7, pitch = 1.9, caption = "A gloomling nips" })
			end
		end
	else
		-- each comes at you from its own side, so they gather round you
		local side = self.side or 0
		self:walk_to(p.position + vector.create(math.cos(side), 0, math.sin(side)) * 0.9, 2.6)
	end
end

function Gloomling:hit(player, shot)
	local held = world.held(player)
	local bop = if shot then shot.power else (held and SWORDS[held] or 1)
	self.bopped = (self.bopped or 0) + bop
	if self.bopped < knobs.bops then
		-- knocked back: it hops off and keeps away a moment
		-- (a few blocks, however far off the one who bopped it stands)
		local d = self.position - player.position
		local away = self.position + d * math.min(2, 4 / math.max(vector.magnitude(d), 0.5))
		self:walk_to(away, 4)
		self:hop(0.6)
		self.back_off = 1.5
		self.nip_in = 2
		sound.play("grumble", { at = self, volume = 0.6, pitch = 2.3, caption = "A gloomling squeaks" })
		return
	end
	sound.play("pop", { at = self, pitch = 0.7, caption = "A gloomling pops" })
	-- what's left of it, for charms
	world.give(player, "Gloom dust", 1)
	where[self.id] = nil
	self:remove()
end

-- Every two seconds after dark: one more near each survival player who has
-- fewer following than the knob says, somewhere dark and dry.
local night = false
world.every(2, function()
	local dark = world.daylight() < DUSK
	if dark and not night then
		for _, p in prey() do
			ui.show(p, "night", "Night falls. Gloomlings keep away from light.", { at = "left" })
			world.after(10, function()
				ui.hide(p, "night")
			end)
		end
	end
	night = if dark then true elseif world.daylight() > DAWN then false else night
	if not dark then
		return
	end
	-- forget the ones that went
	local live = {}
	for _, id in world.creatures(Gloomling) do
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
	for _, p in prey() do
		local following = 0
		for _, at in where do
			if flat(at, p.position) < CHASE + 4 then
				following += 1
			end
		end
		if total < MOST and following < math.floor(knobs.how_many * danger().many + 0.5) and flat(p.position, green) > 30 then
			local a = world.random(0, 2 * math.pi)
			local r = world.random(NEAR, FAR)
			local x, z = p.position.x + math.cos(a) * r, p.position.z + math.sin(a) * r
			local y = world.top(x, z)
			local at = vector.create(x, y + 1.1, z)
			-- on the ground (grass and flowers are fine), not under water
			local dry = world.block(vector.create(x, y + 1, z)) ~= "Water"
			if dry and math.abs(y - p.position.y) < 10 and flat(at, green) > 30 and not world.find_light(at, knobs.shy + 2) then
				where[world.spawn(Gloomling, at)] = at
				total += 1
			end
		end
	end
end)
