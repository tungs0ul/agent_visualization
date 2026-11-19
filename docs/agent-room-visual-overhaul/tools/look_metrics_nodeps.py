#!/usr/bin/env python3
"""Tonal and colour statistics of a screenshot, next to the target's.

    python3 -I tools/look_metrics.py <screenshot.png> [--reference reference/target_reference.png]

No dependencies: macOS `sips` converts each image to a 24-bit BMP, which is read here.
Every 3rd pixel in each direction is sampled. Exit code 0 when every metric is
within tolerance of the reference.

Metrics (from the visual overhaul brief, section 2):
  median luminance, share of pixels with L < 0.15, mean saturation, share of
  saturated glow pixels (S > 0.55 and V > 0.6), average dark colour (L < 0.25),
  95th-percentile luminance.
"""

import argparse
import colorsys
import os
import struct
import subprocess
import sys
import tempfile

STEP = 3


def pixels(path):
    with tempfile.TemporaryDirectory() as tmp:
        bmp = os.path.join(tmp, "image.bmp")
        subprocess.run(["sips", "-s", "format", "bmp", path, "--out", bmp], check=True, capture_output=True)
        data = open(bmp, "rb").read()
    offset, = struct.unpack_from("<I", data, 10)
    width, height = struct.unpack_from("<ii", data, 18)
    bpp, = struct.unpack_from("<H", data, 28)
    if bpp not in (24, 32):
        sys.exit(f"{path}: unsupported BMP depth {bpp}")
    size = bpp // 8
    row = (width * size + 3) & ~3
    out = []
    for y in range(0, abs(height), STEP):
        base = offset + y * row
        for x in range(0, width, STEP):
            b, g, r = data[base + x * size: base + x * size + 3]
            out.append((r / 255.0, g / 255.0, b / 255.0))
    return out


def luminance(rgb):
    def linear(c):
        return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4
    r, g, b = (linear(c) for c in rgb)
    y = 0.2126 * r + 0.7152 * g + 0.0722 * b
    # Perceptual lightness 0..1 (CIE L* / 100).
    return (116 * y ** (1 / 3) - 16) / 100 if y > 0.008856 else 9.033 * y


def metrics(path):
    values = pixels(path)
    lum = sorted(luminance(p) for p in values)
    n = len(lum)
    hsv = [colorsys.rgb_to_hsv(*p) for p in values]
    glow = sum(1 for _, s, v in hsv if s > 0.55 and v > 0.6)
    dark = [p for p in values if luminance(p) < 0.25]
    mean_dark = tuple(sum(p[i] for p in dark) / max(len(dark), 1) for i in range(3))
    return {
        "median luminance": lum[n // 2],
        "share L < 0.15": sum(1 for v in lum if v < 0.15) / n,
        "mean saturation": sum(s for _, s, _ in hsv) / n,
        "saturated glow share": glow / n,
        "p95 luminance": lum[int(n * 0.95)],
        "dark colour": "#%02X%02X%02X" % tuple(int(c * 255) for c in mean_dark),
    }


TOLERANCE = {
    "median luminance": 0.04,
    "share L < 0.15": 0.06,
    "mean saturation": 0.08,
    "saturated glow share": 0.03,
    "p95 luminance": 0.06,
}


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("screenshot")
    here = os.path.dirname(os.path.abspath(__file__))
    parser.add_argument("--reference", default=os.path.join(here, "..", "reference", "target_reference.png"))
    args = parser.parse_args()
    shot, target = metrics(args.screenshot), metrics(args.reference)
    ok = True
    print(f"{'metric':24} {'current':>10} {'target':>10}  verdict")
    for name, value in shot.items():
        goal = target[name]
        if isinstance(value, float):
            within = abs(value - goal) <= TOLERANCE[name]
            ok &= within
            print(f"{name:24} {value:10.3f} {goal:10.3f}  {'ok' if within else ('too high' if value > goal else 'too low')}")
        else:
            print(f"{name:24} {value:>10} {goal:>10}")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
