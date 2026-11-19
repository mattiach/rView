"""Draws the rView logo and writes rview.ico next to this file (needs Pillow).

A black squircle with a thin lighter rim (so it stays visible on a dark taskbar) and a white
geometric "r".
Entries: 16-128 px as 32-bit bitmaps, 256 px PNG-compressed.
"""
import io
import math
import struct
import sys
from pathlib import Path

from PIL import Image, ImageChops, ImageDraw

HERE = Path(__file__).parent
SIZES = [16, 20, 24, 32, 40, 48, 64, 128, 256]
SS = 8  # supersampling
RIM = ((92, 92, 96), (44, 44, 48))  # top, bottom
FILL = ((38, 38, 42), (0, 0, 0))
GLYPH = (255, 255, 255)


def squircle(n: int, inset: float) -> Image.Image:
    """Superellipse mask (exponent 4) inset from the canvas edge."""
    c, a, e = n / 2, n / 2 - inset, 2 / 4
    pts = []
    for i in range(720):
        t = math.tau * i / 720
        x, y = math.cos(t), math.sin(t)
        pts.append((c + a * math.copysign(abs(x) ** e, x), c + a * math.copysign(abs(y) ** e, y)))
    mask = Image.new("L", (n, n), 0)
    ImageDraw.Draw(mask).polygon(pts, fill=255)
    return mask


def gradient(n: int, top: tuple, bottom: tuple) -> Image.Image:
    ramp = Image.linear_gradient("L").resize((n, n))
    return Image.composite(Image.new("RGB", (n, n), bottom), Image.new("RGB", (n, n), top), ramp)


def glyph(n: int, size: int) -> Image.Image:
    """The "r", tightly cropped: a round-capped stem and an arm that leaves it thin and grows
    to full weight (the ring between two circles, the outer one shifted right)."""
    w = n * (0.15 if size <= 24 else 0.115)  # stroke, a little heavier when tiny
    x, top, bottom = n * 0.3, n * 0.3, n * 0.72  # stem centre line
    ri, d = n * 0.115, w * 0.62  # inner radius (tangent to the stem), shift of the outer circle
    ro = ri + w * 0.95
    cx, cy = x + w / 2 + ri, top - w / 2 + ro  # arm top level with the stem top
    end = math.radians(300)  # where the arm stops, clockwise from 3 o'clock

    def disc(cx: float, cy: float, r: float, start: float = 0, stop: float = 360) -> Image.Image:
        m = Image.new("L", (n, n), 0)
        ImageDraw.Draw(m).pieslice((cx - r, cy - r, cx + r, cy + r), start, stop, fill=255)
        return m

    layer = ImageChops.subtract(disc(cx + d, cy, ro), disc(cx, cy, ri))
    layer = ImageChops.multiply(layer, disc(cx, cy, n, 180, math.degrees(end)))
    # round end: a circle as wide as the arm is there
    t = d * math.cos(end) + math.sqrt(ro * ro - (d * math.sin(end)) ** 2)
    mid, half = (ri + t) / 2, (t - ri) / 2
    draw = ImageDraw.Draw(layer)
    for px, py, r in [
        (cx + mid * math.cos(end), cy + mid * math.sin(end), half),
        (x, top, w / 2),
        (x, bottom, w / 2),
    ]:
        draw.ellipse((px - r, py - r, px + r, py + r), fill=255)
    draw.rectangle((x - w / 2, top, x + w / 2, bottom), fill=255)
    return layer.crop(layer.getbbox())


def draw(size: int) -> Image.Image:
    n = size * SS
    tile = gradient(n, *RIM)
    tile.paste(gradient(n, *FILL), mask=squircle(n, max(SS * 0.75, n * 0.012)))
    tile.putalpha(squircle(n, 0))
    g = glyph(n, size)
    tile.paste(GLYPH, ((n - g.width) // 2, (n - g.height) // 2), g)
    return tile.resize((size, size), Image.LANCZOS)


def entry(im: Image.Image) -> bytes:
    w, h = im.size
    if w >= 256:
        out = io.BytesIO()
        im.save(out, "PNG", optimize=True)
        return out.getvalue()
    bgra = im.transpose(Image.FLIP_TOP_BOTTOM).tobytes("raw", "BGRA")
    and_mask = bytes(((w + 31) // 32) * 4 * h)  # alpha channel rules, mask all zero
    return struct.pack("<IiiHHIIiiII", 40, w, h * 2, 1, 32, 0, len(bgra), 0, 0, 0, 0) + bgra + and_mask


def main() -> None:
    images = [draw(s) for s in SIZES]
    blobs = [entry(im) for im in images]
    offset = 6 + 16 * len(blobs)
    out = struct.pack("<HHH", 0, 1, len(blobs))
    for im, blob in zip(images, blobs):
        s = im.width % 256  # 0 means 256
        out += struct.pack("<BBBBHHII", s, s, 0, 0, 1, 32, len(blob), offset)
        offset += len(blob)
    (HERE / "rview.ico").write_bytes(out + b"".join(blobs))
    if len(sys.argv) > 1:  # preview sheet: python make_icon.py preview.png
        sheet = Image.new("RGBA", (sum(SIZES) + 10 * len(SIZES), 256), (32, 32, 32, 255))
        x = 0
        for im in images:
            sheet.alpha_composite(im, (x, 0))
            x += im.width + 10
        sheet.save(sys.argv[1])


main()
