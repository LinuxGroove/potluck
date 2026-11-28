--!strict
local test = require("@potluck/test@0.7")
local world = require("@potluck/world@0.22")

local function boars(): { number }
	return world.creatures(test.kind("Bramble Boar"))
end

-- a grassy wood high in the sky, well away from the Green: where it
-- stands, for a player to stand
local function wood(): vector
	local g = world.spot("green")
	local cx, cz, y = math.floor(g.x) + 90, math.floor(g.z), 90
	for dx = -20, 20 do
		for dz = -20, 20 do
			world.set_block(vector.create(cx + dx, y, cz + dz), "Grass")
			if dx % 8 == 4 and dz % 8 == 4 then
				for dy = 1, 3 do
					world.set_block(vector.create(cx + dx, y + dy, cz + dz), "Oak log")
				end
			end
		end
		test.wait(0)
	end
	return vector.create(cx + 0.5, y + 1, cz + 0.5)
end

test.case("boars root about in the woods by day", function()
	local p = test.player("Ada")
	local at = wood()
	test.move(p, at)
	test.wait(14)
	test.check(#boars() >= 1, "a boar turned up")
end)

test.case("come too close and one charges, then leaves you be a while", function()
	test.danger("normal")
	local p = test.player("Ada", { survival = true })
	local at = wood()
	test.move(p, at)
	world.spawn(test.kind("Bramble Boar"), at + vector.create(3, 0, 0))
	-- it looks about a few seconds first, then snorts and comes
	for _ = 1, 40 do
		test.wait(0.25)
		if test.health(p) < 20 then
			break
		end
	end
	local hurt = test.health(p)
	test.check(hurt < 20, "charged")
	test.wait(4)
	test.same(test.health(p), hurt, "and rests before another")
end)

test.case("on a Peaceful world they never charge", function()
	test.danger("peaceful")
	local p = test.player("Ada", { survival = true })
	local at = wood()
	test.move(p, at)
	local id = world.spawn(test.kind("Bramble Boar"), at + vector.create(2, 0, 0))
	test.wait(6)
	test.same(test.health(p), 20, "not charged")
	test.hit(p, test.creature(id))
	test.wait(3)
	test.same(test.health(p), 20, "not even when bopped")
end)

test.case("beaten, one leaves leather, bristles and meat", function()
	test.danger("peaceful")
	local p = test.player("Ada", { survival = true, carrying = { ["Iron sword"] = 1 } })
	local at = wood()
	test.move(p, at)
	test.hold(p, "Iron sword")
	local id = world.spawn(test.kind("Bramble Boar"), at + vector.create(2, 0, 0))
	test.wait(0.2)
	test.hit(p, test.creature(id))
	test.check(test.creature(id) ~= nil, "tough")
	test.hit(p, test.creature(id))
	test.same(test.creature(id), nil, "beaten with two good bops")
	test.check(world.has(p, "Leather") >= 1, "leather")
	test.check(world.has(p, "Bristles") >= 1, "bristles")
	test.check(world.has(p, "Raw meat") >= 1, "meat")
	test.same(world.xp(p), 5, "and some XP")
end)
