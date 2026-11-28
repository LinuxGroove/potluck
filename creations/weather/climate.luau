--!strict
-- The climate: how likely each kind of weather is in each season, and how
-- hard the wind blows. A remix can make its own.

local world = require("@potluck/world@0.23")

local climate = {}

climate.SEASONS = { "spring", "summer", "autumn", "winter" }

-- each season's odds for each kind of weather, as a share of its total
climate.ODDS = {
	spring = { clear = 3, cloudy = 3, rain = 3, storm = 0.6, fog = 1 },
	summer = { clear = 6, cloudy = 2, rain = 1.5, storm = 1, fog = 0.3 },
	autumn = { clear = 2.5, cloudy = 3, rain = 3, storm = 0.7, fog = 2 },
	winter = { clear = 3, cloudy = 3, snow = 4, fog = 1.5 },
} :: { [string]: { [string]: number } }

-- how hard the wind blows in each kind, blocks a second: the least and the most
climate.WIND = {
	clear = { 0, 2 },
	cloudy = { 1, 4 },
	rain = { 2, 5 },
	storm = { 6, 11 },
	snow = { 1, 4 },
	fog = { 0, 1 },
} :: { [string]: { number } }

-- A kind of weather for a season, by its odds: no storms when `storms` is
-- false, and what falls in winter is snow, rain the rest of the year.
function climate.pick(season: string, storms: boolean): string
	local odds = climate.ODDS[season] or climate.ODDS.spring
	-- in a set order, so a world's weather goes the same way from a seed
	local kinds = {}
	for kind in odds do
		if kind ~= "storm" or storms then
			table.insert(kinds, kind)
		end
	end
	table.sort(kinds)
	local total = 0
	for _, kind in kinds do
		total += odds[kind]
	end
	local roll = world.random() * total
	local kind = kinds[#kinds] or "clear"
	for _, k in kinds do
		roll -= odds[k]
		if roll <= 0 then
			kind = k
			break
		end
	end
	if season == "winter" and kind == "rain" then
		return "snow"
	elseif season ~= "winter" and kind == "snow" then
		return "rain"
	end
	return kind
end

-- The wind for a kind of weather: blocks a second along x and z, any way.
function climate.wind(kind: string): { number }
	local w = climate.WIND[kind] or climate.WIND.clear
	local speed = world.random(w[1], w[2])
	local turn = world.random() * 2 * math.pi
	return { math.cos(turn) * speed, math.sin(turn) * speed }
end

return climate
