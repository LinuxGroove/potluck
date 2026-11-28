--!strict
-- Buckets and fishing's own tests: `potluck-server --test=water`.

local test = require("@potluck/test@0.4")
local world = require("@potluck/world@0.17")

local UP = vector.create(0, 1, 0)

-- a survival player carrying `carrying`, and a pool of water beside them
local function by_water(carrying: { [string]: number }): (any, vector)
	local p = test.player("Ada", { survival = true, carrying = carrying })
	local x, z = p.position.x + 2, p.position.z
	local at = vector.create(x, world.top(x, z) + 1, z)
	world.set_block(at, "Water")
	return p, at
end

-- everything they carry but `but`, counted
local function besides(p, but: string): number
	local n = 0
	for thing, count in world.pockets(p) or {} do
		if thing ~= but then
			n += count
		end
	end
	return n
end

test.case("a bucket fills at water and pours it out on a block", function()
	local p, water = by_water({ Bucket = 1 })
	test.hold(p, "Bucket")
	test.use_block(p, water)
	test.same(world.has(p, "Water bucket"), 1, "full")
	test.same(world.has(p, "Bucket"), 0, "not empty")
	test.hold(p, "Water bucket")
	local x, z = p.position.x - 2, p.position.z
	local ground = vector.create(x, world.top(x, z) + 1, z)
	world.set_block(ground, "Stone")
	world.set_block(ground + UP, "Air")
	test.use_block(p, ground)
	test.same(world.block(ground + UP), "Water", "poured out on top")
	test.same(world.has(p, "Bucket"), 1, "empty again")
	test.check(test.heard("splash") >= 2, "splashes")
end)

test.case("a water bucket leaves a table alone", function()
	local p = test.player("Ada", { survival = true, carrying = { ["Water bucket"] = 1 } })
	test.hold(p, "Water bucket")
	local x, z = p.position.x + 2, p.position.z
	local at = vector.create(x, world.top(x, z) + 1, z)
	world.set_block(at, "Table")
	world.set_block(at + UP, "Air")
	test.use_block(p, at)
	test.same(world.block(at + UP), "Air", "nothing poured")
	test.same(world.has(p, "Water bucket"), 1, "still full")
end)

test.case("a rod in hand catches something after a while", function()
	local p, water = by_water({ ["Fishing rod"] = 1 })
	test.hold(p, "Fishing rod")
	test.use_block(p, water)
	test.check(test.shown(p, "water") ~= nil, "it says it's fishing")
	test.same(test.heard("cast"), 1, "the line goes in")
	test.wait(12)
	test.same(besides(p, "Fishing rod"), 1, "one catch")
	test.same(test.heard("bite"), 1, "a bite")
	test.same(world.has(p, "Fishing rod"), 1, "the rod's still there")
end)

test.case("putting the rod away loses the bite", function()
	local p, water = by_water({ ["Fishing rod"] = 1 })
	test.hold(p, "Fishing rod")
	test.use_block(p, water)
	test.hold(p, nil)
	test.wait(12)
	test.same(besides(p, "Fishing rod"), 0, "nothing caught")
end)
