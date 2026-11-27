--!strict
-- Hot air balloon. Put a Balloon basket down and it fills up; climb in
-- (Interact), hold Burner to rise and let go to sink slowly, Vent to come
-- down faster, and steer as you walk. It's a flier (`flies`, world 0.9): no
-- gravity, and `self:climb` sets how fast it rises or sinks, which it eases
-- into, so the ride is smooth. It never tips or rolls. Bop it once it's
-- landed and empty to pack it back into a basket. Balloons are kept with the
-- world (store), so a server's are still there after a restart.

local world = require("@potluck/world@0.9")
local tweak = require("@potluck/tweak")
local input = require("@potluck/input@0.1")
local ui = require("@potluck/ui@0.1")
local sound = require("@potluck/sound@0.1")
local store = require("@potluck/store")

local knobs = tweak {
	color = { "#c24b41", label = "Envelope", order = 1 },
	band = { "#efe6d2", label = "Panels", order = 2 },
	rise = { 1.5, min = 0.5, max = 3, label = "How fast the burner lifts it", order = 3 },
	sink = { 0.6, min = 0.2, max = 1.5, label = "How fast it sinks", order = 4 },
	drift = { 2, min = 0.5, max = 4, label = "How fast it drifts", order = 5 },
}

local BASKET = "Balloon basket" -- the block that becomes a balloon
local WICKER = "#9a7038"
local IRON = "#3a3a3e"

-- the basket: a floor, four walls to stand between, and a rim
local function basket(): { any }
	return {
		{ "floor strap", size = { 1.3, 0.12, 1.3 }, at = { 0, 0.06, 0 }, color = WICKER },
		{ "wall strap front", size = { 1.3, 1.0, 0.1 }, at = { 0, 0.5, -0.6 }, color = WICKER },
		{ "wall strap back", size = { 1.3, 1.0, 0.1 }, at = { 0, 0.5, 0.6 }, color = WICKER },
		{ "wall strap left", size = { 0.1, 1.0, 1.1 }, at = { -0.6, 0.5, 0 }, color = WICKER },
		{ "wall strap right", size = { 0.1, 1.0, 1.1 }, at = { 0.6, 0.5, 0 }, color = WICKER },
	}
end

-- four ropes up from the basket's corners, and the burner between them
local function rigging(lit: boolean): { any }
	local parts = {}
	for _, c in { { -1, -1 }, { 1, -1 }, { -1, 1 }, { 1, 1 } } do
		table.insert(parts, { "rope strap", size = { 0.05, 1.9, 0.05 }, at = { c[1] * 0.6, 1.95, c[2] * 0.6 }, color = "#c2a277" })
	end
	table.insert(parts, { "burner plate", size = { 0.5, 0.14, 0.5 }, at = { 0, 2.45, 0 }, color = IRON })
	if lit then
		table.insert(parts, { "flame glow", size = { 0.26, 0.5, 0.26 }, at = { 0, 2.8, 0 }, color = "#ffb347", glow = true })
	else
		table.insert(parts, { "pilot glow", size = { 0.1, 0.12, 0.1 }, at = { 0, 2.58, 0 }, color = "#6fa8ff", glow = true })
	end
	return parts
end

-- the envelope, in tiers from its mouth up, each a cross of two boxes so
-- its corners look rounded: one box in each colour, so from any side a band
-- runs down the middle like a balloon's panels
local function envelope(tiers: { { number } }): { any }
	local parts = {}
	for _, t in tiers do
		local y, w, h = t[1], t[2], t[3]
		table.insert(parts, { "cloth", size = { w, h, w * 0.6 }, at = { 0, y, 0 }, color = knobs.color })
		table.insert(parts, { "cloth", size = { w * 0.6, h, w }, at = { 0, y, 0 }, color = knobs.band })
	end
	return parts
end

local function join(...: { any }): { any }
	local all = {}
	for _, list in { ... } do
		for _, p in list do
			table.insert(all, p)
		end
	end
	return all
end

local FULL = {
	{ 3.1, 1.3, 0.5 },
	{ 3.7, 2.6, 0.7 },
	{ 4.5, 3.8, 0.9 },
	{ 5.45, 4.5, 1.0 },
	{ 6.45, 4.6, 1.0 },
	{ 7.35, 4.0, 0.8 },
	{ 7.95, 2.8, 0.4 },
}
local HALF = {
	{ 3.0, 1.3, 0.4 },
	{ 3.5, 3.0, 0.6 },
	{ 4.1, 3.6, 0.6 },
	{ 4.6, 2.6, 0.4 },
}

local SIZE = { 1.4, 4, 1.4 }

-- just put down: the envelope heaped beside the basket
local Empty = world.creature {
	name = "Balloon (empty)",
	count = 0,
	speed = 0,
	size = SIZE,
	body = join(basket(), rigging(false), {
		{ "cloth heap", size = { 2.2, 0.35, 1.3 }, at = { 1.85, 0.18, 0 }, color = knobs.color },
		{ "cloth heap", size = { 1.2, 0.25, 1.0 }, at = { 1.7, 0.45, 0 }, color = knobs.band },
	}),
}

-- the burner's filling it
local Filling = world.creature {
	name = "Balloon (filling)",
	count = 0,
	speed = 0,
	size = SIZE,
	body = join(basket(), rigging(true), envelope(HALF)),
}

local Balloon = world.creature {
	name = "Hot air balloon",
	count = 0,
	speed = knobs.drift,
	rideable = true,
	seat = 0.7,
	flies = true,
	size = SIZE,
	body = join(basket(), rigging(false), envelope(FULL)),
}

-- the same with the burner roaring
local Burning = world.creature {
	name = "Hot air balloon (burner on)",
	count = 0,
	speed = knobs.drift,
	rideable = true,
	seat = 0.7,
	flies = true,
	size = SIZE,
	body = join(basket(), rigging(true), envelope(FULL)),
}

-- where every balloon is (each one says, every tick), kept with the world
local saved: { vector } = store.get("balloons") or {}
local where: { [number]: vector } = {}

local function save()
	local list = {}
	for _, at in where do
		table.insert(list, at)
	end
	store.set("balloons", list)
end

local function roar(self)
	sound.play("whoosh", { at = self, pitch = 0.55, volume = 0.6, caption = "A burner roars" })
end

-- ---- filling up ----

function Empty:spawned()
	self.filled = 0
end

local function filling(self, dt: number)
	where[self.id] = self.position
	self.filled = (self.filled or 0) + dt
	if self.filled >= 1.2 and self.kind == Empty.name then
		self:become(Filling)
		roar(self)
	elseif self.filled >= 3.2 and self.kind == Filling.name then
		self:become(Balloon)
		self:say("Ready! Climb in.")
		sound.play("chime", { at = self, caption = "The balloon's full" })
		save()
	elseif self.kind == Filling.name and math.floor(self.filled * 2) ~= math.floor((self.filled - dt) * 2) then
		roar(self)
	end
end
Empty.tick = filling
Filling.tick = filling

local function not_yet(self, player)
	ui.show(player, "wait", "It's still filling up.", { at = "bottom" })
	world.after(2, function()
		ui.hide(player, "wait")
	end)
end
Empty.used = not_yet
Filling.used = not_yet

-- ---- flying ----

local function fly(self, dt: number)
	where[self.id] = self.position
	local r = self.rider
	local burning = r ~= nil and input.held("burner", r)
	if burning then
		self:climb(knobs.rise)
	elseif r and input.held("vent", r) then
		self:climb(-math.min(knobs.sink * 3, 3))
	else
		-- let go, or nobody aboard: it settles gently back down
		self:climb(-knobs.sink)
	end
	local lit = self.kind == Burning.name
	if burning ~= lit then
		self:become(if burning then Burning else Balloon)
	end
	if burning then
		self.roar_in = (self.roar_in or 0) - dt
		if self.roar_in <= 0 then
			self.roar_in = 0.6
			roar(self)
		end
	end
	-- now and then, where it is now
	self.save_in = (self.save_in or 0) - dt
	if self.save_in <= 0 then
		self.save_in = 5
		save()
	end
end
Balloon.tick = fly
Burning.tick = fly

local function aboard(self, player)
	ui.show(player, "aboard", "Hold Burner to rise, let go to sink. Land to get out.", { at = "bottom" })
	world.after(6, function()
		ui.hide(player, "aboard")
	end)
end
Balloon.ridden = aboard
Burning.ridden = aboard

-- bop it once it's down and empty to pack it up
local function pack(self, player)
	if self.rider then
		return
	end
	if not self:on_ground() then
		self:say("Let it land first.")
		return
	end
	if world.pockets(player) ~= nil and world.give(player, BASKET, 1) == 0 then
		ui.show(player, "full", "Make room in your hotbar to pack it up.", { at = "bottom" })
		world.after(3, function()
			ui.hide(player, "full")
		end)
		return
	end
	sound.play("thud", { at = self, caption = "Packed up" })
	where[self.id] = nil
	self:remove()
	save()
end
Empty.hit = pack
Filling.hit = pack
Balloon.hit = pack
Burning.hit = pack

-- ---- the basket ----

world.on("block_placed", function(player, at: vector, block: string)
	if block ~= BASKET then
		return
	end
	world.set_block(at, "Air")
	if not pcall(world.spawn, Empty, at - vector.create(0, 0.5, 0)) then
		-- too many balloons: back to the hotbar it goes
		world.give(player, BASKET, 1)
	end
end)

-- after a restart, the world's balloons come back full (a reload keeps them)
world.after(0.1, function()
	local here = 0
	for _, kind in { Empty, Filling, Balloon, Burning } do
		here += #world.creatures(kind)
	end
	if here == 0 then
		for _, at in saved do
			pcall(world.spawn, Balloon, at)
		end
	end
end)

