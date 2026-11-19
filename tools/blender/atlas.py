"""Pack animation frames into one atlas PNG, row by row from the top left.

    Blender -b --factory-startup --python tools/blender/atlas.py -- OUT.png COLS FRAME.png ...
"""
import sys

import bpy
import numpy as np

args = sys.argv[sys.argv.index("--") + 1:]
out, cols, frames = args[0], int(args[1]), args[2:]
imgs = [bpy.data.images.load(f) for f in frames]
w, h = imgs[0].size
rows = (len(imgs) + cols - 1) // cols
atlas = np.zeros((rows * h, cols * w, 4), dtype=np.float32)
for i, im in enumerate(imgs):
    px = np.array(im.pixels[:], dtype=np.float32).reshape(h, w, 4)   # bottom-up rows
    r, c = i // cols, i % cols
    y0 = (rows - 1 - r) * h                                         # top row last in a bottom-up buffer
    atlas[y0:y0 + h, c * w:(c + 1) * w] = px
img = bpy.data.images.new("atlas", cols * w, rows * h, alpha=False)
img.pixels = atlas.ravel().tolist()
img.filepath_raw = out
img.file_format = "PNG"
img.save()
print("ATLAS", out, cols * w, "x", rows * h)
