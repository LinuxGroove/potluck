--!strict
-- Rails and carts. Lay Rails and they join the rails beside them by
-- themselves: straight on, round a corner (a Rail bend) and up or down a
-- block (a Rail slope). Put a Cart on a rail and climb in (Interact): Push
-- off sets it rolling, away from where you climbed in from, Brake slows it
-- to a stop, and Interact or Crouch gets you out. Bop an empty cart to take
-- it back.
-- A cart follows the rails with self:follow (world 0.23): no gravity, and
-- it gathers and loses speed gently, so the ride is as calm as a balloon's.
-- It slows going up a slope (and rolls back if it can't make it), speeds up
-- coming down, and coasts to a stop on the flat. A Powered rail with power
-- beside it (live wire, a pulled lever, or a lit inverter or delay facing
-- it) sends a cart on fast, or sets a resting one off; without power it
-- stops it. Carts are kept with the world.

local world = require("@potluck/world@0.23")
local tweak = require("@potluck/tweak")
local input = require("@potluck/input@0.1")
local ui = require("@potluck/ui@0.2")
local sound = require("@potluck/sound@0.2")
local store = require("@potluck/store")

local knobs = tweak {
	push = { 3, min = 1, max = 6, label = "How fast a push sends a cart", order = 1 },
	fast = { 8, min = 3, max = 12, label = "How fast powered rails send it", order = 2 },
	hills = { 3, min = 0.5, max = 6, label = "How much slopes slow it and speed it up", order = 3 },
	coast = { 0.2, min = 0, max = 2, label = "How quickly it slows on the flat", order = 4 },
	wood = { "#8a5a2b", label = "The cart's planks", order = 5 },
}

local RAIL, POWERED, BEND, SLOPE, CART = "Rail", "Powered rail", "Rail bend", "Rail slope", "Cart"
local IS_RAIL = { [RAIL] = true, [POWERED] = true, [BEND] = true, [SLOPE] = true }

-- The four ways, and what's behind and to the right of each.
local D: { [string]: vector } = {
	north = vector.create(0, 0, -1),
	east = vector.create(1, 0, 0),
	south = vector.create(0, 0, 1),
	west = vector.create(-1, 0, 0),
}
local ORDER = { "north", "east", "south", "west" }
local BACK = { north = "south", east = "west", south = "north", west = "east" }
local RIGHT = { north = "east", east = "south", south = "west", west = "north" }
local UP = vector.create(0, 1, 0)
-- how high a cart's feet are above the block a rail is on (its wheels are
-- drawn a little higher, on the rail)
local LIFT = 0
local WHEELS = 2 / 16
-- the fastest anything goes along a path (world 0.23)
local TOP = 12

-- ---- the rails ----

local function rail(c: vector): (string?, string)
	local b = world.block(c)
	if IS_RAIL[b] then
		return b, world.facing(c) or "north"
	end
	return nil, "north"
end

-- A rail's end on side d: 0 at its own height, 1 a block up (a slope's
-- high end), nil if it has none there. A rail runs the way it faces, a bend
-- joins the side behind it to the side on its right, and a slope climbs
-- the way it faces.
local function end_up(c: vector, d: string): number?
	local b, f = rail(c)
	if b == nil then
		return nil
	elseif b == BEND then
		return if d == BACK[f] or d == RIGHT[f] then 0 else nil
	elseif b == SLOPE then
		return if d == f then 1 elseif d == BACK[f] then 0 else nil
	end
	return if d == f or d == BACK[f] then 0 else nil
end

-- The rail joined to this one's end on side d, if there is one: level, a
-- block up or a block down.
local function link(c: vector, d: string): vector?
	local up = end_up(c, d)
	if up == nil then
		return nil
	end
	for nu = 0, 1 do
		local n = c + D[d] + vector.create(0, up - nu, 0)
		if end_up(n, BACK[d]) == nu then
			return n
		end
	end
	return nil
end

-- A rail's other end, from one of them.
local function other(c: vector, d: string): string?
	for _, e in ORDER do
		if e ~= d and end_up(c, e) ~= nil then
			return e
		end
	end
	return nil
end

-- the rail under a cart (on a slope's high edge, the slope or what's past it)
local function under(p: vector): vector?
	local c = vector.create(math.floor(p.x), math.floor(p.y + 0.25), math.floor(p.z))
	if rail(c) then
		return c
	end
	c -= UP
	if rail(c) then
		return c
	end
	return nil
end

-- ---- the way through them ----

local function middle(c: vector): vector
	return c + vector.create(0.5, LIFT + (if rail(c) == SLOPE then 0.5 else 0), 0.5)
end

-- the middle of a rail's side s, at its height there
local function edge(c: vector, s: string): vector
	return c + vector.create(0.5, LIFT + (end_up(c, s) or 0), 0.5) + D[s] * 0.5
end

-- The points a cart goes through on a rail it comes onto at side s and
-- leaves by side out: round a bend in a quarter circle.
local function through(c: vector, s: string, out: string?): { vector }
	local pts = { edge(c, s) }
	if rail(c) == BEND and out then
		local corner = middle(c) + (D[s] + D[out]) * 0.5
		for _, t in { 22.5, 45, 67.5 } do
			local a = math.rad(t)
			table.insert(pts, corner - D[out] * (0.5 * math.cos(a)) - D[s] * (0.5 * math.sin(a)))
		end
	else
		table.insert(pts, middle(c))
	end
	return pts
end

-- what's beside a powered rail that powers it
local function powered(c: vector): boolean
	for _, d in ORDER do
		local q = c + D[d]
		local b = world.block(q)
		if b == "Live wire" or b == "Pulled lever" then
			return true
		end
		if (b == "Lit inverter" or b == "Lit delay") and world.facing(q) == BACK[d] then
			return true
		end
	end
	local b = world.block(c - UP)
	return b == "Live wire" or b == "Pulled lever"
end

-- ---- carts ----

local WHEEL = "#3b3b40"
local IRON = "#a7adb4"

local function body(): { any }
	local wood = knobs.wood
	local parts = {
		{ "plank floor", size = { 0.8, 0.08, 1.0 }, at = { 0, 0.2, 0 }, color = wood },
		{ "plank side", size = { 0.08, 0.42, 1.0 }, at = { -0.38, 0.45, 0 }, color = wood },
		{ "plank side", size = { 0.08, 0.42, 1.0 }, at = { 0.38, 0.45, 0 }, color = wood },
		{ "plank end", size = { 0.68, 0.42, 0.08 }, at = { 0, 0.45, -0.46 }, color = wood },
		{ "plank end", size = { 0.68, 0.42, 0.08 }, at = { 0, 0.45, 0.46 }, color = wood },
		{ "iron rim", size = { 0.86, 0.05, 0.06 }, at = { 0, 0.68, -0.47 }, color = IRON },
		{ "iron rim", size = { 0.86, 0.05, 0.06 }, at = { 0, 0.68, 0.47 }, color = IRON },
		{ "iron rim", size = { 0.06, 0.05, 1.0 }, at = { -0.4, 0.68, 0 }, color = IRON },
		{ "iron rim", size = { 0.06, 0.05, 1.0 }, at = { 0.4, 0.68, 0 }, color = IRON },
	}
	for _, w in { { -0.42, -0.3 }, { 0.42, -0.3 }, { -0.42, 0.3 }, { 0.42, 0.3 } } do
		table.insert(parts, { "iron wheel", size = { 0.08, 0.22, 0.22 }, at = { w[1], 0.11, w[2] }, color = WHEEL })
	end
	-- all of it on the rail, which stands a little above the ground
	for _, part in parts do
		part.at[2] += WHEELS
	end
	return parts
end

local Cart = world.creature {
	name = "Cart",
	count = 0,
	speed = 0,
	home = "here",
	rideable = true,
	-- the rails steer it, not whoever's in it
	steer = false,
	seat = 0.35,
	size = { 0.9, 0.75, 0.9 },
	body = body(),
}

-- where every cart is, kept with the world
local saved: { vector } = store.get("carts") or {}
local where: { [number]: vector } = {}

local function save()
	local list = {}
	for _, at in where do
		table.insert(list, at)
	end
	store.set("carts", list)
end

local function vec(t: { number }): vector
	return vector.create(t[1], t[2], t[3])
end

local function tab(v: vector): { number }
	return { v.x, v.y, v.z }
end

-- How far ahead a cart knows the way: past where it could stop from the
-- fastest it goes.
local AHEAD = 40

-- Adds the rails past the last one it knows of, while there's room.
local function plan(self)
	if not self.at then
		return
	end
	for _ = 1, 12 do
		local left, _, count = self:following()
		if left >= AHEAD or count > 110 then
			return
		end
		local c = vec(self.at)
		local out = other(c, self.from)
		local n = if out then link(c, out) else nil
		if not n or not out then
			-- the end of the line: it slows to a stop at its last point
			return
		end
		local back = BACK[out]
		self:follow(through(n, back, other(n, back)), nil, true)
		self.at = tab(n)
		self.from = back
	end
end

-- Sets a cart off from the rail it's on, out by side d. False if no rail
-- joins that side.
local function start(self, c: vector, d: string): boolean
	local n = link(c, d)
	if not n then
		return false
	end
	local back = BACK[d]
	self:follow(through(n, back, other(n, back)), nil, false)
	self.at = tab(n)
	self.from = back
	plan(self)
	return true
end

-- Sets it off the way that's closest to `toward` (any way that goes, if
-- nothing's said).
local function set_off(self, c: vector, toward: vector?): boolean
	local best, score = nil, -math.huge
	for _, d in ORDER do
		if end_up(c, d) ~= nil and link(c, d) then
			local s = if toward then vector.dot(D[d], toward) else 0
			if s > score then
				best, score = d, s
			end
		end
	end
	return best ~= nil and start(self, c, best)
end

-- it rests on the rail it's on
local function settle(self, c: vector)
	self:follow({ middle(c) }, 0, false)
	self.at = nil
	self.want = 0
end

function Cart:spawned()
	self.want = 0
	local c = under(self.position) or under(self.position - UP)
	if c then
		settle(self, c)
	end
	where[self.id] = self.position
end

function Cart:ridden(player)
	local p = self.position - player.position
	self.away = { p.x, 0, p.z }
	ui.show(player, "cart", "Push off to roll, Brake to slow down. Interact or Crouch to get out.", { at = "bottom" })
	world.after(6, function()
		ui.hide(player, "cart")
	end)
end

function Cart:tick(dt: number)
	local p = self.position
	where[self.id] = p
	local was = if self.was then vec(self.was) else p
	self.was = tab(p)
	local moved = p - was
	local left, along, count = self:following()
	local c = under(p)
	if not c then
		-- off the rails (they were taken up): it stops, and drops to the ground
		if count > 0 then
			self:follow({}, 0, false)
			self.at = nil
		end
		return
	end
	if count == 0 then
		-- put back on a rail
		settle(self, c)
		return
	end
	local b, f = rail(c)
	local resting = along < 0.05 and vector.magnitude(moved) < 0.01
	local want: number = self.want or 0
	local r = self.rider
	-- Push off, on the press
	local pushing = r ~= nil and input.held("push", r)
	if pushing and not self.pushing then
		want = math.max(want, knobs.push)
		if resting or left < 0.3 then
			set_off(self, c, if self.away then vec(self.away) else nil)
		end
	end
	self.pushing = pushing
	if b == POWERED then
		if powered(c) then
			want = math.max(want, knobs.fast)
			if resting and left < 0.3 then
				set_off(self, c, if self.away then vec(self.away) else nil)
			end
		else
			want = 0
		end
	elseif b == SLOPE then
		if moved.y > 0.001 then
			want -= knobs.hills * dt
		elseif moved.y < -0.001 then
			want += knobs.hills * dt
		elseif resting then
			-- stopped on a slope (or couldn't make it up): it rolls back down
			if start(self, c, BACK[f]) then
				want = math.max(want, 0.5)
			end
		end
	else
		want -= knobs.coast * dt
	end
	if r and input.held("brake", r) then
		want = 0
	end
	-- put down a little off its rail: it settles onto it
	if count == 1 and not self.at and left > 0.05 then
		want = math.max(want, 1)
	end
	want = math.clamp(want, 0, TOP)
	self.want = want
	self:follow(nil, want, false)
	plan(self)
	-- the sound of it going
	if along > 0.8 then
		self.rumble_in = (self.rumble_in or 0) - dt
		if self.rumble_in <= 0 then
			self.rumble_in = 0.45
			sound.play("rumble", { at = self, volume = math.min(along / TOP, 1) * 0.5 + 0.1 })
		end
		local here = math.floor(c.x) .. "," .. math.floor(c.z)
		if here ~= self.cell then
			self.cell = here
			self.clacks = (self.clacks or 0) + 1
			if self.clacks % 3 == 0 then
				sound.play("clack", { at = self, volume = 0.35 })
			end
		end
	end
	-- now and then, where it is now
	self.save_in = (self.save_in or 0) - dt
	if self.save_in <= 0 then
		self.save_in = 5
		save()
	end
end

-- bop an empty cart to take it back
function Cart:hit(player)
	if self.rider then
		return
	end
	if world.pockets(player) ~= nil and world.give(player, CART, 1) == 0 then
		ui.show(player, "full", "Make room in your hotbar to take the cart.", { at = "bottom" })
		world.after(3, function()
			ui.hide(player, "full")
		end)
		return
	end
	sound.play("thud", { at = self, caption = "Cart taken back" })
	where[self.id] = nil
	self:remove()
	save()
end

-- ---- laying rails ----

-- the rails that could join one at c: level, a block up or a block down
local function near(c: vector, d: string): vector?
	for _, dy in { 0, 1, -1 } do
		local n = c + D[d] + vector.create(0, dy, 0)
		if rail(n) then
			return n
		end
	end
	return nil
end

-- whether the rail at n has an end free toward c, or could be turned to
-- have one (it's only joined on one side, or none)
local function joined(n: vector): number
	local k = 0
	for _, e in ORDER do
		if end_up(n, e) ~= nil and link(n, e) then
			k += 1
		end
	end
	return k
end

local function reaches(n: vector, c: vector): boolean
	for _, e in ORDER do
		local l = link(n, e)
		if l and l == c then
			return true
		end
	end
	return false
end

-- Shapes the rail at c to join what's round it: the rails that already
-- reach it count most, then those free to turn to meet it (joined on one
-- side or none). It climbs to one a block up, runs straight if that joins
-- as much as a bend would, and bends where two meet at a corner. A powered
-- rail only runs straight.
local function shape(c: vector)
	local name = rail(c)
	if name == nil then
		return
	end
	local plain = name ~= POWERED
	local score: { [string]: number } = {}
	local up: string? = nil
	for _, d in ORDER do
		local n = near(c, d)
		if n then
			local r = reaches(n, c)
			if r or joined(n) < 2 then
				score[d] = if r then 2 else 1
				if n.y > c.y and (up == nil or r) then
					up = d
				end
			end
		end
	end
	local kind, facing = if plain then RAIL else POWERED, world.facing(c) or "north"
	if plain and up then
		kind, facing = SLOPE, up
	else
		-- the shape that joins the most: straight (the way it runs now
		-- winning a tie), or round a corner
		local was = facing
		local best, joins = -1, 0
		for _, f in { "north", "east" } do
			local sum = (score[f] or 0) + (score[BACK[f]] or 0)
			local v = sum + (if f == was or BACK[f] == was then 0.75 else 0.5)
			if v > best then
				best, joins, facing = v, sum, f
			end
		end
		if plain then
			for _, f in ORDER do
				local a, b = score[BACK[f]], score[RIGHT[f]]
				if a and b and a + b > best then
					best, joins, kind, facing = a + b, a + b, BEND, f
				end
			end
		end
		if joins == 0 then
			-- nothing to join: as it was put down
			return
		end
	end
	local now = world.facing(c)
	local same = kind == world.block(c) and (facing == now or ((kind == RAIL or kind == POWERED) and BACK[facing] == now))
	if not same then
		world.set_block(c, kind, facing)
	end
end

-- Laying a rail: it shapes itself, and any rail beside it with a free end
-- turns to meet it.
local function laid(c: vector)
	shape(c)
	for _, d in ORDER do
		local n = near(c, d)
		if n and joined(n) < 2 then
			shape(n)
		end
	end
	-- the one laid may have changed again once they turned
	shape(c)
end

-- the block an event is about, by its corner (events give its middle)
local function cell(p: vector): vector
	return vector.create(math.floor(p.x), math.floor(p.y), math.floor(p.z))
end

world.on("block_broken", function(_player, middle_of: vector, block: string)
	local at = cell(middle_of)
	if IS_RAIL[block] then
		-- the rails beside it turn to meet what's left
		for _, d in ORDER do
			local n = near(at, d)
			if n then
				shape(n)
			end
		end
	end
end)

world.on("block_placed", function(player, middle_of: vector, block: string)
	local at = cell(middle_of)
	if block == RAIL or block == POWERED then
		laid(at)
	elseif block == CART then
		local c = if rail(at) then at elseif rail(at - UP) then at - UP else nil
		if not c then
			-- a cart goes on a rail
			world.set_block(at, "Air")
			world.give(player, CART, 1)
			ui.show(player, "cart", "A cart goes on a rail.", { at = "bottom" })
			world.after(3, function()
				ui.hide(player, "cart")
			end)
			return
		end
		if c ~= at then
			world.set_block(at, "Air")
		end
		if not pcall(world.spawn, Cart, middle(c)) then
			world.give(player, CART, 1)
		end
	end
end)

-- a bend or a slope broken gives back a rail
world.drops(BEND, { gives = RAIL })
world.drops(SLOPE, { gives = RAIL })

-- after a restart, the world's carts come back where they were
world.after(0.1, function()
	if #world.creatures(Cart) == 0 then
		for _, at in saved do
			pcall(world.spawn, Cart, at)
		end
	end
end)
