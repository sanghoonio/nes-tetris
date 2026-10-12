"""Build Brickface (fonts/brickface.woff2) from glyphs.txt: each glyph's pixels traced into
outlines (one contour per edge of the inked shape, so there are no seams between pixels).

    uv run --with fonttools --with brotli python web/font/make_font.py

Also writes font/preview.png (needs pillow) to look at.
"""

import sys
from pathlib import Path

from fontTools.fontBuilder import FontBuilder
from fontTools.pens.ttGlyphPen import TTGlyphPen

HERE = Path(__file__).parent
PX = 100  # font units per pixel
CELL = 8  # pixels per glyph, across and down
ROWS = 7  # inked rows; the 8th is below the baseline


def read_glyphs():
    """{char: [row strings]} from glyphs.txt."""
    lines = [l.rstrip("\n") for l in (HERE / "glyphs.txt").read_text().splitlines() if not l.startswith("# ")]
    glyphs, i = {}, 0
    while i < len(lines):
        if not lines[i].strip():
            i += 1
            continue
        chars, rows = lines[i], lines[i + 1 : i + 1 + ROWS]
        assert len(rows) == ROWS and all(len(r) == 7 for r in rows), f"glyph {chars!r}"
        for c in chars:
            glyphs[c] = rows
        i += 1 + ROWS
    return glyphs


def contours(rows):
    """The outlines of the inked pixels, corners only, in grid points (y down)."""
    ink = {(x, y) for y, row in enumerate(rows) for x, ch in enumerate(row) if ch == "#"}
    # Each ink pixel's edges that border no ink, as directed segments between grid points (gy
    # counts down from the top), going round each shape with the ink on the right. Where two
    # pixels touch only at a corner, a point starts two segments.
    starts = {}
    for x, y in ink:
        for (dx, dy), a, b in (
            ((0, -1), (x, y), (x + 1, y)),
            ((1, 0), (x + 1, y), (x + 1, y + 1)),
            ((0, 1), (x + 1, y + 1), (x, y + 1)),
            ((-1, 0), (x, y + 1), (x, y)),
        ):
            if (x + dx, y + dy) not in ink:
                starts.setdefault(a, []).append(b)
    loops = []
    while starts:
        a = next(iter(starts))
        loop = [a]
        while True:
            b = starts[a].pop()
            if not starts[a]:
                del starts[a]
            if b == loop[0]:
                break
            loop.append(b)
            a = b
        # Corners only.
        n = len(loop)
        corners = [p for i, p in enumerate(loop) if not same_line(loop[i - 1], p, loop[(i + 1) % n])]
        loops.append(corners)
    return loops


def same_line(a, b, c):
    return (b[0] - a[0]) * (c[1] - b[1]) == (b[1] - a[1]) * (c[0] - b[0])


def main():
    glyphs = read_glyphs()
    names = {c: f"uni{ord(c):04X}" for c in glyphs}
    order = [".notdef", "space"] + [names[c] for c in glyphs]
    fb = FontBuilder(PX * CELL, isTTF=True)
    fb.setupGlyphOrder(order)
    cmap = {32: "space", 160: "space"}
    for c, n in names.items():
        cmap[ord(c)] = n
        if c.isalpha() and c.lower() != c:
            cmap[ord(c.lower())] = n
    fb.setupCharacterMap(cmap)
    outlines, lsb = {}, {}
    for name in [".notdef", "space"]:
        outlines[name] = TTGlyphPen(None).glyph()
    for c, rows in glyphs.items():
        pen = TTGlyphPen(None)
        for loop in contours(rows):
            # Grid point (gx, gy from the top) to font units, baseline under row 7. Flipping y
            # turns the loops round, so reverse them: TrueType's outer contours go clockwise.
            pts = [(gx * PX, (ROWS - gy) * PX) for gx, gy in reversed(loop)]
            pen.moveTo(pts[0])
            for p in pts[1:]:
                pen.lineTo(p)
            pen.closePath()
        outlines[names[c]] = pen.glyph()
        lsb[names[c]] = min((gx for loop in contours(rows) for gx, _ in loop), default=0) * PX
    fb.setupGlyf(outlines)
    fb.setupHorizontalMetrics({n: (PX * CELL, lsb.get(n, 0)) for n in order})
    fb.setupHorizontalHeader(ascent=PX * ROWS, descent=-PX)
    fb.setupNameTable({"familyName": "Brickface", "styleName": "Regular"})
    fb.setupOS2(sTypoAscender=PX * ROWS, sTypoDescender=-PX, sTypoLineGap=0, usWinAscent=PX * ROWS, usWinDescent=PX)
    fb.setupPost()
    out = HERE.parent / "public" / "fonts"
    out.mkdir(parents=True, exist_ok=True)
    fb.font.flavor = "woff2"
    fb.save(out / "brickface.woff2")
    print(f"{len(glyphs)} glyphs -> {out / 'brickface.woff2'}")
    if "--preview" in sys.argv:
        preview(glyphs)


def preview(glyphs):
    """font/preview.png: every glyph from glyphs.txt, 4x, in rows of 16."""
    from PIL import Image

    chars = list(glyphs)
    scale, cols = 4, 16
    img = Image.new("RGB", (cols * CELL * scale, -(-len(chars) // cols) * CELL * scale), "black")
    for i, c in enumerate(chars):
        ox, oy = (i % cols) * CELL * scale, (i // cols) * CELL * scale
        for y, row in enumerate(glyphs[c]):
            for x, ch in enumerate(row):
                if ch == "#":
                    for dy in range(scale):
                        for dx in range(scale):
                            img.putpixel((ox + x * scale + dx, oy + y * scale + dy), (236, 238, 236))
    img.save(HERE / "preview.png")


if __name__ == "__main__":
    main()
