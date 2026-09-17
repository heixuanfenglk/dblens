# -*- coding: utf-8 -*-
"""Regenerate multi-resolution dblens.ico properly."""
from pathlib import Path
from PIL import Image, ImageDraw, ImageFont
import struct
import io

ROOT = Path(r"d:\repos\allink")
ASSETS = ROOT / "assets"
ASSETS.mkdir(exist_ok=True)

BG = (30, 90, 160, 255)
FG = (255, 255, 255, 255)
ACCENT = (70, 160, 230, 255)


def make_icon(size: int) -> Image.Image:
    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    pad = max(1, size // 16)
    radius = max(2, size // 5)
    box = [pad, pad, size - pad - 1, size - pad - 1]
    d.rounded_rectangle(box, radius=radius, fill=BG)
    # highlight strip
    hi = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    hd = ImageDraw.Draw(hi)
    hd.rounded_rectangle(box, radius=radius, fill=(255, 255, 255, 40))
    mask = Image.new("L", (size, size), 0)
    ImageDraw.Draw(mask).rectangle([0, 0, size, max(1, size // 2)], fill=255)
    img = Image.alpha_composite(img, Image.composite(hi, Image.new("RGBA", (size, size)), mask))
    d = ImageDraw.Draw(img)

    font_size = max(8, int(size * 0.58))
    font = None
    for fp in [
        r"C:\Windows\Fonts\segoeuib.ttf",
        r"C:\Windows\Fonts\arialbd.ttf",
        r"C:\Windows\Fonts\msyhbd.ttc",
    ]:
        try:
            font = ImageFont.truetype(fp, font_size)
            break
        except OSError:
            continue
    if font is None:
        font = ImageFont.load_default()

    text = "A"
    bbox = d.textbbox((0, 0), text, font=font)
    tw, th = bbox[2] - bbox[0], bbox[3] - bbox[1]
    x = (size - tw) / 2 - bbox[0]
    y = (size - th) / 2 - bbox[1] - size * 0.02
    d.text((x, y), text, font=font, fill=FG)

    r = max(2, size // 14)
    d.ellipse(
        [size - pad - r * 3, size - pad - r * 3, size - pad - r, size - pad - r],
        fill=ACCENT,
    )
    return img


def write_ico(path: Path, sizes: list[int]) -> None:
    """Write a valid multi-size ICO with PNG-compressed entries (Vista+)."""
    entries = []
    for s in sizes:
        im = make_icon(s)
        buf = io.BytesIO()
        im.save(buf, format="PNG")
        entries.append((s, buf.getvalue()))

    # ICONDIR + ICONDIRENTRY*n + data
    count = len(entries)
    offset = 6 + 16 * count
    out = io.BytesIO()
    out.write(struct.pack("<HHH", 0, 1, count))  # reserved, type=icon, count
    data_blobs = []
    for s, png in entries:
        w = 0 if s >= 256 else s
        h = 0 if s >= 256 else s
        out.write(struct.pack("<BBBBHHII", w, h, 0, 0, 1, 32, len(png), offset))
        data_blobs.append(png)
        offset += len(png)
    for blob in data_blobs:
        out.write(blob)
    path.write_bytes(out.getvalue())


sizes = [16, 24, 32, 48, 64, 128, 256]
write_ico(ASSETS / "dblens.ico", sizes)
make_icon(256).save(ASSETS / "dblens.png")
make_icon(32).save(ASSETS / "dblens_32.png")
print("ico", (ASSETS / "dblens.ico").stat().st_size, "bytes")
print("png", (ASSETS / "dblens.png").stat().st_size, "bytes")
