"""Render the app icon from its drawing into the bitmap formats systems want.

usage: python scripts/render-icon.py

Reads assets/icon/axe-grid.svg and writes, next to it:
  axe-grid.png   256 x 256, for the window and for Linux launchers
  axe-grid.ico   16 to 256 pixels, for the Windows executable and shortcuts

Needs Pillow (`pip install pillow`). The drawing uses only rounded
rectangles, circles and paths of straight lines and cubic curves, so this
script draws those itself instead of depending on an SVG library. Run it
again whenever the SVG changes; the outputs are committed.
"""
import re
import sys
import xml.etree.ElementTree as ET
from pathlib import Path

from PIL import Image, ImageDraw

ICON_DIR = Path(__file__).resolve().parent.parent / "assets" / "icon"
# Drawn large and scaled down, which smooths the edges.
OVERSAMPLE = 4
PNG_SIZE = 256
ICO_SIZES = [16, 24, 32, 48, 64, 128, 256]
CURVE_STEPS = 48


def color(text):
    return None if text in (None, "none") else text


def path_points(data):
    """Polylines of a path made of M, H and C commands with absolute coordinates."""
    tokens = re.findall(r"[MHC]|-?\d+(?:\.\d+)?", data)
    lines, current, at, i = [], [], (0.0, 0.0), 0
    while i < len(tokens):
        command = tokens[i]
        i += 1
        if command == "M":
            at = (float(tokens[i]), float(tokens[i + 1]))
            i += 2
            current = [at]
            lines.append(current)
        elif command == "H":
            at = (float(tokens[i]), at[1])
            i += 1
            current.append(at)
        elif command == "C":
            p0 = at
            p1, p2, p3 = [(float(tokens[i + 2 * k]), float(tokens[i + 2 * k + 1])) for k in range(3)]
            i += 6
            for step in range(1, CURVE_STEPS + 1):
                t = step / CURVE_STEPS
                u = 1 - t
                current.append(tuple(
                    u ** 3 * p0[axis] + 3 * u * u * t * p1[axis] + 3 * u * t * t * p2[axis] + t ** 3 * p3[axis]
                    for axis in (0, 1)
                ))
            at = p3
        else:
            raise ValueError(f"unsupported path command {command!r}")
    return lines


def render(svg_path, size):
    root = ET.parse(svg_path).getroot()
    view = [float(v) for v in root.get("viewBox").split()]
    scale = size * OVERSAMPLE / view[2]
    image = Image.new("RGBA", (size * OVERSAMPLE, size * OVERSAMPLE), (0, 0, 0, 0))
    draw = ImageDraw.Draw(image)

    def px(value):
        return float(value) * scale

    for node in root:
        tag = node.tag.split("}")[-1]
        fill, stroke = color(node.get("fill")), color(node.get("stroke"))
        width = max(1, round(px(node.get("stroke-width", 0))))
        if tag == "rect":
            x, y = px(node.get("x", 0)), px(node.get("y", 0))
            box = [x, y, x + px(node.get("width")), y + px(node.get("height"))]
            draw.rounded_rectangle(box, radius=px(node.get("rx", 0)), fill=fill, outline=stroke, width=width)
        elif tag == "circle":
            cx, cy, r = px(node.get("cx")), px(node.get("cy")), px(node.get("r"))
            # The outline is centred on the edge, as SVG draws it.
            half = width / 2 if stroke else 0
            draw.ellipse([cx - r - half, cy - r - half, cx + r + half, cy + r + half], fill=stroke or fill)
            if stroke:
                draw.ellipse([cx - r + half, cy - r + half, cx + r - half, cy + r - half], fill=fill)
        elif tag == "path":
            for line in path_points(node.get("d")):
                draw.line([(px(x), px(y)) for x, y in line], fill=stroke, width=width, joint="curve")
        else:
            raise ValueError(f"unsupported element {tag!r}")
    return image.resize((size, size), Image.LANCZOS)


def main():
    svg = ICON_DIR / "axe-grid.svg"
    render(svg, PNG_SIZE).save(ICON_DIR / "axe-grid.png", optimize=True)
    # Each size is drawn on its own, so the small ones stay crisp.
    frames = [render(svg, size) for size in ICO_SIZES]
    frames[-1].save(ICON_DIR / "axe-grid.ico", sizes=[(s, s) for s in ICO_SIZES], append_images=frames[:-1])
    print("wrote", ICON_DIR / "axe-grid.png", "and", ICON_DIR / "axe-grid.ico")


if __name__ == "__main__":
    sys.exit(main())
