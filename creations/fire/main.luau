--!strict
-- Fire. A flint and steel lights it (Interact while aiming at a block holding
-- one), and it burns for a while, then goes out. Beside wood, leaves, plants,
-- hay or wool it spreads, slowly: at most a block every few seconds from each
-- fire. When a fire goes out the wood or wool it was eating may burn away.
-- Water puts it out, and so does breaking it. Standing in it hurts.
--
-- The fire block is in potluck.toml, drawn in four pictures it shows in turn
-- (frames); with Reduce flashing on, players see the first one only. Knobs
-- turn spreading off, which a server's host may want.

local world = require("@potluck/world@0.14")
local tweak = require("@potluck/tweak")
local store = require("@potluck/store")
local sound = require("@potluck/sound@0.2")

local knobs = tweak {
	spreads = { true, label = "Fire spreads", order = 1 },
	burns = { 30, min = 5, max = 120, label = "Seconds a fire burns", order = 2 },
	spread = { 4, min = 1, max = 30, label = "Seconds between spreading, at quickest", order = 3 },
	eats = { true, label = "Burns away wood and wool", order = 4 },
	hurts = { true, label = "Standing in fire hurts", order = 5 },
	lighter = { 64, min = 8, max = 256, label = "Uses of a flint and steel", order = 6 },
}

local FIRE = "Fire"
local LIGHTER = "Flint and steel"
local MOST = 256 -- fires burning at once, at most

-- What burns: wood and plants by their kind, and these by name. A remix can
-- add its own.
local BURNS: { [string]: boolean } = {
	["Red wool"] = true,
	["Cream wool"] = true,
	["Blue wool"] = true,
	["Yellow wool"] = true,
	Hay = true,
}
-- Never: what other creations light themselves, and fire itself.
local NEVER: { [string]: boolean } = { [FIRE] = true, Campfire = true, Furnace = true, ["Lit furnace"] = true }

local function burns(name: string): boolean
	if NEVER[name] then
		return false
	end
	if BURNS[name] then
		return true
	end
	local ok, kind = pcall(world.kind, name)
	return ok and (kind == "wood" or kind == "plant")
end

local SIDES = {
	vector.create(1, 0, 0),
	vector.create(-1, 0, 0),
	vector.create(0, 1, 0),
	vector.create(0, -1, 0),
	vector.create(0, 0, 1),
	vector.create(0, 0, -1),
}

-- Burning fires, by where they are: when each goes out, and when it may
-- next spread.
type Burning = { at: vector, out: number, next: number }
local fires: { [string]: Burning } = {}
local count = 0
local changed = false

local function key(at: vector): string
	return math.floor(at.x) .. "," .. math.floor(at.y) .. "," .. math.floor(at.z)
end

local function middle(at: vector): vector
	return vector.create(math.floor(at.x) + 0.5, math.floor(at.y) + 0.5, math.floor(at.z) + 0.5)
end

-- whether anything beside `at` burns
local function fuel_by(at: vector): boolean
	for _, d in SIDES do
		if burns(world.block(at + d)) then
			return true
		end
	end
	return false
end

local function watched(at: vector)
	local k = key(at)
	if fires[k] then
		return
	end
	local now = world.time()
	-- with nothing to eat, a fire dies down three times as fast
	local lasts = knobs.burns * world.random(0.7, 1.3) / (if fuel_by(at) then 1 else 3)
	fires[k] = { at = middle(at), out = now + lasts, next = now + knobs.spread * world.random(1, 3) }
	count += 1
	changed = true
end

-- Starts a fire at `at` if there's room for one: air, or a plant it burns up.
local function light(at: vector): boolean
	if count >= MOST then
		return false
	end
	local here = world.block(at)
	if here ~= "Air" and not (burns(here) and world.kind(here) == "plant") then
		return false
	end
	for _, d in SIDES do
		if world.block(at + d) == "Water" then
			return false
		end
	end
	world.set_block(at, FIRE)
	watched(at)
	sound.play("crackle", { at = middle(at), volume = 0.5 })
	return true
end

local function forget(k: string)
	if fires[k] then
		fires[k] = nil
		count -= 1
		changed = true
	end
end

local function put_out(k: string, f: Burning)
	if world.block(f.at) == FIRE then
		world.set_block(f.at, "Air")
	end
	forget(k)
end

-- An air block beside `at`, next to something that burns, picked at random.
local function catch(at: vector): vector?
	local tries = {}
	for _, d in SIDES do
		local p = at + d
		for _, e in SIDES do
			local q = p + e
			if world.block(q) == "Air" and fuel_by(q) then
				table.insert(tries, q)
			end
		end
	end
	if #tries == 0 then
		return nil
	end
	return tries[math.floor(world.random(1, #tries + 0.999))]
end

world.every(0.5, function()
	local now = world.time()
	local spreading: { vector } = {}
	local eaten: { vector } = {}
	for k, f in fires do
		if world.block(f.at) ~= FIRE then
			-- broken, built over or put out by something else
			forget(k)
			continue
		end
		local wet = false
		for _, d in SIDES do
			if world.block(f.at + d) == "Water" then
				wet = true
			end
		end
		if wet then
			put_out(k, f)
			sound.play("hiss", { at = f.at, volume = 0.6 })
			continue
		end
		if now >= f.out then
			put_out(k, f)
			if knobs.eats then
				for _, d in SIDES do
					local p = f.at + d
					local name = world.block(p)
					if burns(name) and world.kind(name) ~= "plant" and world.random() < 0.4 then
						table.insert(eaten, p)
					end
				end
			end
			continue
		end
		if knobs.spreads and now >= f.next then
			f.next = now + knobs.spread * world.random(1, 3)
			local q = catch(f.at)
			if q then
				table.insert(spreading, q)
			end
		end
	end
	-- what burnt away may catch, so fire moves through a wooden house
	for _, p in eaten do
		world.set_block(p, "Air")
		if knobs.spreads then
			light(p)
		end
	end
	for _, q in spreading do
		light(q)
	end
end)

-- Keeps the fires in the world's store, so they burn on after a restart.
world.every(2, function()
	if not changed then
		return
	end
	changed = false
	local now = world.time()
	local kept = {}
	for _, f in fires do
		table.insert(kept, { f.at.x, f.at.y, f.at.z, math.max(1, f.out - now) })
	end
	store.set("fires", kept)
end)

do
	local now = world.time()
	for _, f in (store.get("fires") or {}) :: { { number } } do
		local at = vector.create(f[1], f[2], f[3])
		if world.block(at) == FIRE then
			fires[key(at)] = { at = at, out = now + f[4], next = now + knobs.spread }
			count += 1
		end
	end
end

-- Standing in fire hurts.
world.every(1, function()
	if not knobs.hurts then
		return
	end
	for _, p in world.players() do
		local feet = p.position
		if world.block(feet) == FIRE or world.block(feet + vector.create(0, 1, 0)) == FIRE then
			if world.hurt(p, 1) then
				sound.play_for(p, "crackle", { caption = "You're burning" })
			end
		end
	end
end)

-- Flint and steel: lights the side of the block nearest the player, or its top.
local used: { [number]: number } = {}

world.on("block_used", function(player, at: vector, block: string)
	if world.held(player) ~= LIGHTER or NEVER[block] then
		return
	end
	local eye = player.position + vector.create(0, 1.6, 0)
	local d = eye - at
	local side
	if math.abs(d.y) >= math.abs(d.x) and math.abs(d.y) >= math.abs(d.z) then
		side = vector.create(0, if d.y > 0 then 1 else -1, 0)
	elseif math.abs(d.x) >= math.abs(d.z) then
		side = vector.create(if d.x > 0 then 1 else -1, 0, 0)
	else
		side = vector.create(0, 0, if d.z > 0 then 1 else -1)
	end
	sound.play("strike", { at = at })
	if not (light(at + side) or light(at + vector.create(0, 1, 0))) then
		return
	end
	-- it wears out, in survival
	if world.pockets(player) ~= nil then
		used[player.id] = (used[player.id] or 0) + 1
		if used[player.id] >= knobs.lighter then
			used[player.id] = 0
			world.take(player, LIGHTER, 1)
			sound.play_for(player, "crack", { caption = "Flint and steel wore out" })
		end
	end
end)

-- Fire put down from the Blocks sheet in creative burns like any other.
world.on("block_placed", function(_player, at: vector, block: string)
	if block == FIRE then
		watched(at)
	end
end)

world.on("player_left", function(player)
	used[player.id] = nil
end)

-- Breaking fire puts it out, and gives nothing.
world.drops(FIRE, { gives = false })
