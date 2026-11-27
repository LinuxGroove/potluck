--!strict
-- The Old Greens' own tests. Each case plays in a world of its own, with
-- only this creation in it. Run them with the dev bridge's test tool, or
-- `potluck-server --test=old-greens`.

local test = require("@potluck/test@0.2")
local world = require("@potluck/world@0.5")
local store = require("@potluck/store")

-- the Old Greens, once they're found and built
local function sites(): { any }
	for _ = 1, 40 do
		local all = store.get("sites")
		local built = all ~= nil and #all > 0
		for _, s in all or {} do
			built = built and s.built
		end
		if built then
			return all
		end
		test.wait(0.5)
	end
	error("the Old Greens were never built")
end

-- where a site's hearth is
local function hearth(s: any): vector
	return s.at + vector.create(0, 1, 0)
end

local function site(id: string): any
	for _, s in sites() do
		if s.id == id then
			return s
		end
	end
	error("there's no Old Green " .. id)
end

test.case("three Old Greens stand far from the Green, cold", function()
	local all = sites()
	test.same(#all, 3, "Old Greens")
	local green = world.spot("green")
	for _, s in all do
		test.same(world.block(hearth(s)), "Clay pot", s.id .. "'s hearth")
		local away = vector.magnitude(vector.create(s.at.x - green.x, 0, s.at.z - green.z))
		test.check(away > 80, s.id .. " is only " .. math.floor(away) .. " blocks from the Green")
		test.same(world.block(s.at + vector.create(0, 2, 0)), "Air", "nothing over " .. s.id .. "'s hearth")
	end
	test.same(world.block(site("shore").at), "Sandstone bricks", "the shore's stones")
end)

test.case("without what it wants, a hearth stays cold and says so", function()
	local s = site("woods")
	local p = test.player("Ada", { survival = true, at = s.at + vector.create(2, 1, 2) })
	test.wait(1.2)
	local hint = test.shown(p, "old_green")
	test.check(hint ~= nil and string.find(hint, "4 Oak log", 1, true) ~= nil, "near it, it says what it wants: " .. tostring(hint))
	test.use_block(p, hearth(s))
	test.same(world.block(hearth(s)), "Clay pot", "the hearth")
	test.check(test.heard("cold") == 1, "it sounds cold")
end)

test.case("a lantern and what's round about light one, and it gives something back", function()
	local s = site("shore")
	local p = test.player("Ada", { survival = true, carrying = { Lantern = 1, Sand = 8 }, at = s.at + vector.create(2, 1, 2) })
	test.use_block(p, hearth(s))
	test.same(world.block(hearth(s)), "Brazier", "the hearth")
	local pockets = world.pockets(p)
	test.same(pockets.Lantern, nil, "lanterns left")
	test.same(pockets.Sand, 2, "sand left")
	test.same(pockets["Balloon basket"], 1, "what it gave back")
	test.same(test.heard("relight"), 1, "times it caught")
	test.check(store.get("sites") ~= nil, "it's kept")
	for _, kept in store.get("sites") do
		if kept.id == "shore" then
			test.check(kept.lit, "kept as lit")
		end
	end
end)

test.case("light all three and the island hears of it", function()
	local p = test.player("Ada")
	for _, s in sites() do
		test.move(p, s.at + vector.create(2, 1, 2))
		test.use_block(p, hearth(s))
		test.same(world.block(hearth(s)), "Brazier", s.id .. "'s hearth")
	end
	test.wait(2)
	test.same(test.heard("fanfare"), 1, "fanfares")
end)
