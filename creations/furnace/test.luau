--!strict
-- The furnace creation's own tests: `potluck-server --test=furnace`.

local test = require("@potluck/test@0.4")
local world = require("@potluck/world@0.14")

-- a survival player beside a `block` they've put down
local function beside(block: string, carrying: { [string]: number }): (any, vector)
	carrying[block] = 1
	local p = test.player("Ada", { survival = true, carrying = carrying })
	local x, z = p.position.x + 2, p.position.z
	local at = vector.create(x, world.top(x, z) + 1, z)
	test.place(p, at, block)
	return p, at
end

-- answers the question in front of them with the choice that says `what`
local function pick(p, what: string)
	local q = test.asked(p)
	test.check(q ~= nil, "a question")
	for i, c in q.choices do
		if c == what then
			test.answer(p, i)
			return
		end
	end
	error("no choice " .. what .. " in " .. table.concat(q.choices, ", "))
end

test.case("its blocks are in the world", function()
	for _, name in { "Furnace", "Lit furnace", "Campfire" } do
		test.check(pcall(world.kind, name), name)
	end
end)

test.case("a furnace smelts raw iron into ingots, burning coal", function()
	local p, at = beside("Furnace", { ["Raw iron"] = 3, Coal = 2 })
	test.use_block(p, at)
	pick(p, "Smelt Raw iron")
	test.same(world.has(p, "Raw iron"), 0, "all three went in")
	test.same(world.has(p, "Coal"), 1, "one coal smelts eight")
	test.same(world.block(at), "Lit furnace", "it lit up")
	test.wait(16)
	test.same(world.block(at), "Furnace", "and went out when it was done")
	test.use_block(p, at)
	test.same(world.has(p, "Iron ingot"), 3, "the ingots, taken on using it")
	local q = test.asked(p)
	test.check(q ~= nil and string.find(q.text, "You took 3 Iron ingot", 1, true) ~= nil, q and q.text or "")
end)

test.case("with no fuel, nothing goes in", function()
	local p, at = beside("Furnace", { ["Raw copper"] = 2 })
	test.use_block(p, at)
	pick(p, "Smelt Raw copper")
	local q = test.asked(p)
	test.check(q ~= nil and string.find(q.text, "needs fuel", 1, true) ~= nil, q and q.text or "")
	test.same(world.has(p, "Raw copper"), 2, "nothing taken")
	test.same(world.block(at), "Furnace", "not lit")
end)

test.case("logs burn into charcoal, and planks will do for fuel", function()
	local p, at = beside("Furnace", { ["Oak log"] = 1, Planks = 1 })
	test.use_block(p, at)
	pick(p, "Smelt Oak log")
	test.same(world.has(p, "Planks"), 0, "the planks burn")
	test.wait(6)
	test.use_block(p, at)
	test.same(world.has(p, "Charcoal"), 1)
end)

test.case("a campfire cooks with no fuel", function()
	local p, at = beside("Campfire", { Carrots = 2, Wheat = 3 })
	test.use_block(p, at)
	pick(p, "Cook Carrots")
	test.same(world.has(p, "Carrots"), 0)
	test.wait(11)
	test.use_block(p, at)
	test.same(world.has(p, "Roasted carrots"), 2)
	pick(p, "Cook Wheat")
	test.wait(6)
	test.use_block(p, at)
	test.same(world.has(p, "Bread"), 1, "three wheat make a loaf")
end)

test.case("a campfire can't smelt", function()
	local p, at = beside("Campfire", { ["Raw iron"] = 1, Carrots = 1 })
	test.use_block(p, at)
	local q = test.asked(p)
	test.check(q ~= nil, "a question")
	for _, c in q.choices do
		test.check(string.find(c, "Smelt", 1, true) == nil, c)
	end
end)

test.case("breaking a furnace gives back what was in it", function()
	local p, at = beside("Furnace", { ["Raw gold"] = 4, Coal = 1, ["Stone pickaxe"] = 1 })
	test.use_block(p, at)
	pick(p, "Smelt Raw gold")
	test.wait(6)
	test.hold(p, "Stone pickaxe")
	test.check(test.dig(p, at), "it breaks")
	test.same(world.has(p, "Furnace"), 1, "a lit furnace breaks into a furnace")
	test.same(world.has(p, "Raw gold") + world.has(p, "Gold ingot"), 4, "all the gold back, smelted or not")
	test.check(world.has(p, "Gold ingot") >= 1, "one was done")
end)

test.case("in creative, it says what it's for", function()
	local p = test.player("Cy")
	local x, z = p.position.x + 2, p.position.z
	local at = vector.create(x, world.top(x, z) + 1, z)
	world.set_block(at, "Furnace")
	test.use_block(p, at)
	local q = test.asked(p)
	test.check(q ~= nil and string.find(q.text, "In survival", 1, true) ~= nil, "a word on what it does")
end)
