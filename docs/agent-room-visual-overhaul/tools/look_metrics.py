#!/usr/bin/env python3
"""
look_metrics.py - objective "does it look like the target yet?" check.

Measures tonal / colour statistics of a screenshot and compares them with the
target concept art (reference/target_reference.png). It cannot judge modelling
quality, but it reliably catches the main reasons the current build looks
"cheap": washed-out mid-greys, too little true darkness, low saturation and too
little saturated glow.

Usage
  python3 tools/look_metrics.py screenshots/hero.png
  python3 tools/look_metrics.py screenshots/hero.png --side-by-side out/compare.png
  python3 tools/look_metrics.py shot.png --reference reference/target_reference.png
  python3 tools/look_metrics.py shot.png --crop 0,0,1920,1080   # x0,y0,x1,y1

Requires: pillow, numpy   (pip install pillow numpy)
"""
import argparse
import sys

import numpy as np
from PIL import Image

# Measured from reference/target_reference.png (full frame, HUD included).
TARGET = {
    "luma_p25": 0.106,
    "luma_median": 0.198,
    "luma_p75": 0.336,
    "luma_p95": 0.773,
    "dark_fraction(L<0.15)": 0.358,
    "mean_saturation": 0.741,
    "glow_fraction(S>0.45,V>0.75)": 0.103,
}
# Allowed relative deviation before a metric is flagged.
TOLERANCE = {
    "luma_p25": 0.30,
    "luma_median": 0.25,
    "luma_p75": 0.25,
    "luma_p95": 0.15,
    "dark_fraction(L<0.15)": 0.30,
    "mean_saturation": 0.15,
    "glow_fraction(S>0.45,V>0.75)": 0.40,
}
# Each entry: (hint when the build is TOO HIGH, hint when it is TOO LOW)
HINTS = {
    "luma_median": ("Mid-tones too bright: lower AmbientLight / environment-light intensity, "
                    "darken base_color of floor+walls, lower exposure (raise EV100).",
                    "Mid-tones too dark: raise exposure slightly or add fill light."),
    "luma_p25": ("Shadows too bright / washed out: kill ambient, darken floor & wall albedo, enable SSAO.",
                 "Shadows crushed: add a little ambient or environment light."),
    "dark_fraction(L<0.15)": ("Too much black: raise fill light or env-map intensity.",
                              "Not enough true darkness: floor/walls/ceiling must be deep navy, not slate grey."),
    "mean_saturation": ("Over-saturated: reduce ColorGrading saturation.",
                        "Image is greyish: use saturated navy (#0A1D3D) instead of grey-blue, "
                        "saturate emissive colours, ColorGrading post_saturation 1.1-1.2."),
    "glow_fraction(S>0.45,V>0.75)": ("Too much glow: lower emissive tiers or bloom intensity.",
                                     "Too little saturated glow: emissive colours are too white or too weak, "
                                     "bloom too low, screens not emissive enough."),
    "luma_p95": ("Highlights too bright / clipped: lower emissive tiers or exposure.",
                 "No real highlights: emissive strips need a white-hot core (raise intensity)."),
    "luma_p75": ("Upper mid-tones too bright: surfaces too light.", "Upper mid-tones too dark."),
}

HUE_BINS = [(0, 25, "red/orange"), (25, 50, "amber"), (50, 75, "yellow"), (75, 160, "green"),
            (160, 185, "cyan"), (185, 215, "azure"), (215, 250, "blue"), (250, 290, "violet"),
            (290, 340, "magenta"), (340, 361, "red")]


def load(path, crop=None):
    im = Image.open(path).convert("RGB")
    if crop:
        im = im.crop(crop)
    return im


def hsv(a):
    mx = a.max(-1)
    mn = a.min(-1)
    delta = mx - mn
    s = np.where(mx > 0, delta / np.maximum(mx, 1e-6), 0.0)
    r, g, b = a[..., 0], a[..., 1], a[..., 2]
    h = np.zeros_like(mx)
    nz = delta > 1e-6
    rc = (mx == r) & nz
    gc = (mx == g) & nz & ~rc
    bc = nz & ~rc & ~gc
    h[rc] = ((g[rc] - b[rc]) / delta[rc]) % 6
    h[gc] = ((b[gc] - r[gc]) / delta[gc]) + 2
    h[bc] = ((r[bc] - g[bc]) / delta[bc]) + 4
    return h * 60.0, s, mx


def measure(im):
    a = np.asarray(im).astype(np.float32) / 255.0
    L = 0.2126 * a[..., 0] + 0.7152 * a[..., 1] + 0.0722 * a[..., 2]
    h, s, v = hsv(a)
    p = np.percentile(L, [25, 50, 75, 95])
    glow = (s > 0.45) & (v > 0.75)
    m = {
        "luma_p25": float(p[0]),
        "luma_median": float(p[1]),
        "luma_p75": float(p[2]),
        "luma_p95": float(p[3]),
        "dark_fraction(L<0.15)": float((L < 0.15).mean()),
        "mean_saturation": float(s.mean()),
        "glow_fraction(S>0.45,V>0.75)": float(glow.mean()),
    }
    hues = {}
    if glow.any():
        gh = h[glow]
        for lo, hi, name in HUE_BINS:
            frac = float(((gh >= lo) & (gh < hi)).mean())
            if frac > 0.002:
                hues[name] = frac
    dark = v < 0.35
    dark_rgb = (a[dark].mean(0) * 255).astype(int) if dark.any() else np.array([0, 0, 0])
    return m, hues, "#%02x%02x%02x" % tuple(dark_rgb)


def side_by_side(shot, ref, out):
    h = 900
    s = shot.resize((int(shot.width * h / shot.height), h), Image.LANCZOS)
    r = ref.resize((int(ref.width * h / ref.height), h), Image.LANCZOS)
    canvas = Image.new("RGB", (s.width + r.width + 20, h), (0, 0, 0))
    canvas.paste(s, (0, 0))
    canvas.paste(r, (s.width + 20, 0))
    canvas.save(out)
    print(f"side-by-side written to {out}  (left = build, right = target)")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("screenshot")
    ap.add_argument("--reference", help="compute targets from this image instead of the built-in numbers")
    ap.add_argument("--crop", help="x0,y0,x1,y1 crop applied to the screenshot")
    ap.add_argument("--side-by-side", help="write a comparison PNG to this path (needs --reference or the default path)")
    args = ap.parse_args()

    crop = tuple(int(v) for v in args.crop.split(",")) if args.crop else None
    shot = load(args.screenshot, crop)
    m, hues, dark = measure(shot)

    target = dict(TARGET)
    ref_img = None
    if args.reference:
        ref_img = load(args.reference)
        target, _, _ = measure(ref_img)

    print(f"\n{'metric':34s} {'build':>8s} {'target':>8s}   status")
    print("-" * 70)
    fails = 0
    for k, t in target.items():
        v = m[k]
        dev = (v - t) / t if t else 0.0
        ok = abs(dev) <= TOLERANCE[k]
        fails += 0 if ok else 1
        status = "OK" if ok else ("TOO HIGH" if dev > 0 else "TOO LOW")
        print(f"{k:34s} {v:8.3f} {t:8.3f}   {status} ({dev:+.0%})")
    print("-" * 70)
    print("glow hue mix (target: ~73% azure, ~23% blue, <=4% accents):")
    for name, frac in sorted(hues.items(), key=lambda x: -x[1]):
        print(f"   {name:12s} {frac:6.1%}")
    print(f"average dark colour: {dark}   (target ~#0a1d3d saturated navy, not grey slate)")

    if fails:
        print("\nWhat to change:")
        for k, t in target.items():
            v = m[k]
            dev = (v - t) / t if t else 0.0
            if abs(dev) > TOLERANCE[k] and k in HINTS:
                print(f" - {k}: {HINTS[k][0] if dev > 0 else HINTS[k][1]}")
    print(f"\n{len(target) - fails}/{len(target)} metrics within tolerance.")

    if args.side_by_side:
        if ref_img is None:
            try:
                ref_img = load("reference/target_reference.png")
            except FileNotFoundError:
                print("pass --reference to build the side-by-side image")
                return 1
        side_by_side(shot, ref_img, args.side_by_side)
    return 0 if fails == 0 else 2


if __name__ == "__main__":
    sys.exit(main())
