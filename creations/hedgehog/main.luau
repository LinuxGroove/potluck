--!strict
-- Hedgehog: a creature with no code of its own. Its body is a Blockbench
-- model (hedgehog.bbmodel, made in Blockbench: open it there to change it),
-- and its behaviours do the rest.

local world = require("@potluck/world@0.11")
local tweak = require("@potluck/tweak")

local knobs = tweak {
	count = { 3, min = 0, max = 8, label = "Hedgehogs", order = 1 },
	spines = { "#5b4532", label = "Spines", order = 2 },
}

-- world.model reads the model into the same list of boxes a body is, so
-- it can be changed before it's used: here, the spines take the knob's colour
local body = world.model("hedgehog.bbmodel")
for _, part in body do
	if string.find(part[1], "spines") then
		part.color = knobs.spines
	end
end

world.creature {
	name = "Hedgehog",
	speed = 1.4,
	count = math.floor(knobs.count),
	home = "start",
	body = body,
	behaviours = {
		follow_food = "Carrots",
		flee_when_hit = 5,
		chatter = { "Snuffle.", "Hff hff.", "Snrrk." },
		sound = "snuffle",
	},
}
