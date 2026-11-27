--!strict
-- Old Greens: three ruined greens around the island, a reason to go far.
-- When a world starts it looks for a place for each (a beach, a wood and a
-- mountainside, far from the Green), lays a broken plaza with an arch and a
-- cold hearth there, and remembers where with @potluck/store so they're
-- built only once. Use a hearth with what it wants (a lantern and something
-- from round about) and it's lit again: braziers burn, the arch is mended,
-- and it gives you something back. Every Old Green is in GREENS below, so a
-- remix can add its own, move them or change what they want.

local world = require("@potluck/world@0.5")
local tweak = require("@potluck/tweak")
local ui = require("@potluck/ui@0.1")
local sound = require("@potluck/sound@0.2")
local store = require("@potluck/store")

local knobs = tweak {
	far = { 90, min = 40, max = 160, label = "How far from the Green they lie", order = 1 },
	hint = { true, label = "Say what a hearth wants near it", order = 2 },
}

type Green = {
	id: string,
	name: string,
	-- what its hearth wants, besides the lantern
	block: string,
	count: number,
	-- what it gives back when it's lit
	gift: string,
	gifts: number,
	-- what its stones are
	stone: string,
	-- whether a place will do, and how good it is
	fits: (x: number, y: number, z: number) -> boolean,
}

local SEA = 19
local HEARTH = "Clay pot" -- a cold hearth
local FIRE = "Brazier" -- a lit one

local function block(x: number, y: number, z: number): string
	return world.block(vector.create(x, y, z))
end

-- the ground, under any tree trunk or bush standing on it
local STANDING = { ["Oak log"] = true, ["Birch log"] = true, Bush = true }
local function ground(x: number, z: number): number
	local y = world.top(x, z)
	while y > 1 and STANDING[block(x, y, z)] do
		y -= 1
	end
	return y
end

local LEAVES = { ["Oak leaves"] = true, ["Birch leaves"] = true, ["Pine needles"] = true, ["Oak log"] = true, ["Birch log"] = true }

local GREENS: { Green } = {
	{
		id = "heights",
		name = "the Old Green on the heights",
		block = "Stone",
		count = 6,
		gift = "Stone pickaxe",
		gifts = 1,
		stone = "Cobblestone",
		fits = function(x, y, z)
			local b = block(x, y, z)
			return y >= 52 and (b == "Stone" or b == "Snow" or b == "Grass")
		end,
	},
	{
		id = "shore",
		name = "the Old Green by the shore",
		block = "Sand",
		count = 6,
		gift = "Balloon basket",
		gifts = 1,
		stone = "Sandstone bricks",
		fits = function(x, y, z)
			if block(x, y, z) ~= "Sand" or y < SEA or block(x, y + 1, z) ~= "Air" then
				return false
			end
			-- the sea's near
			for _, d in { { 9, 0 }, { -9, 0 }, { 0, 9 }, { 0, -9 } } do
				if block(x + d[1], SEA, z + d[2]) == "Water" then
					return true
				end
			end
			return false
		end,
	},
	{
		id = "woods",
		name = "the Old Green in the woods",
		block = "Oak log",
		count = 4,
		gift = "Stone axe",
		gifts = 1,
		stone = "Mossy brick",
		fits = function(x, y, z)
			if block(x, y, z) ~= "Grass" then
				return false
			end
			-- trees all round
			local trees = 0
			for i = 0, 7 do
				local a = i * math.pi / 4
				local tx, tz = math.floor(x + math.cos(a) * 7), math.floor(z + math.sin(a) * 7)
				local g = ground(tx, tz)
				if LEAVES[block(tx, g + 1, tz)] or LEAVES[block(tx, g + 4, tz)] or LEAVES[block(tx, g + 6, tz)] then
					trees += 1
				end
			end
			return trees >= 4
		end,
	},
}

local BY_ID: { [string]: Green } = {}
for _, g in GREENS do
	BY_ID[g.id] = g
end

-- what a hearth wants, said plainly
local function wants(g: Green): string
	return "1 Lantern and " .. g.count .. " " .. g.block
end

-- the same pattern of broken stones every time, for a place
local function hash(x: number, z: number, n: number): number
	local h = (x * 73856093 + z * 19349663 + n * 83492791) % 1000003
	return (h * 7919 % 1000003) / 1000003
end

-- ---- finding places ----

-- Each found one: its kind, its middle, which way its arch faces, whether
-- it's built yet and whether it's lit.
type Site = { id: string, at: vector, along_x: boolean, built: boolean, lit: boolean }

local sites: { Site } = store.get("sites") or {}

local function save()
	store.set("sites", sites)
end

local NATURAL = { Grass = true, Sand = true, Stone = true, Snow = true, Dirt = true, Gravel = true }

-- how level a 9 by 9 is round x, z, and whether it's all natural ground
local function level(x: number, z: number): (number?, number)
	local lo, hi = math.huge, -math.huge
	for dz = -4, 4, 4 do
		for dx = -4, 4, 4 do
			local y = ground(x + dx, z + dz)
			if not NATURAL[block(x + dx, y, z + dz)] then
				return nil, 0
			end
			lo, hi = math.min(lo, y), math.max(hi, y)
		end
	end
	return hi - lo, ground(x, z)
end

local STEP = 6
local EDGE = 26
local SIZE = 384
local points: { { number } } = {}
for z = EDGE, SIZE - EDGE, STEP do
	for x = EDGE, SIZE - EDGE, STEP do
		table.insert(points, { x, z })
	end
end
-- the best few of each kind so far: { score, x, y, z }
local found: { [string]: { { number } } } = {}
for _, g in GREENS do
	found[g.id] = {}
end

local function consider(x: number, z: number)
	local green = world.spot("green")
	local start = world.spot("start")
	local away = vector.magnitude(vector.create(x - green.x, 0, z - green.z))
	if away < knobs.far or vector.magnitude(vector.create(x - start.x, 0, z - start.z)) < 40 then
		return
	end
	local spread, y = level(x, z)
	if spread == nil then
		return
	end
	for _, g in GREENS do
		local most = if g.id == "heights" then 7 else 3
		if spread <= most and g.fits(x, y, z) then
			local list = found[g.id]
			table.insert(list, { math.min(away, 160) - spread * 8, x, y, z })
			table.sort(list, function(a, b)
				return a[1] > b[1]
			end)
			if #list > 24 then
				table.remove(list)
			end
		end
	end
end

-- the best of each kind, well apart from each other
local function choose()
	local chosen: { Site } = {}
	for _, g in GREENS do
		for _, c in found[g.id] do
			-- a step up out of the sea, by the shore
			local at = vector.create(c[2], math.max(c[3], SEA + 1), c[4])
			local apart = true
			for _, s in chosen do
				if vector.magnitude(vector.create(at.x - s.at.x, 0, at.z - s.at.z)) < 70 then
					apart = false
				end
			end
			if apart then
				-- the arch stands across the way back to the Green
				local green = world.spot("green")
				local along_x = math.abs(green.z - at.z) > math.abs(green.x - at.x)
				table.insert(chosen, { id = g.id, at = at, along_x = along_x, built = false, lit = false })
				break
			end
		end
	end
	return chosen
end

-- ---- building them ----

-- a place in a site, by how far across (u), up (h) and along to the arch (v)
local function spot(s: Site, u: number, h: number, v: number): vector
	if s.along_x then
		return vector.create(s.at.x + u, s.at.y + h, s.at.z + v)
	end
	return vector.create(s.at.x + v, s.at.y + h, s.at.z + u)
end

local function put(s: Site, u: number, h: number, v: number, name: string)
	world.set_block(spot(s, u, h, v), name)
end

local function hearth(s: Site): vector
	return spot(s, 0, 1, 0)
end

-- The arch's lintel: broken on one side while it's cold, whole once lit.
local function lintel(s: Site, g: Green)
	for u = -2, 2 do
		if s.lit or u < 1 then
			put(s, u, 6, -3, g.stone)
		end
	end
end

-- A clearing round it: no trunk stands on the plaza, and no branches hang over it
-- (a trunk just outside keeps its own side of the tree).
local function clearing(s: Site)
	for v = -7, 7 do
		for u = -7, 7 do
			local inside = math.abs(u) <= 4 and math.abs(v) <= 4
			for h = 1, 24 do
				local p = spot(s, u, h, v)
				local b = world.block(p)
				if (LEAVES[b] and not STANDING[b]) or (inside and STANDING[b]) then
					world.set_block(p, "Air")
				end
			end
		end
	end
end

local function build(s: Site)
	local g = BY_ID[s.id]
	clearing(s)
	for v = -4, 4 do
		for u = -4, 4 do
			local p = spot(s, u, 0, v)
			-- level ground under it, and nothing standing in it
			local y = ground(p.x, p.z)
			-- (built up at its edges, so it stands like a terrace)
			local edge = math.abs(u) == 4 or math.abs(v) == 4
			for h = y + 1, s.at.y - 1 do
				world.set_block(vector.create(p.x, h, p.z), if edge then g.stone else "Dirt")
			end
			for h = 1, 8 do
				put(s, u, h, v, "Air")
			end
			-- an old floor with stones missing
			local r = hash(p.x, p.z, 1)
			local floor = if r < 0.6 then g.stone elseif r < 0.8 then "Cobblestone" elseif r < 0.9 then "Gravel" else block(p.x, y, p.z)
			put(s, u, 0, v, if NATURAL[floor] or floor == g.stone or floor == "Cobblestone" then floor else "Dirt")
			-- what's left of a low wall round it
			if math.abs(u) == 4 or math.abs(v) == 4 then
				local w = hash(p.x, p.z, 2)
				if w < 0.45 then
					put(s, u, 1, v, g.stone)
				end
				if w < 0.12 then
					put(s, u, 2, v, g.stone)
				end
			end
		end
	end
	-- the arch: two columns and a lintel, with a cold pot on each
	for _, u in { -2, 2 } do
		for h = 1, 4 do
			put(s, u, h, -3, "Column")
		end
		put(s, u, 5, -3, g.stone)
	end
	lintel(s, g)
	-- the hearth, on a step
	put(s, 0, 0, 0, g.stone)
	put(s, 0, 1, 0, if s.lit then FIRE else HEARTH)
	if s.lit then
		put(s, -2, 7, -3, "Lantern")
		put(s, 2, 7, -3, "Lantern")
	end
end

-- Lights one: the hearth burns, the arch is mended, lanterns on top.
local function light(s: Site)
	local g = BY_ID[s.id]
	s.lit = true
	put(s, 0, 1, 0, FIRE)
	lintel(s, g)
	put(s, -2, 7, -3, "Lantern")
	put(s, 2, 7, -3, "Lantern")
	save()
	sound.play("relight", { at = hearth(s) })
	local all = true
	for _, o in sites do
		all = all and o.lit
	end
	if all then
		world.after(1.5, function()
			world.say("Every Old Green burns again!")
			sound.play("fanfare", { caption = "Every Old Green is lit" })
		end)
	end
end

-- Looks for places a couple of hundred at a tick, so the world doesn't
-- stutter, then builds one a tick. Found places are kept straight away and
-- each is marked built once it is, so a reload carries on where it was.
local next_point = 1
local searched = #sites > 0
local work: number? = nil
work = world.every(0.1, function()
	for _, s in sites do
		if not s.built then
			build(s)
			s.built = true
			save()
			return
		end
	end
	if searched then
		world.cancel(work)
		return
	end
	local stop = math.min(next_point + 200, #points)
	for i = next_point, stop do
		consider(points[i][1], points[i][2])
	end
	next_point = stop + 1
	if next_point > #points then
		searched = true
		sites = choose()
		save()
	end
end)

-- ---- using a hearth ----

local function site_at(at: vector): Site?
	for _, s in sites do
		local h = hearth(s)
		if math.floor(h.x) == math.floor(at.x) and math.floor(h.y) == math.floor(at.y) and math.floor(h.z) == math.floor(at.z) then
			return s
		end
	end
	return nil
end

local function tell(player, text: string)
	ui.show(player, "old_green", text, { at = "left" })
	world.after(6, function()
		ui.hide(player, "old_green")
	end)
end

world.on("block_used", function(player, at: vector, used: string)
	if used ~= HEARTH then
		return
	end
	local s = site_at(at)
	if s == nil or s.lit then
		return
	end
	local g = BY_ID[s.id]
	if world.pockets(player) ~= nil then
		if world.has(player, "Lantern") < 1 or world.has(player, g.block) < g.count then
			sound.play("cold", { at = at, volume = 0.6 })
			tell(player, "This hearth wants " .. wants(g))
			return
		end
		world.take(player, "Lantern", 1)
		world.take(player, g.block, g.count)
		if world.give(player, g.gift, g.gifts) == 0 then
			-- no room for what it gives back: give it all back
			world.give(player, "Lantern", 1)
			world.give(player, g.block, g.count)
			tell(player, "Your hotbar is full. Make some room first")
			return
		end
		tell(player, "It's lit! It gives you " .. g.gifts .. " " .. g.gift)
	end
	light(s)
	world.say(player.name .. " lit " .. g.name .. ".")
end)

-- a word to anyone who comes near a cold one, once each time
local near: { [number]: string? } = {}
world.every(1, function()
	if not knobs.hint then
		return
	end
	for _, p in world.players() do
		local close: Site? = nil
		for _, s in sites do
			if not s.lit and vector.magnitude(p.position - hearth(s)) < 12 then
				close = s
			end
		end
		local id = if close then close.id else nil
		if close and near[p.id] ~= id then
			local g = BY_ID[close.id]
			if world.pockets(p) ~= nil then
				tell(p, "An Old Green, gone cold. It wants " .. wants(g))
			else
				tell(p, "An Old Green, gone cold. Use its hearth to light it")
			end
		end
		near[p.id] = id
	end
end)

world.on("player_left", function(player)
	near[player.id] = nil
end)
