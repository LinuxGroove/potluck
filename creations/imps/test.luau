--!strict
local test = require("@potluck/test@0.7")
local world = require("@potluck/world@0.21")

local function imps(): { number }
	return world.creatures(test.kind("Ember Imp"))
end

-- a cavern dug out deep under where a player stands, a row at a time (a
-- whole cavern of blocks at once is more than one tick's work)
local function cavern(p): vector
	local x, z, y = math.floor(p.position.x), math.floor(p.position.z), -70
	for dx = -12, 12 do
		for dz = -12, 12 do
			world.set_block(vector.create(x + dx, y - 1, z + dz), "Stone")
			for dy = 0, 6 do
				world.set_block(vector.create(x + dx, y + dy, z + dz), "Air")
			end
		end
		test.wait(0)
	end
	return vector.create(x + 0.5, y, z + 0.5)
end

test.case("deep down, imps come and throw embers", function()
	test.danger("normal")
	local p = test.player("Ada", { survival = true })
	local at = cavern(p)
	test.move(p, at)
	test.wait(14)
	test.check(#imps() >= 1, "imps came")
	test.wait(14)
	test.check(test.health(p) < 20, "and an ember stung")
end)

test.case("none on a Peaceful world, and none near the top", function()
	test.danger("peaceful")
	local p = test.player("Ada", { survival = true })
	local at = cavern(p)
	test.move(p, at)
	test.wait(10)
	test.same(#imps(), 0, "none on Peaceful")
	test.danger("hard")
	test.move(p, vector.create(at.x, world.top(at.x, at.z) + 1, at.z))
	test.wait(10)
	test.same(#imps(), 0, "none up top")
end)

test.case("two bops pop one, and it leaves an ember", function()
	test.danger("normal")
	local p = test.player("Ada", { survival = true })
	local at = cavern(p)
	test.move(p, at)
	local id = world.spawn(test.kind("Ember Imp"), at + vector.create(2, 1, 0))
	test.wait(0.2)
	test.hit(p, test.creature(id))
	test.hit(p, test.creature(id))
	test.same(test.creature(id), nil, "popped")
	test.check(world.has(p, "Ember") >= 1, "an ember")
end)
