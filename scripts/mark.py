#!/usr/bin/env python3
"""Draws Tendril's mark and everything made from it.

The mark is the launcher as a layered object: the search field on top, carrying the tendril
(the same glyph Tendril draws for its own commands), and its results stacked beneath, each a
step deeper. One hue and one light colour: blue and white, or Advantis' pink and cream.

Writes:
  assets/tendril.svg, assets/tendril-pink.svg    the mark
  assets/tendril-small.svg                       the field alone, for 16 to 32 pixels
  assets/tendril-256.png, tendril.ico, .icns     the app icon, for windows, Windows and macOS
  docs/brand/*.png                               the wordmark, on dark and on light

Needs Inkscape (to turn the SVGs into pixels) and Pillow. Run from the repository's root:
  python3 scripts/mark.py
"""

import io
import subprocess
import tempfile
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

PALETTES = {
    "blue": dict(top=("#86b6ff", "#3f74e2"), mid=("#3d6ad6", "#2a52b8"), low=("#2a4ea8", "#1d3a8a"), ink="#ffffff", shade="#0c1a4a"),
    "pink": dict(top=("#ff8aa0", "#ee4767"), mid=("#e23d5e", "#c92c4e"), low=("#c22c4c", "#a11f3d"), ink="#fbf7ee", shade="#4a0c1c"),
}

# On Lucide's 24 grid, as in scripts/icons.py.
CURL = "M5 21c0-6.6 4-11.2 9.6-11.2 3.5 0 5.9 2.1 5.9 5 0 2.5-1.8 4.2-4 4.2-1.8 0-3-1.2-3-2.7"
LEAF = "M10.3 12.6C8.1 7.5 10 3.4 15.8 2.3c1.1 5.2-1.5 9-5.5 10.3Z"


def defs(c):
    def grad(id, a, b):
        return f'<linearGradient id="{id}" x1="0" y1="0" x2=".35" y2="1"><stop offset="0" stop-color="{a}"/><stop offset="1" stop-color="{b}"/></linearGradient>'

    # Drop shadows in SVG 1.1 terms; feDropShadow isn't drawn everywhere.
    def shadow(id, dy, sd, op):
        return (
            f'<filter id="{id}" x="-30%" y="-30%" width="160%" height="170%"><feGaussianBlur in="SourceAlpha" stdDeviation="{sd}"/>'
            f'<feOffset dy="{dy}" result="b"/><feFlood flood-color="{c["shade"]}" flood-opacity="{op}"/><feComposite in2="b" operator="in"/>'
            f'<feMerge><feMergeNode/><feMergeNode in="SourceGraphic"/></feMerge></filter>'
        )

    return (
        f'<defs>{grad("top", *c["top"])}{grad("mid", *c["mid"])}{grad("low", *c["low"])}'
        '<linearGradient id="sheen" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#fff" stop-opacity=".42"/>'
        '<stop offset=".55" stop-color="#fff" stop-opacity="0"/></linearGradient>'
        f'{shadow("deep", 26, 30, 0.38)}{shadow("step", 14, 16, 0.32)}{shadow("ink", 10, 12, 0.28)}</defs>'
    )


def glyph(c, x, y, scale):
    return (
        f'<g filter="url(#ink)" transform="translate({x} {y}) scale({scale})">'
        f'<path d="{CURL}" fill="none" stroke="{c["ink"]}" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round"/>'
        f'<path d="{LEAF}" fill="{c["ink"]}" stroke="{c["ink"]}" stroke-width="1.2" stroke-linejoin="round"/></g>'
    )


def field(x, y, w, h, r):
    return (
        f'<rect x="{x}" y="{y}" width="{w}" height="{h}" rx="{r}" fill="url(#top)" filter="url(#step)"/>'
        f'<rect x="{x}" y="{y}" width="{w}" height="{h}" rx="{r}" fill="url(#sheen)"/>'
        f'<rect x="{x + 3}" y="{y + 3}" width="{w - 6}" height="{h - 6}" rx="{r - 3}" fill="none" stroke="#fff" stroke-opacity=".35" stroke-width="5"/>'
    )


def mark(name):
    c = PALETTES[name]
    return (
        '<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="1024" viewBox="0 0 1024 1024">'
        + defs(c)
        # The results, stacked beneath.
        + '<rect x="262" y="600" width="500" height="236" rx="84" fill="url(#low)" filter="url(#deep)"/>'
        + '<rect x="190" y="480" width="644" height="250" rx="100" fill="url(#mid)" filter="url(#step)"/>'
        + '<rect x="190" y="480" width="644" height="250" rx="100" fill="url(#sheen)" opacity=".5"/>'
        # The field on top, with the tendril.
        + field(118, 168, 788, 440, 148)
        + glyph(c, 296, 196, 17.5)
        + "</svg>\n"
    )


def small(name):
    """At 16 to 32 pixels the stack turns to mush: the field alone, filling the square."""
    c = PALETTES[name]
    return (
        '<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="1024" viewBox="0 0 1024 1024">'
        + defs(c)
        + field(40, 40, 944, 944, 230)
        + glyph(c, 152, 140, 30)
        + "</svg>\n"
    )


def render(svg, px):
    with tempfile.TemporaryDirectory() as tmp:
        src, out = Path(tmp, "m.svg"), Path(tmp, "m.png")
        src.write_text(svg)
        subprocess.run(["inkscape", str(src), "-o", str(out), "-w", str(px), "-h", str(px)], check=True, capture_output=True)
        return Image.open(io.BytesIO(out.read_bytes())).convert("RGBA")


def wordmark(icon, ink):
    font = ImageFont.truetype("fonts/Geist-SemiBold.ttf", 300)
    width = int(font.getlength("Tendril"))
    image = Image.new("RGBA", (480 + 40 + width + 40, 480), (0, 0, 0, 0))
    image.alpha_composite(icon.resize((480, 480), Image.LANCZOS))
    ImageDraw.Draw(image).text((520, 248), "Tendril", font=font, fill=ink, anchor="lm")
    return image


def main():
    assets, brand = Path("assets"), Path("docs/brand")
    brand.mkdir(parents=True, exist_ok=True)
    blue, pink, tiny = mark("blue"), mark("pink"), small("blue")
    (assets / "tendril.svg").write_text(blue)
    (assets / "tendril-pink.svg").write_text(pink)
    (assets / "tendril-small.svg").write_text(tiny)

    big = render(blue, 1024)
    big.resize((256, 256), Image.LANCZOS).save(assets / "tendril-256.png")
    # Each size drawn at its own pixels; the small ones from the simpler field.
    sizes = {px: render(tiny if px <= 32 else blue, px) for px in (16, 20, 24, 32, 48, 64, 128, 256)}
    sizes[256].save(assets / "tendril.ico", sizes=[(p, p) for p in sizes], append_images=[sizes[p] for p in sizes if p != 256])
    big.save(assets / "tendril.icns")

    pink_big = render(pink, 1024)
    for file, icon, ink in [
        ("wordmark-dark.png", big, "#ffffff"),
        ("wordmark-light.png", big, "#14161c"),
        ("wordmark-pink-dark.png", pink_big, "#fbf7ee"),
        ("wordmark-pink-light.png", pink_big, "#ee4767"),
    ]:
        wordmark(icon, ink).save(brand / file, optimize=True)


if __name__ == "__main__":
    main()
