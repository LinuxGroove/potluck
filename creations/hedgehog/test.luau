--!strict
-- The hedgehog's own tests: `potluck-server --test=hedgehog`, or the dev
-- bridge's test tool.

local test = require("@potluck/test@0.2")
local world = require("@potluck/world@0.11")

local function hedgehogs(): { any }
	local all = {}
	for _, id in world.creatures(test.kind("Hedgehog")) do
		table.insert(all, test.creature(id))
	end
	return all
end

local function far(a: vector, b: vector): number
	return vector.magnitude(vector.create(a.x - b.x, 0, a.z - b.z))
end

test.case("its body comes from the Blockbench model", function()
	local body = world.model("hedgehog.bbmodel")
	test.same(#body, 15, "cubes")
	local by: { [string]: any } = {}
	for _, part in body do
		by[part[1]] = part
	end
	test.check(by.face.head and by.nose.head and by.eye_left.head, "the head group's cubes turn with the head")
	test.check(by.leg_front_left.leg and not by.face.leg, "the legs group's cubes walk")
	test.same(by.nose.color, "#1d1916", "the nose's colour, from the texture")
	-- 16 pixels to a block: the snout is 2 wide, 1.5 high, 2 deep
	test.check(math.abs(by.snout.size.x - 0.125) < 0.001 and math.abs(by.snout.size.z - 0.125) < 0.001, "the snout's size")
	test.check(by.snout.at.z < by.spines.at.z, "it faces negative z")
	test.same(#hedgehogs(), 3, "hedgehogs")
end)

test.case("holding carrots, they come trotting", function()
	local p = test.player("Ada")
	local h = hedgehogs()[1]
	local x, z = h.position.x + 7, h.position.z
	test.move(p, vector.create(x, world.top(x, z) + 1, z))
	test.hold(p, "Carrots")
	test.wait(8)
	test.check(far(test.creature(h.id).position, p.position) < 3.5, "it came for the carrots")
end)

test.case("bopped, one snuffles and runs off", function()
	local p = test.player("Ada")
	local h = hedgehogs()[1]
	test.move(p, h.position + vector.create(1.5, 0, 0))
	test.hit(p, h)
	test.wait(4)
	test.check(test.heard("snuffle") >= 1, "it snuffled")
	test.check(far(test.creature(h.id).position, p.position) > 4, "it ran off")
end)
