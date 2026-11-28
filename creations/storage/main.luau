--!strict
-- Storage: crates and barrels keep what you put in them, in survival. Use
-- one (Interact while aiming at it) holding something to put it in, or pick
-- something to take out. What each one holds is kept in the store by where
-- it stands, so a crate keeps its things across a reload, and on a server
-- across restarts.
--
-- A dispenser holds things too, and gives one out each time power reaches
-- it (from the Circuits creation: live wire, a pulled lever, or a lit
-- inverter or delay facing it): a block is put down in front of it, a
-- water bucket poured (the bucket stays in it), a bucket filled from water
-- in front, and anything else handed to the nearest player. A chute holds
-- one kind of thing, and moves things down from the crate, barrel,
-- dispenser or chute over it into the one under it, until power reaches
-- it.

local world = require("@potluck/world@0.15")
local tweak = require("@potluck/tweak")
local ui = require("@potluck/ui@0.1")
local sound = require("@potluck/sound@0.1")
local store = require("@potluck/store@0.1")

local knobs = tweak {
	kinds = { 6, min = 1, max = 7, label = "Kinds of thing one holds", order = 1 },
	moves = { 2, min = 0.5, max = 8, label = "Things a chute moves a second", order = 2 },
	reach = { 3, min = 1, max = 8, label = "How near a dispenser hands things to someone", order = 3 },
}

-- the blocks that hold things
local HOLDERS: { [string]: boolean } = { Crate = true, Barrel = true, Dispenser = true, Chute = true }
-- kinds of thing a chute holds (the others hold the knob's)
local CHUTE_KINDS = 1
-- how many can hold things in one world (the store has 64 KB)
local MOST = 200
-- the most moved at a time (world.give and world.take move 64 at most)
local LOAD = 64

-- what each holds, by where it is: { ["10,20,30"] = { Planks = 12 } }
type Box = { [string]: number }
local boxes: { [string]: Box } = store.get("boxes") or {}

local function save()
	store.set("boxes", boxes)
end

local function key(at: vector): string
	return math.floor(at.x) .. "," .. math.floor(at.y) .. "," .. math.floor(at.z)
end

local function names(box: Box): { string }
	local list = {}
	for name in box do
		table.insert(list, name)
	end
	table.sort(list)
	return list
end

local function filled(): number
	local n = 0
	for _ in boxes do
		n += 1
	end
	return n
end

-- "12 Planks, 4 Stone and 1 Lantern"
local function holds(box: Box): string
	local parts = {}
	for _, name in names(box) do
		table.insert(parts, box[name] .. " " .. name)
	end
	if #parts == 0 then
		return "nothing"
	end
	local last = table.remove(parts)
	return if #parts == 0 then last else table.concat(parts, ", ") .. " and " .. last
end

local function cut(text: string): string
	return if #text <= 30 then text else string.sub(text, 1, 29) .. "…"
end

local open

-- How many kinds of thing a block holds, and how many of each (nil for
-- as many as you like).
local function room(block: string): (number, number?)
	if block == "Chute" then
		return CHUTE_KINDS, LOAD
	end
	return math.floor(knobs.kinds), nil
end

-- How many more of `thing` the holder at `at`, a `block`, takes.
local function takes(at: vector, block: string, thing: string): number
	local box = boxes[key(at)] or {}
	local kinds, each = room(block)
	if box[thing] == nil and #names(box) >= kinds then
		return 0
	end
	if boxes[key(at)] == nil and filled() >= MOST then
		return 0
	end
	return if each then math.max(each - (box[thing] or 0), 0) else LOAD
end

-- Puts what's in their hand in, or says why not.
local function put_in(player, at: vector, block: string, thing: string): string
	local k = key(at)
	local box = boxes[k] or {}
	local kinds = room(block)
	if box[thing] == nil and #names(box) >= kinds then
		return if kinds == 1 then "It holds one kind of thing, and has some " .. names(box)[1] .. " in it." else "It's full: it holds " .. #names(box) .. " kinds of thing already."
	end
	if boxes[k] == nil and filled() >= MOST then
		return "There's no more room in this world's crates and barrels."
	end
	if takes(at, block, thing) <= 0 then
		return "It's full."
	end
	local n = math.min(world.has(player, thing), takes(at, block, thing))
	if n <= 0 or not world.take(player, thing, n) then
		return "You've no " .. thing .. " to put in."
	end
	box[thing] = (box[thing] or 0) + n
	boxes[k] = box
	save()
	sound.play("click", { at = at, caption = "Something's put away" })
	return "Put in " .. n .. " " .. thing .. "."
end

-- Takes one kind of thing out, as much as their hotbar has room for.
local function take_out(player, at: vector, thing: string): string
	local k = key(at)
	local box = boxes[k]
	if box == nil or box[thing] == nil then
		return "There's no " .. thing .. " in it now."
	end
	local given = world.give(player, thing, math.min(box[thing], LOAD))
	if given == 0 then
		return "Your hotbar is full. Make some room first."
	end
	box[thing] -= given
	if box[thing] <= 0 then
		box[thing] = nil
	end
	if next(box) == nil then
		boxes[k] = nil
	end
	save()
	sound.play("click", { at = at, caption = "Something's taken out" })
	return "Took out " .. given .. " " .. thing .. "."
end

function open(player, at: vector, block: string, said: string?)
	if world.pockets(player) == nil then
		ui.ask(player, {
			title = block,
			text = "In creative you've every block already. In survival, crates, barrels, dispensers and chutes keep what you put in them.",
			choices = { "OK" },
		}, function() end)
		return
	end
	local box = boxes[key(at)] or {}
	local choices: { string } = {}
	local does: { () -> string } = {}
	local hand = world.held(player)
	if hand and world.has(player, hand) > 0 then
		table.insert(choices, cut("Put in your " .. hand))
		table.insert(does, function()
			return put_in(player, at, block, hand)
		end)
	end
	for _, name in names(box) do
		if #choices >= 8 then
			break
		end
		table.insert(choices, cut("Take out " .. name))
		table.insert(does, function()
			return take_out(player, at, name)
		end)
	end
	local text = (if said then said .. "\n\n" else "")
		.. (if next(box) then "It holds " .. holds(box) .. "." else "It's empty. Use it while you hold something to put that in.")
	if #choices == 0 then
		ui.ask(player, { title = block, text = text, choices = { "OK" } }, function() end)
		return
	end
	ui.ask(player, { title = block, text = text, choices = choices }, function(who, choice)
		local act = choice and does[choice]
		if act then
			-- ask again, so moving several things is quick
			open(who, at, block, act())
		end
	end)
end

-- ---- dispensers and chutes ----

local AHEAD: { [string]: vector } = {
	north = vector.create(0, 0, -1),
	east = vector.create(1, 0, 0),
	south = vector.create(0, 0, 1),
	west = vector.create(-1, 0, 0),
}
local BACK = { north = "south", east = "west", south = "north", west = "east" }
local UP = vector.create(0, 1, 0)
-- what a dispenser can put a block down into
local OPEN: { [string]: boolean } = { Air = true, Water = true, ["Tall grass"] = true, Fern = true, Bush = true }

-- dispensers and chutes, by where they are, and whether power reaches them
type Machine = { at: vector, block: string, on: boolean }
local machines: { [string]: Machine } = {}
for _, t in (store.get("machines") or {}) :: { { any } } do
	local at = vector.create(t[1], t[2], t[3])
	machines[key(at)] = { at = at, block = t[4], on = t[5] == 1 }
end
local dirty = false

local function keep()
	local list = {}
	for _, m in machines do
		table.insert(list, { m.at.x, m.at.y, m.at.z, m.block, if m.on then 1 else 0 })
	end
	store.set("machines", list)
end

local function corner(at: vector): vector
	return vector.create(math.floor(at.x), math.floor(at.y), math.floor(at.z))
end

-- Whether power reaches c: live wire or a pulled lever beside, under or
-- over it, or a lit inverter or delay beside it facing it.
local function powered(c: vector): boolean
	for side, d in AHEAD do
		local q = c + d
		local b = world.block(q)
		if b == "Live wire" or b == "Pulled lever" then
			return true
		end
		if (b == "Lit inverter" or b == "Lit delay") and world.facing(q) == BACK[side] then
			return true
		end
	end
	for _, q in { c + UP, c - UP } do
		local b = world.block(q)
		if b == "Live wire" or b == "Pulled lever" then
			return true
		end
	end
	return false
end

-- Takes n of a thing out of what the holder at k holds.
local function remove(k: string, thing: string, n: number)
	local box = boxes[k]
	if not box or not box[thing] then
		return
	end
	box[thing] -= n
	if box[thing] <= 0 then
		box[thing] = nil
	end
	if next(box) == nil then
		boxes[k] = nil
	end
end

local function add(k: string, thing: string, n: number)
	local box = boxes[k] or {}
	box[thing] = (box[thing] or 0) + n
	boxes[k] = box
end

-- Gives out one thing: the first it holds, by name.
local function dispense(m: Machine)
	local k = key(m.at)
	local box = boxes[k]
	local front = m.at + AHEAD[world.facing(m.at) or "north"]
	local mid = front + vector.create(0.5, 0.5, 0.5)
	if box == nil then
		sound.play("click", { at = m.at, pitch = 0.6, caption = "A dispenser clicks, empty" })
		return
	end
	local thing = names(box)[1]
	local b = world.block(front)
	local done = false
	if thing == "Water bucket" and OPEN[b] and b ~= "Water" then
		world.set_block(front, "Water")
		remove(k, thing, 1)
		add(k, "Bucket", 1)
		done = true
	elseif thing == "Bucket" and b == "Water" then
		remove(k, thing, 1)
		add(k, "Water bucket", 1)
		done = true
	elseif OPEN[b] and pcall(function()
		world.set_block(front, thing)
	end) then
		-- a block: put down in front
		remove(k, thing, 1)
		done = true
	else
		-- anything else is handed to whoever's near
		local p = world.nearest_player(mid, knobs.reach)
		if p and world.pockets(p) ~= nil and world.give(p, thing, 1) > 0 then
			remove(k, thing, 1)
			done = true
		end
	end
	dirty = true
	if done then
		sound.play("click", { at = m.at, pitch = 1.3, caption = "A dispenser gives something out" })
	else
		sound.play("click", { at = m.at, pitch = 0.6, caption = "A dispenser clicks" })
	end
end

-- Moves one thing from the holder at `from` (if any) into the one at `to`.
local function move(from: vector, to: vector): boolean
	local fk = key(from)
	local box = boxes[fk]
	local into = world.block(to)
	if box == nil or not HOLDERS[into] or not HOLDERS[world.block(from)] then
		return false
	end
	for _, thing in names(box) do
		if takes(to, into, thing) > 0 then
			remove(fk, thing, 1)
			add(key(to), thing, 1)
			dirty = true
			return true
		end
	end
	return false
end

world.every(0.1, function()
	for k, m in machines do
		if m.block == "Dispenser" then
			local on = powered(m.at)
			if on ~= m.on then
				m.on = on
				dirty = true
				if on then
					dispense(m)
				end
			end
		end
	end
end)

-- chutes move a thing down, then take one from over them, so things go
-- down a line of them a block at a time
local since = 0
world.every(0.1, function()
	since += 0.1
	if since < 1 / knobs.moves then
		return
	end
	since = 0
	for _, m in machines do
		if m.block == "Chute" then
			m.on = powered(m.at)
			if not m.on then
				move(m.at, m.at - UP)
				move(m.at + UP, m.at)
			end
		end
	end
end)

world.every(2, function()
	if dirty then
		dirty = false
		save()
		keep()
	end
end)

world.on("block_placed", function(_player, middle: vector, block: string)
	if block == "Dispenser" or block == "Chute" then
		local at = corner(middle)
		machines[key(at)] = { at = at, block = block, on = false }
		keep()
	end
end)

world.on("block_used", function(player, at: vector, block: string)
	if HOLDERS[block] then
		if (block == "Dispenser" or block == "Chute") and not machines[key(at)] then
			-- one put down before this creation was here
			local c = corner(at)
			machines[key(c)] = { at = c, block = block, on = false }
			keep()
		end
		open(player, at, block)
	end
end)

-- Broken, a crate gives what it held to whoever broke it, as much as fits;
-- the rest stays there, for a crate put back in its place.
world.on("block_broken", function(player, at: vector, block: string)
	local k = key(at)
	local box = boxes[k]
	if machines[k] then
		machines[k] = nil
		keep()
	end
	if not HOLDERS[block] or box == nil or world.pockets(player) == nil then
		return
	end
	for _, name in names(box) do
		while box[name] and box[name] > 0 do
			local given = world.give(player, name, math.min(box[name], LOAD))
			if given == 0 then
				break
			end
			box[name] -= given
			if box[name] <= 0 then
				box[name] = nil
			end
		end
	end
	if next(box) == nil then
		boxes[k] = nil
	else
		ui.show(player, "storage", "Your hotbar's full: the rest waits here", { at = "left" })
		world.after(6, function()
			ui.hide(player, "storage")
		end)
	end
	save()
end)

-- machines kept from before that are gone (the world was changed while
-- this creation was off) are forgotten
world.after(0.2, function()
	local gone = false
	for k, m in machines do
		if world.block(m.at) ~= m.block then
			machines[k] = nil
			gone = true
		end
	end
	if gone then
		keep()
	end
end)
