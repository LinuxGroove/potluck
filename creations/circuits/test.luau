--!strict
-- Circuits' own tests: `potluck-server --test=circuits`.

local test = require("@potluck/test@0.7")
local world = require("@potluck/world@0.23")

-- a row of spots in the air east of a player, clear to build in
local function row(p, n: number): { vector }
	local x, z = math.floor(p.position.x) + 3, math.floor(p.position.z) + 3
	local y = world.top(x, z) + 4
	local out = {}
	for i = 0, n - 1 do
		local at = vector.create(x + i, y, z)
		world.set_block(at, "Air")
		table.insert(out, at)
	end
	return out
end

local function lit(at: vector): boolean
	return world.block(at) == "Lit copper lamp"
end

test.case("its blocks are in the world", function()
	for _, name in { "Copper wire", "Live wire", "Lever", "Pulled lever", "Button", "Pressure plate", "Daylight sensor", "Inverter", "Lit inverter", "Delay", "Lit delay", "Copper lamp", "Lit copper lamp", "Copper gate", "Open copper gate", "Piston", "Piston head", "Up piston", "Up piston head", "Note block" } do
		test.check(pcall(world.kind, name), name)
	end
end)

test.case("a lever lights a lamp along a wire, and puts it out", function()
	local p = test.player("Ada")
	local at = row(p, 5)
	test.place(p, at[1], "Lever")
	for i = 2, 4 do
		test.place(p, at[i], "Copper wire")
	end
	test.place(p, at[5], "Copper lamp")
	test.wait(0.5)
	test.check(not lit(at[5]), "dark to start with")
	test.use_block(p, at[1])
	test.wait(0.5)
	test.same(world.block(at[1]), "Pulled lever")
	test.same(world.block(at[3]), "Live wire", "the wire is live")
	test.check(lit(at[5]), "and the lamp lit")
	test.use_block(p, at[1])
	test.wait(0.5)
	test.same(world.block(at[3]), "Copper wire")
	test.check(not lit(at[5]), "out again")
end)

test.case("an inverter faces away from whoever put it down, and turns power round", function()
	local p = test.player("Bo")
	local at = row(p, 4)
	test.place(p, at[1], "Lever")
	test.place(p, at[2], "Copper wire")
	test.place(p, at[3], "Inverter", "east")
	test.place(p, at[4], "Copper lamp")
	test.same(world.facing(at[3]), "east")
	test.same(world.facing(at[2]), nil, "wire doesn't turn")
	test.wait(0.6)
	test.same(world.block(at[3]), "Lit inverter", "nothing in, so power out")
	test.same(world.facing(at[3]), "east", "it still faces east lit")
	test.check(lit(at[4]), "the lamp in front is lit")
	test.use_block(p, at[1])
	test.wait(0.6)
	test.same(world.block(at[3]), "Inverter")
	test.check(not lit(at[4]), "power in, so none out")
end)

test.case("an inverter gives power only in front", function()
	local p = test.player("Cy")
	local at = row(p, 3)
	-- facing west, its front is the lamp at at[1]; the lamp behind stays dark
	test.place(p, at[1], "Copper lamp")
	test.place(p, at[2], "Inverter", "west")
	test.place(p, at[3], "Copper lamp")
	test.wait(0.6)
	test.check(lit(at[1]), "in front")
	test.check(not lit(at[3]), "behind")
end)

test.case("a delay passes power on later, as long as it's set to", function()
	local p = test.player("Di")
	local at = row(p, 5)
	test.place(p, at[1], "Lever")
	test.place(p, at[2], "Copper wire")
	test.place(p, at[3], "Delay", "east")
	test.place(p, at[4], "Copper wire")
	test.place(p, at[5], "Copper lamp")
	-- 0.1, then 0.2, 0.3 and 0.5 seconds
	for _ = 1, 3 do
		test.use_block(p, at[3])
	end
	test.same(test.shown(p, "delay"), "Delay: 0.5 seconds")
	test.wait(0.3)
	test.use_block(p, at[1])
	test.wait(0.3)
	test.check(not lit(at[5]), "not yet")
	test.wait(0.6)
	test.same(world.block(at[3]), "Lit delay")
	test.check(lit(at[5]), "now")
end)

test.case("a button gives power for a moment", function()
	local p = test.player("Ed")
	local at = row(p, 2)
	test.place(p, at[1], "Button")
	test.place(p, at[2], "Copper lamp")
	test.use_block(p, at[1])
	test.wait(0.5)
	test.check(lit(at[2]), "pressed")
	test.wait(1.5)
	test.check(not lit(at[2]), "and let go")
end)

test.case("a pressure plate gives power while someone stands on it", function()
	local p = test.player("Fay")
	local at = row(p, 2)
	test.place(p, at[1], "Pressure plate")
	test.place(p, at[2], "Copper lamp")
	test.wait(0.3)
	test.check(not lit(at[2]), "nobody on it")
	test.move(p, at[1] + vector.create(0.5, 0, 0.5))
	test.wait(0.5)
	test.check(lit(at[2]), "someone on it")
	test.move(p, at[1] + vector.create(0.5, 0, 6.5))
	test.wait(0.5)
	test.check(not lit(at[2]), "they stepped off")
end)

test.case("a daylight sensor gives power by day", function()
	local p = test.player("Gus")
	local at = row(p, 2)
	test.place(p, at[1], "Daylight sensor")
	test.place(p, at[2], "Copper lamp")
	test.wait(0.5)
	test.same(lit(at[2]), world.daylight() >= 0.5)
end)

test.case("wire climbs a step, and gates touching open together", function()
	local p = test.player("Hal")
	local at = row(p, 3)
	local up = at[3] + vector.create(0, 1, 0)
	world.set_block(up, "Air")
	world.set_block(up + vector.create(0, 1, 0), "Air")
	test.place(p, at[1], "Lever")
	test.place(p, at[2], "Copper wire")
	test.place(p, up, "Copper wire")
	test.place(p, up + vector.create(1, 0, 0), "Copper gate")
	test.place(p, up + vector.create(1, 1, 0), "Copper gate")
	test.use_block(p, at[1])
	test.wait(0.5)
	test.same(world.block(up), "Live wire", "up the step")
	test.same(world.block(up + vector.create(1, 0, 0)), "Open copper gate")
	test.same(world.block(up + vector.create(1, 1, 0)), "Open copper gate", "the one above too")
	test.use_block(p, at[1])
	test.wait(0.5)
	test.same(world.block(up + vector.create(1, 1, 0)), "Copper gate", "shut again")
end)

test.case("a live wire breaks into copper wire", function()
	local p = test.player("Ivy", { survival = true, carrying = { Lever = 1, ["Copper wire"] = 1 } })
	local at = row(p, 2)
	test.place(p, at[1], "Lever")
	test.place(p, at[2], "Copper wire")
	test.use_block(p, at[1])
	test.wait(0.5)
	test.same(world.block(at[2]), "Live wire")
	test.check(test.dig(p, at[2]), "it breaks")
	test.same(world.has(p, "Copper wire"), 1)
	test.same(world.has(p, "Live wire"), 0)
	test.check(test.dig(p, at[1]), "and the lever")
	test.same(world.has(p, "Lever"), 1, "a pulled lever breaks into a lever")
end)

test.case("an inverter fed its own power ticks, and a lamp on it blinks no faster than the knob says", function()
	local p = test.player("Jo")
	local at = row(p, 3)
	-- the inverter at at[2] faces east; wire runs from its front round to its back
	local loop = {
		at[3],
		at[3] + vector.create(0, 0, 1),
		at[2] + vector.create(0, 0, 1),
		at[1] + vector.create(0, 0, 1),
		at[1],
	}
	for _, w in loop do
		world.set_block(w, "Air")
		test.place(p, w, "Copper wire")
	end
	local lamp = at[2] + vector.create(0, 0, 2)
	world.set_block(lamp, "Air")
	test.place(p, lamp, "Copper lamp")
	test.place(p, at[2], "Inverter", "east")
	local flips, last = 0, lit(lamp)
	for _ = 1, 30 do
		test.wait(0.1)
		if lit(lamp) ~= last then
			flips += 1
			last = lit(lamp)
		end
	end
	test.check(flips >= 3, "it ticks: " .. flips)
	test.check(flips <= 13, "but no faster than four times a second: " .. flips)
end)

local UP = vector.create(0, 1, 0)

test.case("a piston pushes a line of blocks along, and pulls its head back", function()
	local p = test.player("Kit")
	local at = row(p, 6)
	test.place(p, at[1], "Lever")
	test.place(p, at[2], "Piston", "east")
	world.set_block(at[3], "Stone")
	world.set_block(at[4], "Cobblestone")
	test.wait(0.3)
	test.same(world.block(at[3]), "Stone", "nothing moves without power")
	test.use_block(p, at[1])
	test.wait(0.3)
	test.same(world.block(at[3]), "Piston head", "the head is out")
	test.same(world.facing(at[3]), "east", "facing the way the piston does")
	test.same(world.block(at[4]), "Stone", "the stone pushed one along")
	test.same(world.block(at[5]), "Cobblestone", "and the cobblestone past it")
	test.same(test.heard("push"), 1, "a push")
	test.use_block(p, at[1])
	test.wait(0.3)
	test.same(world.block(at[3]), "Air", "the head back in")
	test.same(world.block(at[4]), "Stone", "what it pushed stays")
	test.same(test.heard("pull"), 1, "a pull")
end)

test.case("a piston can't push what keeps things, and takes its head when it's broken", function()
	local p = test.player("Lu", { survival = true, carrying = { Lever = 1, Piston = 1 } })
	local at = row(p, 4)
	test.place(p, at[1], "Lever")
	test.place(p, at[2], "Piston", "east")
	world.set_block(at[3], "Crate")
	test.use_block(p, at[1])
	test.wait(0.3)
	test.same(world.block(at[3]), "Crate", "the crate stays")
	test.same(test.heard("push"), 0, "no push")
	world.set_block(at[3], "Air")
	test.use_block(p, at[1])
	test.wait(0.3)
	test.use_block(p, at[1])
	test.wait(0.3)
	test.same(world.block(at[3]), "Piston head", "with room, it pushes")
	test.check(test.dig(p, at[2]), "the piston breaks")
	test.same(world.block(at[3]), "Air", "and its head goes with it")
	test.same(world.has(p, "Piston"), 1, "it's given back")
	test.same(world.has(p, "Piston head"), 0, "but no head")
end)

test.case("an up piston lifts what's on it", function()
	local p = test.player("Mo")
	local at = row(p, 2)
	for h = 1, 3 do
		world.set_block(at[1] + UP * h, "Air")
	end
	test.place(p, at[2], "Lever")
	test.place(p, at[1], "Up piston")
	world.set_block(at[1] + UP, "Stone")
	test.use_block(p, at[2])
	test.wait(0.3)
	test.same(world.block(at[1] + UP), "Up piston head")
	test.same(world.block(at[1] + UP * 2), "Stone", "lifted")
	test.use_block(p, at[2])
	test.wait(0.3)
	test.same(world.block(at[1] + UP), "Air", "the head back down")
end)

test.case("a note block plays as power reaches it, a step higher each use, in a voice from what it stands on", function()
	local p = test.player("Ned")
	local at = row(p, 2)
	world.set_block(at[2] - UP, "Stone")
	test.place(p, at[1], "Lever")
	test.place(p, at[2], "Note block")
	test.wait(0.3)
	test.same(test.heard("bell"), 0, "quiet with no power")
	test.use_block(p, at[2])
	test.same(test.shown(p, "note"), "Tuned to G4", "a step up from F#4")
	test.same(test.heard("bell"), 1, "a bell on stone")
	test.use_block(p, at[1])
	test.wait(0.5)
	test.same(test.heard("bell"), 2, "power plays it once")
	test.use_block(p, at[1])
	test.wait(0.3)
	test.same(test.heard("bell"), 2, "and not as power goes")
	world.set_block(at[2] - UP, "Planks")
	test.use_block(p, at[2])
	test.same(test.heard("bass"), 1, "a bass on wood")
	world.set_block(at[2] - UP, "Glass")
	test.use_block(p, at[2])
	test.same(test.heard("chime"), 1, "a chime on glass")
	for _ = 1, 10 do
		test.use_block(p, at[2])
	end
	test.same(test.shown(p, "note"), "Tuned to F#3", "round to the bottom after F#5")
end)
