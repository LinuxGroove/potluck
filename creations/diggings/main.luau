--!strict
-- Gnome Diggings: long ago the Gnome Gardeners' people dug into the hills.
-- When a player comes near a steep hillside far from the Green, and far
-- from any other diggings, a timbered tunnel opens in it: a level gallery
-- with old cart tracks, a slope down into the rock, a hall with a forge
-- room and a store room off it, and a long dark drift beyond. Where it's
-- dug is kept with @potluck/store, so each is dug once.
--
-- Burnt-out lanterns hang along the way; use one with a piece of coal and
-- it lights again. Gnome crates hold what the diggers left (PRIZES below,
-- by room), better the deeper in: tools, ore, ingots, seeds, the diggers'
-- own pick (an item of this creation's, world 0.25, that digs as a copper
-- pickaxe and never wears out), a brass compass whose needle points to the
-- nearest diggings, and torn maps. Use a torn map on nothing to see where
-- its cross is; walk there and a crate is buried under your feet.
--
-- Only natural ground is dug: a hillside with anything built in it, or
-- water, is left alone.

local world = require("@potluck/world@0.25")
local tweak = require("@potluck/tweak")
local store = require("@potluck/store")
local ui = require("@potluck/ui@0.2")
local sound = require("@potluck/sound@0.2")

local knobs = tweak {
	apart = { 300, min = 120, max = 1000, label = "How far apart diggings are", order = 1 },
	away = { 90, min = 40, max = 300, label = "How far from the Green the nearest is", order = 2 },
	prizes = { 1, min = 0.5, max = 3, label = "How generous the crates are", order = 3 },
	maps = { true, label = "Torn maps lead to buried crates", order = 4 },
}

local CRATE = "Gnome crate"
local DARK = "Burnt-out lantern"
local SEA = 19
local MOST = 64 -- diggings a world keeps track of

-- the four ways a tunnel can run into a hill: east, south, west, north
local DIRS = { vector.create(1, 0, 0), vector.create(0, 0, 1), vector.create(-1, 0, 0), vector.create(0, 0, -1) }

-- what a tunnel may be dug through
local NATURAL: { [string]: boolean } = {
	Stone = true, Dirt = true, Grass = true, Gravel = true, Sand = true, Snow = true, Slate = true, Sandstone = true,
	["Coal ore"] = true, ["Copper ore"] = true, ["Iron ore"] = true, ["Gold ore"] = true, ["Crystal ore"] = true, ["Moonsilver ore"] = true,
	Glowcap = true, ["Tall grass"] = true, Fern = true, Bush = true, ["Dry bush"] = true,
	["Red flower"] = true, ["Blue flower"] = true, ["White flower"] = true, ["Violet flower"] = true,
}

-- ---- what's in the crates ----

-- A prize: the thing, its chance in a crate, and from least to most of it.
type Prize = { thing: string, chance: number, least: number, most: number }
local PRIZES: { [string]: { Prize } } = {
	-- the store room: what a digger lived on
	store = {
		{ thing = "Seeds", chance = 1, least = 3, most = 8 },
		{ thing = "Coal", chance = 0.8, least = 2, most = 6 },
		{ thing = "Torch", chance = 0.7, least = 2, most = 6 },
		{ thing = "Bread", chance = 0.4, least = 1, most = 2 },
		{ thing = "Potatoes", chance = 0.3, least = 1, most = 3 },
		{ thing = "Raw copper", chance = 0.5, least = 1, most = 4 },
		{ thing = "Raw iron", chance = 0.35, least = 1, most = 3 },
		{ thing = "Torn map", chance = 0.5, least = 1, most = 1 },
	},
	-- the forge: metal and tools
	forge = {
		{ thing = "Copper ingot", chance = 0.8, least = 1, most = 4 },
		{ thing = "Iron ingot", chance = 0.6, least = 1, most = 3 },
		{ thing = "Copper pickaxe", chance = 0.35, least = 1, most = 1 },
		{ thing = "Hammer", chance = 0.3, least = 1, most = 1 },
		{ thing = "Bucket", chance = 0.3, least = 1, most = 1 },
		{ thing = "Iron pickaxe", chance = 0.12, least = 1, most = 1 },
		{ thing = "Gnome's pick", chance = 0.2, least = 1, most = 1 },
	},
	-- the end of the drift, deepest in: the best of it
	deep = {
		{ thing = "Brass compass", chance = 1, least = 1, most = 1 },
		{ thing = "Gold ingot", chance = 0.7, least = 1, most = 3 },
		{ thing = "Crystal", chance = 0.6, least = 1, most = 3 },
		{ thing = "Iron sword", chance = 0.3, least = 1, most = 1 },
		{ thing = "Gnome's pick", chance = 0.3, least = 1, most = 1 },
		{ thing = "Moonsilver ingot", chance = 0.12, least = 1, most = 1 },
		{ thing = "Torn map", chance = 0.6, least = 1, most = 1 },
	},
	-- buried where a torn map's cross is
	buried = {
		{ thing = "Gold ingot", chance = 0.8, least = 2, most = 5 },
		{ thing = "Iron ingot", chance = 0.7, least = 2, most = 5 },
		{ thing = "Crystal", chance = 0.5, least = 1, most = 3 },
		{ thing = "Moonsilver ingot", chance = 0.25, least = 1, most = 2 },
		{ thing = "Brass compass", chance = 0.3, least = 1, most = 1 },
		{ thing = "Torn map", chance = 0.35, least = 1, most = 1 },
	},
}
-- XP for opening a crate, by where it was
local XP: { [string]: number } = { store = 3, forge = 5, deep = 12, buried = 10 }

-- ---- what's kept ----

-- Each diggings: its mouth (the ground block at the foot of the hill),
-- which way it runs in (DIRS) and whether it's all dug yet.
type Site = { at: vector, dir: number, built: boolean }
local sites: { Site } = store.get("sites") or {}
-- crates this creation put down that are still shut, by place: which
-- PRIZES they hold
local crates: { [string]: string } = store.get("crates") or {}
-- each player's torn map, by their name: where its cross is
local crosses: { [string]: vector } = store.get("crosses") or {}

local function save()
	store.set("sites", sites)
	store.set("crates", crates)
	store.set("crosses", crosses)
end

local function key(at: vector): string
	return string.format("%d,%d,%d", math.floor(at.x), math.floor(at.y), math.floor(at.z))
end

-- Whether a thing is in this world: rails, furnaces and anvils come from
-- other creations.
local here: { [string]: boolean } = {}
local function exists(name: string): boolean
	if here[name] == nil then
		here[name] = (pcall(world.kind, name))
	end
	return here[name] == true
end

-- the same pattern every time, for a place
local function hash(x: number, z: number, n: number): number
	local h = (math.floor(x) * 73856093 + math.floor(z) * 19349663 + n * 83492791) % 1000003
	return (h * 7919 % 1000003) / 1000003
end

-- one line above the hotbar for a few seconds
local shown: { [number]: number } = {}
local function tell(player, text: string)
	ui.show(player, "diggings", text, { at = "bottom" })
	local mine = (shown[player.id] or 0) + 1
	shown[player.id] = mine
	world.after(4, function()
		if shown[player.id] == mine then
			shown[player.id] = nil
			ui.hide(player, "diggings")
		end
	end)
end

-- "north-east", from a step across the ground (north is -z)
local function way(dx: number, dz: number): string
	local a = math.deg(math.atan2(dx, -dz)) % 360
	local names = { "north", "north-east", "east", "south-east", "south", "south-west", "west", "north-west" }
	return names[math.floor((a + 22.5) / 45) % 8 + 1]
end

-- ---- a place in a diggings ----

-- by how far across (u, to the right going in), up (h) and in (v)
local function spot(s: Site, u: number, h: number, v: number): vector
	local f = DIRS[s.dir]
	local r = vector.create(-f.z, 0, f.x)
	return s.at + r * u + f * v + vector.create(0, h, 0)
end

-- The floor's height going in: level for the gallery, then down a block
-- every two to the hall, eight below the mouth.
local GALLERY, SLOPE_TO, HALL_TO, DRIFT_TO = 10, 26, 34, 52
local function floor_at(v: number): number
	if v <= GALLERY then
		return 0
	end
	return -math.min(math.floor((v - GALLERY + 1) / 2), 8)
end

-- what grows on the ground rather than being it
local PLANT: { [string]: boolean } = {
	Glowcap = true, ["Tall grass"] = true, Fern = true, Bush = true, ["Dry bush"] = true,
	["Red flower"] = true, ["Blue flower"] = true, ["White flower"] = true, ["Violet flower"] = true,
}

-- The ground at a column, under trees and plants (or the water's top).
local function ground(x: number, z: number): number
	local top = world.top(x, z)
	for y = top, top - 16, -1 do
		local b = world.block(vector.create(x, y, z))
		if b == "Water" or (NATURAL[b] and not PLANT[b]) then
			return y
		end
	end
	return top
end

-- ---- finding a hillside ----

-- Whether a diggings can go here, running in `dir`: a steep hill ahead,
-- and only natural rock where it'll be dug (a few cave holes are fine).
local function fits(at: vector, dir: number): boolean
	local s: Site = { at = at, dir = dir, built = false }
	local f = DIRS[dir]
	local rise = ground(at.x + f.x * 10, at.z + f.z * 10) - at.y
	local beyond = ground(at.x + f.x * 20, at.z + f.z * 20) - at.y
	if rise < 7 or beyond < 5 then
		return false
	end
	local holes = 0
	local checks = {
		{ 0, 2, 7 }, { 0, 2, 10 }, { 0, -1, 16 }, { 0, -4, 22 }, { 0, -6, 28 }, { -3, -6, 30 }, { 3, -6, 30 },
		{ -8, -6, 30 }, { 8, -6, 30 }, { 0, -6, 40 }, { 0, -6, DRIFT_TO }, { 0, 0, 36 },
	}
	for _, c in checks do
		local b = world.block(spot(s, c[1], c[2], c[3]))
		if b == "Air" then
			holes += 1
		elseif not NATURAL[b] then
			return false
		end
	end
	return holes <= 3
end

-- A foot of a hill near a player, or nil.
local function look_near(p: vector): Site?
	for _ = 1, 6 do
		local a = world.random() * math.pi * 2
		local d = world.random(24, 56)
		local x, z = math.floor(p.x + math.cos(a) * d), math.floor(p.z + math.sin(a) * d)
		local y = ground(x, z)
		local b = world.block(vector.create(x, y, z))
		if y > SEA + 1 and NATURAL[b] and world.block(vector.create(x, y + 1, z)) ~= "Water" then
			local first = math.floor(world.random(1, 4.999))
			for i = 0, 3 do
				local dir = (first + i - 1) % 4 + 1
				if fits(vector.create(x, y, z), dir) then
					return { at = vector.create(x, y, z), dir = dir, built = false }
				end
			end
		end
	end
	return nil
end

local function far_from_others(p: vector, apart: number): boolean
	for _, s in sites do
		if vector.magnitude(vector.create(p.x - s.at.x, 0, p.z - s.at.z)) < apart then
			return false
		end
	end
	return true
end

-- ---- digging one ----

-- what's left to put down, a few hundred a tick: { place, block, facing }
local work: { { any } } = {}
local done = 0 -- how many of work are down
local digging: Site? = nil

local function put(s: Site, u: number, h: number, v: number, name: string, facing: string?)
	table.insert(work, { spot(s, u, h, v), name, facing })
end

-- air, but only where it's natural ground (a cave stays a cave)
local function carve(s: Site, u: number, h: number, v: number)
	table.insert(work, { spot(s, u, h, v), "" })
end

-- a crate that holds a room's prizes
local function crate(s: Site, u: number, h: number, v: number, room: string)
	local at = spot(s, u, h, v)
	table.insert(work, { at, CRATE })
	crates[key(at)] = room
end

-- Posts on each side, a beam across, and now and then a lantern under it.
local function timbers(s: Site, v: number, half: number, tall: number, lantern: boolean)
	local y = floor_at(v)
	for _, u in { -half, half } do
		for h = 1, tall do
			put(s, u, y + h, v, "Oak log")
		end
	end
	for u = -half, half do
		put(s, u, y + tall + 1, v, "Planks")
	end
	if lantern then
		put(s, 0, y + tall, v, DARK)
	end
end

-- a room's air, from u0 to u1 and v0 to v1, `tall` high over floor y
local function room(s: Site, u0: number, u1: number, v0: number, v1: number, y: number, tall: number)
	for v = v0, v1 do
		for u = u0, u1 do
			for h = 1, tall do
				carve(s, u, y + h, v)
			end
		end
	end
end

-- Lays out everything one diggings is, to be put down a tick at a time.
local function plan(s: Site)
	-- the mouth: a frame of logs in the hillside
	room(s, -2, 2, 0, 0, 0, 4)
	for _, u in { -2, 2 } do
		for h = 1, 4 do
			put(s, u, h, 0, "Oak log")
		end
	end
	for u = -2, 2 do
		put(s, u, 5, 0, "Planks")
	end
	-- the gallery and the slope: five wide and four high, timbered every four
	for v = 1, SLOPE_TO do
		local y = floor_at(v)
		room(s, -2, 2, v, v, y, 4)
		-- a boardwalk over any cave it crosses
		for u = -1, 1 do
			table.insert(work, { spot(s, u, y, v), "?Planks" })
		end
		if v % 4 == 0 then
			timbers(s, v, 2, 4, v % 8 == 0)
		end
		-- old cart tracks along the gallery, a few rails gone
		local along = if s.dir % 2 == 1 then "east" else "north"
		if v <= GALLERY and exists("Rail") and hash(spot(s, 0, 0, v).x, spot(s, 0, 0, v).z, 3) > 0.25 then
			put(s, 0, 1, v, "Rail", along)
		end
	end
	-- the hall: nine wide and five high, with a lantern
	local y = floor_at(SLOPE_TO + 1)
	room(s, -4, 4, SLOPE_TO + 1, HALL_TO, y, 5)
	for _, c in { { -4, SLOPE_TO + 1 }, { 4, SLOPE_TO + 1 }, { -4, HALL_TO }, { 4, HALL_TO } } do
		for h = 1, 5 do
			put(s, c[1], y + h, c[2], "Oak log")
		end
	end
	for u = -4, 4 do
		put(s, u, y + 6, SLOPE_TO + 4, "Planks")
	end
	put(s, 0, y + 5, SLOPE_TO + 4, DARK)
	put(s, 3, y + 1, HALL_TO - 1, "Barrel")
	put(s, -3, y + 1, SLOPE_TO + 2, "Gravel")
	put(s, -3, y + 2, SLOPE_TO + 2, "Gravel")
	-- the forge, off to the left
	local mid = SLOPE_TO + 4
	room(s, -5, -5, mid - 1, mid + 1, y, 3)
	room(s, -11, -6, mid - 3, mid + 3, y, 4)
	put(s, -11, y + 1, mid, if exists("Furnace") then "Furnace" else "Cobblestone")
	put(s, -11, y + 1, mid - 1, "Cobblestone")
	put(s, -11, y + 1, mid + 1, "Cobblestone")
	put(s, -9, y + 1, mid + 2, if exists("Anvil") then "Anvil" else "Cobblestone")
	put(s, -7, y + 1, mid - 3, "Table")
	put(s, -6, y + 1, mid - 3, "Stool")
	put(s, -8, y + 4, mid, DARK)
	crate(s, -11, y + 1, mid - 3, "forge")
	crate(s, -11, y + 1, mid + 3, "forge")
	-- the store room, off to the right
	room(s, 5, 5, mid - 1, mid + 1, y, 3)
	room(s, 6, 11, mid - 3, mid + 3, y, 4)
	for i, v in { mid - 2, mid, mid + 2 } do
		crate(s, 11, y + 1, v, "store")
		put(s, 11, y + 1, v + 1, if i == 2 then "Barrel" else "Crate")
	end
	put(s, 7, y + 1, mid + 3, "Bookshelf")
	put(s, 8, y + 1, mid + 3, "Barrel")
	put(s, 6, y + 1, mid - 3, "Clay pot")
	put(s, 8, y + 4, mid, DARK)
	-- the drift: three wide, three high, dark all the way to its end
	for v = HALL_TO + 1, DRIFT_TO do
		room(s, -1, 1, v, v, y, 3)
		for u = -1, 1 do
			table.insert(work, { spot(s, u, y, v), "?Planks" })
		end
		if v % 5 == 0 then
			timbers(s, v, 1, 3, false)
		end
		-- a fall of rock to climb over
		if v == HALL_TO + 9 then
			put(s, -1, y + 1, v, "Gravel")
			put(s, 1, y + 1, v, "Gravel")
		end
	end
	room(s, -2, 2, DRIFT_TO - 3, DRIFT_TO, y, 3)
	crate(s, 0, y + 1, DRIFT_TO, "deep")
	put(s, -2, y + 1, DRIFT_TO, "Crate")
	put(s, 2, y + 1, DRIFT_TO, "Barrel")
end

-- Puts down what's planned, a few hundred blocks a tick.
local function step()
	local last = math.min(done + 250, #work)
	for i = done + 1, last do
		local w = work[i]
		local at, name, facing = w[1] :: vector, w[2] :: string, w[3] :: string?
		local b = world.block(at)
		if name == "" then
			if NATURAL[b] then
				world.set_block(at, "Air")
			end
		elseif string.sub(name, 1, 1) == "?" then
			-- only over a hole
			if b == "Air" or b == "Water" then
				world.set_block(at, string.sub(name, 2))
			end
		elseif NATURAL[b] or b == "Air" then
			world.set_block(at, name, facing)
		end
	end
	done = last
	if done >= #work then
		work, done = {}, 0
		local d = digging
		if d then
			d.built = true
			digging = nil
			save()
		end
	end
end

-- ---- watching players ----

local turn = 0
world.every(0.1, function()
	if #work > 0 then
		step()
		return
	end
	-- one player a few times a second, to keep the looking light
	turn += 1
	if turn % 20 ~= 0 then
		return
	end
	local players = world.players()
	if #players == 0 or #sites >= MOST then
		return
	end
	local p = players[(turn // 20) % #players + 1]
	for _, name in { "green", "start" } do
		local at = world.spot(name)
		if at and vector.magnitude(vector.create(p.position.x - at.x, 0, p.position.z - at.z)) < knobs.away then
			return
		end
	end
	if not far_from_others(p.position, knobs.apart) then
		return
	end
	local s = look_near(p.position)
	if s and far_from_others(s.at, knobs.apart) then
		table.insert(sites, s)
		digging = s
		plan(s)
		save()
	end
end)

-- a diggings left half dug (the world stopped mid-way) is dug on
for _, s in sites do
	if not s.built then
		digging = s
		plan(s)
		break
	end
end

-- ---- the crates ----

type Find = { thing: string, n: number }
-- what's still in crates players' pockets were too full for, by place
local left: { [string]: { Find } } = {}

local function roll(room: string): { Find }
	local out: { Find } = {}
	for _, p in PRIZES[room] or {} do
		if exists(p.thing) and world.random() < math.min(1, p.chance * knobs.prizes) then
			local n = math.floor(world.random(p.least, p.most + 0.999) * knobs.prizes)
			table.insert(out, { thing = p.thing, n = math.clamp(n, 1, 64) })
		end
	end
	if #out == 0 then
		table.insert(out, { thing = "Coal", n = 3 })
	end
	return out
end

-- "You found 2 raw gold, 1 crystal and 3 glass.": as many as fit on a line
-- (60 letters), then "and more"
local function found(finds: { Find }): string
	local parts = {}
	for _, f in finds do
		table.insert(parts, f.n .. " " .. string.lower(f.thing))
	end
	for n = #parts, 1, -1 do
		local line = if n < #parts
			then table.concat(parts, ", ", 1, n) .. " and more"
			elseif n == 1 then parts[1]
			else table.concat(parts, ", ", 1, n - 1) .. " and " .. parts[n]
		line = "You found " .. line .. "."
		if #line <= 60 or n == 1 then
			return line
		end
	end
	return "You found something."
end

local function open(player, at: vector)
	local k = key(at)
	local room = crates[k]
	if not room then
		tell(player, "It's empty.")
		return
	end
	if world.pockets(player) == nil then
		tell(player, "Gnome crates hold prizes for survival.")
		return
	end
	local finds = left[k] or roll(room)
	local got: { Find } = {}
	local still: { Find } = {}
	for _, f in finds do
		local n = world.give(player, f.thing, f.n)
		if n > 0 then
			table.insert(got, { thing = f.thing, n = n })
		end
		if n < f.n then
			table.insert(still, { thing = f.thing, n = f.n - n })
		end
	end
	if #still > 0 then
		left[k] = still
		tell(player, "Your pockets are full.")
		return
	end
	left[k] = nil
	crates[k] = nil
	save()
	world.reward(player, XP[room] or 3)
	world.set_block(at, "Air")
	sound.play("sparkle", { at = at, caption = "A gnome crate opens" })
	tell(player, found(got))
end

-- a burnt-out lantern lights again with a piece of coal
local function relight(player, at: vector)
	if world.pockets(player) ~= nil then
		if world.has(player, "Coal") < 1 then
			tell(player, "A piece of coal would light it again.")
			return
		end
		world.take(player, "Coal", 1)
	end
	world.set_block(at, "Lantern")
	sound.play("click", { at = at, caption = "A lantern's lit" })
end

world.on("block_used", function(player, at: vector, block: string)
	if block == CRATE then
		open(player, at)
	elseif block == DARK then
		relight(player, at)
	end
end)

-- a crate broken open gives nothing, and isn't a crate to put down again
world.drops(CRATE, { gives = false })
world.on("block_broken", function(player, at: vector, block: string)
	if block == CRATE and crates[key(at)] then
		crates[key(at)] = nil
		left[key(at)] = nil
		save()
	end
end)

-- ---- torn maps and the compass ----

local function read_map(player)
	if not knobs.maps then
		tell(player, "The map's too torn to read.")
		return
	end
	local cross = crosses[player.name]
	if not cross then
		-- a cross somewhere out on the land, a walk away
		local a = world.random() * math.pi * 2
		local d = world.random(60, 140)
		cross = vector.create(math.floor(player.position.x + math.cos(a) * d), 0, math.floor(player.position.z + math.sin(a) * d))
		crosses[player.name] = cross
		save()
	end
	local dx, dz = cross.x - player.position.x, cross.z - player.position.z
	local d = math.floor(math.sqrt(dx * dx + dz * dz))
	if d <= 6 then
		tell(player, "The cross is right about here.")
	else
		tell(player, "The map shows a cross about " .. d .. " blocks to the " .. way(dx, dz) .. ".")
	end
end

local function read_compass(player)
	local best: Site? = nil
	local best_d = math.huge
	for _, s in sites do
		local d = vector.magnitude(vector.create(s.at.x - player.position.x, 0, s.at.z - player.position.z))
		if d < best_d then
			best, best_d = s, d
		end
	end
	if not best then
		tell(player, "The needle spins. There are no diggings yet.")
	elseif best_d < 12 then
		tell(player, "The needle points straight down. You're at the diggings.")
	else
		local b = best :: Site
		tell(player, "The needle points " .. way(b.at.x - player.position.x, b.at.z - player.position.z) .. ", about " .. math.floor(best_d) .. " blocks.")
	end
end

world.on("item_used", function(player, item: string?, aim: vector)
	if item == "Torn map" then
		read_map(player)
	elseif item == "Brass compass" then
		read_compass(player)
	end
end)

-- At the cross, a crate is buried under their feet (and the map's used up).
world.every(1, function()
	for _, p in world.players() do
		local cross = crosses[p.name]
		if cross then
			local dx, dz = cross.x - p.position.x, cross.z - p.position.z
			if dx * dx + dz * dz <= 16 then
				local x, z = math.floor(p.position.x), math.floor(p.position.z)
				local y = ground(x, z)
				crosses[p.name] = nil
				world.take(p, "Torn map", 1)
				if world.block(vector.create(x, y + 1, z)) == "Water" or y <= SEA - 1 then
					save()
					tell(p, "The cross is in the water. The map's older than the shore.")
					continue
				end
				local at = vector.create(x, y - 2, z)
				world.set_block(vector.create(x, y - 1, z), "Dirt")
				world.set_block(at, CRATE)
				crates[key(at)] = "buried"
				save()
				sound.play("chime", { at = p.position, caption = "This is the place on the map" })
				tell(p, "This is the place on the map. Dig down!")
			end
		end
	end
end)

world.on("player_left", function(player)
	shown[player.id] = nil
end)
