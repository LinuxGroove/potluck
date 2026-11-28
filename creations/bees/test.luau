--!strict
local test = require("@potluck/test@0.6")
local world = require("@potluck/world@0.20")

local function ground(x: number, z: number): vector
	return vector.create(x, world.top(x, z) + 1, z)
end

local function bees(): { number }
	return world.creatures(test.kind("Bee"))
end

-- a hive with flowers round it, put down by a player in creative
local function garden(p): vector
	local x, z = p.position.x + 4, p.position.z
	local hive = ground(x, z)
	test.place(p, hive, "Beehive")
	for i, d in { { 3, 0 }, { -3, 1 }, { 0, 3 }, { 1, -3 }, { 3, 3 } } do
		local f = ground(x + d[1], z + d[2])
		if world.block(f) == "Air" then
			test.place(p, f, if i % 2 == 0 then "Red flower" else "Violet flower")
		end
	end
	return hive
end

test.case("a hive brings two more bees", function()
	local p = test.player("Ada")
	test.wait(1)
	test.same(#bees(), 4, "bees on the Green")
	garden(p)
	test.wait(1)
	test.same(#bees(), 6, "two more for the hive")
end)

test.case("bees fill a hive with honey, and a full one gives two jars", function()
	local p = test.player("Ada")
	local hive = garden(p)
	test.wait(240)
	test.same(world.block(hive), "Full beehive", "full of honey")
	local s = test.player("Bo", { survival = true })
	test.use_block(s, hive)
	test.same(world.has(s, "Honey"), 2, "two jars")
	test.same(world.block(hive), "Beehive", "empty again")
end)

test.case("a broken full hive gives a hive back, and is forgotten", function()
	local p = test.player("Ada", { survival = true })
	local x, z = p.position.x + 3, p.position.z
	local at = ground(x, z)
	world.set_block(at, "Full beehive")
	test.dig(p, at)
	test.same(world.has(p, "Beehive"), 1, "a hive back")
	test.same(world.has(p, "Full beehive"), 0, "not a full one")
end)

test.case("a bopped bee stings in survival", function()
	local p = test.player("Ada", { survival = true })
	test.wait(1)
	local b = test.creature(bees()[1])
	test.hit(p, b)
	test.wait(0.2)
	test.same(test.health(p), 19, "stung")
end)
