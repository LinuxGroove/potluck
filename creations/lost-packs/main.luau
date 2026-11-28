--!strict
-- Lost packs: on a Normal or Hard world (world.danger, world 0.21), a
-- player hurt too badly in survival drops everything they carry where they
-- fell (world.on("player_fell")), in a pack. Use it, or break it, to take
-- it all back; anyone can, so be quick. What doesn't fit stays in the pack.
-- On Peaceful and Gentle worlds you keep what you carry. What's in each
-- pack is kept with the world (store), by where it lies.

local world = require("@potluck/world@0.21")
local tweak = require("@potluck/tweak")
local store = require("@potluck/store")
local ui = require("@potluck/ui@0.2")
local sound = require("@potluck/sound@0.2")

local knobs = tweak {
	on = { true, label = "Drop what you carry when you fall (Normal and Hard worlds)", order = 1 },
	gentle = { false, label = "On Gentle worlds too", order = 2 },
}

local PACK = "Lost pack"
world.drops(PACK, { gives = false })

-- what's in each pack, and whose it was, by "x,y,z"
type Thing = { name: string, n: number }
type Pack = { owner: string, things: { Thing } }
local packs: { [string]: Pack } = store.get("packs") or {}

local function key(at: vector): string
	return math.floor(at.x) .. "," .. math.floor(at.y) .. "," .. math.floor(at.z)
end

local function save()
	store.set("packs", packs)
end

local function drops_here(): boolean
	local d = world.danger()
	return knobs.on and (d == "normal" or d == "hard" or (d == "gentle" and knobs.gentle))
end

-- a block a pack can lie in: air, or a plant it can sit in place of
local function room(at: vector): boolean
	local b = world.block(at)
	if b == "Air" then
		return true
	end
	local ok, kind = pcall(world.kind, b)
	return ok and kind == "plant"
end

local function solid(at: vector): boolean
	local ok, kind = pcall(world.kind, world.block(at))
	return ok and kind ~= "air" and kind ~= "water" and kind ~= "plant"
end

-- where it lies: where they fell, or the nearest place near it with room,
-- on the ground if there's ground under it (not in water or mid-air)
local function spot(at: vector): vector?
	local x, y, z = math.floor(at.x), math.floor(at.y), math.floor(at.z)
	if y < -127 or y > 127 then
		return nil
	end
	local best: vector? = nil
	local best_d = math.huge
	for dy = 0, 3 do
		for dx = -2, 2 do
			for dz = -2, 2 do
				local p = vector.create(x + dx, y + dy, z + dz)
				if room(p) and packs[key(p)] == nil and world.block(p) ~= PACK then
					-- standing on something beats floating
					local d = dx * dx + dz * dz + dy * dy * 2 + (if solid(p - vector.create(0, 1, 0)) then 0 else 6)
					if d < best_d then
						best, best_d = p, d
					end
				end
			end
		end
	end
	return best
end

world.on("player_fell", function(player, at: vector)
	if not drops_here() then
		return
	end
	local carried = world.pockets(player)
	if carried == nil or next(carried) == nil then
		return
	end
	local p = spot(at)
	if not p then
		return
	end
	local things: { Thing } = {}
	for name, n in carried do
		if n > 0 and world.take(player, name, n) then
			table.insert(things, { name = name, n = n })
		end
	end
	if #things == 0 then
		return
	end
	table.sort(things, function(a, b)
		return a.name < b.name
	end)
	world.set_block(p, PACK)
	packs[key(p)] = { owner = player.name, things = things }
	save()
	ui.show(player, "lost_pack", "You dropped what you carried where you fell. Go back for your pack.", { at = "left" })
	world.after(12, function()
		ui.hide(player, "lost_pack")
	end)
end)

-- gives what's in the pack at `at` to a player, as much as fits; true if
-- it's empty now
local function open(player, at: vector): boolean
	local k = key(at)
	local pack = packs[k]
	if not pack then
		return true
	end
	if world.pockets(player) == nil then
		ui.show(player, "lost_pack", "Only someone in survival can carry what's in it.", { at = "bottom" })
		world.after(4, function()
			ui.hide(player, "lost_pack")
		end)
		return false
	end
	local left: { Thing } = {}
	local got = 0
	for _, t in pack.things do
		local given = world.give(player, t.name, t.n)
		got += given
		if given < t.n then
			table.insert(left, { name = t.name, n = t.n - given })
		end
	end
	local whose = if pack.owner == player.name then "your" else pack.owner .. "'s"
	local said
	if #left == 0 then
		packs[k] = nil
		said = "You took everything from " .. whose .. " pack."
	else
		pack.things = left
		said = if got > 0 then "Your pockets are full: the rest stays in " .. whose .. " pack." else "Your pockets are full."
	end
	save()
	if got > 0 then
		sound.play("pop", { at = at, volume = 0.6, pitch = 0.8, caption = "A pack is opened" })
	end
	ui.show(player, "lost_pack", said, { at = "bottom" })
	world.after(4, function()
		ui.hide(player, "lost_pack")
	end)
	return #left == 0
end

world.on("block_used", function(player, at: vector, block: string)
	if block ~= PACK then
		return
	end
	if open(player, at) then
		world.set_block(at, "Air")
	end
end)

-- broken, it's opened, and what's left lies there still
world.on("block_broken", function(player, at: vector, block: string)
	if block ~= PACK then
		return
	end
	if not open(player, at) and world.block(at) == "Air" then
		world.set_block(at, PACK)
	end
end)

-- a pack someone took away by other means (a creation, a world edit) is
-- forgotten
world.every(30, function()
	local gone = false
	for k, _ in packs do
		local x, y, z = string.match(k, "^(-?%d+),(-?%d+),(-?%d+)$")
		if x and world.block(vector.create(tonumber(x) :: number, tonumber(y) :: number, tonumber(z) :: number)) ~= PACK then
			-- far from everyone the world reads as air; only forget it nearby
			local at = vector.create(tonumber(x) :: number + 0.5, tonumber(y) :: number, tonumber(z) :: number + 0.5)
			if world.nearest_player(at, 24) then
				packs[k] = nil
				gone = true
			end
		end
	end
	if gone then
		save()
	end
end)
