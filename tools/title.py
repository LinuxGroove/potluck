"""Potluck's title: the word in Bungee, made of the game's own dirt with grass
growing on every edge that faces up, a dark outline, a side a block deep and a
soft shadow, on a clear background.

    POTLUCK_DUMP_TEX=/tmp/tex cargo test -p potluck dump_textures
    python3 tools/title.py /tmp/tex assets/title/potluck.png [size]   (Pillow and numpy)

The tiles are the drawn textures the game makes (dirt and grass_top, as PPM).
Size is the letters' height in pixels (default 340, which makes it about 1680
wide: the title shows 560 px wide on a 720-line window and scales with it, so
this is a pixel for a pixel at 2160 lines).
"""
import os
import sys

import numpy as np
from PIL import Image, ImageDraw, ImageFilter, ImageFont

HERE = os.path.dirname(os.path.abspath(__file__))
FONT = os.path.join(HERE, "..", "assets", "fonts", "Bungee-Regular.ttf")
TEX, OUT = sys.argv[1], sys.argv[2]
SIZE = int(sys.argv[3]) if len(sys.argv) > 3 else 340
WORD = "POTLUCK"

DEPTH = round(SIZE * 0.07)  # the side, down and to the right
GRASS = round(SIZE * 0.085)  # how far grass reaches down from an edge that faces up
LINE = max(2, round(SIZE * 0.012))  # the dark line between grass and dirt
EDGE = round(SIZE * 0.025)  # the outline
LIGHT = round(SIZE * 0.018)  # light along the top and left edges
TEXEL = max(1, round(SIZE * 0.22 / 64))  # pixels to a texel of the 64 px tiles

font = ImageFont.truetype(FONT, SIZE)
l, t, r, b = font.getbbox(WORD)
pad = SIZE // 2
W, H = r - l + 2 * pad, b - t + 2 * pad
art = Image.new("L", (W, H))
ImageDraw.Draw(art).text((pad - l, pad - t), WORD, font=font, fill=255)
mask = np.asarray(art, np.float32) / 255.0


def shift(m, dx, dy):
    """m moved right by dx and down by dy, with nothing coming in."""
    o = np.zeros_like(m)
    h, w = m.shape[:2]
    o[max(dy, 0):h + min(dy, 0), max(dx, 0):w + min(dx, 0)] = m[max(-dy, 0):h - max(dy, 0), max(-dx, 0):w - max(dx, 0)]
    return o


def grow(m, r):
    """m grown by r pixels each way, keeping square corners."""
    o = m.copy()
    for d in range(1, r + 1):
        o = np.maximum(o, np.maximum(shift(m, d, 0), shift(m, -d, 0)))
    m, o = o, o.copy()
    for d in range(1, r + 1):
        o = np.maximum(o, np.maximum(shift(m, 0, d), shift(m, 0, -d)))
    return o


def tiles(name, dy=0):
    """A tile from the game repeated over the picture, its pixels kept square."""
    img = Image.open(os.path.join(TEX, name + ".ppm")).convert("RGB")
    img = img.resize((img.width * TEXEL, img.height * TEXEL), Image.NEAREST)
    a = np.asarray(img, np.float32) / 255.0
    reps = (H // a.shape[0] + 2, W // a.shape[1] + 2, 1)
    return np.tile(a, reps)[a.shape[0] - dy % a.shape[0]:][:H, :W]


def hex_rgb(h):
    return np.array([int(h[i:i + 2], 16) / 255.0 for i in (1, 3, 5)], np.float32)


out = np.zeros((H, W, 4), np.float32)


def over(rgb, a):
    """Lays a colour (or picture) over what's there, a as its coverage."""
    a = a[..., None]
    under = out[..., 3:4]
    total = a + under * (1 - a)
    rgb = np.broadcast_to(rgb, out[..., :3].shape)
    out[..., :3] = np.where(total > 0, (rgb * a + out[..., :3] * under * (1 - a)) / np.maximum(total, 1e-6), 0)
    out[..., 3:4] = total


# a soft shadow below and to the right
fall = Image.fromarray((shift(mask, round(DEPTH * 0.9), round(DEPTH * 1.8)) * 255).astype(np.uint8))
fall = np.asarray(fall.filter(ImageFilter.GaussianBlur(SIZE * 0.04)), np.float32) / 255.0
over(hex_rgb("#0a0e14"), fall * 0.45)
# the side: an outline round its far edge, then slices of dirt, darker further back
edged = grow(mask, EDGE)
over(hex_rgb("#22160e"), shift(edged, DEPTH, DEPTH))
for k in range(DEPTH, 0, -1):
    over(hex_rgb("#3b2616" if k > DEPTH * 0.55 else "#5a3a22"), shift(mask, k, k))
over(hex_rgb("#22160e"), edged)
# the face: grass, then a dark line and dirt a little lower, so grass shows on every edge facing up
face = tiles("grass_top")
lower = shift(mask, 0, GRASS)[..., None]
face = face * (1 - lower) + hex_rgb("#2c4a18") * lower
lower = shift(mask, 0, GRASS + LINE)[..., None]
face = face * (1 - lower) + tiles("dirt", GRASS + LINE) * lower
lit = (mask * (1 - shift(mask, LIGHT, LIGHT)))[..., None] * 0.35
face = face * (1 - lit) + lit
over(face, mask)

ys, xs = np.nonzero(out[..., 3] > 0.02)
x0, x1, y0, y1 = max(0, xs.min() - 6), min(W, xs.max() + 7), max(0, ys.min() - 6), min(H, ys.max() + 7)
pic = (np.clip(out[y0:y1, x0:x1], 0, 1) * 255 + 0.5).astype(np.uint8)
Image.fromarray(pic, "RGBA").save(OUT, optimize=True)
print("wrote", OUT, x1 - x0, "x", y1 - y0)
