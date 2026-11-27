--!strict
-- Tools, for survival. Holding a pickaxe, stone and brick break faster; an
-- axe, wood; a shovel, earth, sand and gravel (world.digging, by the kind
-- of block). Every block a tool breaks wears it a little, a bar beside
-- your health shows how much it has left, and a worn out tool breaks.
-- TOOLS below says what each does, so a remix can change them. Swords are
-- for Gloomlings, who say what a sword does to them.

local world = require("@potluck/world@0.8")
local tweak = require("@potluck/tweak")
local ui = require("@potluck/ui@0.2")
local sound = require("@potluck/sound@0.1")

local knobs = tweak {
	faster = { 1, min = 0.5, max = 3, label = "How much faster tools dig", order = 1 },
	wear = { true, label = "Tools wear out", order = 2 },
	last = { 1, min = 0.5, max = 4, label = "How long tools last", order = 3 },
	color = { "#a8b0b8", label = "Wear bar", order = 4 },
}

type Tool = { digs: string, speed: number, uses: number }

-- What each tool digs faster (soft, hard, wood, plant or glass), how much
-- faster, and how many blocks it breaks before it wears out.
local TOOLS: { [string]: Tool } = {
	["Wooden pickaxe"] = { digs = "hard", speed = 2.5, uses = 60 },
	["Stone pickaxe"] = { digs = "hard", speed = 4, uses = 130 },
	["Wooden axe"] = { digs = "wood", speed = 2.5, uses = 60 },
	["Stone axe"] = { digs = "wood", speed = 4, uses = 130 },
	["Wooden shovel"] = { digs = "soft", speed = 2.5, uses = 60 },
	["Stone shovel"] = { digs = "soft", speed = 4, uses = 130 },
}

for name, t in TOOLS do
	world.digging(name, { [t.digs] = math.min(t.speed * knobs.faster, 10) })
end

local function uses(t: Tool): number
	return math.max(1, math.floor(t.uses * knobs.last))
end

-- how many blocks each player's tools have broken, by tool
local worn: { [number]: { [string]: number } } = {}

world.on("block_broken", function(player, at: vector, block: string)
	-- only in survival, where tools are counted
	if not knobs.wear or world.pockets(player) == nil then
		return
	end
	local held = world.held(player)
	local t = held and TOOLS[held]
	if not held or not t then
		return
	end
	local mine = worn[player.id] or {}
	worn[player.id] = mine
	mine[held] = (mine[held] or 0) + 1
	if mine[held] >= uses(t) then
		mine[held] = 0
		if world.take(player, held, 1) then
			ui.show(player, "broke", "Your " .. string.lower(held) .. " broke.", { at = "bottom" })
			world.after(3, function()
				ui.hide(player, "broke")
			end)
			sound.play_for(player, "crack", { pitch = 1.3, caption = "A tool breaks" })
		end
	end
end)

-- who's been told what the bar means
local told: { [number]: boolean } = {}

-- the wear bar, while a tool is in hand
world.every(0.5, function()
	for _, p in world.players() do
		local held = world.held(p)
		local t = held and TOOLS[held]
		if knobs.wear and held and t and world.pockets(p) ~= nil then
			local used = (worn[p.id] or {})[held] or 0
			-- in twentieths, so it isn't sent for every block
			local left = math.ceil((1 - used / uses(t)) * 20) / 20
			ui.meter(p, "wear", left, { color = knobs.color })
			if not told[p.id] then
				told[p.id] = true
				ui.show(p, "wear_says", "The new bar is your tool's wear. It breaks when empty.", { at = "left" })
				world.after(6, function()
					ui.hide(p, "wear_says")
				end)
			end
		else
			ui.meter(p, "wear", nil)
		end
	end
end)

world.on("player_left", function(p)
	worn[p.id] = nil
	told[p.id] = nil
end)
