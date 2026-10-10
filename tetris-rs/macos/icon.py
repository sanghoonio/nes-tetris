"""Draw Tetris.app's icon: a tetromino on black inside a light-blue frame, in NES pixels.

Drawn from code in the NES palette's colours (nothing is taken from the ROM). The blocks and
the frame copy the game's look: the T and O are white squares with a blue rim and one white
highlight pixel; S and Z are solid blocks with a three-pixel white highlight, in level 8's blue and red.
The frame is a 3-pixel light-blue band whose corners have a black diagonal seam, like the
playfield's border.

    uv run --with pillow python3 tetris-rs/macos/icon.py OUT_DIR [--piece t|o|s|z]

Writes OUT_DIR/icon.png (1024x1024) and OUT_DIR/Tetris.iconset for `iconutil`.
"""

import argparse
from pathlib import Path

from PIL import Image

BLACK = (0, 0, 0, 255)
WHITE = (0xEC, 0xEE, 0xEC, 255)  # NES $30
BLUE = (0x30, 0x32, 0xEC, 255)  # NES $12
RED = (0x98, 0x22, 0x20, 255)  # NES $16
FRAME = (0xA0, 0xD6, 0xE4, 255)  # NES $3C
CLEAR = (0, 0, 0, 0)

SIZE = 39  # canvas, in NES pixels
SCALE = 21  # screen pixels per NES pixel (819 of 1024, about the macOS icon body)
PAD = 1  # black between the icon's edge and the frame
BAND = 3  # frame width

# Each piece: its cells (column, row) and how a block is drawn.
PIECES = {
    "t": ([(0, 0), (1, 0), (2, 0), (1, 1)], "rimmed", BLUE),
    "o": ([(0, 0), (1, 0), (0, 1), (1, 1)], "rimmed", BLUE),
    "s": ([(1, 0), (2, 0), (0, 1), (1, 1)], "solid", BLUE),
    "z": ([(0, 0), (1, 0), (1, 1), (2, 1)], "solid", RED),
}


def frame_pixel(x, y):
    depth = min(x, y, SIZE - 1 - x, SIZE - 1 - y)
    if depth < PAD or depth >= PAD + BAND:
        return BLACK
    # In the band: the black seam runs along each corner's diagonal, as in a mitred frame.
    if x == y or x == SIZE - 1 - y:
        return BLACK
    return FRAME


def block(img, x0, y0, style, colour):
    """One 8x8 cell: a 7x7 block and a 1-pixel gap on its right and bottom."""
    for y in range(7):
        for x in range(7):
            if style == "rimmed":
                edge = x == 0 or y == 0 or x == 6 or y == 6
                c = colour if edge else WHITE
            else:
                c = colour
            img.putpixel((x0 + x, y0 + y), c)
    highlight = [(0, 0)] if style == "rimmed" else [(0, 0), (1, 1), (2, 1), (1, 2)]
    for x, y in highlight:
        img.putpixel((x0 + x, y0 + y), WHITE)


def draw(piece):
    img = Image.new("RGBA", (SIZE, SIZE), CLEAR)
    for y in range(SIZE):
        for x in range(SIZE):
            img.putpixel((x, y), frame_pixel(x, y))
    cells, style, colour = PIECES[piece]
    # Centred: n cells span 8n - 1 pixels (the last cell's gap isn't drawn).
    w = 8 * (max(c for c, _ in cells) + 1) - 1
    h = 8 * (max(r for _, r in cells) + 1) - 1
    x0, y0 = (SIZE - w) // 2, (SIZE - h) // 2
    for cx, cy in cells:
        block(img, x0 + cx * 8, y0 + cy * 8, style, colour)
    big = img.resize((SIZE * SCALE, SIZE * SCALE), Image.NEAREST)
    icon = Image.new("RGBA", (1024, 1024), CLEAR)
    icon.paste(big, ((1024 - big.width) // 2, (1024 - big.height) // 2))
    return icon


def main():
    p = argparse.ArgumentParser()
    p.add_argument("out", type=Path)
    p.add_argument("--piece", choices=sorted(PIECES), default="t")
    a = p.parse_args()
    a.out.mkdir(parents=True, exist_ok=True)
    icon = draw(a.piece)
    icon.save(a.out / "icon.png")
    iconset = a.out / "Tetris.iconset"
    iconset.mkdir(exist_ok=True)
    for size in [16, 32, 128, 256, 512]:
        for mult in [1, 2]:
            n = size * mult
            name = f"icon_{size}x{size}{'@2x' if mult == 2 else ''}.png"
            icon.resize((n, n), Image.LANCZOS if n < 512 else Image.NEAREST).save(iconset / name)


if __name__ == "__main__":
    main()
