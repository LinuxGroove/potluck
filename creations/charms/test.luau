--!strict
-- The Charms creation's own tests: `potluck-server --test=charms`.

local test = require("@potluck/test@0.7")
local world = require("@potluck/world@0.22")

-- a survival player carrying `carrying`, with `xp` XP, and a charm table
-- beside them
local function at_a_table(carrying: { [string]: number }, xp: number): (any, vector)
	local p = test.player("Ada", { survival = true, carrying = carrying })
	test.same(world.reward(p, xp), xp, "the XP to spend")
	local x, z = p.position.x + 2, p.position.z
	local at = vector.create(x, world.top(x, z) + 1, z)
	world.set_block(at, "Charm table")
	return p, at
end

-- what the question in front of them says
local function asked(p): (string, string, { string })
	local q = test.asked(p)
	test.check(q ~= nil, "a question")
	local a = q :: any
	return a.title, a.text, a.choices
end

test.case("a charm table makes the pickaxe in hand Swift, for XP and gloom dust", function()
	local p, at = at_a_table({ ["Iron pickaxe"] = 1, ["Gloom dust"] = 4, Crystal = 1 }, 100)
	test.hold(p, "Iron pickaxe")
	test.use_block(p, at)
	local title, text, choices = asked(p)
	test.same(title, "Charm table: Iron pickaxe")
	test.check(string.find(text, "You have 100 XP, level 4.", 1, true) == 1, text)
	test.same(choices[1], "Swift")
	test.same(choices[2], "Sturdy")
	test.same(choices[3], "Close")
	test.answer(p, 1)
	local _, now = asked(p)
	test.check(string.find(now, "Your iron pickaxe is Swift now.", 1, true) == 1, now)
	test.same(world.charms(p, "Iron pickaxe")[1], "Swift", "the charm")
	test.same(world.xp(p), 60, "the XP it took")
	test.same(world.has(p, "Gloom dust"), 2, "the gloom dust it took")
	test.same(world.has(p, "Crystal"), 0, "the crystal it took")
	test.same(test.heard("sparkle"), 1, "a sparkle")
	test.wait(0.5)
	test.same(world.haste(p), 1.5, "digs faster with it in hand")
	test.same(test.shown(p, "charmed"), "Iron pickaxe: Swift", "says so")
	test.hold(p, nil)
	test.wait(0.5)
	test.same(world.haste(p), 1, "and as usual without")
end)

test.case("without the XP, it charms nothing and takes nothing", function()
	local p, at = at_a_table({ ["Stone shovel"] = 1, ["Gloom dust"] = 4, Crystal = 1 }, 10)
	test.hold(p, "Stone shovel")
	test.use_block(p, at)
	test.answer(p, 1)
	local _, text = asked(p)
	test.check(string.find(text, "You'll need 40 XP for that.", 1, true) == 1, text)
	test.same(#world.charms(p, "Stone shovel"), 0, "no charm")
	test.same(world.xp(p), 10, "the XP still there")
	test.same(world.has(p, "Gloom dust"), 4, "the gloom dust still there")
end)

test.case("a Keen sword hits harder, and a charmed tool takes no charm twice", function()
	local p, at = at_a_table({ ["Iron sword"] = 1, ["Gloom dust"] = 3, ["Gold ingot"] = 1 }, 100)
	test.hold(p, "Iron sword")
	test.use_block(p, at)
	local _, _, choices = asked(p)
	test.same(choices[1], "Keen")
	test.same(#choices, 2, "only Keen fits a sword")
	test.answer(p, 1)
	test.wait(0.5)
	test.same(world.might(p), 1.5, "hits harder")
	test.use_block(p, at)
	local _, text = asked(p)
	test.check(string.find(text, "It has every charm it can take.", 1, true) ~= nil, text)
end)

test.case("an empty hand has nothing to charm", function()
	local p, at = at_a_table({}, 20)
	test.hold(p, nil)
	test.use_block(p, at)
	local _, text = asked(p)
	test.check(string.find(text, "Hold a tool or a weapon", 1, true) ~= nil, text)
end)

test.case("in creative, the table says what it's for", function()
	local p = test.player("Bo")
	local x, z = p.position.x + 2, p.position.z
	local at = vector.create(x, world.top(x, z) + 1, z)
	world.set_block(at, "Charm table")
	test.use_block(p, at)
	local _, text = asked(p)
	test.check(string.find(text, "survival", 1, true) ~= nil, text)
end)
