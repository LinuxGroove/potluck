--!strict
local test = require("@potluck/test@0.6")
local world = require("@potluck/world@0.20")
local store = require("@potluck/store")

local function hens(): { number }
	return world.creatures(test.kind("Hen"))
end

local function chicks(): { number }
	return world.creatures(test.kind("Chick"))
end

test.case("hens peck about the Green and lay eggs", function()
	test.wait(1)
	test.same(#hens(), 4, "hens")
	test.wait(250)
	local eggs = 0
	for _, id in hens() do
		local h = test.creature(id)
		if h and world.find_block(h.position, 16, "Egg") then
			eggs += 1
		end
	end
	test.check(eggs > 0, "an egg lies near a hen")
	local laid = store.get("eggs") or {}
	test.check(#laid > 0, "the eggs are kept")
end)

test.case("a pat gives a feather in survival, but not straight away again", function()
	local p = test.player("Ada", { survival = true })
	test.wait(1)
	local h = test.creature(hens()[1])
	test.use(p, h)
	test.same(world.has(p, "Feather"), 1, "a feather")
	test.use(p, h)
	test.same(world.has(p, "Feather"), 1, "no second feather yet")
end)

test.case("two hens fed seeds make a chick, which grows up into a hen", function()
	local p = test.player("Ada", { survival = true, carrying = { Seeds = 6 } })
	test.wait(1)
	test.hold(p, "Seeds")
	local ids = hens()
	local a, b = test.creature(ids[1]), test.creature(ids[2])
	test.use(p, a)
	test.use(p, b)
	test.same(world.has(p, "Seeds"), 4, "each ate one")
	test.wait(25)
	test.same(#chicks(), 1, "a chick")
	-- both rest a while before another
	test.use(p, a)
	test.same(world.has(p, "Seeds"), 4, "a hen that's just made a chick doesn't eat")
	test.wait(16)
	local kept = store.get("flock") or {}
	test.same(#kept, 1, "the chick is kept with the world")
	-- feeding the chick brings it on a minute
	local c = test.creature(chicks()[1])
	local before = c.grow
	test.use(p, c)
	test.check(c.grow < before - 50, "fed, it grows faster")
	test.wait(260)
	test.same(#chicks(), 0, "no chick now")
	test.same(#hens(), 5, "it's a hen")
end)

test.case("a hen not fed doesn't make a chick", function()
	test.wait(60)
	test.same(#chicks(), 0, "no chicks")
end)
