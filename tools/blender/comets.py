"""Render the comet and the meteor that now and then cross the bridge window.

    /Applications/Blender.app/Contents/MacOS/Blender -b --factory-startup \
        --python tools/blender/comets.py

Writes assets/textures/space/comet.png and meteor.png, 2048x512, premultiplied RGBA
like the planets (`space.rs`): colour is the light they add, alpha how much they hide.
Both point their head to the right (+x) with the tail trailing left.

- comet: a tiny bright nucleus in a soft coma, a straight narrow blue ion tail and a
  broader, slightly curved, pale gold dust tail with fine streaks.
- meteor: a lumpy rock rendered in Cycles, lit from one side, trailing a fiery glow
  (white-yellow at the rock, orange, then red fading out) with a few sparks.
"""
import math
import os

import bpy
import numpy as np

REPO = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
OUT = os.path.join(REPO, "assets", "textures", "space")
W, H = 2048, 512
rng = np.random.default_rng(5)
ys, xs = np.mgrid[0:H, 0:W].astype(np.float32)
u = xs / W            # 0 left .. 1 right
v = (ys - H / 2) / H  # -0.5 .. 0.5, 0 on the axis


def save(name, rgb, alpha):
    px = np.zeros((H, W, 4), np.float32)
    px[..., :3] = np.clip(rgb, 0, 1)
    px[..., 3] = np.clip(alpha, 0, 1)
    img = bpy.data.images.new(name, W, H, alpha=True)
    img.pixels = px[::-1].ravel().tolist()          # Blender rows are bottom-up
    img.filepath_raw = os.path.join(OUT, f"{name}.png")
    img.file_format = "PNG"
    img.save()
    print("COMET: wrote", img.filepath_raw)


def streaks(n, scale):
    """Fine lengthwise striation: noise that varies across the tail, barely along it."""
    rows = rng.random(n)
    k = np.interp(np.clip((v / scale + 0.5), 0, 1) * (n - 1), np.arange(n), rows)
    return 0.6 + 0.4 * k


def comet():
    head_x = 0.86
    d = head_x - u                                  # distance behind the head, 0..
    behind = np.clip(d, 0, None)
    # nucleus and coma
    r = np.sqrt(((u - head_x) * W) ** 2 + (v * H) ** 2)
    nucleus = np.exp(-(r / 3.0) ** 2) * 6.0
    coma = np.exp(-(r / 40.0) ** 2) * 1.6 + np.exp(-(r / 110.0) ** 2) * 0.45
    # ion tail: straight, narrow, blue, long
    w_ion = 0.008 + behind * 0.04
    ion = np.exp(-(v / w_ion) ** 2) * np.exp(-behind / 0.55) * (d > 0) * streaks(90, 0.12) * 1.3
    # dust tail: wider, bending down a little, pale gold, fades sooner
    bend = 0.28 * behind ** 2
    w_dust = 0.02 + behind * 0.17
    dust = np.exp(-((v - bend) / w_dust) ** 2) * np.exp(-behind / 0.38) * (d > 0) * streaks(60, 0.4) * 1.5
    rgb = (nucleus[..., None] * np.array([1.0, 1.0, 1.0]) + coma[..., None] * np.array([0.7, 0.9, 1.0])
           + ion[..., None] * np.array([0.25, 0.6, 1.0]) + dust[..., None] * np.array([1.0, 0.85, 0.6]))
    save("comet", rgb, np.zeros((H, W)))           # pure light: hides nothing


def meteor():
    # 1. the rock, rendered in Cycles on transparent film
    bpy.ops.wm.read_factory_settings(use_empty=True)
    sc = bpy.context.scene
    sc.render.engine = "CYCLES"
    sc.cycles.samples = 64
    sc.render.resolution_x, sc.render.resolution_y = 256, 256
    sc.render.film_transparent = True
    sc.view_settings.view_transform = "Standard"
    bpy.ops.mesh.primitive_ico_sphere_add(subdivisions=5, radius=1.0)
    rock = bpy.context.object
    tex = bpy.data.textures.new("lumps", "VORONOI")
    tex.noise_scale = 0.6
    disp = rock.modifiers.new("d", "DISPLACE")
    disp.texture, disp.strength = tex, 0.35
    tex2 = bpy.data.textures.new("grit", "CLOUDS")
    tex2.noise_scale = 0.25
    d2 = rock.modifiers.new("d2", "DISPLACE")
    d2.texture, d2.strength = tex2, 0.08
    rock.scale = (1.25, 0.9, 0.85)
    m = bpy.data.materials.new("rock")
    m.use_nodes = True
    b = m.node_tree.nodes["Principled BSDF"]
    b.inputs["Base Color"].default_value = (0.09, 0.08, 0.075, 1)
    b.inputs["Roughness"].default_value = 0.95
    rock.data.materials.append(m)
    bpy.ops.object.shade_smooth()
    cam = bpy.data.objects.new("cam", bpy.data.cameras.new("cam"))
    cam.data.type, cam.data.ortho_scale = "ORTHO", 3.0
    cam.location, cam.rotation_euler = (0, -10, 0), (math.radians(90), 0, 0)
    sc.collection.objects.link(cam)
    sc.camera = cam
    for loc, energy, col in (((3, -2, 2), 5.0, (1.0, 0.95, 0.9)), ((-3, -1, 0), 2.5, (1.0, 0.45, 0.1))):
        L = bpy.data.objects.new("sun", bpy.data.lights.new("sun", "SUN"))
        L.data.energy, L.data.color = energy, col
        L.rotation_euler = (np.array(loc) * -1.0).tolist() and __import__("mathutils").Vector(loc).to_track_quat("Z", "Y").to_euler()
        sc.collection.objects.link(L)
    tmp = os.path.join(OUT, ".rock.png")
    sc.render.filepath = tmp
    bpy.ops.render.render(write_still=True)
    im = bpy.data.images.load(tmp)
    rk = np.array(im.pixels[:], np.float32).reshape(256, 256, 4)[::-1]
    os.remove(tmp)
    # 2. the trail, painted, and the rock placed at its head (rock is premultiplied
    #    by its own alpha over the glow)
    head_x = 0.88
    d = head_x - u
    behind = np.clip(d, 0, None)
    r = np.sqrt(((u - head_x) * W) ** 2 + (v * H) ** 2)
    width = 0.02 + behind * 0.07
    core = np.exp(-(v / (width * 0.45)) ** 2) * np.exp(-behind / 0.12) * (d > -0.005)
    glow = np.exp(-(v / width) ** 2) * np.exp(-behind / 0.32) * (d > -0.005)
    halo = np.exp(-(r / 60.0) ** 2) * 1.2
    t = np.clip(behind / 0.5, 0, 1)[..., None]
    hot = np.array([1.0, 0.95, 0.7]) * (1 - t) + np.array([1.0, 0.35, 0.05]) * t
    rgb = core[..., None] * hot * 4.0 + glow[..., None] * (np.array([1.0, 0.4, 0.08]) * (1 - t) + np.array([0.6, 0.08, 0.04]) * t) * 2.2
    rgb += halo[..., None] * np.array([1.0, 0.6, 0.25])
    for _ in range(40):                                  # sparks shed along the trail
        sx = head_x - rng.random() ** 1.5 * 0.4
        sy = rng.normal(0, 0.03 + (head_x - sx) * 0.08)
        rr = np.sqrt(((u - sx) * W) ** 2 + ((v - sy) * H) ** 2)
        rgb += np.exp(-(rr / rng.uniform(1.5, 3.0)) ** 2)[..., None] * np.array([1.0, 0.7, 0.3]) * rng.uniform(0.6, 1.5)
    alpha = np.zeros((H, W))
    size = 190
    small = np.array(bpy.data.images.load if False else rk)
    # nearest resample of the 256 rock render to `size` px
    idx = (np.arange(size) * 256 / size).astype(int)
    rk_s = rk[idx][:, idx]
    cx, cy = int(head_x * W), H // 2
    y0, x0 = cy - size // 2, cx - size // 2
    a = rk_s[..., 3:4]
    region = rgb[y0:y0 + size, x0:x0 + size]
    rgb[y0:y0 + size, x0:x0 + size] = rk_s[..., :3] + region * (1 - a)
    alpha[y0:y0 + size, x0:x0 + size] = a[..., 0]
    save("meteor", rgb, alpha)


os.makedirs(OUT, exist_ok=True)
comet()
meteor()
