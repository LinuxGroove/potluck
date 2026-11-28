--!strict
-- Storage's own tests: `potluck-server --test=storage`.

local test = require("@potluck/test@0.7")
local world = require("@potluck/world@0.15")
local store = require("@potluck/store@0.1")

-- a survival player beside a crate they've put down
local function crate(carrying: { [string]: number }): (any, vector)
	carrying.Crate = (carrying.Crate or 0) + 1
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

local UP = vector.create(0, 1, 0)

local function held(at: vector): { [string]: number }
	local kept = store.get("boxes") or {}
	return kept[math.floor(at.x) .. "," .. math.floor(at.y) .. "," .. math.floor(at.z)] or {}
end

-- puts what's in their hand into the holder at `at`
local function put(p, at: vector, thing: string)
	test.hold(p, thing)
	test.use_block(p, at)
	test.answer(p, 1)
end

test.case("a chute moves things down from one crate into another", function()
	local p, low = crate({ Crate = 1, Chute = 1, Stone = 3 })
	local chute, high = low + UP, low + UP * 2
	world.set_block(chute, "Air")
	world.set_block(high, "Air")
	test.place(p, chute, "Chute")
	test.place(p, high, "Crate")
	put(p, high, "Stone")
	test.same(held(high).Stone, 3, "three in the top crate")
	-- two a second, and kept in the store every two seconds
	test.wait(5)
	test.same(held(low).Stone, 3, "all three moved down")
	test.same(next(held(high)), nil, "the top crate's empty")
	test.same(next(held(chute)), nil, "and the chute")
end)

test.case("a chute holds one kind of thing", function()
	local p, at = crate({ Chute = 1, Stone = 2, Planks = 2 })
	local chute = at + vector.create(0, 0, 2)
	world.set_block(chute, "Air")
	world.set_block(chute - UP, "Stone")
	test.place(p, chute, "Chute")
	put(p, chute, "Stone")
	test.same(world.has(p, "Stone"), 0, "the stone's in")
	put(p, chute, "Planks")
	local q = test.asked(p)
	test.check(q ~= nil and string.find(q.text, "one kind of thing", 1, true) ~= nil, q and q.text or "asked")
	test.same(world.has(p, "Planks"), 2, "the planks stay in hand")
end)

test.case("a dispenser faces away from whoever put it down, and keeps things", function()
	local p, at = crate({ Dispenser = 1, Planks = 4 })
	local d = at + vector.create(0, 0, 2)
	world.set_block(d, "Air")
	test.place(p, d, "Dispenser", "east")
	test.same(world.facing(d), "east")
	put(p, d, "Planks")
	test.same(held(d).Planks, 4, "it holds them")
	test.wait(1)
	test.same(world.block(d + vector.create(1, 0, 0)), "Air", "and gives nothing out without power")
end)
