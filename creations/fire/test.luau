--!strict
-- Fire's own tests: `potluck-server --test=fire`.

local test = require("@potluck/test@0.4")
local world = require("@potluck/world@0.14")

-- a spot on the ground a few blocks from a player
local function ground(p, dx: number): vector
	local x, z = p.position.x + dx, p.position.z
	return vector.create(x, world.top(x, z) + 1, z)
end

local SIDES = {
	vector.create(1, 0, 0),
	vector.create(-1, 0, 0),
	vector.create(0, 1, 0),
	vector.create(0, 0, 1),
	vector.create(0, 0, -1),
}

-- how many fires there are round `at`, `r` blocks each way
local function fires_near(at: vector, r: number): number
	local n = 0
	for x = -r, r do
		for y = -1, r do
			for z = -r, r do
				if world.block(at + vector.create(x, y, z)) == "Fire" then
					n += 1
				end
			end
		end
	end
	return n
end

test.case("the fire block is in the world", function()
	test.same(world.kind("Fire"), "plant")
	local p = test.player("Bo")
	local at = ground(p, 3)
	world.set_block(at, "Fire")
	test.same(world.block(at), "Fire")
end)

test.case("a flint and steel lights a fire beside the block it's used on", function()
	local p = test.player("Ada", { survival = true, carrying = { ["Flint and steel"] = 1, Planks = 1 } })
	test.hold(p, "Flint and steel")
	local at = ground(p, 2)
	test.place(p, at, "Planks")
	test.use_block(p, at)
	local lit = false
	for _, d in SIDES do
		if world.block(at + d) == "Fire" then
			lit = true
		end
	end
	test.check(lit, "a fire beside the planks")
	test.same(world.has(p, "Flint and steel"), 1, "one use doesn't wear it out")
end)

test.case("nothing happens with something else in hand", function()
	local p = test.player("Ada", { survival = true, carrying = { Planks = 2 } })
	test.hold(p, "Planks")
	local at = ground(p, 2)
	test.place(p, at, "Planks")
	test.use_block(p, at)
	test.same(fires_near(at, 1), 0)
end)

test.case("a fire with nothing to eat goes out by itself", function()
	local p = test.player("Cy")
	local at = ground(p, 3)
	world.set_block(at - vector.create(0, 1, 0), "Stone")
	test.place(p, at, "Fire")
	test.same(world.block(at), "Fire")
	-- 30 seconds a fire, a third of that with nothing to burn, give or take
	test.wait(16)
	test.same(world.block(at), "Air", "gone out")
end)

test.case("water puts a fire out", function()
	local p = test.player("Cy")
	local at = ground(p, 3)
	test.place(p, at, "Fire")
	world.set_block(at + vector.create(1, 0, 0), "Water")
	test.wait(1)
	test.same(world.block(at), "Air", "put out")
end)

test.case("fire spreads along wood, and burns it away", function()
	local p = test.player("Di")
	local start = ground(p, 4)
	local planks: { vector } = {}
	for i = 0, 5 do
		local at = start + vector.create(i, 0, 0)
		world.set_block(at, "Planks")
		table.insert(planks, at)
	end
	test.place(p, start + vector.create(0, 1, 0), "Fire")
	local most = 0
	for _ = 1, 30 do
		test.wait(2)
		most = math.max(most, fires_near(start + vector.create(3, 0, 0), 4))
	end
	test.check(most >= 2, "it spread: " .. most .. " fires at most")
	local left = 0
	for _, at in planks do
		if world.block(at) == "Planks" then
			left += 1
		end
	end
	test.check(left < 6, "some planks burnt away")
end)

test.case("standing in fire hurts, in survival", function()
	local p = test.player("Ed", { survival = true })
	local feet = vector.create(math.floor(p.position.x) + 0.5, math.floor(p.position.y) + 0.5, math.floor(p.position.z) + 0.5)
	world.set_block(feet, "Fire")
	test.wait(3)
	test.check(test.health(p) < 20, "hurt: " .. test.health(p))
end)

test.case("breaking a fire puts it out, and gives nothing", function()
	local p = test.player("Fay", { survival = true })
	local at = ground(p, 2)
	world.set_block(at, "Fire")
	test.check(test.dig(p, at), "it breaks")
	test.same(world.block(at), "Air")
	test.same(world.has(p, "Fire"), 0, "nothing to carry")
end)
