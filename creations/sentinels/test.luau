--!strict
local test = require("@potluck/test@0.7")
local world = require("@potluck/world@0.21")

local function sentinels(): { number }
	return world.creatures(test.kind("Stone Sentinel"))
end

-- a cave deep down with moonsilver in its wall: where to stand, and the ore
local function seam(p): (vector, vector)
	local x, z, y = math.floor(p.position.x), math.floor(p.position.z), -100
	for dx = -10, 10 do
		for dz = -10, 10 do
			world.set_block(vector.create(x + dx, y - 1, z + dz), "Stone")
			for dy = 0, 3 do
				world.set_block(vector.create(x + dx, y + dy, z + dz), "Air")
			end
		end
		test.wait(0)
	end
	local ore = vector.create(x + 8, y, z)
	world.set_block(ore, "Moonsilver ore")
	return vector.create(x - 6 + 0.5, y, z + 0.5), ore
end

test.case("a sentinel stands over moonsilver, and wakes when you come close", function()
	test.danger("normal")
	local p = test.player("Ada", { survival = true })
	local at, ore = seam(p)
	test.move(p, at)
	test.wait(8)
	test.same(#sentinels(), 1, "one stands guard")
	test.wait(4)
	test.same(test.health(p), 20, "asleep while you keep off")
	test.move(p, ore + vector.create(-2, 0, 0.5))
	test.wait(8)
	test.check(test.health(p) < 20, "awake, it slams")
end)

test.case("none on a Peaceful world", function()
	test.danger("peaceful")
	local p = test.player("Ada", { survival = true })
	local at, ore = seam(p)
	test.move(p, ore + vector.create(-2, 0, 0.5))
	test.wait(10)
	test.same(#sentinels(), 0, "none")
end)

test.case("beaten, it leaves a sentinel core, and that seam stays unguarded", function()
	test.danger("gentle")
	local p = test.player("Ada", { survival = true, carrying = { ["Iron sword"] = 1 } })
	local at, ore = seam(p)
	test.move(p, at)
	test.hold(p, "Iron sword")
	test.wait(8)
	local id = sentinels()[1]
	test.check(id ~= nil, "one stands guard")
	for _ = 1, 3 do
		test.hit(p, test.creature(id))
	end
	test.check(test.creature(id) ~= nil, "tough")
	test.hit(p, test.creature(id))
	test.same(test.creature(id), nil, "beaten")
	test.same(world.has(p, "Sentinel core"), 1, "a core")
	test.wait(12)
	test.same(#sentinels(), 0, "and no other comes")
end)
