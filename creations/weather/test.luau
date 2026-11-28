--!strict
-- Weather and seasons' own tests: `potluck-server --test=weather`.

local test = require("@potluck/test@0.8")
local world = require("@potluck/world@0.23")
local sky = require("@potluck/sky@0.1")
local store = require("@potluck/store")
local climate = require("./climate")

-- the sun goes round once: past midnight
local function a_day_passes()
	test.daytime(0.9)
	test.wait(1.2)
	test.daytime(0.05)
	test.wait(1.2)
end

test.case("a week makes a season, and four of them a year", function()
	local name, progress = sky.season()
	test.same(name, "spring", "a new world starts in spring")
	test.check(progress > 0.25 and progress < 0.45, "after the thaw: " .. progress)
	for _ = 1, 3 do
		a_day_passes()
	end
	name, progress = sky.season()
	test.same(name, "spring", "still spring after three days")
	test.check(progress > 0.7 and progress < 0.9, "and most of the way through it: " .. progress)
	for _ = 1, 4 do
		a_day_passes()
	end
	test.same((sky.season()), "summer", "a week on, summer")
	for _ = 1, 14 do
		a_day_passes()
	end
	test.same((sky.season()), "winter", "then autumn, then winter")
	for _ = 1, 7 do
		a_day_passes()
	end
	test.same((sky.season()), "spring", "and round to spring again")
	test.same(store.get("days"), 30, "the days are kept with the world")
end)

test.case("it snows in winter, never rains, and summer is mostly fine", function()
	local seen: { [string]: { [string]: number } } = {}
	for _, season in climate.SEASONS do
		local counts: { [string]: number } = {}
		for _ = 1, 400 do
			local kind = climate.pick(season, true)
			counts[kind] = (counts[kind] or 0) + 1
		end
		seen[season] = counts
	end
	test.same(seen.winter.rain, nil, "no rain in winter")
	test.same(seen.winter.storm, nil, "nor storms")
	test.check((seen.winter.snow or 0) > 100, "but snow: " .. tostring(seen.winter.snow))
	test.same(seen.summer.snow, nil, "no snow in summer")
	test.check((seen.summer.clear or 0) > 200, "summer's mostly clear: " .. tostring(seen.summer.clear))
	test.check((seen.autumn.fog or 0) > (seen.summer.fog or 0) * 2, "autumn's foggier than summer")
	for _ = 1, 200 do
		test.check(climate.pick("summer", false) ~= "storm", "no storms when they're off")
	end
	local w = climate.wind("storm")
	local speed = math.sqrt(w[1] * w[1] + w[2] * w[2])
	test.check(speed >= 6 and speed <= 11, "a storm's wind is strong: " .. speed)
end)

test.case("a storm throws lightning near players, at the top of what's there", function()
	local p = test.player("Ada")
	local struck: { vector } = {}
	sky.on("strike", function(at: vector)
		table.insert(struck, at)
	end)
	sky.set_weather("storm", { strength = 1, over = 0 })
	test.same(sky.weather().kind, "storm")
	test.wait(25)
	test.check(#struck >= 1, "lightning struck: " .. #struck)
	for _, at in struck do
		local d = at - p.position
		test.check(math.sqrt(d.x * d.x + d.z * d.z) <= 50, "near the player")
		test.same(world.block(at), "Air", "just above what it hit")
		test.check(world.block(at - vector.create(0, 1, 0)) ~= "Air", "on top of something")
	end
	-- and none once it clears
	sky.set_weather("clear", { over = 0 })
	test.wait(1)
	local before = #struck
	test.wait(25)
	test.same(#struck, before, "no lightning without a storm")
end)

test.case("lightning is rate-limited, and every creation hears where it struck", function()
	local heard = 0
	sky.on("strike", function()
		heard += 1
	end)
	local made = 0
	for _ = 1, 6 do
		if sky.strike(vector.create(10, 60, 10)) then
			made += 1
		end
	end
	test.same(made, 3, "three at once")
	test.wait(0.3)
	test.same(heard, 3, "and each one heard")
	test.wait(1.2)
	test.check(sky.strike(vector.create(10, 60, 10)), "another a second later")
	-- and what's said about the weather is checked
	test.check(not pcall(function()
		sky.set_weather("sleet" :: any)
	end), "there's no sleet")
	test.check(not pcall(function()
		sky.set_weather("rain", { strength = 2 })
	end), "strength is 0 to 1")
	sky.set_weather("rain", { strength = 0.4, wind = vector.create(3, 0, 4) })
	local now = sky.weather()
	test.same(now.kind, "rain")
	test.check(math.abs(now.strength - 0.4) < 1e-6, "as heavy as it said")
	test.check(math.abs(now.wind.x - 3) < 1e-6 and math.abs(now.wind.z - 4) < 1e-6 and now.wind.y == 0, "the wind as it said")
	sky.set_season("autumn", 0.5)
	local name, progress = sky.season()
	test.same(name, "autumn")
	test.same(progress, 0.5)
end)
