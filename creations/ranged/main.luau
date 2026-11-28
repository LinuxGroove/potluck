--!strict
-- Bows, slings and spears. Hold one and Interact, and it's shot the way
-- you look (world.on("item_used"), world.aims and world.shoot, world
-- 0.19): a bow shoots an Arrow you carry, a sling a Cobblestone, and a
-- spear flies itself, one at a time, and is back in your hand when it
-- lands. A creature it hits hears `hit` with the shot, whose power says
-- how hard (a Gloomling pops at 2). Arrows that land on a block are
-- sometimes found again. WEAPONS below says how each shoots, so a remix
-- can change them or add its own. In creative there's no need to carry
-- arrows or stones.

local world = require("@potluck/world@0.19")
local tweak = require("@potluck/tweak")
local ui = require("@potluck/ui@0.2")
local sound = require("@potluck/sound@0.2")

local knobs = tweak {
	power = { 1, min = 0.5, max = 3, label = "How hard shots hit", order = 1 },
	found = { 0.5, min = 0, max = 1, label = "Share of arrows found after a miss", order = 2 },
	hint = { true, label = "Say how to shoot", order = 3 },
}

-- What each shoots (nil for itself), how it looks, how fast it flies
-- (blocks a second) and falls (0 not at all, 1 like a stone), how hard it
-- hits, how long till the next, and its sound.
type Weapon = { ammo: string?, look: "arrow" | "stone" | "spear", speed: number, drop: number, power: number, wait: number, sound: string }
local WEAPONS: { [string]: Weapon } = {
	Bow = { ammo = "Arrow", look = "arrow", speed = 38, drop = 0.45, power = 2, wait = 0.7, sound = "twang" },
	Sling = { ammo = "Cobblestone", look = "stone", speed = 24, drop = 1, power = 1, wait = 0.5, sound = "whirr" },
	Spear = { look = "spear", speed = 24, drop = 0.6, power = 3, wait = 0.6, sound = "whoosh" },
}

-- each is aimed wherever it points, at a block or a creature in reach too
for name in WEAPONS do
	world.aims(name)
end

-- one line above the hotbar for a few seconds, for each player
local shown: { [number]: number } = {}
local function tell(player, text: string)
	ui.show(player, "ranged", text, { at = "bottom" })
	local mine = (shown[player.id] or 0) + 1
	shown[player.id] = mine
	world.after(3, function()
		if shown[player.id] == mine then
			shown[player.id] = nil
			ui.hide(player, "ranged")
		end
	end)
end

-- when each player can shoot again, and whose spears are in the air
local ready: { [number]: number } = {}
local spears: { [number]: boolean } = {}

-- whether a player is still here
local function here(id: number): boolean
	for _, p in world.players() do
		if p.id == id then
			return true
		end
	end
	return false
end

world.on("item_used", function(player, item: string?, aim: vector)
	local w = item and WEAPONS[item]
	if not w then
		return
	end
	local pid = player.id
	if world.time() < (ready[pid] or 0) or (item == "Spear" and spears[pid]) then
		return
	end
	-- in survival it takes what it shoots; in creative there's plenty
	local survival = world.pockets(player) ~= nil
	if w.ammo and survival and not world.take(player, w.ammo, 1) then
		-- "You've no arrows to shoot."
		local what = string.lower(w.ammo) .. (if w.ammo == "Arrow" then "s" else "")
		tell(player, "You've no " .. what .. " to shoot.")
		return
	end
	ready[pid] = world.time() + w.wait
	local ammo = w.ammo
	local opts = { aim = aim, look = w.look, speed = w.speed, drop = w.drop, power = w.power * knobs.power }
	local id = world.shoot(player, opts, function(at: vector, block: string?, creature: number?)
		if creature then
			sound.play("thud", { at = at, volume = 0.7 })
		end
		if item == "Spear" then
			spears[pid] = nil
		elseif ammo == "Arrow" and block and survival and here(pid) and world.random() < knobs.found then
			world.give(pid, "Arrow", 1)
		end
	end)
	if id and item == "Spear" then
		spears[pid] = true
	end
	sound.play(w.sound, { at = player.position })
end)

-- a word to each player the first time they hold one
local told: { [number]: boolean } = {}
world.every(0.5, function()
	if not knobs.hint then
		return
	end
	for _, p in world.players() do
		local held = world.held(p)
		if held and WEAPONS[held] and not told[p.id] then
			told[p.id] = true
			ui.show(p, "ranged_says", "Interact to shoot, the way you look.", { at = "left" })
			world.after(6, function()
				ui.hide(p, "ranged_says")
			end)
		end
	end
end)

world.on("player_left", function(p)
	ready[p.id] = nil
	spears[p.id] = nil
	told[p.id] = nil
	shown[p.id] = nil
end)
