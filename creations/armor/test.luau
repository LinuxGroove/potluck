--!strict
-- Armor's own tests: `potluck-server --test=armor`.

local test = require("@potluck/test@0.8")
local world = require("@potluck/world@0.18")
local sky = require("@potluck/sky@0.1")

test.case("using a piece on nothing puts it on, and it keeps off part of a hurt", function()
	local p = test.player("Ada", { survival = true, carrying = { ["Iron coat"] = 1 } })
	world.hurt(p, 10)
	test.wait(0.6)
	test.same(test.health(p), 10, "the whole hurt, with nothing on")
	world.heal(p, 10)
	test.hold(p, "Iron coat")
	test.use_item(p, vector.create(0, 1, 0))
	test.same(world.has(p, "Iron coat"), 0, "it's on, not in the pockets")
	test.check(string.find(test.shown(p, "armor") or "", "put on the iron coat", 1, true) ~= nil, test.shown(p, "armor") or "")
	test.wait(0.6)
	world.hurt(p, 10)
	test.wait(0.6)
	-- six points keep off 21%
	local h = test.health(p)
	test.check(h > 11.5 and h < 12.5, "about 12 left, not " .. h)
end)

test.case("a new piece swaps for the old one", function()
	local p = test.player("Ada", { survival = true, carrying = { ["Wool cap"] = 1, ["Moonsilver helmet"] = 1 } })
	test.hold(p, "Wool cap")
	test.use_item(p)
	test.hold(p, "Moonsilver helmet")
	test.use_item(p)
	test.same(world.has(p, "Wool cap"), 1, "the cap's back in the pockets")
	test.same(world.has(p, "Moonsilver helmet"), 0, "the helmet's on")
end)

test.case("an empty hand on nothing takes a piece off", function()
	local p = test.player("Ada", { survival = true, carrying = { Shield = 1, ["Copper boots"] = 1 } })
	test.hold(p, "Shield")
	test.use_item(p)
	test.hold(p, "Copper boots")
	test.use_item(p)
	test.hold(p, nil)
	test.use_item(p)
	local q = test.asked(p)
	test.check(q ~= nil, "a question")
	local choices = (q :: any).choices
	test.same(choices[1], "Take off copper boots")
	test.same(choices[2], "Take off shield")
	test.answer(p, 1)
	test.same(world.has(p, "Copper boots"), 1, "the boots are back")
	test.same(world.has(p, "Shield"), 0, "the shield's still on")
end)

test.case("only armor goes on", function()
	local p = test.player("Ada", { survival = true, carrying = { ["Iron pickaxe"] = 1 } })
	test.hold(p, "Iron pickaxe")
	test.use_item(p)
	test.same(world.has(p, "Iron pickaxe"), 1, "still in hand")
end)

-- a player out on a stone floor high in the sky, on a winter's night
local function out_on_a_winter_night(carrying: { [string]: number }): any
	test.danger("normal")
	sky.set_season("winter", 0.5)
	test.daytime(0)
	local mid = vector.create(300.5, 110, 300.5)
	for dx = -2, 2 do
		for dz = -2, 2 do
			world.set_block(mid + vector.create(dx, 0, dz), "Stone")
		end
	end
	local p = test.player("Ada", { survival = true, carrying = carrying })
	return test.move(p, mid + vector.create(0, 1, 0))
end

test.case("winter nights are cold out under the sky, but wool keeps them off", function()
	local p = out_on_a_winter_night({ ["Wool cap"] = 1, ["Wool coat"] = 1 })
	local told = false
	for _ = 1, 32 do
		test.wait(0.5)
		told = told or string.find(test.shown(p, "armor") or "", "cold", 1, true) ~= nil
	end
	local h = test.health(p) :: number
	test.check(h < 20 and h >= 6, "the cold hurts, a little: " .. h)
	test.check(told, "and they're told why")
	test.hold(p, "Wool cap")
	test.use_item(p)
	test.hold(p, "Wool coat")
	test.use_item(p)
	test.wait(6)
	h = test.health(p) :: number
	test.wait(16)
	test.same(test.health(p), h, "dressed in wool, it doesn't")
end)

test.case("a light, a roof or the day keep the cold off too, and it never takes the last of your health", function()
	local p = out_on_a_winter_night({})
	world.set_block(p.position + vector.create(2, 0, 0), "Lantern")
	test.wait(16)
	test.same(test.health(p), 20, "a lantern beside them")
	world.set_block(p.position + vector.create(2, 0, 0), "Air")
	world.set_block(p.position + vector.create(0, 3, 0), "Stone")
	test.wait(16)
	test.same(test.health(p), 20, "a roof over them")
	world.set_block(p.position + vector.create(0, 3, 0), "Air")
	test.daytime(0.5)
	test.wait(16)
	test.same(test.health(p), 20, "and by day")
	test.daytime(0)
	test.wait(120)
	test.same(test.health(p), 6, "never below 6")
end)
