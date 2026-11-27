"""Potluck's block textures at high resolution: procedural materials in Blender,
baked with Cycles into the maps the game's lighting uses.

    python tools/blocks.py -- [out dir] [resolution] [only=name,name]   (with the bpy wheel)
    blender -b -P tools/blocks.py -- [out dir] [resolution]

The out dir defaults to assets/blocks and the resolution to 128 px a block.
Each block gets <name>.png (colour, no lighting), <name>_n.png (tangent-space
normal, green up), <name>_r.png (roughness) and, for leaves, <name>_a.png
(where light shows through). The game builds them in when they are there and
draws its own 32 px textures for anything missing, so a block can be added a
step at a time. CYCLES_DEVICE=OPTIX (or CUDA, HIP) bakes on a graphics card.

Every texture tiles: noise is sampled on a torus in 4D, and brick and board
counts divide the block evenly. The colours follow the palettes in
crates/potluck/src/textures.rs, so baked and drawn blocks sit together. The
node helpers come from Cinderkirk's materials.py.
"""
import math
import os
import sys

import bpy

argv = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
pos = [a for a in argv if not a.startswith("only=")]
HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.abspath(pos[0] if pos else os.path.join(HERE, "..", "assets", "blocks"))
RES = int(pos[1]) if len(pos) > 1 else 128
ONLY = next((a[5:].split(",") for a in argv if a.startswith("only=")), None)
TAU = math.tau


def lin(h):
    """A hex sRGB colour as linear RGB, which is what node colours are."""
    def ch(v):
        v /= 255.0
        return v / 12.92 if v <= 0.04045 else ((v + 0.055) / 1.055) ** 2.4
    return (ch((h >> 16) & 255), ch((h >> 8) & 255), ch(h & 255))


# the drawn textures' palettes, darkest first
GRASS = [0x2a5a24, 0x376f2a, 0x4a8731, 0x5f9c39, 0x78b146]
DIRT = [0x4a3020, 0x664330, 0x7d563b, 0x936948]
STONE = [0x55595f, 0x6b7077, 0x80868c, 0x979ca1, 0xaeb2b5]
SAND = [0xc9b27a, 0xd9c38a, 0xe6d29c, 0xf0e0b0]
GRAVEL = [0x6c6a66, 0x86817a, 0x9e978d, 0xb8b0a3]
BARK = [0x3b2a1c, 0x4e3824, 0x634830, 0x7a5a3c]
RING = [0x9c7a4c, 0xb48f5a, 0xc9a46b, 0xdcb97e]
LEAF = [0x1f4d22, 0x2b6a2b, 0x3d8a34, 0x56a63f, 0x74bd4c]
PINE = [0x15362a, 0x1d4a34, 0x275f3f, 0x34744a]
BIRCH = [0x4d7f2e, 0x66a03a, 0x86bb48, 0xa6d15c]
BIRCH_BARK = [0xbab3a6, 0xd8d2c6, 0xeeeae2]
PLANK = [0x7a5230, 0x93653c, 0xa97748, 0xbd8a56]
MOSS = [0x4f5a52, 0x606d61, 0x6f7f6a, 0x5f8a4a, 0x76a254]
SNOW = [0xcfd9e3, 0xdee6ee, 0xeef3f8, 0xffffff]
HAY = [0xa6801f, 0xc29b2c, 0xd9b545, 0xead06a]
BEDROCK = [0x1d1f22, 0x2c2f33, 0x3d4146]
WOOL = {
    "wool_red": [0x8e2325, 0xb3332f, 0xcc463b, 0xde5d4b],
    "wool_cream": [0xc9bfa6, 0xddd3ba, 0xece4cd, 0xf6efdc],
    "wool_blue": [0x1f4f8a, 0x2c65a8, 0x3c7cc2, 0x5595d6],
    "wool_yellow": [0xc28a17, 0xdca424, 0xeebd3a, 0xf8d35a],
}


class Graph:
    """A small helper for writing node graphs as expressions."""

    def __init__(self, mat):
        self.t = mat.node_tree
        self.t.nodes.clear()
        self.uv = self.node("ShaderNodeTexCoord").outputs["UV"]
        sep = self.node("ShaderNodeSeparateXYZ")
        self.feed(sep.inputs[0], self.uv)
        self.u, self.v = sep.outputs["X"], sep.outputs["Y"]
        self.alpha = None

    def node(self, kind, **props):
        nd = self.t.nodes.new(kind)
        for k, v in props.items():
            setattr(nd, k, v)
        return nd

    def feed(self, sock, v):
        if isinstance(v, bpy.types.NodeSocket):
            self.t.links.new(v, sock)
        elif isinstance(v, (tuple, list)) and len(v) == 3 and sock.type == "RGBA":
            sock.default_value = (*v, 1.0)
        elif isinstance(v, (int, float)) and sock.type == "RGBA":
            sock.default_value = (v, v, v, 1.0)
        else:
            sock.default_value = v

    def math(self, op, a, b=0.0, c=0.0, clamp=False):
        nd = self.node("ShaderNodeMath", operation=op, use_clamp=clamp)
        for i, v in enumerate((a, b, c)):
            self.feed(nd.inputs[i], v)
        return nd.outputs[0]

    def add(self, a, b): return self.math("ADD", a, b)
    def mul(self, a, b): return self.math("MULTIPLY", a, b)
    def sub(self, a, b): return self.math("SUBTRACT", a, b)
    def clamp(self, a): return self.math("ADD", a, 0.0, clamp=True)
    def maxf(self, a, b): return self.math("MAXIMUM", a, b)
    def minf(self, a, b): return self.math("MINIMUM", a, b)

    def remap(self, v, lo, hi, to_lo=0.0, to_hi=1.0, smooth=True):
        nd = self.node("ShaderNodeMapRange", interpolation_type="SMOOTHSTEP" if smooth else "LINEAR")
        for k, x in (("Value", v), ("From Min", lo), ("From Max", hi), ("To Min", to_lo), ("To Max", to_hi)):
            self.feed(nd.inputs[k], x)
        return nd.outputs["Result"]

    def mixc(self, fac, a, b, blend="MIX"):
        nd = self.node("ShaderNodeMix", data_type="RGBA", blend_type=blend, clamp_factor=True)
        self.feed(nd.inputs[0], fac)
        self.feed(nd.inputs[6], a)
        self.feed(nd.inputs[7], b)
        return nd.outputs[2]

    def mixf(self, fac, a, b):
        nd = self.node("ShaderNodeMix", data_type="FLOAT", clamp_factor=True)
        self.feed(nd.inputs[0], fac)
        self.feed(nd.inputs[2], a)
        self.feed(nd.inputs[3], b)
        return nd.outputs[0]

    def shade(self, col, k):
        """A colour times a number (k above 1 brightens)."""
        return self.mixc(1.0, col, self.mixc(self.clamp(self.mul(k, 0.5)), (0.0, 0.0, 0.0), (2.0, 2.0, 2.0)), blend="MULTIPLY")

    def ramp(self, fac, hexes, ease=True):
        """A palette from dark to light, looked up by fac."""
        nd = self.node("ShaderNodeValToRGB")
        cr = nd.color_ramp
        cr.interpolation = "EASE" if ease else "LINEAR"
        n = len(hexes)
        while len(cr.elements) < n:
            cr.elements.new(0.5)
        for i, h in enumerate(hexes):
            e = cr.elements[i]
            e.position = i / (n - 1)
            e.color = (*lin(h), 1.0)
        self.feed(nd.inputs["Fac"], fac)
        return nd.outputs["Color"]

    def torus(self, r, ox=0.0, ry=None):
        """UV on a torus in 4D, so noise sampled there tiles in both
        directions; ry stretches it (bark, straw)."""
        ry = r if ry is None else ry
        a = self.mul(self.u, TAU)
        b = self.mul(self.v, TAU)
        comb = self.node("ShaderNodeCombineXYZ")
        self.feed(comb.inputs["X"], self.add(self.mul(self.math("COSINE", a), r), ox))
        self.feed(comb.inputs["Y"], self.mul(self.math("SINE", a), r))
        self.feed(comb.inputs["Z"], self.mul(self.math("COSINE", b), ry))
        return comb.outputs[0], self.mul(self.math("SINE", b), ry)

    def noise(self, r, detail=6.0, rough=0.55, distort=0.0, seed=0.0, ry=None):
        vec, w = self.torus(r, ox=seed * 17.0, ry=ry)
        nd = self.node("ShaderNodeTexNoise", noise_dimensions="4D")
        self.feed(nd.inputs["Vector"], vec)
        self.feed(nd.inputs["W"], w)
        nd.inputs["Scale"].default_value = 1.0
        nd.inputs["Detail"].default_value = detail
        nd.inputs["Roughness"].default_value = rough
        nd.inputs["Distortion"].default_value = distort
        return nd.outputs["Fac"]

    def voronoi(self, r, feature="F1", out="Distance", seed=0.0, rand=1.0, ry=None):
        vec, w = self.torus(r, ox=seed * 23.0, ry=ry)
        nd = self.node("ShaderNodeTexVoronoi", voronoi_dimensions="4D", feature=feature)
        self.feed(nd.inputs["Vector"], vec)
        self.feed(nd.inputs["W"], w)
        nd.inputs["Scale"].default_value = 1.0
        nd.inputs["Randomness"].default_value = rand
        return nd.outputs[out]

    def cell_value(self, r, seed=0.0):
        """A random number per voronoi cell, for colouring stones one by one."""
        c = self.voronoi(r, out="Color", seed=seed)
        sep = self.node("ShaderNodeSeparateColor")
        self.feed(sep.inputs[0], c)
        return sep.outputs[0]

    def bricks(self, across, rows, mortar, offset=0.5, smooth=0.1):
        """Fac is 1 in the mortar; a random shade per brick comes out too."""
        nd = self.node("ShaderNodeTexBrick", offset=offset, offset_frequency=2, squash=1.0, squash_frequency=2)
        self.feed(nd.inputs["Vector"], self.uv)
        nd.inputs["Color1"].default_value = (1.0, 1.0, 1.0, 1.0)
        nd.inputs["Color2"].default_value = (0.0, 0.0, 0.0, 1.0)
        nd.inputs["Scale"].default_value = 1.0
        nd.inputs["Mortar Size"].default_value = mortar
        nd.inputs["Mortar Smooth"].default_value = smooth
        nd.inputs["Brick Width"].default_value = 1.0 / across
        nd.inputs["Row Height"].default_value = 1.0 / rows
        sep = self.node("ShaderNodeSeparateColor")
        self.feed(sep.inputs[0], nd.outputs["Color"])
        return sep.outputs[0], nd.outputs["Fac"]

    def finish(self, color, rough, height, bump=0.6, dist=0.02, alpha=None):
        bsdf = self.node("ShaderNodeBsdfPrincipled")
        self.feed(bsdf.inputs["Base Color"], color)
        self.feed(bsdf.inputs["Roughness"], rough)
        b = self.node("ShaderNodeBump")
        b.inputs["Strength"].default_value = bump
        b.inputs["Distance"].default_value = dist * 3.0
        self.feed(b.inputs["Height"], height)
        self.t.links.new(b.outputs["Normal"], bsdf.inputs["Normal"])
        out = self.node("ShaderNodeOutputMaterial")
        self.t.links.new(bsdf.outputs["BSDF"], out.inputs["Surface"])
        self.maps = {"albedo": color, "rough": rough, "alpha": alpha}
        self.bsdf, self.out = bsdf, out


def top_edge(g, depth, wobble, seed):
    """1 in a band along the top of a side face (where grass or snow hangs
    over), with a ragged lower edge."""
    edge = g.add(depth, g.mul(g.sub(g.noise(5.0, 3.0, 0.6, seed=seed), 0.5), wobble))
    drips = g.mul(g.remap(g.noise(9.0, 2.0, 0.5, seed=seed + 1, ry=1.0), 0.6, 0.75), 0.08)
    return g.remap(g.sub(1.0, g.v), g.add(edge, drips), g.sub(g.add(edge, drips), 0.012), 0.0, 1.0)


# --- the blocks ------------------------------------------------------------

def grass_surface(g, seed=0.0):
    broad = g.noise(2.5, 6.0, 0.55, seed=seed)
    blades = g.voronoi(5.0, "F1", seed=seed + 1)
    tips = g.remap(blades, 0.0, 0.45, 1.0, 0.0)
    fine = g.noise(14.0, 3.0, 0.6, seed=seed + 2)
    patches = g.noise(0.7, 3.0, 0.5, seed=seed + 3)
    t = g.add(g.add(g.mul(broad, 0.5), g.mul(patches, 0.35)), g.add(g.mul(tips, 0.35), g.mul(fine, 0.2)))
    col = g.ramp(g.remap(t, 0.3, 1.25, 0.0, 1.0, smooth=False), GRASS)
    height = g.add(g.mul(tips, 0.7), g.mul(fine, 0.3))
    return col, height


def b_grass_top(g):
    col, height = grass_surface(g)
    g.finish(col, g.remap(height, 0.0, 1.0, 0.95, 0.8), height, bump=0.9, dist=0.01)


def dirt_surface(g, seed=10.0):
    soil = g.noise(3.0, 8.0, 0.6, distort=0.3, seed=seed)
    clumps = g.voronoi(3.0, "SMOOTH_F1", seed=seed + 1)
    col = g.ramp(g.remap(g.add(g.mul(soil, 0.8), g.mul(clumps, 0.3)), 0.25, 0.85, 0.0, 1.0, smooth=False), DIRT)
    pd = g.voronoi(1.4, seed=seed + 2)
    gate = g.remap(g.cell_value(1.4, seed=seed + 2), 0.62, 0.64)
    pebble = g.mul(g.remap(pd, 0.3, 0.22), gate)
    stone = g.ramp(g.cell_value(1.4, seed=seed + 2), STONE[1:4])
    col = g.mixc(pebble, col, stone)
    height = g.add(g.mul(soil, 0.4), g.add(g.mul(pebble, 0.7), g.mul(g.sub(1.0, clumps), 0.2)))
    rough = g.mixf(pebble, 0.95, 0.75)
    return col, rough, height


def b_dirt(g):
    col, rough, height = dirt_surface(g)
    g.finish(col, rough, height, bump=0.9)


def b_grass_side(g):
    col, rough, height = dirt_surface(g, seed=12.0)
    grass, gh = grass_surface(g, seed=3.0)
    over = top_edge(g, 0.16, 0.1, seed=20.0)
    shadow = g.mul(g.remap(g.sub(1.0, g.v), 0.12, 0.3, 1.0, 0.0), g.sub(1.0, over))
    col = g.mixc(g.mul(shadow, 0.45), col, (0.02, 0.012, 0.006))
    col = g.mixc(over, col, grass)
    height = g.mixf(over, height, g.add(gh, 0.6))
    g.finish(col, g.mixf(over, rough, 0.88), height, bump=0.9)


def b_stone(g, seed=30.0, pal=STONE, crack_r=0.6):
    grain = g.noise(3.0, 10.0, 0.62, seed=seed)
    strata = g.noise(1.5, 4.0, 0.5, distort=0.6, seed=seed + 1)
    t = g.add(g.mul(grain, 0.7), g.mul(strata, 0.4))
    col = g.ramp(g.remap(t, 0.3, 0.9, 0.0, 1.0, smooth=False), pal)
    d = g.add(g.voronoi(crack_r, "DISTANCE_TO_EDGE", seed=seed + 2), g.mul(g.sub(g.noise(6.0, 3.0, 0.5, seed=seed + 3), 0.5), 0.05))
    crack = g.remap(d, 0.0, 0.025, 1.0, 0.0)
    col = g.mixc(g.mul(crack, 0.8), col, g.shade(g.ramp(0.0, pal), 0.6))
    height = g.sub(g.add(g.mul(grain, 0.6), g.mul(strata, 0.3)), g.mul(crack, 0.6))
    g.finish(col, g.add(0.72, g.mul(crack, 0.2)), height, bump=0.8)


def b_bedrock(g):
    b_stone(g, seed=40.0, pal=BEDROCK, crack_r=1.0)


def b_sand(g):
    grains = g.noise(20.0, 2.0, 0.5, seed=50)
    ripple = g.math("SINE", g.add(g.mul(g.v, TAU * 5.0), g.mul(g.noise(2.0, 3.0, 0.5, seed=51), 6.0)))
    ripple = g.remap(ripple, -1.0, 1.0)
    t = g.add(g.mul(grains, 0.5), g.mul(ripple, 0.35))
    col = g.ramp(g.remap(t, 0.2, 0.85, 0.0, 1.0, smooth=False), SAND)
    g.finish(col, 0.95, g.add(g.mul(ripple, 0.6), g.mul(grains, 0.4)), bump=0.6, dist=0.01)


def b_gravel(g):
    d = g.voronoi(1.7, "SMOOTH_F1", seed=60)
    cell = g.cell_value(1.7, seed=60)
    edge = g.remap(g.voronoi(1.7, "DISTANCE_TO_EDGE", seed=60), 0.0, 0.06, 1.0, 0.0)
    speck = g.noise(16.0, 2.0, 0.5, seed=61)
    col = g.ramp(g.add(g.mul(cell, 0.8), g.mul(speck, 0.25)), GRAVEL)
    col = g.mixc(g.mul(edge, 0.7), col, g.shade(g.ramp(0.0, GRAVEL), 0.45))
    height = g.sub(g.sub(1.0, d), g.mul(edge, 0.5))
    g.finish(col, g.mixf(edge, 0.8, 0.95), height, bump=1.0)


def bark(g, seed=70.0, pal=BARK):
    furrow = g.noise(3.0, 6.0, 0.55, distort=0.4, seed=seed, ry=0.6)
    plates = g.voronoi(1.0, "DISTANCE_TO_EDGE", seed=seed + 1, ry=0.5)
    t = g.add(g.mul(furrow, 0.75), g.mul(g.remap(plates, 0.0, 0.12), 0.35))
    col = g.ramp(g.remap(t, 0.25, 1.0, 0.0, 1.0, smooth=False), pal)
    return col, g.add(g.mul(furrow, 0.8), g.mul(plates, 0.6))


def b_log_side(g):
    col, height = bark(g)
    g.finish(col, 0.9, height, bump=1.0)


def b_log_top(g):
    du = g.sub(g.u, 0.5)
    dv = g.sub(g.v, 0.5)
    r = g.math("SQRT", g.add(g.mul(du, du), g.mul(dv, dv)))
    warp = g.mul(g.sub(g.noise(3.0, 4.0, 0.5, seed=80), 0.5), 0.05)
    ring = g.remap(g.math("SINE", g.mul(g.add(r, warp), TAU * 9.0)), -1.0, 1.0)
    grain = g.noise(14.0, 2.0, 0.5, seed=81)
    col = g.ramp(g.add(g.mul(ring, 0.7), g.mul(grain, 0.2)), RING)
    # bark round the edge, cracks out from the middle
    edge = g.remap(g.maxf(g.math("ABSOLUTE", du), g.math("ABSOLUTE", dv)), 0.43, 0.445)
    bark_col, bh = bark(g, seed=82.0)
    ang = g.math("ARCTAN2", dv, du)
    crack = g.mul(g.remap(g.math("SINE", g.add(g.mul(ang, 7.0), g.mul(warp, 40.0))), 0.97, 1.0), g.remap(r, 0.05, 0.35))
    col = g.mixc(g.mul(crack, 0.7), col, g.shade(g.ramp(0.0, RING), 0.5))
    col = g.mixc(edge, col, bark_col)
    height = g.mixf(edge, g.sub(g.add(g.mul(ring, 0.3), g.mul(grain, 0.1)), g.mul(crack, 0.5)), g.add(bh, 0.3))
    g.finish(col, g.mixf(edge, 0.7, 0.9), height, bump=0.7)


def leaves(g, pal, holes, seed):
    cells = g.voronoi(1.6, "SMOOTH_F1", seed=seed)
    tone = g.cell_value(1.6, seed=seed)
    vein = g.remap(g.voronoi(1.6, "DISTANCE_TO_EDGE", seed=seed), 0.0, 0.04, 1.0, 0.0)
    lit = g.add(g.mul(tone, 0.6), g.mul(g.sub(1.0, cells), 0.5))
    col = g.ramp(g.remap(lit, 0.1, 1.0, 0.0, 1.0, smooth=False), pal)
    col = g.mixc(g.mul(vein, 0.6), col, g.shade(g.ramp(0.0, pal), 0.5))
    gap = g.noise(2.0, 3.0, 0.6, seed=seed + 1)
    alpha = g.remap(g.add(gap, g.mul(vein, 0.2)), holes, holes + 0.02)
    g.finish(col, 0.65, g.sub(g.sub(1.0, cells), g.mul(vein, 0.4)), bump=0.8, alpha=alpha)


def b_leaves(g): leaves(g, LEAF, 0.36, 90.0)
def b_pine(g): leaves(g, PINE, 0.33, 95.0)
def b_birch_leaves(g): leaves(g, BIRCH, 0.38, 99.0)


def b_birch_side(g):
    base = g.noise(4.0, 5.0, 0.5, seed=110)
    col = g.ramp(g.remap(base, 0.3, 0.8), BIRCH_BARK)
    marks = g.remap(g.noise(0.5, 5.0, 0.6, seed=111, ry=4.0), 0.6, 0.63)
    peel = g.remap(g.noise(2.0, 3.0, 0.5, seed=112, ry=0.8), 0.68, 0.72)
    col = g.mixc(g.mul(peel, 0.5), col, (0.55, 0.42, 0.30))
    col = g.mixc(marks, col, (0.022, 0.020, 0.019))
    g.finish(col, g.mixf(marks, 0.6, 0.85), g.sub(base, g.mul(marks, 0.6)), bump=0.6)


def b_planks(g):
    shade, seam = g.bricks(1.0, 4.0, 0.012, offset=0.5)
    n = g.noise(2.0, 4.0, 0.5, seed=120, ry=0.4)
    grain = g.remap(g.math("SINE", g.add(g.mul(g.v, TAU * 28.0), g.mul(n, 14.0))), -1.0, 1.0)
    knots = g.remap(g.voronoi(0.5, seed=121), 0.06, 0.0)
    t = g.add(g.mul(grain, 0.45), g.add(g.mul(shade, 0.35), g.mul(knots, -0.4)))
    col = g.ramp(g.remap(t, -0.2, 0.9, 0.0, 1.0, smooth=False), PLANK)
    col = g.mixc(seam, col, (0.035, 0.02, 0.01))
    height = g.sub(g.sub(g.add(g.mul(grain, 0.2), 0.8), g.mul(seam, 0.8)), g.mul(knots, 0.2))
    g.finish(col, g.add(0.62, g.mul(grain, 0.12)), height, bump=0.8, dist=0.01)


def b_mossy_brick(g):
    shade, mortar = g.bricks(2.0, 4.0, 0.03, smooth=0.3)
    chip = g.noise(7.0, 5.0, 0.6, seed=130)
    mortar = g.remap(g.add(mortar, g.mul(g.sub(chip, 0.5), 0.6)), 0.4, 0.6)
    stone = g.ramp(g.add(g.mul(shade, 0.35), g.mul(chip, 0.6)), MOSS[:3])
    moss_n = g.noise(2.2, 6.0, 0.6, seed=131)
    moss = g.clamp(g.add(g.remap(moss_n, 0.5, 0.62), g.mul(mortar, g.remap(moss_n, 0.38, 0.5))))
    moss_col = g.ramp(g.noise(20.0, 3.0, 0.6, seed=132), MOSS[2:] + [LEAF[4]])
    col = g.mixc(mortar, stone, (0.05, 0.055, 0.052))
    col = g.mixc(moss, col, moss_col)
    height = g.add(g.sub(g.add(g.mul(chip, 0.3), 0.6), g.mul(mortar, 0.7)), g.mul(moss, 0.25))
    g.finish(col, g.mixf(moss, 0.82, 0.95), height, bump=1.0)


def b_cobble(g):
    d = g.voronoi(0.8, "SMOOTH_F1", seed=140)
    cell = g.cell_value(0.8, seed=140)
    edge = g.remap(g.voronoi(0.8, "DISTANCE_TO_EDGE", seed=140), 0.0, 0.05, 1.0, 0.0)
    grain = g.noise(10.0, 3.0, 0.6, seed=141)
    col = g.ramp(g.add(g.mul(cell, 0.75), g.mul(grain, 0.3)), STONE)
    col = g.mixc(edge, col, (0.03, 0.03, 0.032))
    height = g.sub(g.add(g.mul(g.sub(1.0, d), 0.8), g.mul(grain, 0.15)), g.mul(edge, 0.8))
    g.finish(col, g.mixf(edge, 0.75, 0.95), height, bump=1.0)


def snow_surface(g, seed=150.0):
    drift = g.noise(2.0, 5.0, 0.5, seed=seed)
    crystals = g.noise(22.0, 2.0, 0.5, seed=seed + 1)
    col = g.ramp(g.remap(g.add(g.mul(drift, 0.7), g.mul(crystals, 0.3)), 0.3, 0.85), SNOW)
    return col, g.add(g.mul(drift, 0.7), g.mul(crystals, 0.15)), g.remap(crystals, 0.6, 0.75, 0.75, 0.45)


def b_snow(g):
    col, height, rough = snow_surface(g)
    g.finish(col, rough, height, bump=0.5)


def b_snow_side(g):
    grain = g.noise(3.0, 10.0, 0.62, seed=160)
    col = g.ramp(g.remap(grain, 0.3, 0.8, 0.0, 1.0, smooth=False), STONE)
    snow, sh, srough = snow_surface(g, seed=161.0)
    over = top_edge(g, 0.24, 0.12, seed=162.0)
    col = g.mixc(over, col, snow)
    g.finish(col, g.mixf(over, 0.75, srough), g.mixf(over, grain, g.add(sh, 0.5)), bump=0.7)


def straw(g, seed):
    strands = g.noise(8.0, 4.0, 0.6, seed=seed, ry=0.4)
    fine = g.noise(22.0, 2.0, 0.5, seed=seed + 1, ry=1.0)
    t = g.add(g.mul(strands, 0.75), g.mul(fine, 0.3))
    return g.ramp(g.remap(t, 0.25, 0.95, 0.0, 1.0, smooth=False), HAY), t


def b_hay_side(g):
    col, t = straw(g, 170.0)
    # two bands of twine round the bale
    vv = g.math("FRACT", g.mul(g.v, 2.0))
    band = g.mul(g.remap(vv, 0.18, 0.2), g.remap(vv, 0.3, 0.28))
    twist = g.remap(g.math("SINE", g.add(g.mul(g.u, TAU * 24.0), g.mul(vv, 60.0))), -1.0, 1.0)
    twine = g.mixc(twist, (0.18, 0.06, 0.025), (0.32, 0.13, 0.05))
    col = g.mixc(band, col, twine)
    g.finish(col, 0.9, g.add(t, g.mul(band, 0.5)), bump=1.0)


def b_hay_top(g):
    ends = g.voronoi(6.0, "SMOOTH_F1", seed=180)
    col, t = straw(g, 181.0)
    col = g.mixc(g.mul(g.remap(ends, 0.0, 0.3, 1.0, 0.0), 0.5), col, g.ramp(0.95, HAY))
    g.finish(col, 0.9, g.add(g.mul(t, 0.6), g.mul(g.sub(1.0, ends), 0.4)), bump=0.9)


def wool(pal, seed):
    def build(g):
        felt = g.noise(8.0, 6.0, 0.55, seed=seed)
        fibres = g.noise(28.0, 2.0, 0.6, seed=seed + 1)
        # a knit of little loops, 16 across
        knit = g.remap(g.math("SINE", g.mul(g.add(g.u, g.mul(g.math("FRACT", g.mul(g.v, 16.0)), 0.05)), TAU * 16.0)), -1.0, 1.0)
        rowv = g.remap(g.math("SINE", g.mul(g.v, TAU * 16.0)), -1.0, 1.0)
        loops = g.mul(knit, rowv)
        t = g.add(g.mul(felt, 0.55), g.add(g.mul(loops, 0.25), g.mul(fibres, 0.2)))
        col = g.ramp(g.remap(t, 0.15, 0.95, 0.0, 1.0, smooth=False), pal)
        g.finish(col, 0.97, g.add(g.mul(loops, 0.6), g.mul(fibres, 0.3)), bump=0.5, dist=0.008)
    return build


BLOCKS = {
    "grass_top": b_grass_top,
    "grass_side": b_grass_side,
    "dirt": b_dirt,
    "stone": b_stone,
    "sand": b_sand,
    "gravel": b_gravel,
    "log_side": b_log_side,
    "log_top": b_log_top,
    "leaves": b_leaves,
    "pine": b_pine,
    "birch_leaves": b_birch_leaves,
    "birch_side": b_birch_side,
    "planks": b_planks,
    "mossy_brick": b_mossy_brick,
    "cobble": b_cobble,
    "snow": b_snow,
    "snow_side": b_snow_side,
    "hay_side": b_hay_side,
    "hay_top": b_hay_top,
    "bedrock": b_bedrock,
    **{name: wool(pal, 190.0 + i * 5) for i, (name, pal) in enumerate(WOOL.items())},
}


# --- baking ----------------------------------------------------------------

def setup():
    bpy.ops.wm.read_factory_settings(use_empty=True)
    sc = bpy.context.scene
    sc.render.engine = "CYCLES"
    device = os.environ.get("CYCLES_DEVICE", "CPU").upper()
    sc.cycles.device = "CPU"
    if device != "CPU":
        prefs = bpy.context.preferences.addons["cycles"].preferences
        prefs.compute_device_type = device
        prefs.get_devices()
        for d in prefs.devices:
            d.use = d.type == device
        if any(d.use for d in prefs.devices):
            sc.cycles.device = "GPU"
        else:
            print(f"no {device} device, baking on the CPU", flush=True)
    # samples smooth the procedural detail inside each texel
    sc.cycles.samples = int(os.environ.get("BAKE_SAMPLES", 64 if sc.cycles.device == "GPU" else 4))
    sc.render.bake.margin = 0
    bpy.ops.mesh.primitive_plane_add(size=1.0)
    return bpy.context.active_object


def bake_to(g, name, kind, source=None, non_color=False):
    """Bakes one map. Colour maps are baked as emission from the socket that
    feeds them, so lighting never gets into them; the normal map is a NORMAL
    bake (OpenGL style, green up, as the game reads it)."""
    img = bpy.data.images.new(name, RES, RES, alpha=False)
    if non_color:
        img.colorspace_settings.name = "Non-Color"
    nd = g.node("ShaderNodeTexImage", image=img)
    for n in g.t.nodes:
        n.select = False
    nd.select = True
    g.t.nodes.active = nd
    if kind == "NORMAL":
        g.t.links.new(g.bsdf.outputs["BSDF"], g.out.inputs["Surface"])
        bpy.ops.object.bake(type="NORMAL", normal_space="TANGENT")
    else:
        em = g.node("ShaderNodeEmission")
        g.feed(em.inputs["Color"], source)
        em.inputs["Strength"].default_value = 1.0
        g.t.links.new(em.outputs["Emission"], g.out.inputs["Surface"])
        bpy.ops.object.bake(type="EMIT")
        g.t.links.new(g.bsdf.outputs["BSDF"], g.out.inputs["Surface"])
    path = os.path.join(OUT, name + ".png")
    img.filepath_raw = path
    img.file_format = "PNG"
    img.save()
    print("baked", path, flush=True)


def main():
    os.makedirs(OUT, exist_ok=True)
    for name, build in BLOCKS.items():
        if ONLY and name not in ONLY:
            continue
        plane = setup()
        mat = bpy.data.materials.new(name)
        mat.use_nodes = True
        plane.data.materials.append(mat)
        g = Graph(mat)
        build(g)
        bake_to(g, name, "EMIT", g.maps["albedo"])
        bake_to(g, name + "_n", "NORMAL", non_color=True)
        bake_to(g, name + "_r", "EMIT", g.maps["rough"], non_color=True)
        if g.maps["alpha"] is not None:
            bake_to(g, name + "_a", "EMIT", g.maps["alpha"], non_color=True)


main()
