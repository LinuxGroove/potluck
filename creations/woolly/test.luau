--!strict
-- The woolly's own tests. Each case plays in a world of its own, with only
-- this creation in it. Run them with the dev bridge's test tool, or
-- `potluck-server --test=woolly`.

local test = require("@potluck/test@0.1")
local world = require("@potluck/world@0.10")

local function sheep(kind: string?): { any }
	local all = {}
	for _, id in world.creatures(if kind then test.kind(kind) else nil) do
		table.insert(all, test.creature(id))
	end
	return all
end

local function far(a: vector, b: vector): number
	return vector.magnitude(vector.create(a.x - b.x, 0, a.z - b.z))
end

test.case("a flock grazes the Green", function()
	test.same(#sheep("Woolly"), 4, "woollies")
	local p = test.player("Ada")
	for _, s in sheep() do
		test.check(far(s.position, p.position) < 20, "one wandered off to " .. tostring(s.position))
	end
end)

test.case("used, one follows you about", function()
	local p = test.player("Ada")
	local s = sheep("Woolly")[1]
	test.move(p, s.position)
	test.use(p, s)
	local x, z = s.position.x + 8, s.position.z
	p = test.move(p, vector.create(x, world.top(x, z) + 1, z))
	test.wait(6)
	test.check(far(test.creature(s.id).position, p.position) < 3.5, "it kept up")
end)

test.case("holding wheat, they come to you, and wander off when you put it away", function()
	local p = test.player("Ada")
	local s = sheep("Woolly")[1]
	local x, z = s.position.x + 7, s.position.z
	test.move(p, vector.create(x, world.top(x, z) + 1, z))
	test.wait(2)
	test.check(far(test.creature(s.id).position, p.position) > 4, "it came before the wheat came out")
	test.hold(p, "Wheat")
	test.wait(6)
	test.check(far(test.creature(s.id).position, p.position) < 3.5, "it came for the wheat")
	test.hold(p, nil)
	local here = test.creature(s.id).position
	x, z = here.x + 10, here.z
	test.move(p, vector.create(x, world.top(x, z) + 1, z))
	test.wait(5)
	test.check(far(test.creature(s.id).position, p.position) > 4, "it went after them with no wheat")
end)

test.case("hit, one runs off", function()
	local p = test.player("Ada")
	local s = sheep("Woolly")[1]
	test.move(p, s.position + vector.create(1, 0, 0))
	local before = far(s.position, p.position)
	test.hit(p, s)
	test.wait(2)
	test.check(far(test.creature(s.id).position, p.position) > before + 4, "it got away")
end)

test.case("sheared in survival, its wool grows back and the flock stays the same", function()
	local p = test.player("Ada", { survival = true })
	local s = sheep("Woolly")[1]
	test.use(p, s)
	test.same(world.pockets(p)["Cream wool"], 1, "wool from shearing")
	test.same(test.creature(s.id).kind, "Shorn woolly", "what it is now")
	test.wait(1)
	test.same(#sheep(), 4, "sheep, shorn or not, just after")
	test.use(p, s)
	test.same(world.pockets(p)["Cream wool"], 1, "a shorn one gives no more")
	test.wait(181)
	test.same(test.creature(s.id).kind, "Woolly", "what it is once it's grown back")
	test.same(#sheep(), 4, "sheep once it's grown back")
end)

test.case("shears get more wool", function()
	local p = test.player("Ada", { survival = true, carrying = { Shears = 1 } })
	test.hold(p, "Shears")
	local s = sheep("Woolly")[1]
	test.use(p, s)
	test.same(world.pockets(p)["Cream wool"], 3, "wool from shearing with shears")
end)

test.case("two woollies fed wheat have a lamb, which grows up into a woolly", function()
	local p = test.player("Ada", { survival = true, carrying = { Wheat = 4 } })
	test.wait(1)
	test.hold(p, "Wheat")
	local two = sheep("Woolly")
	test.use(p, two[1])
	test.use(p, two[2])
	test.same(world.has(p, "Wheat"), 2, "each ate one")
	test.same(world.has(p, "Cream wool"), 0, "feeding isn't shearing")
	test.wait(25)
	test.same(#sheep("Lamb"), 1, "a lamb")
	test.wait(320)
	test.same(#sheep("Lamb"), 0, "no lamb now")
	test.same(#sheep("Woolly"), 5, "it's a woolly")
end)
