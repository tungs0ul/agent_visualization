"""Build the Agent Room sci-fi workstations and props as GLB files.

Run headless from the repo root (Blender 5.2):

    /Applications/Blender.app/Contents/MacOS/Blender -b --factory-startup \
        --python tools/blender/stations.py [-- name ...]

With no names it writes every model into assets/models/stations/:
main_desk, bookshelf, lab, writing_desk, plant, wall_panel,
pillar, document. Pass names after `--` to rebuild only some of them.

Conventions (the app relies on them): metres; modelled Z-up facing Blender -Y,
so in glTF (Y up) each model stands on y=0 with its front (where the robot
works, facing the camera) toward +Z. Origin = floor centre of the station.
Every file is one mesh with shared material names (Panel, PanelDark, Trim,
GlowCyan, GlowOrange, GlowPurple, Holo, Glass, Leaf, Pot, plus Paper,
GlowGreen and Book* where used) so the app can find and tweak them.
"""

import math
import os
import random
import sys
from contextlib import contextmanager

import bmesh
import bpy
from mathutils import Euler, Matrix, Vector

REPO = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
OUT_DIR = os.path.join(REPO, "assets", "models", "stations")
ARGS = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
rad = math.radians


# ---------------------------------------------------------------- materials
def make_material(name, color, rough, metal=0.0, emit=None, strength=0.0, alpha=1.0):
    mat = bpy.data.materials.get(name) or bpy.data.materials.new(name)
    mat.use_nodes = True
    bsdf = mat.node_tree.nodes.get("Principled BSDF")
    bsdf.inputs["Base Color"].default_value = (*color, 1.0)
    bsdf.inputs["Roughness"].default_value = rough
    bsdf.inputs["Metallic"].default_value = metal
    if emit is not None:
        bsdf.inputs["Emission Color"].default_value = (*emit, 1.0)
        bsdf.inputs["Emission Strength"].default_value = strength
    if alpha < 1.0:
        bsdf.inputs["Alpha"].default_value = alpha
        mat.surface_render_method = "BLENDED"
        mat.use_backface_culling = False
    return mat


CYAN = (0.05, 0.72, 1.0)
ORANGE = (1.0, 0.38, 0.06)
PURPLE = (0.55, 0.22, 1.0)
GREEN = (0.15, 1.0, 0.35)
MAT_SPECS = {
    "Panel": dict(color=(0.85, 0.86, 0.88), rough=0.32),
    "PanelDark": dict(color=(0.025, 0.035, 0.06), rough=0.12, metal=0.2),
    "Trim": dict(color=(0.45, 0.48, 0.53), rough=0.35, metal=0.8),
    "GlowCyan": dict(color=CYAN, rough=0.3, emit=CYAN, strength=6.0),
    "GlowOrange": dict(color=ORANGE, rough=0.3, emit=ORANGE, strength=6.0),
    "GlowPurple": dict(color=PURPLE, rough=0.3, emit=PURPLE, strength=6.0),
    "GlowGreen": dict(color=GREEN, rough=0.3, emit=GREEN, strength=6.0),
    "Holo": dict(color=(0.1, 0.75, 1.0), rough=0.2, emit=(0.1, 0.7, 1.0), strength=2.0, alpha=0.35),
    "HoloDark": dict(color=(0.01, 0.05, 0.08), rough=0.2, emit=(0.02, 0.25, 0.35), strength=1.0, alpha=0.6),
    "Glass": dict(color=(0.7, 0.92, 1.0), rough=0.05, alpha=0.2),
    "Paper": dict(color=(0.93, 0.96, 1.0), rough=0.6, emit=(0.85, 0.93, 1.0), strength=1.2),
    "Leaf": dict(color=(0.04, 0.32, 0.13), rough=0.45),
    "Pot": dict(color=(0.9, 0.91, 0.93), rough=0.25),
    "BookBlue": dict(color=(0.12, 0.25, 0.65), rough=0.55),
    "BookTeal": dict(color=(0.05, 0.5, 0.55), rough=0.55),
    "BookRed": dict(color=(0.7, 0.16, 0.2), rough=0.55),
    "BookAmber": dict(color=(0.9, 0.55, 0.12), rough=0.55),
    "BookCream": dict(color=(0.86, 0.82, 0.72), rough=0.6),
}
BOOKS = ["BookBlue", "BookTeal", "BookRed", "BookAmber", "BookCream", "PanelDark", "Panel"]


def mat(name):
    return bpy.data.materials.get(name) or make_material(name, **MAT_SPECS[name])


# ---------------------------------------------------------------- scene / grouping
PARTS = []
STACK = []


def reset_scene():
    for ob in list(bpy.data.objects):
        bpy.data.objects.remove(ob, do_unlink=True)
    for m in list(bpy.data.meshes):
        bpy.data.meshes.remove(m)
    PARTS.clear()
    STACK.clear()


@contextmanager
def at(loc=(0, 0, 0), rot=(0, 0, 0)):
    """Local frame for the parts made inside the block (rot in degrees, XYZ)."""
    e = bpy.data.objects.new("grp", None)
    bpy.context.scene.collection.objects.link(e)
    e.location = loc
    e.rotation_euler = Euler([rad(a) for a in rot])
    if STACK:
        e.parent = STACK[-1]
    STACK.append(e)
    try:
        yield e
    finally:
        STACK.pop()


def part(bm, material, loc=(0, 0, 0), rot=(0, 0, 0), bevel=0.0, segs=2, angle=35):
    me = bpy.data.meshes.new("part")
    bm.to_mesh(me)
    bm.free()
    ob = bpy.data.objects.new("part", me)
    bpy.context.scene.collection.objects.link(ob)
    ob.location = loc
    ob.rotation_euler = Euler([rad(a) for a in rot])
    if STACK:
        ob.parent = STACK[-1]
    ob["mat"] = material
    if bevel > 0:
        m = ob.modifiers.new("bevel", "BEVEL")
        m.width = bevel
        m.segments = segs
        m.limit_method = "ANGLE"
        m.angle_limit = rad(angle)
    PARTS.append(ob)
    return ob


# ---------------------------------------------------------------- primitives
def box(center, size, material, bevel=0.0, segs=2, rot=(0, 0, 0)):
    bm = bmesh.new()
    bmesh.ops.create_cube(bm, size=1.0)
    bmesh.ops.scale(bm, vec=size, verts=bm.verts)
    return part(bm, material, center, rot, bevel, segs)


def revolve(profile, segs=48, a0=0.0, a1=360.0, phase=0.0, closed=True):
    """Revolve a (r, z) profile around Z. Angle 0 = +Y (back), 90 = +X, 180 = -Y (front)."""
    full = abs(a1 - a0) >= 359.999
    n = segs if full else segs + 1
    angles = [rad(phase + a0 + (a1 - a0) * i / segs) for i in range(n)]
    bm = bmesh.new()
    rings = []
    for r, z in profile:
        if r < 1e-6:
            v = bm.verts.new((0, 0, z))
            rings.append([v] * n)
        else:
            rings.append([bm.verts.new((r * math.sin(a), r * math.cos(a), z)) for a in angles])

    def face(vs):
        out = []
        for v in vs:
            if v not in out:
                out.append(v)
        if len(out) >= 3:
            try:
                bm.faces.new(out)
            except ValueError:
                pass

    P = len(profile)
    for j in range(P if closed else P - 1):
        A, B = rings[j], rings[(j + 1) % P]
        for i in range(segs):
            i2 = (i + 1) % n
            face([A[i], A[i2], B[i2], B[i]])
    if not full and closed:
        face([rings[j][0] for j in range(P)])
        face([rings[j][n - 1] for j in reversed(range(P))])
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    return bm


def disc(center, r, z0, z1, material, segs=48, phase=0.0, bevel=0.0, bsegs=2, r_top=None, rot=(0, 0, 0)):
    rt = r if r_top is None else r_top
    bm = revolve([(0, z0), (r, z0), (rt, z1), (0, z1)], segs, phase=phase)
    return part(bm, material, center, rot, bevel=bevel, segs=bsegs)


def ring(center, r_in, r_out, z0, z1, material, segs=48, a0=0.0, a1=360.0, phase=0.0,
         bevel=0.0, bsegs=2, rot=(0, 0, 0)):
    bm = revolve([(r_in, z0), (r_out, z0), (r_out, z1), (r_in, z1)], segs, a0, a1, phase)
    return part(bm, material, center, rot, bevel, bsegs)


def lathe(center, profile, material, segs=40, bevel=0.0, rot=(0, 0, 0)):
    """Open (r, z) profile from axis to axis, e.g. a pot or a tank."""
    return part(revolve(profile, segs, closed=False), material, center, rot, bevel)


def sphere(center, radii, material, seg=24, rings_=12, rot=(0, 0, 0)):
    bm = bmesh.new()
    bmesh.ops.create_uvsphere(bm, u_segments=seg, v_segments=rings_, radius=1.0)
    bmesh.ops.scale(bm, vec=radii, verts=bm.verts)
    return part(bm, material, center, rot)


def tube_path(points, radius, material, sides=12, samples=8):
    """Smooth pipe through points (Catmull-Rom), parallel-transported frames."""
    pts = [Vector(p) for p in points]
    path = []
    ext = [pts[0] * 2 - pts[1]] + pts + [pts[-1] * 2 - pts[-2]]
    for i in range(1, len(ext) - 2):
        p0, p1, p2, p3 = ext[i - 1], ext[i], ext[i + 1], ext[i + 2]
        for s in range(samples):
            t = s / samples
            path.append(0.5 * ((2 * p1) + (-p0 + p2) * t + (2 * p0 - 5 * p1 + 4 * p2 - p3) * t * t
                               + (-p0 + 3 * p1 - 3 * p2 + p3) * t ** 3))
    path.append(pts[-1])
    bm = bmesh.new()
    tang = (path[1] - path[0]).normalized()
    up = Vector((0, 0, 1)) if abs(tang.z) < 0.9 else Vector((1, 0, 0))
    nrm = tang.cross(up).normalized()
    loops = []
    for k, p in enumerate(path):
        if 0 < k < len(path) - 1:
            t2 = (path[k + 1] - path[k - 1]).normalized()
        else:
            t2 = (path[min(k + 1, len(path) - 1)] - path[max(k - 1, 0)]).normalized()
        nrm = (nrm - t2 * nrm.dot(t2)).normalized()
        bin_ = t2.cross(nrm)
        loops.append([bm.verts.new(p + radius * (math.cos(2 * math.pi * s / sides) * nrm
                                                 + math.sin(2 * math.pi * s / sides) * bin_))
                      for s in range(sides)])
    for a, b in zip(loops, loops[1:]):
        for s in range(sides):
            bm.faces.new([a[s], a[(s + 1) % sides], b[(s + 1) % sides], b[s]])
    bm.faces.new(list(reversed(loops[0])))
    bm.faces.new(loops[-1])
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    return part(bm, material)


def leaf(length, width, elev, droop, material, rot_z, base=(0, 0, 0), segs=7):
    """A V-folded blade growing along local +X, then rotated about Z."""
    bm = bmesh.new()
    rows = []
    for i in range(segs + 1):
        t = i / segs
        x = length * t * math.cos(rad(elev))
        z = length * t * math.sin(rad(elev)) - droop * t * t
        w = width * (math.sin(math.pi * min(1.0, t * 1.05)) ** 0.7) * 0.5
        if i == segs:
            w = 0.0
        rows.append([bm.verts.new((x, -w, z + w * 0.25)), bm.verts.new((x, 0, z + w * 0.05 - 0.002)),
                     bm.verts.new((x, w, z + w * 0.25))])
    for a, b in zip(rows, rows[1:]):
        for j in range(2):
            vs = [a[j], a[j + 1], b[j + 1], b[j]]
            uniq = []
            for v in vs:
                if v not in uniq:
                    uniq.append(v)
            bm.faces.new(uniq)
    return part(bm, material, base, (0, 0, rot_z))


# ---------------------------------------------------------------- reusable pieces
def seg2(p0, p1, th, material, y=-0.008, depth=0.004):
    """A thin glowing bar between two points of the local XZ plane (a line on a holo)."""
    dx, dz = p1[0] - p0[0], p1[1] - p0[1]
    L = math.hypot(dx, dz)
    box(((p0[0] + p1[0]) / 2, y, (p0[1] + p1[1]) / 2), (L + th, depth, th), material,
        rot=(0, -math.degrees(math.atan2(dz, dx)), 0))


def outline(x0, z0, x1, z1, th, material, y=-0.008):
    for p, q in (((x0, z0), (x1, z0)), ((x1, z0), (x1, z1)), ((x1, z1), (x0, z1)), ((x0, z1), (x0, z0))):
        seg2(p, q, th, material, y)


def text_lines(x0, z0, width, n, step, th, material, rng, y=-0.008, indents=None, min_frac=0.45):
    for i in range(n):
        ind = (indents[i % len(indents)] if indents else 0) * width * 0.08
        lw = (width - ind) * rng.uniform(min_frac, 1.0)
        box((x0 + ind + lw / 2, y, z0 - i * step), (lw, 0.004, th), material)


def holo_icon(center, kind, s=0.14, rot=(0, 0, 0), m="GlowCyan"):
    """Small holographic icon as thin emissive outlines in the local XZ plane, facing -Y."""
    th = s * 0.06
    with at(center, rot):
        if kind in ("file", "doc"):
            w, h, c = s * 0.7, s, s * 0.25
            for p, q in (((-w / 2, -h / 2), (w / 2, -h / 2)), ((w / 2, -h / 2), (w / 2, h / 2 - c)),
                         ((w / 2, h / 2 - c), (w / 2 - c, h / 2)), ((w / 2 - c, h / 2), (-w / 2, h / 2)),
                         ((-w / 2, h / 2), (-w / 2, -h / 2))):
                seg2(p, q, th, m, 0)
            if kind == "doc":
                for i in range(3):
                    seg2((-w * 0.28, h * 0.12 - i * h * 0.2), (w * 0.28, h * 0.12 - i * h * 0.2), th * 0.8,
                         "GlowOrange", 0)
        elif kind == "code":
            seg2((-s * 0.18, s * 0.25), (-s * 0.45, 0), th, m, 0)
            seg2((-s * 0.45, 0), (-s * 0.18, -s * 0.25), th, m, 0)
            seg2((s * 0.18, s * 0.25), (s * 0.45, 0), th, m, 0)
            seg2((s * 0.45, 0), (s * 0.18, -s * 0.25), th, m, 0)
            seg2((s * 0.08, s * 0.32), (-s * 0.08, -s * 0.32), th, "GlowOrange", 0)
        elif kind == "tool":
            ring((0, 0, 0), s * 0.22, s * 0.3, -th / 2, th / 2, m, segs=20, rot=(90, 0, 0))
            for k in range(6):
                a = rad(k * 60)
                seg2((math.cos(a) * s * 0.3, math.sin(a) * s * 0.3), (math.cos(a) * s * 0.42, math.sin(a) * s * 0.42),
                     th * 1.6, m, 0)
            disc((0, 0, 0), s * 0.08, -th / 2, th / 2, "GlowOrange", segs=12, rot=(90, 0, 0))
        elif kind == "search":
            ring((-s * 0.08, 0, s * 0.08), s * 0.2, s * 0.27, -th / 2, th / 2, m, segs=20, rot=(90, 0, 0))
            seg2((s * 0.1, -s * 0.1), (s * 0.36, -s * 0.36), th * 1.8, m, 0)
        elif kind == "book":
            for sx in (-1, 1):
                with at((0, 0, 0), (0, 0, sx * 14)):
                    outline(0, -s * 0.4, sx * s * 0.45, s * 0.4, th, m, 0)
                    for i in range(3):
                        seg2((sx * s * 0.1, s * 0.2 - i * s * 0.18), (sx * s * 0.35, s * 0.2 - i * s * 0.18), th * 0.8,
                             "GlowOrange", 0)
        elif kind == "chip":
            outline(-s * 0.3, -s * 0.3, s * 0.3, s * 0.3, th, m, 0)
            box((0, 0, 0), (s * 0.3, th, s * 0.3), "GlowOrange")
            for k in (-1, 0, 1):
                for sx in (-1, 1):
                    seg2((sx * s * 0.3, k * s * 0.15), (sx * s * 0.42, k * s * 0.15), th, m, 0)
                    seg2((k * s * 0.15, sx * s * 0.3), (k * s * 0.15, sx * s * 0.42), th, m, 0)


def holo_content(kind, w, h, rng, f=-0.008):
    """Draw the 'content' geometry of a holo panel spanning x in +-w/2, z in +-h/2."""
    if kind == "bars":
        n = 7
        bw = (w * 0.75) / n
        for i in range(n):
            bh = (0.25 + 0.6 * rng.random()) * h * 0.7
            x = -w * 0.375 + bw * (i + 0.5)
            box((x, f, -h * 0.38 + bh / 2), (bw * 0.6, 0.004, bh), "GlowOrange" if i == 4 else "GlowCyan")
        box((0, f, -h * 0.4), (w * 0.8, 0.004, 0.006), "GlowCyan")
    elif kind == "lines":
        box((-w * 0.25, f, h * 0.36), (w * 0.4, 0.004, 0.02), "GlowOrange")
        text_lines(-w * 0.4, h * 0.22, w * 0.8, 6, h * 0.11, 0.01, "GlowCyan", rng)
    elif kind == "graph":
        prev = None
        for i in range(9):
            x = -w * 0.4 + i * w * 0.1
            z = -h * 0.3 + h * 0.55 * (0.5 + 0.45 * math.sin(i * 0.9 + rng.random() * 2))
            if prev:
                seg2(prev, (x, z), 0.01, "GlowCyan", f)
            prev = (x, z)
        box((0, f, -h * 0.38), (w * 0.8, 0.004, 0.006), "GlowCyan")
        box((-w * 0.4, f, 0), (0.006, 0.004, h * 0.76), "GlowCyan")
        box((w * 0.25, f, h * 0.38), (w * 0.25, 0.004, 0.016), "GlowOrange")
    elif kind == "ring":
        ring((-w * 0.18, f, 0), h * 0.22, h * 0.28, -0.002, 0.002, "GlowCyan", segs=40, rot=(90, 0, 0))
        ring((-w * 0.18, f, 0), h * 0.30, h * 0.32, -0.002, 0.002, "GlowOrange", segs=40, a0=20, a1=250,
             rot=(90, 0, 0))
        text_lines(w * 0.12, h * 0.18, w * 0.28, 4, h * 0.12, 0.012, "GlowCyan", rng)
    elif kind == "page":
        box((0, -0.004, 0), (w * 0.86, 0.004, h * 0.9), "Paper")
        box((-w * 0.15, -0.008, h * 0.36), (w * 0.5, 0.004, 0.025), "GlowCyan")
        text_lines(-w * 0.36, h * 0.25, w * 0.72, 9, h * 0.07, 0.009, "PanelDark", rng, y=-0.008)
    elif kind == "editor":
        box((0, f, h / 2 - h * 0.07), (w * 0.94, 0.004, 0.006), "GlowCyan")
        for i, m in enumerate(("GlowOrange", "GlowGreen", "GlowPurple")):
            disc((-w / 2 + 0.04 + i * 0.03, f, h / 2 - h * 0.035), 0.009, -0.002, 0.002, m, segs=10, rot=(90, 0, 0))
        box((-w / 2 + 0.04 + 0.15, f, h / 2 - h * 0.035), (0.12, 0.004, 0.012), "GlowCyan")
        indents = [0, 1, 2, 2, 1, 2, 3, 2, 0, 1]
        n = 9
        step = (h * 0.78) / n
        for i in range(n):
            z = h / 2 - h * 0.16 - i * step
            box((-w / 2 + 0.035, f, z), (0.025, 0.004, 0.008), "Trim")
            x = -w / 2 + 0.08 + indents[i] * 0.035
            for k in range(rng.randint(1, 3)):
                lw = rng.uniform(0.04, 0.14)
                if x + lw > w / 2 - 0.04:
                    break
                m = ["GlowPurple", "GlowCyan", "GlowOrange", "GlowCyan"][(i + k) % 4]
                box((x + lw / 2, f, z), (lw, 0.004, 0.012), m)
                x += lw + 0.018
            if i == 5:
                box((x + 0.01, f - 0.002, z), (0.008, 0.004, step * 0.8), "Paper")  # cursor
    elif kind == "terminal":
        box((0, f, h / 2 - h * 0.06), (w * 0.94, 0.004, 0.006), "GlowGreen")
        n = 8
        step = (h * 0.75) / n
        for i in range(n):
            z = h / 2 - h * 0.16 - i * step
            m = "GlowGreen" if i % 3 != 2 else "GlowCyan"
            if i == n - 1:
                seg2((-w / 2 + 0.04, z + 0.012), (-w / 2 + 0.058, z), 0.007, "GlowGreen", f)
                seg2((-w / 2 + 0.058, z), (-w / 2 + 0.04, z - 0.012), 0.007, "GlowGreen", f)
                box((-w / 2 + 0.11, f, z), (0.06, 0.004, 0.01), "GlowGreen")
                box((-w / 2 + 0.165, f - 0.002, z), (0.02, 0.004, step * 0.7), "Paper")
            else:
                lw = rng.uniform(0.25, 0.85) * (w - 0.1)
                box((-w / 2 + 0.05 + lw / 2, f, z), (lw, 0.004, 0.01), m)
    elif kind == "progress":
        box((-w * 0.2, f, h * 0.36), (w * 0.45, 0.004, 0.022), "GlowCyan")
        text_lines(-w * 0.42, h * 0.2, w * 0.84, 4, h * 0.1, 0.01, "GlowCyan", rng)
        outline(-w * 0.42, -h * 0.32, w * 0.42, -h * 0.22, 0.006, "GlowCyan", f)
        box((-w * 0.42 + w * 0.84 * 0.36, f, -h * 0.27), (w * 0.84 * 0.7, 0.004, h * 0.07), "GlowOrange")
        box((w * 0.32, f, -h * 0.39), (w * 0.12, 0.004, 0.014), "GlowOrange")
    elif kind == "files":
        for r_ in range(2):
            for c in range(3):
                x = -w * 0.3 + c * w * 0.3
                z = h * 0.14 - r_ * h * 0.42
                holo_icon((x, f, z), "doc" if (r_ + c) % 2 else "file", min(w, h) * 0.28)
                box((x, f, z - h * 0.19), (w * 0.18, 0.004, 0.008), "GlowCyan")
    elif kind == "book":
        for sx in (-1, 1):
            with at((0, 0, 0), (0, 0, sx * 12)):
                box((sx * w * 0.235, -0.004, 0), (w * 0.44, 0.004, h * 0.86), "Paper")
                text_lines(sx * w * 0.235 - w * 0.18, h * 0.3, w * 0.36, 8, h * 0.08, 0.008, "PanelDark", rng,
                           y=-0.009)
        box((0, f, 0), (0.008, 0.004, h * 0.9), "GlowCyan")
    elif kind == "search":
        outline(-w * 0.42, h * 0.22, w * 0.42, h * 0.38, 0.007, "GlowCyan", f)
        holo_icon((w * 0.34, f, h * 0.3), "search", h * 0.15)
        box((-w * 0.22, f, h * 0.3), (w * 0.3, 0.004, 0.01), "GlowCyan")
        for i in range(3):
            z = h * 0.08 - i * h * 0.17
            box((-w * 0.36, f, z), (h * 0.1, 0.004, h * 0.1), "GlowOrange" if i == 0 else "GlowCyan")
            box((-w * 0.05, f, z + h * 0.025), (w * 0.5, 0.004, 0.012), "GlowCyan")
            box((-w * 0.12, f, z - h * 0.03), (w * 0.36, 0.004, 0.007), "GlowCyan")
    elif kind == "doc":
        box((-w * 0.2, f, h * 0.38), (w * 0.45, 0.004, 0.026), "GlowOrange")
        box((-w * 0.27, f, h * 0.31), (w * 0.3, 0.004, 0.01), "GlowCyan")
        text_lines(-w * 0.4, h * 0.2, w * 0.8, 5, h * 0.065, 0.01, "GlowCyan", rng)
        outline(-w * 0.4, -h * 0.4, -w * 0.02, -h * 0.13, 0.006, "GlowCyan", f)
        seg2((-w * 0.38, -h * 0.38), (-w * 0.25, -h * 0.22), 0.006, "GlowOrange", f)
        seg2((-w * 0.25, -h * 0.22), (-w * 0.04, -h * 0.38), 0.006, "GlowOrange", f)
        text_lines(w * 0.04, -h * 0.15, w * 0.36, 4, h * 0.065, 0.01, "GlowCyan", rng)
    elif kind == "card":
        outline(-w * 0.38, h * 0.02, w * 0.38, h * 0.42, 0.005, "GlowCyan", f)
        box((-w * 0.08, f, -h * 0.08), (w * 0.6, 0.004, 0.022), "GlowOrange")
        text_lines(-w * 0.38, -h * 0.18, w * 0.76, 3, h * 0.08, 0.009, "GlowCyan", rng)
    elif kind == "cpu":
        box((-w * 0.22, f, h * 0.37), (w * 0.4, 0.004, 0.018), "GlowCyan")
        for i in range(4):
            z = h * 0.2 - i * h * 0.14
            box((-w * 0.36, f, z), (w * 0.1, 0.004, 0.012), "GlowCyan")
            outline(-w * 0.27, z - 0.012, w * 0.15, z + 0.012, 0.004, "GlowCyan", f)
            fill = rng.uniform(0.3, 0.95)
            m = "GlowOrange" if fill > 0.8 else "GlowCyan"
            box((-w * 0.27 + w * 0.42 * fill / 2, f, z), (w * 0.42 * fill, 0.004, 0.016), m)
        for r_ in range(2):
            for c in range(3):
                m = "GlowPurple" if (r_ * 3 + c) in (1, 5) else "GlowCyan"
                box((w * 0.26 + (c - 1) * w * 0.07, f, h * 0.12 - r_ * h * 0.17), (w * 0.05, 0.004, h * 0.11), m)


def holo_screen(center, w, h, yaw=0.0, tilt=10.0, content="bars", frame="GlowCyan", seed=1, bg="Holo"):
    """Floating translucent screen facing -Y (local), yawed about Z, tilted back."""
    rng = random.Random(seed)
    with at(center, (tilt, 0, yaw)):
        box((0, 0, 0), (w, 0.004, h), bg)
        t = 0.008
        for z in (h / 2, -h / 2):
            box((0, -0.002, z), (w + t, 0.006, t), frame)
        for x in (w / 2, -w / 2):
            box((x, -0.002, 0), (t, 0.006, h), frame)
        for sx in (-1, 1):
            box((sx * (w / 2 - 0.035), -0.004, h / 2 + 0.012), (0.05, 0.006, 0.01), frame)
            box((sx * (w / 2 + 0.012), -0.004, -h / 2 + 0.05), (0.008, 0.006, 0.06), "GlowOrange")
        holo_content(content, w, h, rng)


def floor_disc(cx, cy, r, h=0.04, segs=64):
    """Circular illuminated platform: white rim, dark glossy top, glowing rings."""
    disc((cx, cy, 0), r, 0.0, h, "Panel", segs=segs, bevel=0.012, bsegs=2)
    disc((cx, cy, 0), r - 0.1, h, h + 0.003, "PanelDark", segs=segs)
    ring((cx, cy, 0), r - 0.1, r - 0.075, h - 0.002, h + 0.005, "GlowCyan", segs=segs)
    ring((cx, cy, 0), r - 0.012, r + 0.004, h * 0.35, h * 0.65, "GlowCyan", segs=segs)
    ring((cx, cy, 0), r * 0.42, r * 0.42 + 0.015, h, h + 0.005, "GlowCyan", segs=segs)
    for k in range(4):
        a = 45 + 90 * k
        ring((cx, cy, 0), r - 0.2, r - 0.17, h, h + 0.006, "GlowOrange", segs=6, a0=a - 12, a1=a + 12)


def glow_strip_sector(r, z, a0, a1, material, h=0.02, depth=0.012, segs=24):
    ring((0, 0, 0), r - depth, r, z - h / 2, z + h / 2, material, segs=segs, a0=a0, a1=a1)


def potted_plant(loc, scale=1.0, seed=3, leaves=16):
    rng = random.Random(seed)
    s = scale
    with at(loc):
        lathe((0, 0, 0), [(0, 0), (0.13 * s, 0), (0.16 * s, 0.03 * s), (0.2 * s, 0.3 * s),
                          (0.21 * s, 0.36 * s), (0.19 * s, 0.37 * s), (0, 0.37 * s)], "Pot", segs=40,
              bevel=0.006 * s)
        ring((0, 0, 0), 0.155 * s, 0.168 * s, 0.07 * s, 0.09 * s, "GlowCyan", segs=40)
        disc((0, 0, 0), 0.185 * s, 0.37 * s, 0.375 * s, "PanelDark", segs=32)
        for i in range(leaves):
            az = i * 137.5 + rng.uniform(-10, 10)
            elev = rng.uniform(35, 75)
            L = rng.uniform(0.45, 0.8) * s
            leaf(L, rng.uniform(0.09, 0.14) * s, elev, L * rng.uniform(0.25, 0.5), "Leaf", az,
                 base=(0, 0, 0.36 * s))


def check_badge(center, r=0.07, rot=(0, 0, 0)):
    """Round dark badge with a green glowing check mark, facing -Y (local)."""
    with at(center, rot):
        disc((0, 0, 0), r, -0.01, 0.01, "Trim", segs=32, bevel=0.004)
        with at((0, -0.012, 0), (90, 0, 0)):
            disc((0, 0, 0), r * 0.82, -0.002, 0.002, "PanelDark", segs=32)
            ring((0, 0, 0), r * 0.82, r * 0.9, -0.003, 0.003, "GlowGreen", segs=32)
        box((-r * 0.22, -0.016, -r * 0.08), (r * 0.42, 0.006, r * 0.16), "GlowGreen", rot=(0, -45, 0))
        box((r * 0.18, -0.016, r * 0.08), (r * 0.8, 0.006, r * 0.16), "GlowGreen", rot=(0, 52, 0))


# ---------------------------------------------------------------- stations
def build_main_desk():
    # three round tiers rising to 0.35 m, glowing cyan rims, orange inset rings
    top = 0.35
    tiers = ((2.0, 0.0, 0.14), (1.62, 0.14, 0.26), (1.25, 0.26, top))
    for r, z0, z1 in tiers:
        disc((0, 0, 0), r, z0, z1, "Panel", segs=72, bevel=0.022, bsegs=3)
        zm = (z0 + z1) / 2
        ring((0, 0, 0), r - 0.01, r + 0.006, zm - 0.009, zm + 0.009, "GlowCyan", segs=72)
        for a in (180, 0):  # step lights on the riser, front and back
            ring((0, 0, 0), r - 0.01, r + 0.008, z1 - 0.035, z1 - 0.022, "GlowOrange", segs=6, a0=a - 14,
                 a1=a + 14)
    ring((0, 0, 0), 1.80, 1.825, 0.138, 0.145, "GlowOrange", segs=72)
    ring((0, 0, 0), 1.43, 1.445, 0.258, 0.265, "GlowCyan", segs=72)
    ring((0, 0, 0), 1.02, 1.05, top - 0.002, top + 0.005, "GlowOrange", segs=72)
    ring((0, 0, 0), 1.07, 1.085, top - 0.002, top + 0.003, "PanelDark", segs=72)
    for k in range(16):  # panel seams on the treads
        a = 11.25 + 22.5 * k
        for r0, r1, z in ((1.64, 1.78, 0.14), (1.27, 1.42, 0.26)):
            rm = (r0 + r1) / 2
            box((rm * math.sin(rad(a)), rm * math.cos(rad(a)), z), (0.01, r1 - r0, 0.006), "PanelDark",
                rot=(0, 0, -a))
    # robot pad
    disc((0, 0, 0), 0.55, top, top + 0.004, "PanelDark", segs=48)
    ring((0, 0, 0), 0.55, 0.575, top - 0.002, top + 0.007, "GlowCyan", segs=48)
    for k in range(4):
        a = 45 + 90 * k
        box((0.42 * math.sin(rad(a)), 0.42 * math.cos(rad(a)), top + 0.006), (0.1, 0.018, 0.004), "Trim",
            rot=(0, 0, -a))

    # low curved console wall behind the robot (Blender +Y = glTF -Z)
    ring((0, 0, 0), 0.97, 1.22, top, top + 0.05, "Trim", segs=40, a0=-66, a1=66, bevel=0.012)
    ring((0, 0, 0), 1.0, 1.2, top + 0.05, top + 0.72, "PanelDark", segs=40, a0=-64, a1=64)
    for a0, a1 in ((-64, -22.5), (-21.5, 21.5), (22.5, 64)):
        ring((0, 0, 0), 0.975, 1.21, top + 0.07, top + 0.70, "Panel", segs=14, a0=a0, a1=a1, bevel=0.012)
        # storage compartment door with a glowing handle
        am = (a0 + a1) / 2
        ring((0, 0, 0), 0.962, 0.975, top + 0.22, top + 0.58, "PanelDark", segs=8, a0=am - 13, a1=am + 13)
        ring((0, 0, 0), 0.955, 0.965, top + 0.53, top + 0.545, "GlowCyan", segs=4, a0=am - 5, a1=am + 5)
    glow_strip_sector(0.976, top + 0.11, -62, 62, "GlowCyan", h=0.014, depth=0.02)
    ring((0, 0, 0), 0.9, 1.28, top + 0.72, top + 0.80, "Panel", segs=48, a0=-68, a1=68, bevel=0.025, bsegs=3)
    ring((0, 0, 0), 0.95, 1.17, top + 0.80, top + 0.806, "PanelDark", segs=48, a0=-62, a1=62)
    glow_strip_sector(0.91, top + 0.76, -67, 67, "GlowCyan", h=0.018, depth=0.02)
    ring((0, 0, 0), 1.17, 1.28, top + 0.80, top + 0.93, "Panel", segs=40, a0=-66, a1=66, bevel=0.02)
    glow_strip_sector(1.17, top + 0.88, -62, 62, "GlowOrange", h=0.012, depth=0.01)
    # embedded control surfaces
    for a in (-16, -8, 8, 16):
        x, y = 1.03 * math.sin(rad(a)), 1.03 * math.cos(rad(a))
        box((x, y, top + 0.809), (0.06, 0.04, 0.006), "GlowOrange" if a == 8 else "Trim", rot=(0, 0, -a),
            bevel=0.003)
    for a in (-24, 24):
        x, y = 1.06 * math.sin(rad(a)), 1.06 * math.cos(rad(a))
        disc((x, y, top + 0.806), 0.05, 0.0, 0.006, "Trim", segs=24)
        ring((x, y, top + 0.806), 0.035, 0.045, 0.004, 0.009, "GlowCyan", segs=24)
    # floating holo panels: big one at the back, one left, one right (chest/head height)
    holo_screen((0, 1.2, top + 1.36), 1.05, 0.58, yaw=0, tilt=-8, content="bars", seed=1)
    for a, c, s in ((-64, "graph", 2), (64, "lines", 3)):
        r = 1.05
        holo_screen((r * math.sin(rad(a)), r * math.cos(rad(a)), top + 1.1), 0.58, 0.4, yaw=-a * 0.55, tilt=-6,
                    content=c, seed=s)
        disc((r * math.sin(rad(a)), r * math.cos(rad(a)), top), 0.07, 0.0, 0.03, "Trim", segs=24, bevel=0.008)
        disc((r * math.sin(rad(a)), r * math.cos(rad(a)), top), 0.045, 0.03, 0.036, "GlowCyan", segs=24)
    for a, kind, z in ((-34, "file", 1.2), (34, "code", 1.2), (0, "tool", 1.72)):
        r = 1.18
        holo_icon((r * math.sin(rad(a)), r * math.cos(rad(a)), top + z), kind, 0.14, rot=(0, 0, -a))
    # hologram projector on the left of the desk
    pa = -50
    with at((1.08 * math.sin(rad(pa)), 1.08 * math.cos(rad(pa)), top + 0.80)):
        disc((0, 0, 0), 0.08, 0.0, 0.04, "Trim", segs=32, bevel=0.01)
        disc((0, 0, 0), 0.055, 0.04, 0.05, "GlowCyan", segs=32)
        lathe((0, 0, 0), [(0, 0.05), (0.03, 0.05), (0.11, 0.26), (0, 0.26)], "Holo", segs=32)
        sphere((0, 0, 0.31), (0.05, 0.05, 0.05), "GlowPurple", seg=20, rings_=10)
        ring((0, 0, 0), 0.085, 0.095, 0.305, 0.315, "GlowCyan", segs=32, rot=(20, 0, 0))
    # inbox tray on the front edge at x = +0.6
    tr = 1.05
    ta = math.degrees(math.asin(0.6 / tr))
    with at((0.6, tr * math.cos(rad(ta)), top + 0.806), (0, 0, -ta)):
        box((0, 0, 0.01), (0.34, 0.24, 0.02), "Trim", bevel=0.006)
        box((0, 0, 0.021), (0.3, 0.2, 0.004), "PanelDark")
        for sx in (-1, 1):
            box((sx * 0.165, 0, 0.035), (0.012, 0.24, 0.03), "Panel", bevel=0.004)
        box((0, 0.115, 0.035), (0.34, 0.012, 0.03), "Panel", bevel=0.004)
        box((0, -0.122, 0.012), (0.26, 0.006, 0.008), "GlowOrange")
    # light pylons at the back and a plant on the lower tier
    for sx in (-1, 1):
        with at((sx * 1.3, 0.75, 0.26)):
            disc((0, 0, 0), 0.09, 0.0, 0.05, "Trim", segs=8, phase=22.5, bevel=0.01)
            disc((0, 0, 0), 0.055, 0.05, 1.1, "Panel", segs=8, phase=22.5, bevel=0.012)
            for k in range(2):
                a = 180 + (k * 2 - 1) * 45
                ring((0, 0, 0), 0.05, 0.058, 0.15, 0.95, "GlowCyan", segs=1, a0=a - 12, a1=a + 12)
            sphere((0, 0, 1.16), (0.05, 0.05, 0.05), "GlowOrange", seg=16, rings_=8)
    potted_plant((-1.27, -1.27, 0.14), scale=0.8, seed=5, leaves=12)


def build_writing_desk():
    floor_disc(0, -0.3, 1.45)
    dz = 0.78
    with at((0, 0, 0.04)):
        box((0, 0, dz - 0.03), (2.4, 0.78, 0.06), "Panel", bevel=0.03, segs=3)
        box((0, 0.02, dz + 0.002), (2.2, 0.62, 0.004), "PanelDark")
        box((0, -0.392, dz - 0.03), (2.2, 0.012, 0.014), "GlowCyan")
        # pedestals with storage drawers and a back modesty panel
        for sx in (-1, 1):
            with at((sx * 1.0, 0, 0)):
                box((0, 0, 0.36), (0.36, 0.7, 0.72), "PanelDark", bevel=0.02)
                for z0, z1 in ((0.04, 0.38), (0.4, 0.7)):
                    box((0, -0.01, (z0 + z1) / 2), (0.355, 0.7, z1 - z0 - 0.01), "Panel", bevel=0.02, segs=3)
                    box((0, -0.362, z1 - 0.06), (0.16, 0.006, 0.012), "GlowCyan")
                box((-sx * 0.181, 0, 0.36), (0.004, 0.5, 0.012), "GlowOrange")
        box((0, 0.3, 0.42), (1.66, 0.04, 0.5), "PanelDark", bevel=0.01)
        box((0, 0.28, 0.2), (1.6, 0.006, 0.012), "GlowCyan")
        # embedded keyboard / control surface
        with at((0, -0.18, dz + 0.004), (4, 0, 0)):
            box((0, 0, 0.008), (0.62, 0.2, 0.016), "PanelDark", bevel=0.006)
            box((0, -0.101, 0.004), (0.56, 0.004, 0.006), "GlowCyan")
            for r in range(4):
                for c in range(13):
                    x = -0.255 + c * 0.0425 + (r % 2) * 0.01
                    box((x, -0.06 + r * 0.04, 0.018), (0.034, 0.03, 0.006), "Trim", bevel=0.002, segs=1)
            box((0, -0.075, 0.018), (0.24, 0.022, 0.006), "Trim", bevel=0.002, segs=1)
        box((0.45, -0.18, dz + 0.006), (0.14, 0.11, 0.006), "PanelDark", bevel=0.003)
        box((0.45, -0.236, dz + 0.006), (0.1, 0.004, 0.004), "GlowCyan")
        box((-0.45, -0.2, dz + 0.012), (0.2, 0.26, 0.016), "Paper", rot=(0, 0, 8), bevel=0.002, segs=1)
        box((-0.43, -0.2, dz + 0.024), (0.012, 0.15, 0.008), "GlowOrange", rot=(0, 0, 20))
        # projector bar at the back edge, holo cluster: editor, page, progress
        box((0, 0.33, dz + 0.02), (1.6, 0.06, 0.04), "Trim", bevel=0.012)
        box((0, 0.298, dz + 0.02), (1.5, 0.006, 0.012), "GlowCyan")
        holo_screen((0, 0.34, 1.5), 1.0, 0.56, yaw=0, tilt=-8, content="editor", seed=8)
        holo_screen((-0.8, 0.2, 1.42), 0.46, 0.6, yaw=24, tilt=-6, content="page", seed=9)
        holo_screen((0.8, 0.2, 1.38), 0.5, 0.4, yaw=-24, tilt=-6, content="progress", seed=10)
        holo_icon((-0.38, 0.32, 1.92), "doc", 0.13)
        holo_icon((0.38, 0.32, 1.92), "code", 0.13)
        # lamp strip arching over the left of the desk
        with at((-1.02, 0.25, dz)):
            disc((0, 0, 0), 0.07, 0.0, 0.02, "Trim", segs=24, bevel=0.005)
            tube_path([(0, 0, 0.0), (0, 0, 0.25), (0.04, -0.05, 0.42), (0.2, -0.16, 0.48), (0.34, -0.22, 0.45)],
                      0.014, "Panel", sides=10, samples=6)
            with at((0.36, -0.23, 0.43), (0, 18, -30)):
                box((0, 0, 0), (0.2, 0.05, 0.025), "Panel", bevel=0.01)
                box((0, 0, -0.014), (0.17, 0.03, 0.004), "GlowOrange")
        potted_plant((1.0, 0.18, dz), scale=0.45, seed=12, leaves=10)
        box((0.75, -0.2, dz + 0.05), (0.07, 0.07, 0.1), "Panel", bevel=0.012)
        ring((0.75, -0.2, dz), 0.03, 0.037, 0.06, 0.07, "GlowCyan", segs=16)


# ---------------------------------------------------------------- activity holograms
def build_holo_document():
    holo_screen((0, 0, 0.395), 0.6, 0.78, tilt=0, content="doc", seed=31)


def build_holo_editor():
    holo_screen((0, 0, 0.285), 0.86, 0.56, tilt=0, content="editor", seed=32)
    for i, w in enumerate((0.16, 0.14)):
        box((-0.43 + 0.1 + i * 0.18, -0.002, 0.585), (w, 0.004, 0.035), "Holo")
        box((-0.43 + 0.1 + i * 0.18, -0.006, 0.585), (w * 0.6, 0.004, 0.008), "GlowOrange" if i == 0 else "GlowCyan")


def build_holo_terminal():
    holo_screen((0, 0, 0.255), 0.76, 0.5, tilt=0, content="terminal", seed=33, bg="HoloDark", frame="GlowGreen")


def build_holo_cards():
    kinds = ("card", "card", "card")
    for i, (x, roll, z) in enumerate(((-0.27, 12, 0.242), (0.0, 0, 0.272), (0.27, -12, 0.242))):
        with at((x, -0.03 * (i == 1), z), (0, roll, 0)):
            holo_screen((0, 0, 0), 0.3, 0.42, tilt=0, content=kinds[i], seed=40 + i)
            holo_icon((0, -0.01, 0.12), ("search", "doc", "file")[i], 0.1)


def build_holo_nodes():
    rng = random.Random(5)
    c = Vector((0, 0, 0.245))
    pts = []
    n = 13
    for i in range(n):
        y = 1 - 2 * (i + 0.5) / n
        rr = math.sqrt(1 - y * y)
        th = i * 2.39996
        p = Vector((math.cos(th) * rr, math.sin(th) * rr, y)) * 0.2 * rng.uniform(0.75, 1.1)
        pts.append(c + p)
    edges = set()
    for i, p in enumerate(pts):
        near = sorted(range(n), key=lambda j: (pts[j] - p).length)[1:3]
        for j in near:
            edges.add((min(i, j), max(i, j)))
    for i in range(0, n, 3):
        tube_path([c, pts[i]], 0.004, "GlowCyan", sides=6, samples=1)
    for i, j in edges:
        tube_path([pts[i], pts[j]], 0.004, "GlowCyan", sides=6, samples=1)
    for i, p in enumerate(pts):
        m = ("GlowCyan", "GlowCyan", "GlowOrange", "GlowPurple")[i % 4]
        sphere(p, (0.022,) * 3, m, seg=12, rings_=6)
    sphere(c, (0.055,) * 3, "GlowPurple", seg=20, rings_=10)
    sphere(c, (0.08,) * 3, "Holo", seg=20, rings_=10)
    ring(c, 0.25, 0.258, -0.002, 0.002, "GlowCyan", segs=48, rot=(70, 10, 0))
    ring(c, 0.22, 0.226, -0.002, 0.002, "GlowOrange", segs=48, a0=0, a1=220, rot=(-60, 30, 0))


def build_holo_capsule():
    build_document()


def build_plant():
    potted_plant((0, 0, 0), scale=1.55, seed=21, leaves=22)


def build_wall_panel():
    W, H, T = 3.0, 3.0, 0.2
    wx0, wx1, wz0, wz1 = -1.1, 1.1, 0.75, 2.45
    yc = T / 2
    # structural parts around the window opening
    box((0, yc, wz0 / 2), (W, T, wz0), "Panel", bevel=0.02)
    box((0, yc, (wz1 + H) / 2), (W, T, H - wz1), "Panel", bevel=0.02)
    for sx in (-1, 1):
        box((sx * (W / 2 + wx1) / 2, yc, (wz0 + wz1) / 2), (W / 2 - wx1, T, wz1 - wz0), "Panel", bevel=0.02)
    # chamfered window frame
    fr = 0.06
    with at((0, -0.02, 0)):
        box((0, 0, wz0 + fr / 2), (wx1 - wx0 + 2 * fr, 0.08, fr), "Trim", bevel=0.015)
        box((0, 0, wz1 - fr / 2), (wx1 - wx0 + 2 * fr, 0.08, fr), "Trim", bevel=0.015)
        for x in (wx0 + fr / 2, wx1 - fr / 2):
            box((x, 0, (wz0 + wz1) / 2), (fr, 0.08, wz1 - wz0), "Trim", bevel=0.015)
        # mullion
        box((0, 0.04, (wz0 + wz1) / 2), (0.04, 0.05, wz1 - wz0 - 0.1), "Trim", bevel=0.01)
        box((0, -0.042, wz0 + fr + 0.008), (wx1 - wx0 - 0.1, 0.008, 0.014), "GlowCyan")
        box((0, -0.042, wz1 - fr - 0.008), (wx1 - wx0 - 0.1, 0.008, 0.014), "GlowCyan")
    # panel seams and dark insets on the lower part
    for x in (-0.75, 0.0, 0.75):
        box((x, -0.002, wz0 / 2), (0.012, 0.01, wz0 - 0.12), "PanelDark")
    for x in (-1.125, -0.375, 0.375, 1.125):
        box((x, -0.004, 0.42), (0.5, 0.012, 0.3), "PanelDark", bevel=0.01)
        box((x, -0.011, 0.42), (0.36, 0.004, 0.012), "GlowCyan" if x in (-0.375, 0.375) else "GlowOrange")
    box((0, -0.006, 0.06), (W - 0.06, 0.014, 0.02), "GlowCyan")
    # upper header: seam line, orange strip, small vents
    box((0, -0.004, wz1 + 0.18), (W - 0.1, 0.01, 0.018), "GlowOrange")
    box((0, -0.002, H - 0.08), (W, 0.01, 0.012), "PanelDark")
    for i in range(6):
        box((-0.25 + i * 0.1, -0.004, wz1 + 0.36), (0.06, 0.012, 0.03), "PanelDark", bevel=0.005)
    # vertical side strips
    for sx in (-1, 1):
        box((sx * 1.33, -0.004, (wz0 + wz1) / 2), (0.016, 0.012, wz1 - wz0 - 0.2), "GlowCyan")
        box((sx * 1.4, -0.002, H / 2), (0.01, 0.01, H - 0.2), "PanelDark")


def build_pillar():
    H = 3.0
    disc((0, 0, 0), 0.36, 0.0, 0.14, "Panel", segs=8, phase=22.5, bevel=0.02, bsegs=3)
    ring((0, 0, 0), 0.335, 0.365, 0.06, 0.08, "GlowCyan", segs=8, phase=22.5)
    disc((0, 0, 0), 0.3, 0.14, 0.2, "Trim", segs=8, phase=22.5, bevel=0.01)
    disc((0, 0, 0), 0.24, 0.2, H - 0.2, "Panel", segs=8, phase=22.5, bevel=0.02, bsegs=3)
    disc((0, 0, 0), 0.3, H - 0.2, H - 0.14, "Trim", segs=8, phase=22.5, bevel=0.01)
    disc((0, 0, 0), 0.36, H - 0.14, H, "Panel", segs=8, phase=22.5, bevel=0.02, bsegs=3)
    ring((0, 0, 0), 0.335, 0.365, H - 0.08, H - 0.06, "GlowCyan", segs=8, phase=22.5)
    apo = 0.24 * math.cos(rad(22.5))
    # recessed channels with a light strip on 4 faces (front, back, sides)
    for a in (0, 90, 180, 270):
        with at((0, 0, 0), (0, 0, -a)):
            box((0, apo, H / 2), (0.08, 0.012, H - 0.6), "PanelDark", bevel=0.004)
            box((0, apo + 0.006, H / 2), (0.022, 0.012, H - 0.7), "GlowCyan" if a != 90 else "GlowOrange")
    for z in (0.9, 2.1):
        ring((0, 0, 0), 0.236, 0.252, z - 0.01, z + 0.01, "PanelDark", segs=8, phase=22.5)


def build_document():
    rng = random.Random(2)
    with at((0, 0, 0)):
        disc((0, 0, 0), 0.1, 0.0, 0.012, "Trim", segs=32, bevel=0.003)
        ring((0, 0, 0), 0.085, 0.1, 0.012, 0.016, "GlowCyan", segs=32)
        z = 0.03
        for k in range(3):
            with at((0, 0, z), (0, 0, rng.uniform(-10, 10))):
                box((0, 0, 0), (0.16, 0.21, 0.008), "Paper", bevel=0.002, segs=1)
                box((-0.08, 0, 0.0), (0.004, 0.21, 0.01), "GlowCyan")
                if k == 2:
                    box((-0.02, 0.07, 0.0045), (0.09, 0.016, 0.002), "GlowCyan")
                    for i in range(6):
                        lw = rng.uniform(0.06, 0.11)
                        box((-0.06 + lw / 2, 0.035 - i * 0.022, 0.0045), (lw, 0.006, 0.002), "PanelDark")
                    box((0.05, -0.08, 0.0045), (0.03, 0.03, 0.002), "GlowOrange")
            z += 0.022
        # translucent data capsule around the stack
        lathe((0, 0, 0), [(0, 0.015), (0.08, 0.02), (0.135, 0.06), (0.145, 0.11), (0.135, 0.16), (0.08, 0.2),
                          (0, 0.205)], "Holo", segs=32)


STATIONS = {
    "main_desk": build_main_desk,
    "writing_desk": build_writing_desk,
    "plant": build_plant,
    "wall_panel": build_wall_panel,
    "pillar": build_pillar,
    "document": build_document,
    "holo_document": build_holo_document,
    "holo_editor": build_holo_editor,
    "holo_terminal": build_holo_terminal,
    "holo_cards": build_holo_cards,
    "holo_nodes": build_holo_nodes,
    "holo_capsule": build_holo_capsule,
}


# ---------------------------------------------------------------- merge + export
def finish(name):
    bpy.context.view_layer.update()
    dg = bpy.context.evaluated_depsgraph_get()
    verts, faces, mat_idx, names = [], [], [], []
    for ob in PARTS:
        ev = ob.evaluated_get(dg)
        me = bpy.data.meshes.new_from_object(ev, depsgraph=dg)
        me.transform(ob.matrix_world)
        mname = ob["mat"]
        if mname not in names:
            names.append(mname)
        mi = names.index(mname)
        off = len(verts)
        verts.extend(v.co.copy() for v in me.vertices)
        for p in me.polygons:
            faces.append([off + i for i in p.vertices])
            mat_idx.append(mi)
        bpy.data.meshes.remove(me)
    for ob in list(bpy.data.objects):
        bpy.data.objects.remove(ob, do_unlink=True)
    mesh = bpy.data.meshes.new(name)
    mesh.from_pydata(verts, [], faces)
    mesh.validate()
    for n in names:
        mesh.materials.append(mat(n))
    mesh.polygons.foreach_set("material_index", mat_idx)
    mesh.shade_smooth()
    mesh.set_sharp_from_angle(angle=rad(36))
    ob = bpy.data.objects.new(name, mesh)
    bpy.context.scene.collection.objects.link(ob)
    path = os.path.join(OUT_DIR, f"{name}.glb")
    os.makedirs(OUT_DIR, exist_ok=True)
    bpy.ops.export_scene.gltf(
        filepath=path,
        export_format="GLB",
        export_apply=True,
        export_animations=False,
        export_yup=True,
        export_lights=False,
        export_cameras=False,
    )
    tris = sum(len(p.vertices) - 2 for p in mesh.polygons)
    print(f"STATION: wrote {path}, {tris} triangles, materials {names}")


for mname, spec in MAT_SPECS.items():
    make_material(mname, **spec)
for key in ARGS or list(STATIONS):
    reset_scene()
    STATIONS[key]()
    finish(key)
