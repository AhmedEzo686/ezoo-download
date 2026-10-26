#!/usr/bin/env python3
"""Render Ezoo's original SVG brand assets. Requires Python 3 and ImageMagick 7.

Run from any directory: python3 packaging/render-icons.py
The SVG is the editable source; no existing bitmap is used as input.
"""

from pathlib import Path
import shutil
import struct
import subprocess
import tempfile


def main():
    root = Path(__file__).resolve().parent.parent
    magick = shutil.which("magick")
    if not magick:
        raise SystemExit("ImageMagick 7 is required to render the SVG (magick).")
    source = root / "packaging/ezoo-icon.svg"
    with tempfile.TemporaryDirectory(prefix="ezoo-icons-") as temporary:
        temporary = Path(temporary)
        master = temporary / "master.png"
        subprocess.run([
            magick, "-background", "none", str(source), "-colorspace", "sRGB",
            "-depth", "8", "-strip", "-define", "png:color-type=6", str(master),
        ], check=True)
        sizes = (16, 24, 32, 48, 64, 128, 256, 1024)
        images = {}
        for size in sizes:
            image = temporary / f"icon{size}.png"
            subprocess.run([
                magick, str(master), "-filter", "Lanczos", "-resize", f"{size}x{size}",
                "-depth", "8", "-strip", "-define", "png:color-type=6", str(image),
            ], check=True)
            images[size] = image.read_bytes()
        (root / "crates/app/ui/icon.png").write_bytes(images[1024])
        (root / "crates/app/ui/icon-window.png").write_bytes(images[256])
        for size in (16, 32, 48, 128):
            (root / f"extension/icons/icon{size}.png").write_bytes(images[size])

        # ICO stores native-size PNGs, avoiding a second rasterization or palette loss.
        ico_sizes = (16, 24, 32, 48, 64, 128, 256)
        directory = bytearray(struct.pack("<HHH", 0, 1, len(ico_sizes)))
        offset = 6 + 16 * len(ico_sizes)
        for size in ico_sizes:
            data = images[size]
            directory.extend(struct.pack("<BBBBHHII", size % 256, size % 256, 0, 0, 1, 32, len(data), offset))
            offset += len(data)
        (root / "crates/app/icon.ico").write_bytes(directory + b"".join(images[size] for size in ico_sizes))
    print("Rendered app, window, Windows and extension icons from packaging/ezoo-icon.svg")


if __name__ == "__main__":
    main()
