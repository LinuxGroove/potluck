--!strict
-- Storage: crates and barrels keep what you put in them, in survival. Use
-- one (Interact while aiming at it) holding something to put it in, or pick
-- something to take out. What each one holds is kept in the store by where
-- it stands, so a crate keeps its things across a reload, and on a server
-- across restarts.

local world = require("@potluck/world@0.8")
local tweak = require("@potluck/tweak")
local ui = require("@potluck/ui@0.1")
local sound = require("@potluck/sound@0.1")
local store = require("@potluck/store@0.1")

local knobs = tweak {
	kinds = { 6, min = 1, max = 7, label = "Kinds of thing one holds", order = 1 },
}

-- the blocks that hold things
local HOLDERS: { [string]: boolean } = { Crate = true, Barrel = true }
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

-- Puts what's in their hand in, or says why not.
local function put_in(player, at: vector, block: string, thing: string): string
	local k = key(at)
	local box = boxes[k] or {}
	if box[thing] == nil and #names(box) >= math.floor(knobs.kinds) then
		return "It's full: it holds " .. #names(box) .. " kinds of thing already."
	end
	if boxes[k] == nil and filled() >= MOST then
		return "There's no more room in this world's crates and barrels."
	end
	local n = math.min(world.has(player, thing), LOAD)
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
			text = "In creative you've every block already. In survival, crates and barrels keep what you put in them.",
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

world.on("block_used", function(player, at: vector, block: string)
	if HOLDERS[block] then
		open(player, at, block)
	end
end)

-- Broken, a crate gives what it held to whoever broke it, as much as fits;
-- the rest stays there, for a crate put back in its place.
world.on("block_broken", function(player, at: vector, block: string)
	local k = key(at)
	local box = boxes[k]
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
