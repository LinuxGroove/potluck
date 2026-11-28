--!strict
-- Farming. In survival, Interact on grass or dirt to dig it into farmland;
-- with a hoe in hand (world.held) it digs a square at once, wider the
-- better the hoe (HOES below). Put a crop on farmland and it's planted:
-- wheat, carrots, potatoes, beans or a pumpkin (CROPS below), or use seeds
-- on it for wheat. Each starts as a shoot and ripens after a few minutes
-- (faster by day and near water). A watering can (fill it at water) takes
-- time off the crops round the one you use it on, and a sickle harvests
-- the ripe ones round the one it cuts.
-- Break a ripe crop for more than you planted; break a shoot and you get
-- your seed back. Planted crops are kept with the world (store), so a
-- server's fields keep growing after a restart.
--
-- Out in the wild: tall grass gives seeds now and then, oak leaves an
-- apple, and wild potatoes, beans, pumpkins and berry bushes come up on
-- the grass round survival players. Use a berry bush to pick its berries;
-- they grow back. The crops, shoots and bushes are this creation's own
-- blocks, drawn in potluck.toml.

local world = require("@potluck/world@0.22")
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
	wild = { 0.3, min = 0, max = 1, label = "How often wild crops come up", order = 6 },
	berries = { 3, min = 1, max = 20, label = "Minutes for berries to grow back", order = 7 },
	seeds = { 0.5, min = 0, max = 1, label = "Share of tall grass with seeds", order = 8 },
	apples = { 0.08, min = 0, max = 1, label = "Share of oak leaves with an apple", order = 9 },
}

-- Each crop: what it looks like while it grows, and what a shoot broken
-- too soon gives back. A remix can add its own (with blocks of its own).
type Kind = { shoot: string, seed: string }
local CROPS: { [string]: Kind } = {
	Wheat = { shoot = "Wheat shoots", seed = "Seeds" },
	Carrots = { shoot = "Carrot shoots", seed = "Carrots" },
	Potatoes = { shoot = "Potato shoots", seed = "Potatoes" },
	Beans = { shoot = "Bean shoots", seed = "Beans" },
	Pumpkin = { shoot = "Pumpkin vine", seed = "Pumpkin" },
}
-- what's planted by using it on farmland, rather than putting it down
local SOWN: { [string]: string } = { Seeds = "Wheat" }
-- what a ripe crop gives besides itself: wheat gives seeds to sow again
local ALSO: { [string]: string } = { Wheat = "Seeds" }
-- what comes up in the wild, each as likely as its count here
local WILD = { "Potatoes", "Beans", "Pumpkin", "Berry bush", "Berry bush", "Carrots" }
local BUSH, BARE = "Berry bush", "Bare berry bush"

local SOIL = "Farmland"
local MOST = 400 -- crops kept at once
local MOST_WILD = 48 -- wild ones about at once
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

-- what each crop looks like as it grows, by its shoot's name
local function shoot_of(crop: string): string?
	local c = CROPS[crop]
	return c and c.shoot
end

local crops: { [string]: Crop } = store.get("crops") or {}
local count = 0
for _ in crops do
	count += 1
end
-- bushes picked bare, and the seconds till their berries are back
local bare: { [string]: number } = store.get("bare") or {}
-- wild crops and bushes about, by where
local wild: { [string]: string } = store.get("wild") or {}
local wilds = 0
for _ in wild do
	wilds += 1
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

-- starts a crop growing at `at` (on farmland), as its shoot
local function plant(at: vector, crop: string): boolean
	local shoot = shoot_of(crop)
	if not shoot or world.block(at - UP) ~= SOIL or count >= MOST then
		return false
	end
	world.set_block(at, shoot)
	crops[key(at)] = { crop = crop, left = knobs.minutes * 60, wet = world.find_block(at, 4, "Water") ~= nil, ripe = false }
	count += 1
	changed = true
	sound.play("rustle", { at = at, volume = 0.5, caption = "Planted" })
	return true
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

-- picking a bush's berries, which grow back
local function pick(player, at: vector)
	if world.pockets(player) ~= nil and world.give(player, "Berries", math.floor(world.random(2, 4))) == 0 then
		tell(player, "Your pockets are full.")
		return
	end
	world.set_block(at, BARE)
	bare[key(at)] = knobs.berries * 60
	changed = true
	sound.play("rustle", { at = at, volume = 0.6, caption = "Picking berries" })
end

-- digging farmland, sowing seeds and picking berries
world.on("block_used", function(player, at: vector, block: string)
	if block == BUSH then
		pick(player, at)
		return
	elseif block == BARE then
		tell(player, "Its berries haven't grown back yet.")
		return
	end
	if world.pockets(player) == nil then
		return
	end
	local held = world.held(player)
	if held == CAN then
		water(player, at, block)
		return
	end
	local sown = held and SOWN[held]
	if sown then
		-- seeds go in farmland, or in the farmland under a crop's spot
		local soil = if block == SOIL then at else at - UP
		if world.block(soil) == SOIL and world.block(soil + UP) == "Air" and count < MOST and world.take(player, held :: string, 1) then
			plant(soil + UP, sown)
		end
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

-- planting, and bare bushes put down again
world.on("block_placed", function(_player, at: vector, block: string)
	if block == BARE then
		bare[key(at)] = knobs.berries * 60
		changed = true
		return
	end
	plant(at, block)
end)

-- harvesting, and what the wild gives
world.on("block_broken", function(player, at: vector, block: string)
	local k = key(at)
	local survival = world.pockets(player) ~= nil
	if block == SOIL then
		-- the crop on it goes with it
		forget(key(at + UP))
		return
	elseif block == "Tall grass" then
		if survival and world.random() < knobs.seeds then
			world.give(player, "Seeds", 1)
		end
		return
	elseif block == "Oak leaves" then
		if survival and world.random() < knobs.apples then
			world.give(player, "Apple", 1)
		end
		return
	elseif block == BUSH then
		-- it comes away bare (world.drops below), and its berries with it
		if survival then
			world.give(player, "Berries", 2)
		end
		bare[k] = nil
		return
	elseif block == BARE then
		bare[k] = nil
		changed = true
		return
	end
	local c = crops[k]
	if not c then
		return
	end
	forget(k)
	local kind = CROPS[c.crop]
	if c.ripe and block == c.crop then
		-- a little XP for each ripe crop (world.reward)
		local reaped = 1
		if knobs.harvest > 0 then
			world.give(player, c.crop, knobs.harvest)
		end
		if ALSO[c.crop] then
			world.give(player, ALSO[c.crop], 1)
		end
		-- a sickle reaps the ripe ones round it too
		if world.held(player) == SICKLE then
			for _, n in round(at) do
				local o = crops[n.k]
				if o.ripe and world.block(n.at) == o.crop then
					forget(n.k)
					world.set_block(n.at, "Air")
					world.give(player, o.crop, 1 + knobs.harvest)
					if ALSO[o.crop] then
						world.give(player, ALSO[o.crop], 1)
					end
					reaped += 1
				end
			end
		end
		world.reward(player, reaped)
		sound.play("pop", { at = at, caption = "Harvest" })
	elseif not c.ripe and kind and block == kind.shoot then
		-- too soon: the seed back
		world.give(player, kind.seed, 1)
	end
end)

-- a shoot broken gives nothing of itself (block_broken gives the seed
-- back), and a bush with berries comes away bare
for _, kind in CROPS do
	world.drops(kind.shoot, { gives = false })
end
world.drops(BUSH, { gives = BARE })

-- growing: faster by day and near water; and bushes growing berries again
world.every(EVERY, function()
	local rate = (0.5 + 0.5 * world.daylight()) * EVERY
	for k, c in crops do
		local at = spot(k)
		local now = world.block(at)
		if c.ripe then
			if now ~= c.crop then
				forget(k)
			end
		elseif now ~= shoot_of(c.crop) then
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
	for k, left in bare do
		local at = spot(k)
		if world.block(at) ~= BARE then
			bare[k] = nil
		elseif left <= EVERY then
			bare[k] = nil
			world.set_block(at, BUSH)
		else
			bare[k] = left - EVERY
		end
		changed = true
	end
	if changed then
		store.set("crops", crops)
		store.set("bare", bare)
		changed = false
	end
end)

-- wild crops and bushes coming up on the grass round survival players,
-- out of sight (12 to 32 blocks off), a few dozen at most
world.every(20, function()
	-- forget the ones that have gone (picked, or dug up)
	for k, name in wild do
		local now = world.block(spot(k))
		if now ~= name and now ~= BARE then
			wild[k] = nil
			wilds -= 1
		end
	end
	for _, p in world.players() do
		if wilds >= MOST_WILD or world.pockets(p) == nil or world.random() >= knobs.wild then
			continue
		end
		local turn = world.random() * 2 * math.pi
		local far = world.random(12, 32)
		local x, z = p.position.x + math.cos(turn) * far, p.position.z + math.sin(turn) * far
		local ground = vector.create(x, world.top(x, z), z)
		if world.block(ground) == "Grass" and world.block(ground + UP) == "Air" then
			local name = WILD[math.floor(world.random(1, #WILD + 1))] or "Berry bush"
			world.set_block(ground + UP, name)
			wild[key(ground + UP)] = name
			wilds += 1
		end
	end
	store.set("wild", wild)
end)

world.on("player_left", function(p)
	cans[p.id] = nil
	shown[p.id] = nil
end)
