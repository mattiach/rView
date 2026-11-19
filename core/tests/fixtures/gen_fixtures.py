"""Regenerate the encoder-made fixtures of core/tests/fixtures.

Independent encoders (not project dependencies; the project stays pure Rust):
  Pillow 13 (libwebp, libavif, libjpeg-turbo), pillow-heif 1.8.1 (libheif 1.23.6 + x265),
  imagecodecs (libjxl).
Each lossy file gets a reference decoded by an independent decoder (`*.ref.png`), so the tests
compare two decoders on the same bitstream.

Usage (isolated env):  pip install --target <dir> pillow pillow-heif imagecodecs
                       python gen_fixtures.py <dir>
"""
import gzip
import io
import os
import sys

sys.path.insert(0, sys.argv[1])
import imagecodecs  # noqa: E402
import numpy as np  # noqa: E402
import pillow_heif  # noqa: E402
from PIL import Image  # noqa: E402

OUT = os.path.dirname(os.path.abspath(__file__))


def photo(w, h, seed=1):
    """Photo-like content: smooth gradients, edges, some noise."""
    rng = np.random.default_rng(seed)
    y, x = np.mgrid[0:h, 0:w].astype(np.float32)
    r = 128 + 100 * np.sin(x / w * 6.0) * np.cos(y / h * 3.0)
    g = 128 + 90 * np.cos(x / w * 4.0 + y / h * 2.0)
    b = 128 + 80 * np.sin((x + y) / (w + h) * 9.0)
    img = np.stack([r, g, b], -1) + rng.normal(0, 6, (h, w, 3))
    img[(x > w * 0.6) & (x < w * 0.7)] = [240, 240, 240]  # a hard edge
    return np.clip(img, 0, 255).astype(np.uint8)


def quadrants(w, h):
    """TL red, TR green, BL blue, BR white (orientation tests)."""
    a = np.zeros((h, w, 3), np.uint8)
    a[: h // 2, : w // 2] = [255, 0, 0]
    a[: h // 2, w // 2 :] = [0, 255, 0]
    a[h // 2 :, : w // 2] = [0, 0, 255]
    a[h // 2 :, w // 2 :] = [255, 255, 255]
    return a


def save(name, data):
    with open(os.path.join(OUT, name), "wb") as f:
        f.write(data)
    print(f"{name}: {len(data)} bytes")


# AVIF (libavif/aom), reference decoded by Pillow (libavif/dav1d)
buf = io.BytesIO()
Image.fromarray(photo(320, 240)).save(buf, "AVIF", quality=80)
save("photo.avif", buf.getvalue())
ref = io.BytesIO()
Image.open(io.BytesIO(buf.getvalue())).convert("RGB").save(ref, "PNG")
save("photo.avif.ref.png", ref.getvalue())

# animated WebP, lossless, known durations and loop count
frames = [Image.fromarray(np.full((48, 64, 3), c, np.uint8)) for c in ([255, 0, 0], [0, 255, 0], [0, 0, 255])]
buf = io.BytesIO()
frames[0].save(buf, "WEBP", save_all=True, append_images=frames[1:], duration=[40, 60, 80], loop=0, lossless=True)
save("anim.webp", buf.getvalue())

# CMYK JPEG (Adobe APP14, inverted CMYK as Photoshop writes it), reference decoded by libjpeg (Pillow)
buf = io.BytesIO()
Image.fromarray(photo(320, 240)).convert("CMYK").save(buf, "JPEG", quality=92)
save("cmyk.jpg", buf.getvalue())
ref = io.BytesIO()
Image.open(io.BytesIO(buf.getvalue())).convert("RGB").save(ref, "PNG")
save("cmyk.jpg.ref.png", ref.getvalue())

# SVGZ: gzip of an SVG with four quadrants
svg = (
    b'<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10">'
    b'<rect width="10" height="5" fill="#f00"/><rect x="10" width="10" height="5" fill="#0f0"/>'
    b'<rect y="5" width="10" height="5" fill="#00f"/><rect x="10" y="5" width="10" height="5" fill="#fff"/></svg>'
)
save("quadrants.svgz", gzip.compress(svg, mtime=0))

# HEIC, 8 orientations: pixels stored unrotated + irot/imir written by libheif from the EXIF value
# (as iPhones do). Reference = libheif decode, which applies the transformations.
for o in range(1, 9):
    img = Image.fromarray(quadrants(96, 64))
    exif = Image.Exif()
    exif[0x0112] = o
    buf = io.BytesIO()
    pillow_heif.from_pillow(img).save(buf, quality=95, exif=exif.tobytes())
    save(f"orient{o}.heic", buf.getvalue())
    ref = io.BytesIO()
    heif = pillow_heif.open_heif(io.BytesIO(buf.getvalue()))
    heif.to_pillow().convert("RGB").save(ref, "PNG")
    save(f"orient{o}.heic.ref.png", ref.getvalue())

# HEIC colour matrix: same quadrants as BT.601 (libheif/x265 default) and as BT.709
for name, kw in [("heic_bt601", {}), ("heic_bt709", {"matrix_coefficients": 1})]:
    buf = io.BytesIO()
    pillow_heif.from_pillow(Image.fromarray(quadrants(96, 64))).save(buf, quality=95, **kw)
    save(f"{name}.heic", buf.getvalue())
    ref = io.BytesIO()
    pillow_heif.open_heif(io.BytesIO(buf.getvalue())).to_pillow().convert("RGB").save(ref, "PNG")
    save(f"{name}.heic.ref.png", ref.getvalue())

# JPEG XL > 24 MP (libjxl, lossy), for the reduced preview path
big = photo(6000, 4200, seed=2)
save("large.jxl", imagecodecs.jpegxl_encode(big, distance=3.0, effort=3))
