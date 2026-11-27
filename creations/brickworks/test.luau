--!strict
-- Brickworks' own tests: `potluck-server --test=brickworks`.

local test = require("@potluck/test@0.2")
local world = require("@potluck/world@0.12")

-- a survival player beside a kiln they've put down
local function kiln(carrying: { [string]: number }): (any, vector)
	carrying.Kiln = 1
	local p = test.player("Ada", { survival = true, carrying = carrying })
	local x, z = p.position.x + 2, p.position.z
	local at = vector.create(x, world.top(x, z) + 1, z)
	test.place(p, at, "Kiln")
	return p, at
end

test.case("its blocks are in the world, by name", function()
	for _, name in { "Red bricks", "Terracotta", "Roof tiles", "Clay slab", "Paper lantern", "Kiln" } do
		test.check(pcall(world.kind, name), name)
	end
	test.same(world.kind("Red bricks"), "hard", "kind, from potluck.toml")
	local p = test.player("Bo")
	local x, z = p.position.x + 3, p.position.z
	local at = vector.create(x, world.top(x, z) + 1, z)
	world.set_block(at, "Roof tiles")
	test.same(world.block(at), "Roof tiles")
end)

test.case("the kiln fires dirt into bricks", function()
	local p, at = kiln({ Dirt = 9, Sand = 1 })
	test.same(world.block(at), "Kiln")
	test.use_block(p, at)
	local q = test.asked(p)
	test.same(q.choices[1], "4 Red bricks for 4 Dirt")
	test.answer(p, 1)
	test.same(world.has(p, "Red bricks"), 4)
	test.same(world.has(p, "Dirt"), 5)
	q = test.asked(p)
	test.check(string.find(q.text, "Fired 4 Red bricks", 1, true) ~= nil, q.text)
	-- one sand isn't enough for glass
	test.answer(p, 5)
	q = test.asked(p)
	test.check(string.find(q.text, "You'll need 2 Sand", 1, true) ~= nil, q.text)
	test.same(world.has(p, "Sand"), 1, "nothing taken")
end)

test.case("in creative, it says what it's for", function()
	local p = test.player("Cy")
	local x, z = p.position.x + 2, p.position.z
	local at = vector.create(x, world.top(x, z) + 1, z)
	world.set_block(at, "Kiln")
	test.use_block(p, at)
	local q = test.asked(p)
	test.check(q ~= nil and string.find(q.text, "In survival", 1, true) ~= nil, "a word on what it does")
end)
