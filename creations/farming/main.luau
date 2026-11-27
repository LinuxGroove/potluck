--!strict
-- Farming. In survival, Interact on grass or dirt to dig it into farmland.
-- Put wheat or carrots on farmland and they're planted: they start as
-- shoots and ripen after a few minutes (faster by day and near water).
-- Break a ripe crop for more than you planted; break a shoot and you get
-- your seed back. Planted crops are kept with the world (store), so a
-- server's fields keep growing after a restart.

local world = require("@potluck/world@0.5")
local tweak = require("@potluck/tweak")
local store = require("@potluck/store")
local sound = require("@potluck/sound@0.1")

local knobs = tweak {
	minutes = { 4, min = 1, max = 20, label = "Minutes to ripen", order = 1 },
	harvest = { 2, min = 0, max = 4, label = "Extra from a ripe crop", order = 2 },
	till = { true, label = "Interact on grass digs farmland", order = 3 },
}

-- what each crop looks like while it grows
local SHOOTS: { [string]: string } = { Wheat = "Tall grass", Carrots = "Fern" }
local SOIL = "Farmland"
local MOST = 400 -- crops kept at once
local EVERY = 5 -- seconds between growing

type Crop = { crop: string, left: number, wet: boolean, ripe: boolean }

local UP = vector.create(0, 1, 0)

local function key(at: vector): string
	return string.format("%d,%d,%d", math.floor(at.x), math.floor(at.y), math.floor(at.z))
end

local function spot(k: string): vector
	local x, y, z = string.match(k, "^(-?%d+),(-?%d+),(-?%d+)$")
	return vector.create(tonumber(x) :: number + 0.5, tonumber(y) :: number + 0.5, tonumber(z) :: number + 0.5)
end

local crops: { [string]: Crop } = store.get("crops") or {}
local count = 0
for _ in crops do
	count += 1
end
local changed = false

local function forget(k: string)
	if crops[k] then
		crops[k] = nil
		count -= 1
		changed = true
	end
end

-- digging farmland
world.on("block_used", function(player, at: vector, block: string)
	if not knobs.till or world.pockets(player) == nil then
		return
	end
	if (block == "Grass" or block == "Dirt") and world.block(at + UP) == "Air" then
		world.set_block(at, SOIL)
		sound.play("dig", { at = at, volume = 0.6, caption = "Digging" })
	end
end)

-- planting
world.on("block_placed", function(player, at: vector, block: string)
	local shoot = SHOOTS[block]
	if not shoot or world.block(at - UP) ~= SOIL or count >= MOST then
		return
	end
	world.set_block(at, shoot)
	crops[key(at)] = { crop = block, left = knobs.minutes * 60, wet = world.find_block(at, 4, "Water") ~= nil, ripe = false }
	count += 1
	changed = true
	sound.play("rustle", { at = at, volume = 0.5, caption = "Planted" })
end)

-- harvesting
world.on("block_broken", function(player, at: vector, block: string)
	local k = key(at)
	if block == SOIL then
		-- the crop on it goes with it
		forget(key(at + UP))
		return
	end
	local c = crops[k]
	if not c then
		return
	end
	forget(k)
	if c.ripe and block == c.crop then
		if knobs.harvest > 0 then
			world.give(player, c.crop, knobs.harvest)
		end
		sound.play("pop", { at = at, caption = "Harvest" })
	elseif not c.ripe and block == SHOOTS[c.crop] then
		-- too soon: the seed back
		world.give(player, c.crop, 1)
	end
end)

-- growing: faster by day and near water
world.every(EVERY, function()
	local rate = (0.5 + 0.5 * world.daylight()) * EVERY
	for k, c in crops do
		local at = spot(k)
		local now = world.block(at)
		if c.ripe then
			if now ~= c.crop then
				forget(k)
			end
		elseif now ~= SHOOTS[c.crop] then
			forget(k)
		else
			c.left -= if c.wet then rate * 1.5 else rate
			changed = true
			if c.left <= 0 then
				c.ripe = true
				world.set_block(at, c.crop)
			end
		end
	end
	if changed then
		store.set("crops", crops)
		changed = false
	end
end)
