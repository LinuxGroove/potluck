--!strict
local test = require("@potluck/test@0.7")
local world = require("@potluck/world@0.21")

local function crawlers(): { number }
	return world.creatures(test.kind("Cave Crawler"))
end

-- a dark room dug out well under where a player stands, with them in it
local function cave(p): vector
	local x, z = math.floor(p.position.x), math.floor(p.position.z)
	local y = world.top(x, z) - 16
	for dx = -9, 9 do
		for dz = -9, 9 do
			world.set_block(vector.create(x + dx, y - 1, z + dz), "Stone")
			for dy = 0, 2 do
				world.set_block(vector.create(x + dx, y + dy, z + dz), "Air")
			end
			world.set_block(vector.create(x + dx, y + 3, z + dz), "Stone")
		end
		test.wait(0)
	end
	return vector.create(x + 0.5, y, z + 0.5)
end

test.case("crawlers come at you in a dark cave", function()
	test.danger("normal")
	local p = test.player("Ada", { survival = true })
	local at = cave(p)
	test.move(p, at)
	test.wait(10)
	test.check(#crawlers() >= 2, "crawlers came out")
	test.wait(10)
	test.check(test.health(p) < 20, "and nipped")
end)

test.case("a light keeps them off", function()
	test.danger("normal")
	local p = test.player("Ada", { survival = true })
	local at = cave(p)
	world.set_block(at + vector.create(1, 0, 0), "Lantern")
	test.move(p, at)
	test.wait(12)
	test.same(#crawlers(), 0, "none came")
end)

test.case("none on a Peaceful world, and none for a player in creative", function()
	test.danger("peaceful")
	local p = test.player("Ada", { survival = true })
	local at = cave(p)
	test.move(p, at)
	test.wait(10)
	test.same(#crawlers(), 0, "none on Peaceful")
	test.danger("hard")
	local c = test.player("Cy")
	test.move(c, at)
	test.move(p, at + vector.create(0, 40, 0))
	test.wait(10)
	test.same(#crawlers(), 0, "none for Cy")
end)

test.case("three bops pop one, and it leaves silk", function()
	test.danger("normal")
	local p = test.player("Ada", { survival = true })
	local at = cave(p)
	test.move(p, at)
	local id = world.spawn(test.kind("Cave Crawler"), at + vector.create(2, 0, 0))
	test.wait(0.2)
	local c = test.creature(id)
	test.hit(p, c)
	test.hit(p, c)
	test.check(test.creature(id) ~= nil, "still there after two")
	test.hit(p, c)
	test.same(test.creature(id), nil, "popped")
	test.check(world.has(p, "Silk") >= 1, "silk")
end)
