--!strict
-- The balloon's client half. This file runs on each player's own game, not
-- on the world's host: main.luau says when you climb in and when you're
-- out again (with net.send), and while you're aboard this shows how high
-- you are and what's below, five times a second, from this game's own copy
-- of the world (net 0.2), with nothing sent while it runs.

local net = require("@potluck/net@0.2")
local ui = require("@potluck/ui@0.1")
local world = require("@potluck/world@0.12")

local gauge: number? = nil -- the timer that keeps the height up to date
local last: number? = nil -- the height a moment ago, to tell rising from sinking

local function stop()
	if gauge then
		world.cancel(gauge)
		gauge = nil
	end
	last = nil
	ui.hide(nil, "height")
end

local function show()
	local me = net.me()
	local x, z = me.position.x, me.position.z
	local ground = world.top(x, z)
	local up = math.max(0, me.position.y - (ground + 1))
	local below = world.block(vector.create(x, ground, z))
	local line = string.format("%d m up", math.floor(up + 0.5))
	if last and up > last + 0.05 then
		line ..= ", rising"
	elseif last and up < last - 0.05 then
		line ..= ", sinking"
	end
	if up > 1 then
		line ..= " · over " .. string.lower(below)
	end
	last = up
	ui.show(nil, "height", line, { at = "right" })
end

-- true when you climb in, false when you're out again
net.on("aboard", function(on: any)
	stop()
	if on then
		show()
		gauge = world.every(0.2, show)
	end
end)

-- started afresh (the creation's files changed): ask whether we're aboard
net.send("hello")
