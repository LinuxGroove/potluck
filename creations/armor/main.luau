--!strict
-- Armor, for survival. Hold a piece and use it on nothing (Interact looking
-- at the sky or far off: world.on("item_used"), world 0.18) to put it on,
-- swapping it for what you wore there before. A cap or helmet, a coat,
-- leggings, boots and a shield each keep off part of every hurt
-- (world.guard), more for better stuff: ARMOR below says how much, and
-- armor other creations make ([items.*] with kind = "armor", world.item,
-- 0.25) guards as its slot and guard say. A bar beside your health shows
-- how well you're guarded. Use an empty hand on nothing to see what you
-- wear and take a piece off. What each player wears is kept with the world
-- (store), by their name, and every game draws it on them (world.wear,
-- 0.24).
--
-- Winter nights out under the sky are cold (@potluck/sky), and so is
-- falling snow. With two pieces of wool or leather (or made ones that say
-- warm) on, or one that's Warm (the Charms creation's charm), or a fire or
-- a light near, or a roof overhead, they're warm. Cold, a player doesn't
-- heal by themselves, and in a Normal or Hard world the cold hurts, though
-- never below 6.

local world = require("@potluck/world@0.25")
local tweak = require("@potluck/tweak")
local store = require("@potluck/store")
local ui = require("@potluck/ui@0.2")
local sound = require("@potluck/sound@0.2")
local sky = require("@potluck/sky@0.1")

local knobs = tweak {
	strong = { 1, min = 0, max = 2, label = "How much armor keeps off", order = 1 },
	color = { "#8fb3d9", label = "Armor bar", order = 2 },
	cold = { true, label = "Winter nights are cold", order = 3 },
}

-- Where a piece goes, and its guard: each point keeps off 3.5% of a hurt,
-- up to 85% (a whole set of moonsilver and a shield is 23).
type Piece = { slot: string, guard: number }
local ARMOR: { [string]: Piece } = {
	Shield = { slot = "arm", guard = 3 },
}
local PER_POINT, MOST = 0.035, 0.85
local FULL = 23 -- the bar's full at this

-- each material: its pieces' names (head, body, legs, feet) and guards
local MATERIALS = {
	{ names = { "Wool cap", "Wool coat", "Wool leggings", "Wool boots" }, guards = { 1, 3, 2, 1 } },
	{ names = { "Leather cap", "Leather coat", "Leather leggings", "Leather boots" }, guards = { 2, 4, 3, 1 } },
	{ names = { "Copper helmet", "Copper coat", "Copper leggings", "Copper boots" }, guards = { 2, 5, 4, 1 } },
	{ names = { "Iron helmet", "Iron coat", "Iron leggings", "Iron boots" }, guards = { 2, 6, 5, 2 } },
	{ names = { "Moonsilver helmet", "Moonsilver coat", "Moonsilver leggings", "Moonsilver boots" }, guards = { 3, 8, 6, 3 } },
}
local SLOTS = { "head", "body", "legs", "feet", "arm" }
for _, m in MATERIALS do
	for i, name in m.names do
		ARMOR[name] = { slot = SLOTS[i], guard = m.guards[i] }
	end
end

-- A piece by name: one of ARMOR's, or armor another creation made. `warm`
-- is whether it keeps the cold off as wool does.
local function piece_of(name: string): (Piece?, boolean)
	if ARMOR[name] then
		return ARMOR[name], string.sub(name, 1, 5) == "Wool " or string.sub(name, 1, 8) == "Leather "
	end
	local it = world.item(name)
	if it and it.kind == "armor" and it.slot then
		return { slot = it.slot, guard = it.guard or 0 }, it.warm == true
	end
	return nil, false
end

-- what each player wears, by name: slot to piece
local worn: { [string]: { [string]: string } } = store.get("worn") or {}

local function save()
	store.set("worn", worn)
end

-- one line above the hotbar for a few seconds, for each player
local shown: { [number]: number } = {}
local function tell(player, text: string)
	ui.show(player, "armor", text, { at = "bottom" })
	local mine = (shown[player.id] or 0) + 1
	shown[player.id] = mine
	world.after(3, function()
		if shown[player.id] == mine then
			shown[player.id] = nil
			ui.hide(player, "armor")
		end
	end)
end

-- a player's guard, all their pieces together
local function guard_of(player): number
	local n = 0
	for _, piece in worn[player.name] or {} do
		local a = piece_of(piece)
		if a then
			n += a.guard
		end
	end
	return n
end

-- What they wear, and a choice to take each off.
local function wearing(player)
	local mine = worn[player.name] or {}
	local pieces, slots, choices = {}, {}, {}
	for _, slot in SLOTS do
		if mine[slot] then
			table.insert(pieces, mine[slot])
			table.insert(slots, slot)
			table.insert(choices, "Take off " .. string.lower(mine[slot]))
		end
	end
	if #pieces == 0 then
		return
	end
	local text = "You wear " .. string.lower(table.concat(pieces, ", ")) .. ". Hold a piece and use it on nothing to put it on."
	ui.ask(player, { title = "Armor", text = text, choices = choices }, function(who, choice)
		local piece = choice and pieces[choice]
		if not piece then
			return
		end
		if world.give(who, piece, 1) == 0 then
			tell(who, "Your pockets are full.")
			return
		end
		mine[slots[choice]] = nil
		save()
		sound.play("don_soft", { at = who.position, volume = 0.6 })
		wearing(who)
	end)
end

world.on("item_used", function(player, item: string?, aim: vector)
	if world.pockets(player) == nil then
		return
	end
	if item == nil then
		wearing(player)
		return
	end
	local piece, warm = piece_of(item)
	if not piece or not world.take(player, item, 1) then
		return
	end
	local mine = worn[player.name] or {}
	local before = mine[piece.slot]
	if before and world.give(player, before, 1) == 0 then
		-- no room for what comes off
		world.give(player, item, 1)
		tell(player, "Make room for your " .. string.lower(before) .. " first.")
		return
	end
	mine[piece.slot] = item
	worn[player.name] = mine
	save()
	sound.play(if warm then "don_soft" else "don_metal", { at = player.position })
	tell(player, "You put on the " .. string.lower(item) .. ".")
end)

-- who's been told what the bar means
local told: { [number]: boolean } = {}

-- what every game draws each player wearing, as last said: in survival
-- what they wear, and nothing otherwise
local drawn: { [number]: { [string]: string } } = {}
local function show_worn(p, on: boolean)
	local mine = if on then worn[p.name] or {} else {}
	local was = drawn[p.id] or {}
	for _, slot in SLOTS do
		if was[slot] ~= mine[slot] then
			world.wear(p, slot, mine[slot])
		end
	end
	drawn[p.id] = table.clone(mine)
end

-- how much each player's armor keeps off, and the bar
world.every(0.5, function()
	for _, p in world.players() do
		local g = if world.pockets(p) ~= nil then guard_of(p) else 0
		show_worn(p, world.pockets(p) ~= nil)
		world.guard(p, 1 - math.min(g * PER_POINT * knobs.strong, MOST))
		if g > 0 then
			ui.meter(p, "guard", math.min(g / FULL, 1), { color = knobs.color })
			if not told[p.id] then
				told[p.id] = true
				ui.show(p, "guard_says", "The blue bar is your armor. The fuller it is, the less hurts hurt.", { at = "left" })
				world.after(6, function()
					ui.hide(p, "guard_says")
				end)
			end
		else
			ui.meter(p, "guard", nil)
		end
	end
end)

-- What keeps the cold off: two warm pieces (wool, leather or made ones that
-- say so), or one that's Warm.
local function dressed_warm(p): boolean
	local n = 0
	for _, piece in worn[p.name] or {} do
		if table.find(world.charms(p, piece), "Warm") then
			return true
		end
		local _, warm = piece_of(piece)
		if warm then
			n += 1
		end
	end
	return n >= 2
end

-- Whether it's cold where they are: out under the sky on a winter night,
-- or in falling snow, with no fire or light near.
local function out_in_the_cold(p): boolean
	local snowing = sky.weather().kind == "snow"
	if not snowing and ((sky.season()) ~= "winter" or world.daylight() > 0.3) then
		return false
	end
	local head = p.position + vector.create(0, 1.6, 0)
	if world.top(head.x, head.z) > math.floor(head.y) then
		return false
	end
	return world.find_light(p.position, 5) == nil
end

-- Who's cold now. The cold hurts every few seconds in a Normal or Hard
-- world, never below this.
local cold: { [number]: boolean } = {}
local COLD_DOWN_TO = 6

world.every(5, function()
	local danger = world.danger()
	for _, p in world.players() do
		local now = knobs.cold and danger ~= "peaceful" and world.pockets(p) ~= nil and out_in_the_cold(p) and not dressed_warm(p)
		if now ~= (cold[p.id] == true) then
			cold[p.id] = if now then true else nil
			world.healing(p, if now then 0 else 1)
			tell(p, if now then "You're cold. Wool or leather, a fire, a light or a roof keeps you warm." else "You're warm again.")
		end
		if now and (danger == "normal" or danger == "hard") then
			world.hurt(p, if danger == "hard" then 2 else 1, { down_to = COLD_DOWN_TO, guarded = false })
		end
	end
end)

world.on("player_left", function(p)
	told[p.id] = nil
	shown[p.id] = nil
	drawn[p.id] = nil
	cold[p.id] = nil
end)
