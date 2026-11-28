--!strict
-- Hunger, for survival. A food bar sits beside your health bar and goes
-- down slowly. When you get hungry you eat food you carry, a bite at a
-- time (FOODS below says what's food and how filling it is). On an empty
-- belly you stop healing by yourself (world.healing) and it slowly hurts,
-- but never below half your health (world.hurt's down_to), and armor
-- doesn't help (guarded = false). When you're well fed you heal faster.

local world = require("@potluck/world@0.18")
local tweak = require("@potluck/tweak")
local ui = require("@potluck/ui@0.2")
local sound = require("@potluck/sound@0.1")

local knobs = tweak {
	minutes = { 20, min = 5, max = 60, label = "Minutes from full to empty", order = 1 },
	color = { "#e0a040", label = "Food bar", order = 2 },
	well_fed = { true, label = "Heal faster when well fed", order = 3 },
	hurts = { true, label = "An empty belly hurts", order = 4 },
	down_to = { 10, min = 1, max = 19, label = "It hurts you down to (of 20)", order = 5 },
}

local FULL = 20
local HUNGRY = 12 -- eat when the bar is this low or lower
local WELL_FED = 16 -- heal faster at this or more
local HURT_EVERY = 5 -- seconds between hurts on an empty belly

-- What's food, most filling first: a remix can add its own.
local FOODS = {
	{ name = "Cooked fish", fills = 8, says = "You ate some cooked fish." },
	{ name = "Bread", fills = 8, says = "You ate some bread." },
	{ name = "Roasted carrots", fills = 7, says = "You ate some roasted carrots." },
	{ name = "Carrots", fills = 6, says = "You ate some carrots." },
	{ name = "Raw fish", fills = 3, says = "You ate some raw fish." },
	{ name = "Wheat", fills = 2, says = "You chewed some wheat." },
}

local food: { [number]: number } = {}
local told: { [number]: number } = {} -- when they were last told they're hungry
local empty: { [number]: number } = {} -- seconds on an empty belly

-- has a bite of something that fits, if they carry any
local function eat(p, f: number): number
	for _, kind in FOODS do
		if f + kind.fills <= FULL and world.has(p, kind.name) >= 1 and world.take(p, kind.name, 1) then
			ui.show(p, "ate", kind.says, { at = "bottom" })
			world.after(3, function()
				ui.hide(p, "ate")
			end)
			sound.play_for(p, "crack", { volume = 0.4, caption = "Crunch" })
			return f + kind.fills
		end
	end
	return f
end

world.every(1, function()
	local now = world.time()
	for _, p in world.players() do
		if world.pockets(p) == nil then
			-- creative: no hunger
			if food[p.id] then
				food[p.id] = nil
				ui.meter(p, "food", nil)
				world.healing(p, 1)
			end
			continue
		end
		local f = (food[p.id] or FULL) - FULL / (knobs.minutes * 60)
		f = math.max(f, 0)
		if f <= HUNGRY then
			f = eat(p, f)
		end
		food[p.id] = f
		-- the bar moves in whole steps, so it isn't sent every second
		ui.meter(p, "food", math.ceil(f) / FULL, { color = knobs.color })
		if f <= 0 then
			world.healing(p, 0)
			empty[p.id] = (empty[p.id] or 0) + 1
			if knobs.hurts and empty[p.id] % HURT_EVERY == 0 then
				world.hurt(p, 1, { down_to = knobs.down_to, guarded = false })
			end
			if now - (told[p.id] or -1000) > 120 then
				told[p.id] = now
				local says = if knobs.hurts
					then "You're starving. Carry carrots or wheat and you'll eat."
					else "You're hungry, so you won't heal. Carry carrots or wheat."
				ui.show(p, "hungry", says, { at = "left" })
				world.after(8, function()
					ui.hide(p, "hungry")
				end)
			end
		else
			empty[p.id] = nil
			if f >= WELL_FED and knobs.well_fed then
				world.healing(p, 2)
			else
				world.healing(p, 1)
			end
		end
	end
end)

world.on("player_left", function(p)
	food[p.id] = nil
	told[p.id] = nil
	empty[p.id] = nil
end)
