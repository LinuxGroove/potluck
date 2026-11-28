--!strict
local test = require("@potluck/test@0.6")
local world = require("@potluck/world@0.20")
local store = require("@potluck/store")

local function goats(): { number }
	return world.creatures(test.kind("Goat"))
end

local function kids(): { number }
	return world.creatures(test.kind("Kid"))
end

test.case("a goat gives milk into a bucket, and not again straight away", function()
	local p = test.player("Ada", { survival = true, carrying = { Bucket = 2 } })
	test.wait(1)
	test.same(#goats(), 3, "goats")
	test.hold(p, "Bucket")
	local g = test.creature(goats()[1])
	test.use(p, g)
	test.same(world.has(p, "Milk bucket"), 1, "milk")
	test.same(world.has(p, "Bucket"), 1, "one bucket left")
	test.use(p, g)
	test.same(world.has(p, "Milk bucket"), 1, "no more yet")
	-- another goat has milk to give
	test.use(p, test.creature(goats()[2]))
	test.same(world.has(p, "Milk bucket"), 2, "milk from the next")
end)

test.case("two goats fed wheat have a kid, which grows up into a goat", function()
	local p = test.player("Ada", { survival = true, carrying = { Wheat = 4 } })
	test.wait(1)
	test.hold(p, "Wheat")
	local ids = goats()
	test.use(p, test.creature(ids[1]))
	test.use(p, test.creature(ids[2]))
	test.same(world.has(p, "Wheat"), 2, "each ate one")
	test.wait(25)
	test.same(#kids(), 1, "a kid")
	test.wait(16)
	local kept = store.get("herd") or {}
	test.same(#kept, 1, "the kid is kept with the world")
	test.wait(300)
	test.same(#kids(), 0, "no kid now")
	test.same(#goats(), 4, "it's a goat")
end)

test.case("goats don't have kids unfed", function()
	test.wait(60)
	test.same(#kids(), 0, "no kids")
end)
