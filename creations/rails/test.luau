--!strict
-- Rails and carts' own tests: `potluck-server --test=rails`.

local test = require("@potluck/test@0.7")
local world = require("@potluck/world@0.23")

local UP = vector.create(0, 1, 0)
local LIFT = 0

-- a flat stone floor high in the sky: its north-west corner, where rails
-- go on it
local function floor(): vector
	local g = world.spot("green")
	local x0, z0, y = math.floor(g.x) + 60, math.floor(g.z) - 12, 90
	for dz = 0, 23 do
		for dx = 0, 11 do
			world.set_block(vector.create(x0 + dx, y, z0 + dz), "Stone")
		end
		test.wait(0)
	end
	return vector.create(x0, y + 1, z0)
end

local function at(o: vector, x: number, y: number, z: number): vector
	return o + vector.create(x, y, z)
end

local function carts(): { number }
	return world.creatures(test.kind("Cart"))
end

-- a cart put down on the rail at c, by a player standing at `from`
local function cart_on(c: vector, from: vector): (any, number)
	local p = test.player("Ada", { survival = true, carrying = { Cart = 1 }, at = from })
	test.place(p, c + UP, "Cart")
	test.wait(0.2)
	local list = carts()
	test.same(#list, 1, "a cart")
	return p, list[1]
end

local function where(id: number): vector
	local c = test.creature(id)
	test.check(c ~= nil, "the cart's there")
	return (c :: any).position
end

-- rides a cart a while, a tenth of a second at a time: where it went
local function ride(id: number, seconds: number): { vector }
	local went = { where(id) }
	for _ = 1, math.floor(seconds * 10) do
		test.wait(0.1)
		table.insert(went, where(id))
	end
	return went
end

-- the most its speed changed from one tenth of a second to the next, where
-- it kept on the same way (a turn or the foot of a slope shortens the
-- straight line between two places it was)
local function jolt(went: { vector }): number
	local most = 0
	for i = 3, #went do
		local a, b = went[i - 1] - went[i - 2], went[i] - went[i - 1]
		local la, lb = vector.magnitude(a), vector.magnitude(b)
		if la < 1e-4 or lb < 1e-4 or vector.dot(a, b) / (la * lb) > 0.995 then
			most = math.max(most, math.abs(lb - la) * 10)
		end
	end
	return most
end

test.case("rails join round a corner and up a slope by themselves", function()
	local o = floor()
	local p = test.player("Ada", { at = at(o, 0, 0, 0) })
	local a, b, c, d = at(o, 4, 0, 4), at(o, 4, 0, 5), at(o, 4, 0, 6), at(o, 5, 0, 6)
	for _, r in { a, b, c, d } do
		test.place(p, r, "Rail", "east")
	end
	test.wait(0.2)
	test.same(world.block(b), "Rail", "a straight rail")
	test.check(world.facing(b) == "north" or world.facing(b) == "south", "running on north and south: " .. tostring(world.facing(b)))
	test.same(world.block(c), "Rail bend", "the corner bends")
	test.same(world.facing(c), "west", "joining north and east")
	test.same(world.block(d), "Rail", "and the rail past it")
	-- a rail on a step up north of the first: the first climbs to it
	world.set_block(at(o, 4, 0, 3), "Stone")
	test.place(p, at(o, 4, 1, 3), "Rail", "east")
	test.wait(0.2)
	test.same(world.block(a), "Rail slope", "a slope up to it")
	test.same(world.facing(a), "north", "climbing north")
	test.check(world.facing(at(o, 4, 1, 3)) == "north" or world.facing(at(o, 4, 1, 3)) == "south", "the one on the step turns to meet it")
	-- a powered rail never bends
	test.place(p, at(o, 8, 0, 4), "Powered rail", "east")
	test.place(p, at(o, 8, 0, 5), "Rail", "north")
	test.place(p, at(o, 9, 0, 4), "Rail", "north")
	test.wait(0.2)
	test.same(world.block(at(o, 8, 0, 4)), "Powered rail", "still a powered rail")
end)

test.case("a cart rolls along smoothly, round a corner, and stops at the end", function()
	local o = floor()
	for z = 2, 11 do
		world.set_block(at(o, 4, 0, z), "Rail", "north")
	end
	world.set_block(at(o, 4, 0, 12), "Rail bend", "west")
	for x = 5, 9 do
		world.set_block(at(o, x, 0, 12), "Rail", "east")
	end
	local p, id = cart_on(at(o, 4, 0, 3), at(o, 4, 0, 0))
	test.same(world.has(p, "Cart"), 0, "the cart put down")
	local rest = where(id)
	test.check(math.abs(rest.y - (o.y + LIFT)) < 0.01, "it sits on the rail: " .. rest.y)
	test.ride(p, test.creature(id))
	test.press(p, "push")
	test.wait(0.2)
	test.release(p, "push")
	local went = ride(id, 12)
	for _, w in went do
		test.check(math.abs(w.y - (o.y + LIFT)) < 0.01, "level all the way: " .. w.y)
	end
	local last = went[#went]
	test.check(vector.magnitude(last - at(o, 9.5, LIFT, 12.5)) < 0.3, "it came to rest at the end of the line: " .. tostring(last))
	test.check(jolt(went) < 0.4, "it gathers and loses speed gently: " .. jolt(went))
	local turned = false
	for _, w in went do
		turned = turned or (w.x > o.x + 6 and math.abs(w.z - (o.z + 12.5)) < 0.05)
	end
	test.check(turned, "it went round the corner and on east")
	test.check(test.heard("rumble") > 0, "it rumbles along")
end)

test.case("up a slope it slows and rolls back, and down one it speeds up", function()
	local o = floor()
	for z = 2, 8 do
		world.set_block(at(o, 4, 0, z), "Rail", "north")
	end
	-- three steps up to the south, and a rail at the top
	for i = 0, 2 do
		for h = 0, i do
			world.set_block(at(o, 4, h, 10 + i), "Stone")
		end
		world.set_block(at(o, 4, i, 9 + i), "Rail slope", "south")
	end
	world.set_block(at(o, 4, 3, 12), "Rail", "north")
	local p, id = cart_on(at(o, 4, 0, 4), at(o, 4, 0, 2))
	test.ride(p, test.creature(id))
	test.press(p, "push")
	test.wait(0.2)
	test.release(p, "push")
	local went = ride(id, 14)
	local high, back = 0, false
	for i, w in went do
		high = math.max(high, w.y - o.y)
		back = back or (i > 1 and w.z < went[i - 1].z - 0.01 and high > 0.5)
	end
	test.check(high > 0.5 and high < 3, "it climbed some way, not to the top: " .. high)
	test.check(back, "and rolled back down")
	test.check(jolt(went) < 0.4, "gently: " .. jolt(went))
	-- back on the flat, it went on north faster than it had gone into the slope
	local fastest = 0
	for i = 2, #went do
		local v = went[i] - went[i - 1]
		if went[i].z < o.z + 8 and v.z < 0 then
			fastest = math.max(fastest, -v.z * 10)
		end
	end
	test.check(fastest > 1, "it came back down with some speed: " .. fastest)
	local rest = went[#went]
	test.check(math.abs(rest.y - (o.y + LIFT)) < 0.01, "and rests on the flat: " .. rest.y)
end)

test.case("a powered rail with no power brings a cart to a stop", function()
	local o = floor()
	for z = 2, 20 do
		world.set_block(at(o, 4, 0, z), if z == 6 then "Powered rail" else "Rail", "north")
	end
	local p, id = cart_on(at(o, 4, 0, 3), at(o, 4, 0, 1))
	test.ride(p, test.creature(id))
	test.press(p, "push")
	test.wait(0.2)
	test.release(p, "push")
	local went = ride(id, 8)
	local last = went[#went]
	test.check(last.z > o.z + 6 and last.z < o.z + 10, "it stopped soon after the powered rail: " .. (last.z - o.z))
	test.check(vector.magnitude(last - went[#went - 5]) < 0.01, "and stays stopped")
	test.check(jolt(went) < 0.4, "gently: " .. jolt(went))
end)

test.case("Brake stops a cart, and an empty one is bopped back into a cart", function()
	local o = floor()
	for z = 2, 20 do
		world.set_block(at(o, 4, 0, z), "Rail", "north")
	end
	local p, id = cart_on(at(o, 4, 0, 3), at(o, 4, 0, 1))
	test.ride(p, test.creature(id))
	test.press(p, "push")
	test.wait(1.5)
	test.release(p, "push")
	test.press(p, "brake")
	local went = ride(id, 3)
	test.check(vector.magnitude(went[#went] - went[#went - 3]) < 0.01, "stopped")
	test.check(went[#went].z < o.z + 12, "well before the end: " .. (went[#went].z - o.z))
	test.release(p, "brake")
	test.hit(p, test.creature(id))
	test.same(#carts(), 1, "not while someone's in it")
	test.get_off(p)
	test.hit(p, test.creature(id))
	test.same(#carts(), 0, "bopped empty, it's gone")
	test.same(world.has(p, "Cart"), 1, "back into a cart")
end)

test.case("a cart goes on a rail, and a bend or a slope breaks into a rail", function()
	local o = floor()
	local p = test.player("Ada", { survival = true, carrying = { Cart = 1 }, at = at(o, 2, 0, 2) })
	test.place(p, at(o, 4, 0, 4), "Cart")
	test.wait(0.2)
	test.same(#carts(), 0, "no cart off the rails")
	test.same(world.block(at(o, 4, 0, 4)), "Air", "nor the block")
	test.same(world.has(p, "Cart"), 1, "it's given back")
	world.set_block(at(o, 6, 0, 4), "Rail bend", "north")
	test.dig(p, at(o, 6, 0, 4))
	test.same(world.has(p, "Rail"), 1, "a bend breaks into a rail")
	world.set_block(at(o, 7, 0, 4), "Rail slope", "east")
	test.dig(p, at(o, 7, 0, 4))
	test.same(world.has(p, "Rail"), 2, "and so does a slope")
end)
