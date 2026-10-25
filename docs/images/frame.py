"""Frames the raw app screenshot in docs/images/raw for the README.

The window gets rounded corners, a hairline border and a soft shadow, on a transparent
background so it sits well on GitHub's light and dark themes. Run from the repository
root after replacing the raw screenshot:

    python docs/images/frame.py

Needs Pillow (pip install pillow). The raw screenshot is the window's contents without
the title bar, as CONTRIBUTING.md describes.
"""

from pathlib import Path

from PIL import Image, ImageChops, ImageDraw, ImageFilter

HERE = Path(__file__).parent
SHOTS = ["dashboard"]

PAD = 64  # room for the shadow
RADIUS = 18  # window corner radius


def rounded_mask(size, radius):
    # Drawn at 4x and shrunk, so the corners are antialiased.
    w, h = size
    big = Image.new("L", (w * 4, h * 4), 0)
    ImageDraw.Draw(big).rounded_rectangle((0, 0, w * 4 - 1, h * 4 - 1), radius * 4, fill=255)
    return big.resize(size, Image.LANCZOS)


def frame(shot):
    shot = shot.convert("RGBA")
    w, h = shot.size
    size = (w + 2 * PAD, h + 2 * PAD)
    mask = rounded_mask((w, h), RADIUS)

    # The shadow's opacity: a wide soft one for depth and a tight one to seat the window.
    alpha = Image.new("L", size, 0)
    for blur, dy, strength in ((28, 14, 70), (4, 2, 40)):
        layer = Image.new("L", size, 0)
        layer.paste(mask.point(lambda v: v * strength // 255), (PAD, PAD + dy))
        alpha = ImageChops.lighter(alpha, layer.filter(ImageFilter.GaussianBlur(blur)))
    canvas = Image.new("RGBA", size, (0, 0, 0, 0))
    canvas.putalpha(alpha)

    # A hairline border, since the app's white edges otherwise melt into a light page.
    edge = ImageChops.subtract(mask, mask.filter(ImageFilter.MinFilter(3)))
    window = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    window.paste(shot, (0, 0), mask)
    border = Image.new("RGBA", (w, h), (31, 35, 40, 255))
    border.putalpha(edge.point(lambda v: v * 50 // 255))
    window = Image.alpha_composite(window, border)

    canvas.alpha_composite(window, (PAD, PAD))
    return canvas


def main():
    for name in SHOTS:
        out = HERE / f"{name}.png"
        frame(Image.open(HERE / "raw" / f"{name}.png")).save(out, optimize=True)
        print(f"{out}: {out.stat().st_size // 1024} KB")


if __name__ == "__main__":
    main()
