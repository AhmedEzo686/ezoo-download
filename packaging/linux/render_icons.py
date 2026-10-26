#!/usr/bin/env python3
"""Rasterize the Ezoo SVG icon set to all PNG sizes needed for Linux packaging.

Sources (defaults): packaging/linux/hicolor/scalable/apps/*.svg
Outputs (defaults): packaging/linux/icons-out/
  icons-out/app-{size}.png            hicolor app icon (scalable source: ezoo-download.svg)
  icons-out/tray-{size}.png           tray / SNI pixmaps (ezoo-download-tray.svg)
  icons-out/ext{size}.png             browser extension icons
  icons-out/icon.png / icon-window.png  Slint UI icons (256px)
  icons-out/tray32.rgba               raw RGBA for the Windows tray (kept in sync)
  icons-out/ezoo-preview.png          512 px preview for README

Set EZOO_ICONS_SRC / EZOO_ICONS_OUT to render from / to other directories.
"""
import cairosvg, os

# Paths are resolved from this script's own directory, so the script works
# for anyone checking out the repo — no local paths to edit.
# Override with EZOO_ICONS_SRC / EZOO_ICONS_OUT when rendering elsewhere.
HERE = os.path.dirname(os.path.abspath(__file__))
SRC = os.environ.get("EZOO_ICONS_SRC", os.path.join(HERE, "hicolor", "scalable", "apps"))
# the symbolic icon lives in hicolor/symbolic, not scalable
SYM_SRC = os.environ.get("EZOO_ICONS_SYM", os.path.join(HERE, "hicolor", "symbolic", "apps"))
OUT = os.environ.get("EZOO_ICONS_OUT", os.path.join(HERE, "icons-out"))
os.makedirs(OUT, exist_ok=True)

APP = f"{SRC}/ezoo-download.svg"
TRAY = f"{SRC}/ezoo-download-tray.svg"
SYM = f"{SYM_SRC}/ezoo-download-symbolic.svg"

def render(svg, size, name):
    cairosvg.svg2png(url=svg, write_to=f"{OUT}/{name}", output_width=size, output_height=size)
    print(f"  {name} ({size}px)")

# hicolor app icons
for s in [16, 22, 24, 32, 48, 64, 128, 256, 512]:
    render(APP, s, f"app-{s}.png")

# tray pixmaps (StatusNotifierItem: 16..64)
for s in [16, 22, 24, 32, 48, 64]:
    render(TRAY, s, f"tray-{s}.png")

# extension icons (Chrome/Firefox want 16/32/48/128)
for s in [16, 32, 48, 128]:
    render(TRAY, s, f"ext{s}.png")

# symbolic previews
for s in [16, 24]:
    render(SYM, s, f"symbolic-{s}.png")

# Slint UI icons (same paths/sizes as upstream: 256px)
render(APP, 256, "icon.png")
render(APP, 256, "icon-window.png")

# tray32.rgba: raw RGBA 32x32 from the tray svg (Windows path stays in sync)
cairosvg.svg2png(url=TRAY, write_to=f"{OUT}/tray32.png", output_width=32, output_height=32)
from PIL import Image
im = Image.open(f"{OUT}/tray32.png").convert("RGBA")
with open(f"{OUT}/tray32.rgba", "wb") as f:
    f.write(im.tobytes())
print("  tray32.rgba (32x32 RGBA)")

# big preview for README
cairosvg.svg2png(url=APP, write_to=f"{OUT}/ezoo-preview.png", output_width=512, output_height=512)
print("done")
