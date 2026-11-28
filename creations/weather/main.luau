--!strict
-- Weather and the seasons (@potluck/sky). The days are counted as the sun
-- goes round (world.daytime), and every few days make a season: spring,
-- summer, autumn and winter in turn. The weather comes in spells of a few
-- minutes, picked by the season: summer is mostly clear, autumn wet and
-- foggy, and in winter it snows instead of raining. Storms throw lightning
-- at the tallest trees near players (sky.strike), which Fire can catch.
-- The game draws all of it on every game in the world, and plays the rain
-- and the thunder; Farming, Fire and Armor read the season and the
-- weather for themselves. What day it is and the spell are kept with the
-- world (store), so a server's year goes on after a restart.

local world = require("@potluck/world@0.23")
local sky = require("@potluck/sky@0.1")
local tweak = require("@potluck/tweak")
local store = require("@potluck/store")

local knobs = tweak {
	days = { 7, min = 1, max = 30, label = "Days in a season", order = 1 },
	seasons = { true, label = "Seasons go by", order = 2 },
	changes = { true, label = "Weather changes", order = 3 },
	storms = { true, label = "Storms and lightning", order = 4 },
	minutes = { 5, min = 1, max = 30, label = "Minutes a spell of weather lasts, about", order = 5 },
}

local climate = require("./climate")
local SEASONS = climate.SEASONS

-- the days gone by, and the time of day last time it looked
local days: number = store.get("days") or 0
local last = world.daytime()

-- the spell of weather now: its kind, how heavy, the wind, and the seconds
-- of it left
type Spell = { kind: string, strength: number, wind: { number }, left: number }
local spell: Spell = store.get("spell") or { kind = "clear", strength = 0, wind = { 0, 0 }, left = 60 }

local function season(): (string, number)
	local per = math.max(1, math.floor(knobs.days))
	local into = days % (per * 4)
	local name = SEASONS[math.floor(into / per) + 1] or "spring"
	-- how far through it, with today's part of a day
	local progress = math.clamp(((into % per) + last) / per, 0, 1)
	return name, progress
end

local function save()
	store.set("days", days)
	store.set("spell", spell)
end

-- Says the weather now, coming in over `over` seconds.
local function show(over: number)
	sky.set_weather(spell.kind, { strength = spell.strength, wind = vector.create(spell.wind[1], 0, spell.wind[2]), over = over })
end

-- A new spell for the season, a few minutes long.
local function next_spell()
	local name = season()
	local kind = if knobs.changes then climate.pick(name, knobs.storms) else "clear"
	spell = {
		kind = kind,
		strength = if kind == "clear" then 0 else world.random(0.3, 1),
		wind = climate.wind(kind),
		left = knobs.minutes * 60 * world.random(0.6, 1.5),
	}
	show(30)
	save()
end

-- At once, as the world starts: the season and the weather it was in.
sky.set_season(season())
show(0)

-- The days, and the season they make.
local said = ""
world.every(1, function()
	local now = world.daytime()
	if now < last - 0.5 then
		-- past midnight
		if knobs.seasons then
			days += 1
		end
		save()
	end
	last = now
	local name, progress = season()
	-- a word to the season every few seconds of its progress, so it isn't
	-- sent to every game each second
	local say = name .. string.format("%.3f", progress)
	if say ~= said then
		said = say
		sky.set_season(name, progress)
	end
	spell.left -= 1
	if spell.left <= 0 then
		next_spell()
	end
end)

-- The tallest thing near `at`, of a dozen places tried round it: a tree's
-- crown, a tower's top. Where lightning goes, just above it.
local function tallest(at: vector): vector?
	local best: vector? = nil
	for _ = 1, 12 do
		local turn = world.random() * 2 * math.pi
		local far = world.random(8, 48)
		local x, z = math.floor(at.x + math.cos(turn) * far), math.floor(at.z + math.sin(turn) * far)
		local y = world.top(x, z)
		-- leaves aren't solid to world.top: climb through a tree's crown
		local up = y + 1
		while up <= y + 12 and world.block(vector.create(x, up, z)) ~= "Air" do
			up += 1
		end
		-- (deep water, or a column nobody's near enough to have)
		local open = world.block(vector.create(x, up, z)) == "Air" and world.block(vector.create(x, up - 1, z)) ~= "Air"
		if open and (best == nil or up > (best :: vector).y) then
			best = vector.create(x + 0.5, up, z + 0.5)
		end
	end
	return best
end

-- Lightning in a storm, every so often, near someone to see it: in any
-- storm, whoever's creation brought it.
local function storm_again()
	local now = sky.weather()
	local wait = if now.kind == "storm" then world.random(6, 20) / math.max(0.3, now.strength) else 5
	world.after(wait, function()
		if sky.weather().kind == "storm" and knobs.storms then
			local players = world.players()
			if #players > 0 then
				local p = players[math.floor(world.random(1, #players + 0.999))]
				local at = tallest(p.position)
				if at then
					sky.strike(at)
				end
			end
		end
		storm_again()
	end)
end
storm_again()
