--!strict
-- The Gnome Diggings' own tests: `potluck-server --test=diggings`.

local test = require("@potluck/test@0.8")
local world = require("@potluck/world@0.25")
local store = require("@potluck/store")

local function on_ground(x: number, z: number): vector
	return vector.create(x, world.top(x, z) + 1, z)
end

-- Walks a player round the island, far from the Green, until a diggings
-- opens near them; says where its first crate is.
local function find_diggings(p): (any, vector?)
	local green = world.spot("green") :: vector
	for i = 0, 23 do
		local a = i * math.pi / 12
		for _, d in { 130, 100, 160 } do
			local x, z = math.floor(green.x + math.cos(a) * d), math.floor(green.z + math.sin(a) * d)
			if world.top(x, z) > 21 then
				test.move(p, on_ground(x, z))
				test.wait(4.5)
				local sites = store.get("sites") :: { any }?
				if sites and #sites > 0 then
					-- let it finish digging
					test.wait(3)
					local crates = store.get("crates") :: { [string]: string }?
					for k, room in crates or {} do
						local x, y, z = string.match(k, "(-?%d+),(-?%d+),(-?%d+)")
						return sites[1], vector.create(tonumber(x) :: number, tonumber(y) :: number, tonumber(z) :: number)
					end
					return sites[1], nil
				end
			end
		end
	end
	return nil, nil
end

test.case("diggings open in a hillside far from the Green, and their crates give prizes once", function()
	local p = test.player("Ada", { survival = true })
	test.wait(3)
	test.same(#(store.get("sites") or {}), 0, "none by the Green")
	local site, crate = find_diggings(p)
	test.check(site ~= nil, "a diggings opened somewhere on the island")
	local s = site :: any
	test.check(s.built, "and it's all dug")
	-- the mouth is a frame of logs, with the way in clear
	local f = ({ vector.create(1, 0, 0), vector.create(0, 0, 1), vector.create(-1, 0, 0), vector.create(0, 0, -1) })[s.dir]
	local r = vector.create(-f.z, 0, f.x)
	test.same(world.block(s.at + r * 2 + vector.create(0, 1, 0)), "Oak log", "a post at the mouth")
	test.same(world.block(s.at + f * 6 + vector.create(0, 2, 0)), "Air", "the gallery's dug")
	local crates = store.get("crates") :: { [string]: string }
	local rooms: { [string]: number } = {}
	for _, room in crates do
		rooms[room] = (rooms[room] or 0) + 1
	end
	test.same(rooms.store, 3, "three crates in the store room")
	test.same(rooms.forge, 2, "two in the forge")
	test.same(rooms.deep, 1, "one at the end of the drift")
	-- opening one
	test.check(crate ~= nil, "a crate")
	local at = crate :: vector
	test.same(world.block(at), "Gnome crate")
	test.move(p, at + vector.create(1, 0, 0))
	test.use_block(p, at)
	local got = 0
	for _, n in world.pockets(p) or {} do
		got += n
	end
	test.check(got > 0, "something in their pockets")
	test.check(string.find(test.shown(p, "diggings") or "", "You found", 1, true) ~= nil, test.shown(p, "diggings") or "")
	test.same(world.block(at), "Air", "the crate's opened")
	test.same((store.get("crates") :: { [string]: string })[string.format("%d,%d,%d", at.x, at.y, at.z)], nil, "and it's gone from the list")
	-- no more diggings near this one
	test.wait(6)
	test.same(#(store.get("sites") :: { any }), 1, "only one")
end)

test.case("a burnt-out lantern lights again with a piece of coal", function()
	local p = test.player("Ada", { survival = true, carrying = { Coal = 1 } })
	local at = p.position + vector.create(2, 1, 0)
	world.set_block(at, "Burnt-out lantern")
	test.use_block(p, at)
	test.same(world.block(at), "Lantern", "lit")
	test.same(world.has(p, "Coal"), 0, "the coal's used")
	local again = p.position + vector.create(-2, 1, 0)
	world.set_block(again, "Burnt-out lantern")
	test.use_block(p, again)
	test.same(world.block(again), "Burnt-out lantern", "no coal, no light")
	test.check(string.find(test.shown(p, "diggings") or "", "coal", 1, true) ~= nil, "says what it wants")
end)

test.case("a torn map leads to a crate buried out on the land", function()
	local p = test.player("Ada", { survival = true, carrying = { ["Torn map"] = 1 } })
	test.hold(p, "Torn map")
	test.use_item(p)
	local says = test.shown(p, "diggings") or ""
	test.check(string.find(says, "The map shows a cross about", 1, true) ~= nil, says)
	local cross = (store.get("crosses") :: { [string]: vector })["Ada"]
	test.check(cross ~= nil, "a cross")
	-- read again, it's the same cross
	test.use_item(p)
	test.same((store.get("crosses") :: { [string]: vector })["Ada"], cross)
	-- walk there (a dry spot near it, off the Green)
	local at = on_ground(cross.x, cross.z)
	test.move(p, at)
	test.wait(1.5)
	local here = test.shown(p, "diggings") or ""
	if string.find(here, "water", 1, true) then
		-- the cross fell in the sea: the map's used up all the same
		test.same(world.has(p, "Torn map"), 0)
		return
	end
	test.check(string.find(here, "Dig down", 1, true) ~= nil, here)
	test.same(world.has(p, "Torn map"), 0, "the map's done its work")
	local x, z = math.floor(at.x), math.floor(at.z)
	local found = false
	for y = at.y - 1, at.y - 6, -1 do
		if world.block(vector.create(x, y, z)) == "Gnome crate" then
			found = true
		end
	end
	test.check(found, "a crate under their feet")
end)

test.case("the brass compass points to the nearest diggings", function()
	local p = test.player("Ada", { survival = true, carrying = { ["Brass compass"] = 1 } })
	test.hold(p, "Brass compass")
	test.use_item(p)
	test.check(string.find(test.shown(p, "diggings") or "", "spins", 1, true) ~= nil, "none yet")
end)

test.case("the diggers' pick digs stone as a copper pickaxe does", function()
	local pick = world.item("Gnome's pick")
	test.check(pick ~= nil, "the pick is an item")
	local it = pick :: any
	test.same(it.kind, "tool")
	test.same(it.digs.level, 3)
	test.same(it.stack, 1)
	test.same(it.by, "diggings")
end)
