--!strict
-- Storage's own tests: `potluck-server --test=storage`.

local test = require("@potluck/test@0.2")
local world = require("@potluck/world@0.8")
local store = require("@potluck/store@0.1")

-- a survival player beside a crate they've put down
local function crate(carrying: { [string]: number }): (any, vector)
	carrying.Crate = 1
	local p = test.player("Ada", { survival = true, carrying = carrying })
	local x, z = p.position.x + 2, p.position.z
	local at = vector.create(x, world.top(x, z) + 1, z)
	test.place(p, at, "Crate")
	return p, at
end

test.case("put in what's in your hand, and take it out again", function()
	local p, at = crate({ Planks = 10, Stone = 3 })
	test.same(world.block(at), "Crate", "the crate")
	test.hold(p, "Planks")
	test.use_block(p, at)
	local q = test.asked(p)
	test.check(q ~= nil and string.find(q.text, "empty") ~= nil, "it starts empty")
	test.same(q.choices[1], "Put in your Planks")
	test.answer(p, 1)
	test.same(world.has(p, "Planks"), 0, "planks left in the hotbar")
	q = test.asked(p)
	test.check(string.find(q.text, "It holds 10 Planks", 1, true) ~= nil, q.text)
	test.hold(p, "Stone")
	test.use_block(p, at)
	test.answer(p, 1)
	q = test.asked(p)
	test.check(string.find(q.text, "10 Planks and 3 Stone", 1, true) ~= nil, q.text)
	-- nothing left in the hand to put in, so only what it holds to take out
	test.same(q.choices[1], "Take out Planks")
	test.same(q.choices[2], "Take out Stone")
	test.answer(p, 1)
	test.same(world.has(p, "Planks"), 10, "planks back")
	-- kept in the store by where it is
	local kept = store.get("boxes") or {}
	local box = kept[math.floor(at.x) .. "," .. math.floor(at.y) .. "," .. math.floor(at.z)]
	test.check(box ~= nil and box.Stone == 3 and box.Planks == nil, "kept in the store")
end)

test.case("broken, a crate gives back what it held", function()
	local p, at = crate({ Planks = 5 })
	test.hold(p, "Planks")
	test.use_block(p, at)
	test.answer(p, 1)
	test.same(world.has(p, "Planks"), 0)
	test.dig(p, at)
	test.same(world.has(p, "Planks"), 5, "the planks came back")
	test.same(next(store.get("boxes") or {}), nil, "nothing's left in the store")
end)

test.case("in creative, it says what it's for", function()
	local p = test.player("Bo")
	local x, z = p.position.x + 2, p.position.z
	local at = vector.create(x, world.top(x, z) + 1, z)
	test.place(p, at, "Barrel")
	test.use_block(p, at)
	local q = test.asked(p)
	test.check(q ~= nil and string.find(q.text, "In creative", 1, true) ~= nil, "told")
end)
