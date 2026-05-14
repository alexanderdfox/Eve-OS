#!/usr/bin/env python3
"""Regenerate kernel/assets/cursor_noto/*.rgba from Noto Emoji PNGs (Apache-2.0).

Requires: pillow, network. Output: eight 16x16 RGBA files for cursor presets.
"""
from __future__ import annotations

import urllib.request
from io import BytesIO
from pathlib import Path

BASE = "https://raw.githubusercontent.com/googlefonts/noto-emoji/v2.042/png/72/"
# Order must match kernel/src/cursor_emoji.rs preset indices.
CODES = ["1f5b1", "1f600", "2764", "2b50", "1f680", "1f431", "1f44d", "1f308"]
SIZE = 16
ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "kernel" / "assets" / "cursor_noto"


def main() -> None:
    try:
        from PIL import Image
    except ImportError:
        raise SystemExit("pip install pillow")

    OUT.mkdir(parents=True, exist_ok=True)
    for i, cp in enumerate(CODES):
        url = f"{BASE}emoji_u{cp}.png"
        raw = urllib.request.urlopen(url, timeout=60).read()
        im = Image.open(BytesIO(raw)).convert("RGBA")
        im = im.resize((SIZE, SIZE), Image.Resampling.LANCZOS)
        path = OUT / f"preset_{i:02}.rgba"
        path.write_bytes(im.tobytes())
        print(path, len(im.tobytes()))


if __name__ == "__main__":
    main()
