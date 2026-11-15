"""Draws Tendril's icons into ui/icons/: each as an outline and a solid, in the style of Blight's
dashboard icons (apps/dashboard/src/lib/icons.ts): Lucide's 24 square, round ends, one colour.
The outline is the usual one; the solid is for the row you're on.

An icon is a list of parts:
  shape(d)   a closed outline; filled in the solid
  line(d)    a stroke in both; heavier in the solid
  cut(d)     a stroke inside a shape; in the solid it's cut out of the fill
  dot(x,y,r) a filled circle, a little bigger in the solid
  hole(x,y,r) a filled circle in the outline, cut out of the fill in the solid

Run: python3 scripts/icons.py
"""
from pathlib import Path

OUT = Path(__file__).resolve().parent.parent / "ui" / "icons"
STROKE, HEAVY = 1.75, 2.4

def shape(d): return ("shape", d)
def line(d): return ("line", d)
def cut(d): return ("cut", d)
def dot(x, y, r): return ("dot", (x, y, r))
def hole(x, y, r): return ("hole", (x, y, r))
def circle(x, y, r): return shape(f"M{x - r} {y}a{r} {r} 0 1 0 {2 * r} 0a{r} {r} 0 1 0 {-2 * r} 0Z")
def rect(x, y, w, h, r): return shape(f"M{x + r} {y}h{w - 2 * r}a{r} {r} 0 0 1 {r} {r}v{h - 2 * r}a{r} {r} 0 0 1 {-r} {r}h{-(w - 2 * r)}a{r} {r} 0 0 1 {-r} {-r}v{-(h - 2 * r)}a{r} {r} 0 0 1 {r} {-r}Z")

STAR = "M12 3l2.7 5.6 6.1.9-4.4 4.3 1 6.1L12 17.1 6.6 19.9l1-6.1-4.4-4.3 6.1-.9Z"
SPARK = "M12 3c.7 4.7 2.3 6.3 7 7-4.7.7-6.3 2.3-7 7-.7-4.7-2.3-6.3-7-7 4.7-.7 6.3-2.3 7-7Z"
PALETTE = "M12 3a9 9 0 0 0 0 18c1.3 0 2-.9 2-1.9 0-1.3-1-1.7-1-2.9 0-1 .8-1.7 1.8-1.7h2.3A3.9 3.9 0 0 0 21 10.6C21 6.4 17 3 12 3Z"

ICONS = {
    # Tendril's mark: a tendril curling as it reaches out, with a leaf where it starts.
    "tendril": [line("M5 21c0-6.6 4-11.2 9.6-11.2 3.5 0 5.9 2.1 5.9 5 0 2.5-1.8 4.2-4 4.2-1.8 0-3-1.2-3-2.7"),
                shape("M10.3 12.6C8.1 7.5 10 3.4 15.8 2.3c1.1 5.2-1.5 9-5.5 10.3Z")],
    "app": [rect(3.5, 3.5, 7, 7, 2), rect(13.5, 3.5, 7, 7, 2), rect(3.5, 13.5, 7, 7, 2), rect(13.5, 13.5, 7, 7, 2)],
    "lock": [rect(5, 10.5, 14, 10, 2.5), line("M8 10.5V7.5a4 4 0 0 1 8 0v3"), hole(12, 15.5, 1.4)],
    "moon": [shape("M20 14.3A8 8 0 1 1 9.7 4a6.3 6.3 0 0 0 10.3 10.3Z"), line("M17.5 3v3.5"), line("M15.75 4.75h3.5")],
    "logout": [shape("M10.5 3.5H6.5a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2h4Z"), line("M14.5 16.5 19 12l-4.5-4.5"), line("M19 12h-9.5")],
    "restart": [line("M20 12a8 8 0 1 1-2.4-5.7"), shape("M20.5 3.5v5h-5Z")],
    "power": [line("M12 3v8"), line("M17.7 6.6a8 8 0 1 1-11.4 0")],
    "settings": [shape("M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8Z"), cut("M14 3v5h5"),
                 cut("M8.5 13h7"), cut("M8.5 17h7"), hole(10.5, 13, 1.6), hole(14, 17, 1.6)],
    "refresh": [line("M4.5 10a7.6 7.6 0 0 1 13.5-3.4L20 9"), line("M20 4.5V9h-4.5"),
                line("M19.5 14a7.6 7.6 0 0 1-13.5 3.4L4 15"), line("M4 19.5V15h4.5")],
    "palette": [shape(PALETTE), hole(7.6, 11.2, 1.3), hole(9.6, 7.2, 1.3), hole(14.2, 7, 1.3)],
    "swatch": [shape("M4 5.5A1.5 1.5 0 0 1 5.5 4h4A1.5 1.5 0 0 1 11 5.5V17a3.5 3.5 0 0 1-7 0Z"),
               line("M11 8.5l2.7-2.7a1.5 1.5 0 0 1 2.1 0l2.4 2.4a1.5 1.5 0 0 1 0 2.1L11 17.5"),
               line("M13.5 13H18.5A1.5 1.5 0 0 1 20 14.5v4A1.5 1.5 0 0 1 18.5 20H7.5"), hole(7.5, 17, 1.1)],
    "quit": [circle(12, 12, 9), cut("M9 9l6 6"), cut("M15 9l-6 6")],
    "globe": [circle(12, 12, 9), cut("M3.5 12h17"), cut("M12 3c2.4 2.5 3.6 5.5 3.6 9s-1.2 6.5-3.6 9c-2.4-2.5-3.6-5.5-3.6-9S9.6 5.5 12 3Z")],
    "link": [line("M10 14a4.5 4.5 0 0 0 6.4 0l3-3a4.5 4.5 0 0 0-6.4-6.4l-1 1"), line("M14 10a4.5 4.5 0 0 0-6.4 0l-3 3a4.5 4.5 0 0 0 6.4 6.4l1-1")],
    "calculator": [rect(5, 2.5, 14, 19, 2.5), cut("M8.5 6.5h7v3h-7Z"), hole(8.75, 13.5, 1), hole(12, 13.5, 1), hole(15.25, 13.5, 1),
                   hole(8.75, 17, 1), hole(12, 17, 1), hole(15.25, 17, 1)],
    "copy": [rect(8.5, 8.5, 12, 12, 2.5), line("M15.5 5.5V5A1.5 1.5 0 0 0 14 3.5H5A1.5 1.5 0 0 0 3.5 5v9A1.5 1.5 0 0 0 5 15.5h.5")],
    "folder": [shape("M3.5 7A1.5 1.5 0 0 1 5 5.5h4.4l2 2H19A1.5 1.5 0 0 1 20.5 9v9A1.5 1.5 0 0 1 19 19.5H5A1.5 1.5 0 0 1 3.5 18Z")],
    "open": [line("M13.5 4.5h6v6"), line("M19.5 4.5 11 13"), line("M17.5 14v4.5A1.5 1.5 0 0 1 16 20H6a1.5 1.5 0 0 1-1.5-1.5v-10A1.5 1.5 0 0 1 6 7h4.5")],
    "eraser": [shape("M4.6 14.6l9.3-9.3a2 2 0 0 1 2.8 0l2.6 2.6a2 2 0 0 1 0 2.8L11 18.9H7.8l-3.2-3.2a.8.8 0 0 1 0-1.1Z"), cut("M9.3 9.6l5.1 5.1"), line("M12 20.5h7.5")],
    "arrow": [line("M5 12h13.5"), line("M13 6.5 18.5 12 13 17.5")],
    "check": [line("M5 12.5l4.5 4.5L19 7.5")],
    # The Appearance settings.
    "blur": [circle(12, 12, 4), dot(12, 3.8, 1.2), dot(17.8, 6.2, 1.2), dot(20.2, 12, 1.2), dot(17.8, 17.8, 1.2),
             dot(12, 20.2, 1.2), dot(6.2, 17.8, 1.2), dot(3.8, 12, 1.2), dot(6.2, 6.2, 1.2)],
    "window": [rect(3, 4.5, 18, 15, 2.5), cut("M3.5 9h17"), hole(6.5, 6.8, .9), hole(9.3, 6.8, .9)],
    "opacity": [circle(12, 12, 8.5), shape("M12 3.5a8.5 8.5 0 0 1 0 17Z")],
    "sparkle": [shape(SPARK), line("M18.5 15.5v4"), line("M16.5 17.5h4")],
    "gradient": [line("M3.5 12h5"), dot(11.5, 12, 1.5), dot(15, 12, 1.15), dot(18, 12, .85), dot(20.6, 12, .6)],
    "resize": [line("M4 9V5.5A1.5 1.5 0 0 1 5.5 4H9"), line("M15 4h3.5A1.5 1.5 0 0 1 20 5.5V9"),
               line("M20 15v3.5a1.5 1.5 0 0 1-1.5 1.5H15"), line("M9 20H5.5A1.5 1.5 0 0 1 4 18.5V15"), rect(8.5, 8.5, 7, 7, 1.5)],
    "columns": [rect(3, 4.5, 18, 15, 2.5), cut("M10 4.5v15"), cut("M13 9.5h4.5"), cut("M13 13h3")],
    "star": [shape(STAR)],
    "number": [rect(4, 4, 16, 16, 3.5), cut("M10.5 9.6 12.6 8v8"), cut("M10.4 16h4.4")],
    "heading": [line("M5 6.5h10"), line("M5 12h14"), line("M5 17.5h11")],
    "pointer": [shape("M5.5 4.5l5.3 14.6 2.2-6.1 6.1-2.2Z"), line("M14 14l5 5")],
    "login": [line("M14 3.5h3.5a2 2 0 0 1 2 2v13a2 2 0 0 1-2 2H14"), line("M9.5 16.5 14 12 9.5 7.5"), line("M14 12H3.5")],
    "wave": [line("M7.5 12.5V6a1.5 1.5 0 0 1 3 0v5"), line("M10.5 10.5V4.5a1.5 1.5 0 0 1 3 0v6"),
             line("M13.5 10.5V5.5a1.5 1.5 0 0 1 3 0V12"), shape("M16.5 9.5a1.5 1.5 0 0 1 3 0v4.5a7 7 0 0 1-7 7h-1a6 6 0 0 1-4.6-2.1L4 15.4a1.5 1.5 0 0 1 2.2-2.1l1.3 1.2V12.5")],
}

def svg(body):
    return ('<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" '
            'stroke="#000" stroke-linecap="round" stroke-linejoin="round">' + body + "</svg>\n")

def circ(x, y, r, fill="#000"):
    return f'<circle cx="{x}" cy="{y}" r="{r}" fill="{fill}" stroke="none"/>'

def outline(parts):
    out = []
    for kind, d in parts:
        if kind in ("shape", "line", "cut"):
            out.append(f'<path d="{d}" stroke-width="{STROKE}"/>')
        else:
            out.append(circ(*d))
    return svg("".join(out))

def solid(parts):
    shapes = [d for k, d in parts if k == "shape"]
    cuts = [d for k, d in parts if k == "cut"]
    holes = [d for k, d in parts if k == "hole"]
    loose_dots = [d for k, d in parts if k == "dot"]
    body = []
    if shapes:
        # The fill, with the details cut out of it. The fill keeps the outline's edge so the
        # solid is the same size as the outline.
        mask = ['<rect width="24" height="24" fill="#fff"/>']
        mask += [f'<path d="{d}" stroke="#000" stroke-width="{STROKE}"/>' for d in cuts]
        mask += [circ(x, y, r * 1.15) for x, y, r in holes]
        body.append('<defs><mask id="m" maskUnits="userSpaceOnUse" x="0" y="0" width="24" height="24">' + "".join(mask) + "</mask></defs>")
        body.append('<g mask="url(#m)">' + "".join(f'<path d="{d}" fill="#000" stroke-width="{STROKE}"/>' for d in shapes) + "</g>")
    body += [f'<path d="{d}" stroke-width="{HEAVY}"/>' for k, d in parts if k == "line"]
    body += [circ(x, y, r * 1.2) for x, y, r in loose_dots]
    return svg("".join(body))

if __name__ == "__main__":
    OUT.mkdir(parents=True, exist_ok=True)
    for old in OUT.glob("*.svg"):
        old.unlink()
    for name, parts in ICONS.items():
        (OUT / f"{name}.svg").write_text(outline(parts))
        (OUT / f"{name}-solid.svg").write_text(solid(parts))
    print(f"{len(ICONS)} icons, outline and solid, in {OUT}")
