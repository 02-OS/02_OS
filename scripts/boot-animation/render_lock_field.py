#!/usr/bin/env python3
"""Render the greeter field from the desktop wallpaper.

The picture stays recognizable under a dark veil, with a soft oval where the
0 rests. That oval's center matches drawPortalFrame on a 16:9 frame. Pillow
and numpy are needed only to regenerate this asset, not to build an ISO.
"""
from pathlib import Path

import numpy as np
from PIL import Image

ROOT = Path(__file__).resolve().parents[2]
WALLPAPER = ROOT / "profile/airootfs/usr/share/backgrounds/02os/desktop.jpg"
OUTPUT = ROOT / "profile/airootfs/usr/share/backgrounds/02os/portal-halo.png"
WIDTH = 1920
HEIGHT = 1080
# How much of the dark field covers the picture. The wallpaper stays visible.
VEIL = 0.58
# drawPortalFrame fit on this frame: canvas width is 40% of the width.
SCALE = (WIDTH * 0.4) * 0.59375 / 680
ZERO_X = -170 * SCALE + WIDTH / 2
ZERO_Y = HEIGHT / 2 + 0.5 * SCALE


def cover(path):
    image = Image.open(path).convert("RGB")
    scale = max(WIDTH / image.width, HEIGHT / image.height)
    resized = image.resize(
        (round(image.width * scale), round(image.height * scale)),
        Image.Resampling.LANCZOS,
    )
    left = (resized.width - WIDTH) // 2
    top = (resized.height - HEIGHT) // 2
    return resized.crop((left, top, left + WIDTH, top + HEIGHT))


def main():
    picture = np.asarray(cover(WALLPAPER)).astype(np.float32) / 255
    y, x = np.mgrid[0:HEIGHT, 0:WIDTH]
    # The logo 0 is 264 units wide and 533 tall. The glow is that capsule,
    # enlarged, so the light sits on the same shape that just expanded.
    half_width = 132 * SCALE * 2.35
    half_height = 266.5 * SCALE * 2.05
    half_segment = max(half_height - half_width, 0)
    along = np.clip(y - ZERO_Y, -half_segment, half_segment)
    radial = np.hypot(x - ZERO_X, y - ZERO_Y - along)
    oval = np.exp(-(radial ** 2) / (2 * (half_width * 0.92) ** 2))
    length = np.exp(-((np.abs(y - ZERO_Y) / (half_height * 1.05)) ** 2) * 0.35)
    glow = oval * (0.72 + 0.28 * length)

    # A wider, dimmer pool keeps the centered login card from sitting in a void.
    card = np.exp(-(
        ((x - WIDTH / 2) / (WIDTH * 0.36)) ** 2
        + ((y - HEIGHT / 2) / (HEIGHT * 0.40)) ** 2
    ))
    vx = (x - WIDTH / 2) / (WIDTH / 2)
    vy = (y - HEIGHT / 2) / (HEIGHT / 2)
    vignette = np.clip(1 - 0.55 * (vx * vx * 0.85 + vy * vy) ** 1.2, 0, 1)

    veil_color = np.array([9, 16, 24], dtype=np.float32) / 255
    cyan = np.array([0, 186, 243], dtype=np.float32) / 255
    # Edges fall a little darker so the picture reads in the middle.
    darkness = VEIL + (1 - VEIL) * 0.28 * (1 - vignette)
    rgb = picture * (1 - darkness[..., None]) + veil_color * darkness[..., None]
    light = np.clip(glow * 0.10 + card * 0.03, 0, 1)
    rgb = np.clip(rgb + cyan * light[..., None], 0, 1)

    image = Image.fromarray(np.rint(np.clip(rgb, 0, 1) * 255).astype(np.uint8), "RGB")
    OUTPUT.parent.mkdir(parents=True, exist_ok=True)
    image.save(OUTPUT, format="PNG", optimize=True)
    print(f"wrote {OUTPUT} {image.size[0]}x{image.size[1]}")


if __name__ == "__main__":
    main()
