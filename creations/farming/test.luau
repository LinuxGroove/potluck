--!strict
-- Farming's own tests: `potluck-server --test=farming`.

local test = require("@potluck/test@0.4")
local world = require("@potluck/world@0.20")

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
	test.same(count(mid + UP, 1, "Wheat shoots"), 9, "nine shoots")
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
	-- each gives itself and two more, and seeds to sow again
	test.same(world.has(p, "Wheat"), 27, "wheat")
	test.same(world.has(p, "Seeds"), 9, "seeds")
end)

test.case("seeds sow wheat, and potatoes, beans and pumpkins grow too", function()
	local p = test.player("Ada", { survival = true, carrying = { Seeds = 2, Potatoes = 1, Beans = 1, Pumpkin = 1 } })
	local mid = patch()
	for dx = -2, 2 do
		world.set_block(mid + vector.create(dx, 0, 0), "Farmland")
	end
	test.hold(p, "Seeds")
	test.use_block(p, mid + vector.create(-2, 0, 0))
	test.same(world.block(mid + vector.create(-2, 1, 0)), "Wheat shoots", "seeds on farmland")
	test.same(world.has(p, "Seeds"), 1, "one seed sown")
	test.place(p, mid + vector.create(-1, 1, 0), "Potatoes")
	test.place(p, mid + vector.create(0, 1, 0), "Beans")
	test.place(p, mid + vector.create(1, 1, 0), "Pumpkin")
	test.same(world.block(mid + vector.create(-1, 1, 0)), "Potato shoots")
	test.same(world.block(mid + vector.create(0, 1, 0)), "Bean shoots")
	test.same(world.block(mid + vector.create(1, 1, 0)), "Pumpkin vine")
	-- broken too soon, a shoot gives its seed back and nothing else
	test.dig(p, mid + vector.create(-2, 1, 0))
	test.same(world.has(p, "Seeds"), 2, "the seed back")
	test.same(world.has(p, "Wheat shoots"), 0, "and no shoot")
	test.wait(500)
	test.same(world.block(mid + vector.create(-1, 1, 0)), "Potatoes", "potatoes ripen")
	test.same(world.block(mid + vector.create(1, 1, 0)), "Pumpkin", "and a pumpkin")
	test.dig(p, mid + vector.create(1, 1, 0))
	test.same(world.has(p, "Pumpkin"), 3, "a pumpkin and two more")
end)

test.case("a berry bush gives berries, and they grow back", function()
	local p = test.player("Ada", { survival = true })
	local mid = patch()
	local at = mid + UP
	world.set_block(at, "Berry bush")
	test.use_block(p, at)
	local picked = world.has(p, "Berries")
	test.check(picked >= 2 and picked <= 4, "two to four berries, got " .. picked)
	test.same(world.block(at), "Bare berry bush", "picked bare")
	test.use_block(p, at)
	test.same(world.has(p, "Berries"), picked, "nothing more from a bare bush")
	test.wait(190)
	test.same(world.block(at), "Berry bush", "the berries are back")
	-- dug up, a bush comes away bare, with its berries
	test.dig(p, at)
	test.same(world.has(p, "Bare berry bush"), 1, "a bare bush to plant again")
	test.same(world.has(p, "Berries"), picked + 2, "and its berries")
end)

test.case("tall grass gives seeds now and then", function()
	local p = test.player("Ada", { survival = true })
	local mid = patch()
	for dx = -3, 3 do
		for dz = -3, 3 do
			world.set_block(mid + vector.create(dx, 1, dz), "Tall grass")
		end
	end
	for dx = -3, 3 do
		for dz = -3, 3 do
			test.dig(p, mid + vector.create(dx, 1, dz))
		end
	end
	local n = world.has(p, "Seeds")
	test.check(n > 5 and n < 45, "about half of 49 give a seed, got " .. n)
end)
