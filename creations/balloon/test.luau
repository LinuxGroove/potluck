--!strict
-- The balloon's own tests. Each case plays in a world of its own, with only
-- this creation in it; nothing here reaches the world anyone plays in. Run
-- them with the dev bridge's test tool, or `potluck-server --test=balloon`.

local test = require("@potluck/test@0.2")
local world = require("@potluck/world@0.9")

local BASKET = "Balloon basket"

-- a player on the Green, and the open ground three blocks east of them
local function setup(opts: any?)
	local p = test.player("Ada", opts)
	local x, z = p.position.x + 3, p.position.z
	local at = vector.create(x, world.top(x, z) + 1, z)
	return p, at
end

local function only(kind: string): any
	local ids = world.creatures(test.kind(kind))
	test.same(#ids, 1, "how many of " .. kind)
	return test.creature(ids[1])
end

-- put a basket down and wait for it to fill
local function filled(p, at)
	test.place(p, at, BASKET)
	test.wait(4)
	return only("Hot air balloon")
end

test.case("a basket put down fills up into a balloon", function()
	local p, at = setup()
	test.place(p, at, BASKET)
	test.same(world.block(at), "Air", "where the basket went")
	only("Balloon (empty)")
	test.wait(2)
	only("Balloon (filling)")
	test.check(test.heard("burner") > 0, "the burner roars as it fills")
	test.wait(2)
	only("Hot air balloon")
end)

test.case("the burner lifts it and Vent brings it back down", function()
	local p, at = setup()
	local b = filled(p, at)
	local ground = b.position.y
	test.ride(p, b)
	test.check(test.shown(p, "aboard") ~= nil, "riders are told how to fly it")
	test.press(p, "burner")
	test.same(only("Hot air balloon (burner on)").id, b.id, "the burner's lit")
	local roars = test.heard("burner")
	test.wait(4)
	test.check(test.heard("burner") >= roars + 4, "it roars while it's lit")
	test.release(p, "burner")
	local high = test.creature(b.id).position.y
	test.check(high > ground + 2, string.format("it rose from %.1f to only %.1f", ground, high))
	test.press(p, "vent")
	for _ = 1, 30 do
		test.wait(1)
		if test.creature(b.id).position.y < ground + 0.2 then
			break
		end
	end
	test.check(test.creature(b.id).position.y < ground + 0.2, "it came back down")
end)

test.case("a landed balloon packs back into a basket", function()
	local p, at = setup({ survival = true, carrying = { [BASKET] = 1 } })
	local b = filled(p, at)
	test.same(world.pockets(p)[BASKET], nil, "baskets carried once it's down")
	test.hit(p, b)
	test.same(#world.creatures(test.kind("Hot air balloon")), 0, "balloons left")
	test.same(world.pockets(p)[BASKET], 1, "baskets carried once it's packed")
end)

test.case("nobody packs it up while someone's aboard", function()
	local p, at = setup()
	local friend = test.player("Bo")
	local b = filled(p, at)
	test.ride(p, b)
	test.hit(friend, b)
	only("Hot air balloon")
end)
