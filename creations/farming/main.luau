--!strict
-- Farming. In survival, Interact on grass or dirt to dig it into farmland;
-- with a hoe in hand (world.held) it digs a square at once, wider the
-- better the hoe (HOES below). Put wheat or carrots on farmland and
-- they're planted: they start as shoots and ripen after a few minutes
-- (faster by day and near water). A watering can (fill it at water) takes
-- time off the crops round the one you use it on, and a sickle harvests
-- the ripe ones round the one it cuts.
-- Break a ripe crop for more than you planted; break a shoot and you get
-- your seed back. Planted crops are kept with the world (store), so a
-- server's fields keep growing after a restart.

local world = require("@potluck/world@0.17")
local tweak = require("@potluck/tweak")
local store = require("@potluck/store")
local ui = require("@potluck/ui@0.2")
local sound = require("@potluck/sound@0.1")

local knobs = tweak {
	minutes = { 4, min = 1, max = 20, label = "Minutes to ripen", order = 1 },
	harvest = { 2, min = 0, max = 4, label = "Extra from a ripe crop", order = 2 },
	till = { true, label = "Interact on grass digs farmland", order = 3 },
	watering = { 1, min = 0, max = 4, label = "Minutes a watering saves", order = 4 },
	fills = { 16, min = 1, max = 64, label = "Waterings in a full can", order = 5 },
}

-- what each crop looks like while it grows
local SHOOTS: { [string]: string } = { Wheat = "Tall grass", Carrots = "Fern" }
local SOIL = "Farmland"
local MOST = 400 -- crops kept at once
local EVERY = 5 -- seconds between growing

type Crop = { crop: string, left: number, wet: boolean, ripe: boolean }

local UP = vector.create(0, 1, 0)

-- how far each hoe digs farmland round the block you use it on, each way:
-- a copper hoe 3 by 3, iron 5 by 5 and moonsilver 7 by 7
local HOES: { [string]: number } = { ["Copper hoe"] = 1, ["Iron hoe"] = 2, ["Moonsilver hoe"] = 3 }

-- what doesn't dig farmland when it's in hand: what carries water and
-- the rod (the water creation's)
local NOT_FOR_DIGGING: { [string]: boolean } = { Bucket = true, ["Water bucket"] = true, ["Watering can"] = true, ["Fishing rod"] = true }
local CAN, SICKLE = "Watering can", "Sickle"

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

-- waterings left in each player's can
local cans: { [number]: number } = {}

-- one line above the hotbar for a few seconds, for each player
local shown: { [number]: number } = {}
local function tell(player, text: string)
	ui.show(player, "farming", text, { at = "bottom" })
	local mine = (shown[player.id] or 0) + 1
	shown[player.id] = mine
	world.after(3, function()
		if shown[player.id] == mine then
			shown[player.id] = nil
			ui.hide(player, "farming")
		end
	end)
end

-- the crops in the square round `at` (at the same height), and where
local function round(at: vector): { { k: string, at: vector } }
	local out = {}
	for dx = -1, 1 do
		for dz = -1, 1 do
			local q = at + vector.create(dx, 0, dz)
			if crops[key(q)] then
				table.insert(out, { k = key(q), at = q })
			end
		end
	end
	return out
end

-- digs one block into farmland, if it's grass or dirt open to the sky
local function till(at: vector): boolean
	local b = world.block(at)
	if (b == "Grass" or b == "Dirt") and world.block(at + UP) == "Air" then
		world.set_block(at, SOIL)
		return true
	end
	return false
end

-- watering: fills at water, and takes time off the crops round the one
-- (or the farmland) it's used on
local function water(player, at: vector, block: string)
	if block == "Water" then
		cans[player.id] = math.floor(knobs.fills)
		sound.play("splash", { at = at, volume = 0.5, caption = "Filling a watering can" })
		tell(player, "Your watering can is full.")
		return
	end
	local over = if block == SOIL then at + UP else at
	local near = round(over)
	if #near == 0 then
		return
	end
	local left = cans[player.id] or 0
	if left <= 0 then
		tell(player, "Fill your watering can at water first.")
		return
	end
	cans[player.id] = left - 1
	for _, n in near do
		local c = crops[n.k]
		if not c.ripe then
			c.left -= knobs.watering * 60
			c.wet = true
			changed = true
		end
	end
	sound.play("splash", { at = over, volume = 0.4, caption = "Watering" })
	tell(player, if left > 1 then "Watered. " .. (left - 1) .. " waterings left." else "Watered. Your can is empty.")
end

-- digging farmland
world.on("block_used", function(player, at: vector, block: string)
	if world.pockets(player) == nil then
		return
	end
	local held = world.held(player)
	if held == CAN then
		water(player, at, block)
		return
	end
	if not knobs.till or (held and NOT_FOR_DIGGING[held]) or not till(at) then
		return
	end
	local reach = held and HOES[held] or 0
	for dx = -reach, reach do
		for dz = -reach, reach do
			if dx ~= 0 or dz ~= 0 then
				till(at + vector.create(dx, 0, dz))
			end
		end
	end
	sound.play("dig", { at = at, volume = 0.6, caption = "Digging" })
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
		-- a sickle reaps the ripe ones round it too
		if world.held(player) == SICKLE then
			for _, n in round(at) do
				local o = crops[n.k]
				if o.ripe and world.block(n.at) == o.crop then
					forget(n.k)
					world.set_block(n.at, "Air")
					world.give(player, o.crop, 1 + knobs.harvest)
				end
			end
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

world.on("player_left", function(p)
	cans[p.id] = nil
	shown[p.id] = nil
end)
