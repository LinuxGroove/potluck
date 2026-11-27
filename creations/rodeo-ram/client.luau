--!strict
-- Rodeo Ram's client half. This file runs on each player's own game, not
-- on the world's host: main.luau says when your ride starts and how it
-- ended (with net.send), and this keeps the ride's clock at the bottom of
-- your screen, ten times a second, with nothing sent while it runs.

local net = require("@potluck/net@0.1")
local ui = require("@potluck/ui@0.1")
local sound = require("@potluck/sound@0.1")
local world = require("@potluck/world@0.2")

local clock: number? = nil -- the timer that keeps the clock going
local started = 0
local best: number? = nil
local passed = false

local function seconds(t: number): string
	return string.format("%.1f s", t)
end

local function stop()
	if clock then
		world.cancel(clock)
		clock = nil
	end
end

-- on the ram: { best = the record, if there is one }
net.on("ride", function(info: any)
	stop()
	started = world.time()
	best = info and info.best
	passed = false
	clock = world.every(0.1, function()
		local t = world.time() - started
		local line = "Riding " .. seconds(t)
		if best then
			if t > best and not passed then
				passed = true
				sound.play("sparkle", { volume = 0.6, caption = "Past the record" })
			end
			line ..= if passed then "  ·  past the record!" else "  ·  record " .. seconds(best)
		end
		ui.show(nil, "ride", line, { at = "bottom" })
	end)
end)

-- off again: { time = how long, record = whether it's the longest yet }
net.on("done", function(info: any)
	stop()
	if info.time < 0.5 then
		ui.hide(nil, "ride")
		return
	end
	local line = "You rode " .. seconds(info.time)
	ui.show(nil, "ride", if info.record then line .. ", the longest yet!" else line, { at = "bottom" })
	world.after(4, function()
		if not clock then
			ui.hide(nil, "ride")
		end
	end)
end)
