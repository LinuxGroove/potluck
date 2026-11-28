--!strict
-- Mining's own tests: `potluck-server --test=mining`.

local test = require("@potluck/test@0.7")
local world = require("@potluck/world@0.22")

-- the tools creation gives pickaxes their levels; its own tests don't run it
world.digging("Wooden pickaxe", { hard = 2.5, level = 1 })
world.digging("Stone pickaxe", { hard = 4, level = 2 })
world.digging("Copper pickaxe", { hard = 5, level = 3 })
world.digging("Iron pickaxe", { hard = 6.5, level = 4 })

-- a survival player holding `tool`, and a spot beside them with `block`
local function beside(tool: string, block: string, carrying: { [string]: number }?): (any, vector)
	local c = carrying or {}
	c[tool] = 1
	local p = test.player("Ada", { survival = true, carrying = c })
	test.hold(p, tool)
	local x, z = p.position.x + 2, p.position.z
	local at = vector.create(x, world.top(x, z) + 1, z)
	world.set_block(at, block)
	return p, at
end

test.case("a wooden pickaxe can't break iron ore", function()
	local p, at = beside("Wooden pickaxe", "Iron ore")
	test.check(not test.dig(p, at), "it doesn't break")
	test.same(world.block(at), "Iron ore", "still there")
	test.same(world.has(p, "Raw iron"), 0, "no raw iron")
end)

test.case("coal ore breaks into coal with a wooden pickaxe", function()
	local p, at = beside("Wooden pickaxe", "Coal ore")
	test.check(test.dig(p, at), "it breaks")
	test.same(world.block(at), "Air", "gone")
	local coal = world.has(p, "Coal")
	test.check(coal >= 1 and coal <= 2, "1 or 2 coal, not " .. coal)
	test.same(world.has(p, "Coal ore"), 0, "not the ore itself")
end)

test.case("mining ore gives XP, the rarer the more", function()
	local p, at = beside("Iron pickaxe", "Coal ore")
	test.dig(p, at)
	test.same(world.xp(p), 1, "coal")
	world.set_block(at, "Moonsilver ore")
	test.dig(p, at)
	test.same(world.xp(p), 9, "and moonsilver")
	world.set_block(at, "Stone")
	test.dig(p, at)
	test.same(world.xp(p), 9, "stone gives none")
end)

test.case("a stone pickaxe breaks copper ore but not iron ore", function()
	local p, at = beside("Stone pickaxe", "Copper ore")
	test.check(test.dig(p, at), "copper breaks")
	local copper = world.has(p, "Raw copper")
	test.check(copper >= 1 and copper <= 3, "1 to 3 raw copper, not " .. copper)
	world.set_block(at, "Iron ore")
	test.check(not test.dig(p, at), "iron doesn't")
end)

test.case("a copper pickaxe breaks iron ore into raw iron", function()
	local p, at = beside("Copper pickaxe", "Iron ore")
	test.check(test.dig(p, at), "it breaks")
	test.same(world.has(p, "Raw iron"), 1, "raw iron")
	world.set_block(at, "Gold ore")
	test.check(not test.dig(p, at), "but gold needs better")
end)

test.case("an iron pickaxe breaks gold, crystal and moonsilver", function()
	local p, at = beside("Iron pickaxe", "Gold ore")
	test.check(test.dig(p, at), "gold breaks")
	world.set_block(at, "Crystal ore")
	test.check(test.dig(p, at), "crystal breaks")
	world.set_block(at, "Moonsilver ore")
	test.check(test.dig(p, at), "moonsilver breaks")
	test.same(world.has(p, "Raw gold"), 1, "raw gold")
	test.same(world.has(p, "Raw moonsilver"), 1, "raw moonsilver")
	test.check(world.has(p, "Crystal") >= 1, "crystal")
end)

test.case("bare hands can't break ores, but break stone as ever", function()
	local p = test.player("Ada", { survival = true })
	test.hold(p, nil)
	local x, z = p.position.x + 2, p.position.z
	local at = vector.create(x, world.top(x, z) + 1, z)
	world.set_block(at, "Copper ore")
	test.check(not test.dig(p, at), "the ore stays")
	world.set_block(at, "Stone")
	test.check(test.dig(p, at), "stone breaks")
	test.same(world.has(p, "Stone"), 1, "and gives itself")
end)

test.case("an old chest gives its prizes and falls apart", function()
	local p, at = beside("Stone pickaxe", "Old chest")
	test.use_block(p, at)
	test.same(world.block(at), "Air", "the chest is gone")
	local text = test.shown(p, "found") or ""
	test.check(string.find(text, "You found ", 1, true) == 1, text)
	local got = 0
	for thing, n in world.pockets(p) or {} do
		if thing ~= "Stone pickaxe" then
			got += n
		end
	end
	test.check(got > 0, "something in the pockets")
end)

test.case("with full pockets, the rest stays in the chest", function()
	local p, at = beside("Stone pickaxe", "Old chest", { Dirt = 99 * 35 })
	test.use_block(p, at)
	test.same(world.block(at), "Old chest", "the chest stays")
	test.same(test.shown(p, "found"), "Your pockets are full.")
end)

test.case("a creative player is told chests are for survival", function()
	local p = test.player("Bo")
	local x, z = p.position.x + 2, p.position.z
	local at = vector.create(x, world.top(x, z) + 1, z)
	world.set_block(at, "Old chest")
	test.use_block(p, at)
	test.same(world.block(at), "Old chest", "still there")
	test.same(test.shown(p, "found"), "Old chests hold prizes for survival.")
end)

test.case("gravel turns up flint now and then", function()
	local p, at = beside("Wooden pickaxe", "Gravel")
	local flint = 0
	for _ = 1, 60 do
		world.set_block(at, "Gravel")
		test.dig(p, at)
		flint = world.has(p, "Flint")
	end
	test.check(flint >= 1 and flint < 30, "some flint, not every time: " .. flint)
	test.check(world.has(p, "Gravel") >= 60, "and the gravel too")
end)
