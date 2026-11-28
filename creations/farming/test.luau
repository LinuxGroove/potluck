--!strict
-- Farming's own tests: `potluck-server --test=farming`.

local test = require("@potluck/test@0.4")
local world = require("@potluck/world@0.17")

local UP = vector.create(0, 1, 0)

-- a 7 by 7 patch of grass open to the sky, up in the air, and its middle
local function patch(): vector
	local mid = vector.create(200.5, 110.5, 200.5)
	for dx = -3, 3 do
		for dz = -3, 3 do
			local at = mid + vector.create(dx, 0, dz)
			world.set_block(at, "Grass")
			world.set_block(at + UP, "Air")
		end
	end
	return mid
end

-- how many of `block` there are in the square `reach` each way round `mid`
local function count(mid: vector, reach: number, block: string): number
	local n = 0
	for dx = -reach, reach do
		for dz = -reach, reach do
			if world.block(mid + vector.create(dx, 0, dz)) == block then
				n += 1
			end
		end
	end
	return n
end

-- nine shoots of wheat planted round `mid`, and a player with a can
local function sown(): (any, vector)
	local p = test.player("Ada", { survival = true, carrying = { ["Copper hoe"] = 1, ["Watering can"] = 1, Sickle = 1, Wheat = 9 } })
	local mid = patch()
	test.hold(p, "Copper hoe")
	test.use_block(p, mid)
	for dx = -1, 1 do
		for dz = -1, 1 do
			test.place(p, mid + vector.create(dx, 1, dz), "Wheat")
		end
	end
	return p, mid
end

test.case("a hand digs one block of farmland, a copper hoe three by three", function()
	local p = test.player("Ada", { survival = true, carrying = { ["Copper hoe"] = 1, ["Water bucket"] = 1 } })
	local mid = patch()
	test.hold(p, nil)
	test.use_block(p, mid)
	test.same(count(mid, 3, "Farmland"), 1, "one by hand")
	world.set_block(mid, "Grass")
	test.hold(p, "Copper hoe")
	test.use_block(p, mid)
	test.same(count(mid, 1, "Farmland"), 9, "the square")
	test.same(count(mid, 3, "Farmland"), 9, "and no more")
	-- what carries water doesn't dig
	local other = mid + vector.create(3, 0, 3)
	test.hold(p, "Water bucket")
	test.use_block(p, other)
	test.same(world.block(other), "Grass", "a water bucket leaves grass be")
end)

test.case("a watering can, filled at water, brings crops on", function()
	local p, mid = sown()
	test.same(count(mid + UP, 1, "Tall grass"), 9, "nine shoots")
	test.hold(p, "Watering can")
	test.use_block(p, mid + UP)
	test.check(string.find(test.shown(p, "farming") or "", "Fill", 1, true) == 1, "an empty can says so")
	local water = mid + vector.create(3, 1, 3)
	world.set_block(water, "Water")
	test.use_block(p, water)
	test.same(test.shown(p, "farming"), "Your watering can is full.")
	-- four minutes to ripen, and each watering takes one off
	for _ = 1, 4 do
		test.use_block(p, mid + UP)
	end
	test.check(string.find(test.shown(p, "farming") or "", "12 waterings left", 1, true) ~= nil, test.shown(p, "farming") or "")
	test.wait(6)
	test.same(count(mid + UP, 1, "Wheat"), 9, "all nine ripe")
end)

test.case("a sickle reaps the ripe crops round the one it cuts", function()
	local p, mid = sown()
	local water = mid + vector.create(3, 1, 3)
	world.set_block(water, "Water")
	test.hold(p, "Watering can")
	test.use_block(p, water)
	for _ = 1, 4 do
		test.use_block(p, mid + UP)
	end
	test.wait(6)
	test.hold(p, "Sickle")
	test.dig(p, mid + UP)
	test.same(count(mid + UP, 1, "Air"), 9, "all nine cut")
	-- each gives itself and two more
	test.same(world.has(p, "Wheat"), 27, "wheat")
end)
