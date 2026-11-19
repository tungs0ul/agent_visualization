"""Build the Agent Room environment: the HQ floor and the space-station shell around it.

Run headless from the repo root (Blender 5.2):

    /Applications/Blender.app/Contents/MacOS/Blender -b --factory-startup \
        --python tools/blender/environment.py [-- OUT_DIR]

Writes two files into assets/models/environment/ (or OUT_DIR):

* floor.glb  - one mesh "Floor": dark titanium panels with fine seams, a polished
  centre disc with a hexagon inlay, thin LED lines and flush docking rings under
  the command station and the eight crew desks. Floor top is y = 0.
* shell.glb  - the architecture: curved back/side walls (4.5 m) with a panoramic
  window (x -7.5..7.5) in the back wall, ribs, light strips, a partial ceiling overhang, a door,
  a holographic display, server racks, backlit panels, two pillars and four plants.
  Separate nodes: Walls, Glass, Door, Holo, Racks, Pillars, Plants.

Modelled in Blender Z-up; the exporter writes glTF Y-up, so Blender (x, y, z) ends
up at glTF (x, z, -y). Plan positions below are Blender (x, y) = glTF (x, -z):
the back wall is at Blender +y (glTF -z), the open front at Blender -y (glTF +z).
"""

import math
import os
import sys

import bmesh
import bpy
from mathutils import Vector

REPO = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
OUT_DIR = os.path.join(REPO, "assets", "models", "environment")
if "--" in sys.argv:
    args = sys.argv[sys.argv.index("--") + 1:]
    if args:
        OUT_DIR = os.path.abspath(args[0])

# Workstation spots in Blender plan coordinates (glTF z negated) and ring radius: the
# bridge's command station and eight crew desks (PLAN.md M27). Keep in step with
# `world3d::Spot` and `world3d::DESKS` in the app.
DESKS = [(-5.4, -2.4), (5.4, -2.4), (-2.6, 0.6), (2.6, 0.6), (-6.8, 0.6), (6.8, 0.6), (-4.6, 3.6), (4.6, 3.6)]
STATIONS = [((0.0, 3.6), 2.4)] + [((x, -z), 1.3) for x, z in DESKS]

# ---------------------------------------------------------------- scene reset
for ob in list(bpy.data.objects):
    bpy.data.objects.remove(ob, do_unlink=True)
for coll in (bpy.data.meshes, bpy.data.materials):
    for item in list(coll):
        coll.remove(item)
scene = bpy.context.scene


# ---------------------------------------------------------------- materials
def make_material(name, color, rough, metal=0.0, emit=None, strength=0.0, alpha=1.0):
    mat = bpy.data.materials.new(name)
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
    return mat


CYAN = (0.0, 0.62, 1.0)
PURPLE = (0.45, 0.12, 1.0)
ORANGE = (1.0, 0.38, 0.05)
MATS = {m.name: m for m in [
    make_material("Floor", (0.03, 0.04, 0.07), 0.16, metal=0.9),
    make_material("FloorAlt", (0.06, 0.08, 0.13), 0.14, metal=0.9),
    make_material("Seam", (0.008, 0.009, 0.012), 0.6, metal=0.3),
    make_material("Pad", (0.02, 0.03, 0.06), 0.18, metal=0.7),
    make_material("Panel", (0.1, 0.13, 0.2), 0.24, metal=0.7),
    make_material("Metal", (0.42, 0.5, 0.62), 0.28, metal=1.0),
    make_material("Graphite", (0.02, 0.03, 0.055), 0.35, metal=0.6),
    make_material("GlowCyan", CYAN, 0.3, emit=CYAN, strength=9.0),
    make_material("GlowPurple", PURPLE, 0.3, emit=PURPLE, strength=2.0),
    make_material("GlowOrange", ORANGE, 0.3, emit=ORANGE, strength=5.0),
    make_material("Glass", (0.12, 0.18, 0.26), 0.04, alpha=0.15),
    make_material("Holo", (0.05, 0.7, 1.0), 0.2, emit=(0.05, 0.7, 1.0), strength=4.0, alpha=0.35),
    make_material("Leaf", (0.06, 0.32, 0.12), 0.5),
    make_material("Pot", (0.82, 0.83, 0.85), 0.2),
]}


# ---------------------------------------------------------------- mesh builder
class Builder:
    """Collects faces with materials into one bmesh, then becomes one object."""

    def __init__(self, name):
        self.name = name
        self.bm = bmesh.new()
        self.mats = []

    def mi(self, mat):
        if mat not in self.mats:
            self.mats.append(mat)
        return self.mats.index(mat)

    def face(self, pts, mat, smooth=False):
        f = self.bm.faces.new([self.bm.verts.new(p) for p in pts])
        f.material_index = self.mi(mat)
        f.smooth = smooth
        return f

    def grid(self, rows, mat, smooth=True, closed_u=False):
        """rows: list of vertex rows (same length); quads between neighbouring rows."""
        vr = [[self.bm.verts.new(p) for p in row] for row in rows]
        n = len(rows[0])
        out = []
        for a, b in zip(vr, vr[1:]):
            for i in range(n if closed_u else n - 1):
                j = (i + 1) % n
                f = self.bm.faces.new([a[i], a[j], b[j], b[i]])
                f.material_index = self.mi(mat)
                f.smooth = smooth
                out.append(f)
        return out

    def finish(self, collection):
        me = bpy.data.meshes.new(self.name)
        self.bm.to_mesh(me)
        self.bm.free()
        for m in self.mats:
            me.materials.append(MATS[m])
        ob = bpy.data.objects.new(self.name, me)
        collection.objects.link(ob)
        return ob


def obox(b, c, t, n, ht, hn, z0, z1, mat, top=True, bottom=False):
    """Box centred at plan point c, half sizes ht along t and hn along n, z0..z1."""
    c, t, n = Vector((c[0], c[1], 0)), Vector((t[0], t[1], 0)), Vector((n[0], n[1], 0))
    q = [c - t * ht - n * hn, c + t * ht - n * hn, c + t * ht + n * hn, c - t * ht + n * hn]
    lo = [Vector((p.x, p.y, z0)) for p in q]
    hi = [Vector((p.x, p.y, z1)) for p in q]
    for i in range(4):
        j = (i + 1) % 4
        b.face([lo[i], lo[j], hi[j], hi[i]], mat)
    if top:
        b.face(hi, mat)
    if bottom:
        b.face(list(reversed(lo)), mat)


def box(b, x0, y0, x1, y1, z0, z1, mat, top=True, bottom=False):
    obox(b, ((x0 + x1) / 2, (y0 + y1) / 2), (1, 0), (0, 1), (x1 - x0) / 2, (y1 - y0) / 2,
         z0, z1, mat, top, bottom)


def annulus(b, cx, cy, r0, r1, z, mat, seg=96, rot=0.0):
    pts0, pts1 = [], []
    for i in range(seg):
        a = rot + 2 * math.pi * i / seg
        pts0.append(Vector((cx + r0 * math.cos(a), cy + r0 * math.sin(a), z)))
        pts1.append(Vector((cx + r1 * math.cos(a), cy + r1 * math.sin(a), z)))
    b.grid([pts0, pts1], mat, smooth=False, closed_u=True)


def disc(b, cx, cy, r, z, mat, seg=96):
    b.face([Vector((cx + r * math.cos(2 * math.pi * i / seg),
                    cy + r * math.sin(2 * math.pi * i / seg), z)) for i in range(seg)], mat)


def rect(b, x0, y0, x1, y1, z, mat):
    b.face([(x0, y0, z), (x1, y0, z), (x1, y1, z), (x0, y1, z)], mat)


def cylinder(b, cx, cy, r0, r1, z0, z1, mat, seg=32, cap=True):
    lo = [Vector((cx + r0 * math.cos(2 * math.pi * i / seg), cy + r0 * math.sin(2 * math.pi * i / seg), z0))
          for i in range(seg)]
    hi = [Vector((cx + r1 * math.cos(2 * math.pi * i / seg), cy + r1 * math.sin(2 * math.pi * i / seg), z1))
          for i in range(seg)]
    b.grid([lo, hi], mat, smooth=True, closed_u=True)
    if cap:
        b.face(hi, mat)


# ================================================================ FLOOR
floor_coll = bpy.data.collections.new("floor")
scene.collection.children.link(floor_coll)
fb = Builder("Floor")

XS_F = [-10.1, -7.5, -5.0, -2.5, 0.0, 2.5, 5.0, 7.5, 10.1]
YS_F = [-6.5, -5.0, -2.5, 0.0, 2.5, 5.0, 6.95]
GAP, CH, THICK = 0.007, 0.012, 0.08


def floor_panel(x0, y0, x1, y1, mat):
    """Panel with a fine chamfer on its top edges; the dark seam plane shows between."""
    x0, y0, x1, y1 = x0 + GAP, y0 + GAP, x1 - GAP, y1 - GAP
    top = [Vector(p) for p in ((x0 + CH, y0 + CH, 0), (x1 - CH, y0 + CH, 0),
                               (x1 - CH, y1 - CH, 0), (x0 + CH, y1 - CH, 0))]
    mid = [Vector(p) for p in ((x0, y0, -CH), (x1, y0, -CH), (x1, y1, -CH), (x0, y1, -CH))]
    low = [Vector((p.x, p.y, -THICK)) for p in mid]
    fb.face(top, mat)
    for i in range(4):
        j = (i + 1) % 4
        fb.face([mid[i], mid[j], top[j], top[i]], mat)
        fb.face([low[i], low[j], mid[j], mid[i]], mat)


for i in range(len(XS_F) - 1):
    for j in range(len(YS_F) - 1):
        outer = i in (0, len(XS_F) - 2) or j in (0, len(YS_F) - 2)
        floor_panel(XS_F[i], YS_F[j], XS_F[i + 1], YS_F[j + 1], "FloorAlt" if outer else "Floor")
rect(fb, -10.1, -6.5, 10.1, 6.95, -0.03, "Seam")

# Centre: one seamless polished disc with a glow edge and a hexagon inlay around it.
Z1, Z2, Z3 = 0.002, 0.004, 0.006
disc(fb, 0, 0, 3.3, Z1, "FloorAlt", seg=96)
annulus(fb, 0, 0, 3.3, 3.34, Z2, "Metal")
annulus(fb, 0, 0, 3.34, 3.36, Z2, "GlowCyan")
annulus(fb, 0, 0, 4.10, 4.14, Z1, "Metal", seg=6)
annulus(fb, 0, 0, 4.16, 4.172, Z1, "GlowPurple", seg=6)

# Docking rings: dark pad, glowing ring, metal bezel; four orange markers on the main ring.
for (cx, cy), r in STATIONS:
    disc(fb, cx, cy, r - 0.05, Z2, "Pad", seg=72)
    annulus(fb, cx, cy, r - 0.05, r - 0.02, Z3, "Metal", seg=72)
    annulus(fb, cx, cy, r - 0.02, r + 0.04, Z3, "GlowCyan", seg=72)
    annulus(fb, cx, cy, r + 0.04, r + 0.11, Z3, "Metal", seg=72)
    annulus(fb, cx, cy, r + 0.11, r + 0.125, Z3, "Seam", seg=72)
    inner = r * 0.62
    annulus(fb, cx, cy, inner, inner + 0.012, Z3, "GlowCyan", seg=72)
for k in range(4):
    a = math.pi / 4 + k * math.pi / 2
    c, t = Vector((math.cos(a), math.sin(a), 0)), Vector((-math.sin(a), math.cos(a), 0))
    p = c * 2.62
    q = [p - t * 0.14 - c * 0.025, p + t * 0.14 - c * 0.025, p + t * 0.14 + c * 0.025, p - t * 0.14 + c * 0.025]
    fb.face([Vector((v.x, v.y, Z3)) for v in q], "GlowOrange")

# Thin LED lines in the floor, away from the stations.
W = 0.018
rect(fb, -8.4, 5.75 - W, 8.4, 5.75 + W, Z1, "GlowCyan")
for sx in (-1, 1):
    x = sx * 8.35
    rect(fb, x - W, -6.0, x + W, 4.9, Z1, "GlowCyan")
    rect(fb, x - W + sx * 0.12, -1.2, x + W + sx * 0.12, 2.2, Z1, "GlowPurple")
    # corner turn from the side line into the back line
    for k in range(6):
        a0 = math.pi / 2 * k / 6
        a1 = math.pi / 2 * (k + 1) / 6
        cx, cy, rr = sx * 7.55, 4.9, 0.8
        pts = []
        for a, r in ((a0, rr - W), (a1, rr - W), (a1, rr + W), (a0, rr + W)):
            pts.append(Vector((cx + sx * r * math.cos(a), cy + r * math.sin(a), Z1)))
        if sx < 0:
            pts.reverse()
        fb.face(pts, "GlowCyan")
    # orange status dashes near the front corners
    for k in range(3):
        y = -5.2 + k * 0.22
        rect(fb, x - sx * 0.35 - 0.12, y, x - sx * 0.35 + 0.12, y + 0.06, Z1, "GlowOrange")

# Front edge: metal lip with a light strip and a dark fascia dropping away.
box(fb, -10.1, -6.66, 10.1, -6.5, -0.04, 0.025, "Metal")
rect(fb, -10.1, -6.6, 10.1, -6.56, 0.026, "GlowCyan")
fb.face([(-10.1, -6.66, -0.04), (10.1, -6.66, -0.04), (10.1, -6.66, -0.7), (-10.1, -6.66, -0.7)], "Graphite")
fb.face([(-10.1, -6.67, -0.2), (10.1, -6.67, -0.2), (10.1, -6.67, -0.23), (-10.1, -6.67, -0.23)], "GlowCyan")

floor_ob = fb.finish(floor_coll)

# ================================================================ SHELL
shell_coll = bpy.data.collections.new("shell")
scene.collection.children.link(shell_coll)

H = 4.5          # wall height
R_C = 2.6        # back corner radius
XB, YB = 7.5, 6.95
XS = 10.1
YC = YB - R_C    # 4.35, where the corner arcs meet the side walls
Y_END = -5.0     # side walls stop here; the front stays open
ARC_SEG = 14

L_SIDE = YC - Y_END
L_ARC = math.pi / 2 * R_C
L_BACK = 2 * XB
S1, S2, S3, S4 = L_SIDE, L_SIDE + L_ARC, L_SIDE + L_ARC + L_BACK, L_SIDE + 2 * L_ARC + L_BACK
S_END = S4 + L_SIDE


def at(s):
    """Wall line point, tangent and inward normal at arc length s (left front -> right front)."""
    if s <= S1:
        return Vector((-XS, Y_END + s)), Vector((0, 1)), Vector((1, 0))
    if s <= S2:
        a = math.pi - (s - S1) / R_C
        c = Vector((-XB, YC))
    elif s <= S3:
        return Vector((-XB + (s - S2), YB)), Vector((1, 0)), Vector((0, -1))
    elif s <= S4:
        a = math.pi / 2 - (s - S3) / R_C
        c = Vector((XB, YC))
    else:
        return Vector((XS, YC - (s - S4))), Vector((0, -1)), Vector((-1, 0))
    r = Vector((math.cos(a), math.sin(a)))
    return c + r * R_C, Vector((r.y, -r.x)), -r


KNOTS = sorted({S1 + L_ARC * k / ARC_SEG for k in range(ARC_SEG + 1)}
               | {S3 + L_ARC * k / ARC_SEG for k in range(ARC_SEG + 1)} | {S2})


def s_of_x_back(x):
    return S2 + x + XB


def s_of_y_left(y):
    return y - Y_END


def s_of_y_right(y):
    return S4 + YC - y


def samples(s0, s1):
    return [s0] + [k for k in KNOTS if s0 + 1e-6 < k < s1 - 1e-6] + [s1]


def wpt(s, d, z):
    p, _, n = at(s)
    q = p + n * d
    return Vector((q.x, q.y, z))


def ribbon(b, s0, s1, z0, z1, d0, d1, mat, top=True, bottom=False, back=False, ends=True):
    """Wall-following slab between offsets d0 < d1 (d1 faces the room)."""
    ss = samples(s0, s1)
    b.grid([[wpt(s, d1, z0) for s in ss], [wpt(s, d1, z1) for s in ss]], mat)
    if top:
        b.grid([[wpt(s, d1, z1) for s in ss], [wpt(s, d0, z1) for s in ss]], mat)
    if bottom:
        b.grid([[wpt(s, d0, z0) for s in ss], [wpt(s, d1, z0) for s in ss]], mat)
    if back:
        b.grid([[wpt(s, d0, z1) for s in ss], [wpt(s, d0, z0) for s in ss]], mat)
    if ends:
        for s in (s0, s1):
            b.face([wpt(s, d0, z0), wpt(s, d1, z0), wpt(s, d1, z1), wpt(s, d0, z1)], mat)


def wall_box(b, s, ht, z0, z1, d0, d1, mat, top=True):
    p, t, n = at(s)
    c = p + n * ((d0 + d1) / 2)
    obox(b, c, t, n, ht, (d1 - d0) / 2, z0, z1, mat, top=top)


def wall_frame(s, d):
    """Local frame on the wall: origin, tangent, up, normal (all 3D)."""
    p, t, n = at(s)
    o = p + n * d
    return Vector((o.x, o.y, 0)), Vector((t.x, t.y, 0)), Vector((0, 0, 1)), Vector((n.x, n.y, 0))


def wall_rect(b, s, u0, u1, z0, z1, d, mat):
    o, t, up, _ = wall_frame(s, d)
    b.face([o + t * u0 + up * z0, o + t * u1 + up * z0, o + t * u1 + up * z1, o + t * u0 + up * z1], mat)


def wall_annulus(b, s, u, z, r0, r1, d, mat, seg=48):
    o, t, up, _ = wall_frame(s, d)
    rows = []
    for r in (r0, r1):
        rows.append([o + t * (u + r * math.cos(2 * math.pi * i / seg)) + up * (z + r * math.sin(2 * math.pi * i / seg))
                     for i in range(seg)])
    b.grid(rows, mat, smooth=False, closed_u=True)


# ---- walls, ribs, strips, overhang
wb = Builder("Walls")
WIN0, WIN1 = S2, S3
SILL, HEAD = 0.5, 3.95

RIBS = sorted([s_of_y_left(y) for y in (-2.7, 0.8)] + [S1, (S1 + S2) / 2, S2]
              + [s_of_x_back(x) for x in (-4.5, 4.5)] + [S3, (S3 + S4) / 2, S4]
              + [s_of_y_right(y) for y in (0.8, -2.7)] + [0.0, S_END])

# structural shell behind everything (outside the window opening)
for s0, s1 in ((0.0, WIN0), (WIN1, S_END)):
    ribbon(wb, s0, s1, 0.0, H, -0.35, -0.02, "Graphite", top=True, back=True)
# below and above the window
ribbon(wb, WIN0, WIN1, 0.0, SILL, -0.35, 0.06, "Graphite", top=False)
ribbon(wb, WIN0, WIN1, SILL, SILL + 0.06, -0.35, 0.14, "Metal")
ribbon(wb, WIN0, WIN1, SILL - 0.08, SILL - 0.05, 0.06, 0.065, "GlowCyan", top=False, ends=False)
ribbon(wb, WIN0, WIN1, HEAD, H, -0.35, -0.02, "Graphite", bottom=True, back=True)
ribbon(wb, WIN0, WIN1, HEAD - 0.04, HEAD + 0.18, -0.35, 0.06, "Metal", bottom=True)
ribbon(wb, WIN0, WIN1, HEAD - 0.06, HEAD - 0.04, 0.0, 0.04, "GlowCyan", top=False, ends=False)
for x in (-1.5, 1.5):
    wall_box(wb, s_of_x_back(x), 0.06, SILL, HEAD, -0.3, 0.02, "Metal")
    wall_box(wb, s_of_x_back(x), 0.012, SILL + 0.1, HEAD - 0.1, 0.02, 0.03, "GlowCyan")

# ceramic skin panels between ribs, split horizontally with fine seams
BANDS = [(0.26, 1.55), (1.57, 2.86), (2.88, 4.08)]
for a, c in zip(RIBS, RIBS[1:]):
    if a >= WIN0 - 1e-6 and c <= WIN1 + 1e-6:
        continue
    for z0, z1 in BANDS:
        ribbon(wb, a + 0.2, c - 0.2, z0, z1, -0.02, 0.03, "Panel", top=False)

# baseboard with a cove light, top band, and the partial ceiling overhang
ribbon(wb, 0.0, S_END, 0.0, 0.22, -0.02, 0.12, "Graphite")
ribbon(wb, 0.0, S_END, 0.22, 0.245, 0.06, 0.09, "GlowCyan", ends=False)
ribbon(wb, 0.0, WIN0, 4.08, 4.3, -0.02, 0.08, "Metal", bottom=True)
ribbon(wb, WIN1, S_END, 4.08, 4.3, -0.02, 0.08, "Metal", bottom=True)
OVER = 1.5
ribbon(wb, 0.0, S_END, 4.3, 4.5, -0.35, OVER, "Graphite", bottom=True)
ribbon(wb, 0.0, S_END, 4.25, 4.3, OVER - 0.12, OVER - 0.06, "GlowCyan", ends=False, top=False, bottom=True)
ribbon(wb, 0.0, S_END, 4.3, 4.52, OVER, OVER + 0.04, "Metal", ends=False)

# ribs: metal fins with an inset light line; brackets under the overhang
for s in RIBS:
    ht = 0.13 if s in (0.0, S_END) else 0.17
    wall_box(wb, s, ht, 0.0, 4.3, -0.02, 0.32, "Metal")
    wall_box(wb, s, 0.025, 0.5, 3.9, 0.32, 0.335, "GlowCyan")
    wall_box(wb, s, ht * 0.8, 3.95, 4.3, 0.32, 0.9, "Graphite", top=False)

# side wall end caps (front of the side walls)
for s in (0.0, S_END):
    wall_box(wb, s, 0.12, 0.0, H + 0.02, -0.4, OVER + 0.05, "Metal")
    wall_box(wb, s, 0.13, 0.4, 4.0, 0.33, 0.36, "GlowCyan")

# backlit illuminated wall panels
def backlit(s0, s1, z0, z1, glow):
    ribbon(wb, s0, s1, z0 - 0.03, z1 + 0.03, 0.03, 0.05, glow, ends=True)
    ribbon(wb, s0 + 0.03, s1 - 0.03, z0, z1, 0.05, 0.12, "Panel")
    ribbon(wb, s0 + 0.12, s1 - 0.12, z1 - 0.18, z1 - 0.16, 0.12, 0.125, glow, top=False, ends=False)


backlit(s_of_y_right(0.55), s_of_y_right(-2.45), 0.7, 3.5, "GlowCyan")
backlit(s_of_y_left(-4.75), s_of_y_left(-2.95), 0.9, 3.3, "GlowCyan")
backlit(s_of_y_right(-2.95), s_of_y_right(-4.75), 0.9, 3.3, "GlowCyan")
# curved corner light slots
for sc in ((S1 + S2) / 2, (S3 + S4) / 2):
    for off in (-0.85, 0.85):
        ribbon(wb, sc + off - 0.5, sc + off + 0.5, 1.1, 1.13, 0.03, 0.05, "GlowPurple", top=False)
        ribbon(wb, sc + off - 0.5, sc + off + 0.5, 3.3, 3.33, 0.03, 0.05, "GlowPurple", top=False)

walls_ob = wb.finish(shell_coll)

# ---- panoramic glass
gb = Builder("Glass")
ribbon(gb, WIN0, WIN1, SILL, HEAD, -0.2, -0.19, "Glass", top=False, ends=False)
gb.finish(shell_coll)

# ---- door (left wall, closed, illuminated frame)
db = Builder("Door")
SD = s_of_y_left(-0.95)
DW, DH = 1.0, 2.8
wall_box(db, SD - DW - 0.12, 0.12, 0.0, DH + 0.25, 0.0, 0.25, "Metal")
wall_box(db, SD + DW + 0.12, 0.12, 0.0, DH + 0.25, 0.0, 0.25, "Metal")
wall_box(db, SD, DW + 0.24, DH, DH + 0.25, 0.0, 0.25, "Metal")
wall_box(db, SD - DW - 0.005, 0.015, 0.0, DH, 0.02, 0.24, "GlowCyan")
wall_box(db, SD + DW + 0.005, 0.015, 0.0, DH, 0.02, 0.24, "GlowCyan")
wall_box(db, SD, DW, DH - 0.03, DH, 0.02, 0.24, "GlowCyan")
wall_box(db, SD, DW, 0.0, DH, -0.03, 0.0, "Graphite")
wall_box(db, SD, 0.02, 0.05, DH - 0.05, 0.0, 0.01, "GlowCyan")
for side in (-1, 1):
    u = SD + side * (DW / 2 + 0.01)
    wall_box(db, u, DW / 2 - 0.02, 0.0, DH - 0.04, 0.02, 0.09, "Panel")
    wall_box(db, u, DW / 2 - 0.2, 0.9, 0.93, 0.09, 0.1, "Graphite")
    wall_box(db, u, DW / 2 - 0.2, 2.2, 2.23, 0.09, 0.1, "Graphite")
wall_box(db, SD, 0.3, DH + 0.08, DH + 0.14, 0.25, 0.27, "GlowOrange")
wall_box(db, SD + DW + 0.55, 0.12, 1.25, 1.5, 0.03, 0.1, "Graphite")
wall_box(db, SD + DW + 0.55, 0.05, 1.33, 1.42, 0.1, 0.11, "GlowCyan")
db.finish(shell_coll)

# ---- holographic display (left wall, back bay)
hb = Builder("Holo")
SH = s_of_y_left(2.6)
wall_box(hb, SH, 1.55, 0.95, 3.45, 0.02, 0.14, "Graphite")
for z0, z1 in ((0.95, 0.98), (3.42, 3.45)):
    wall_box(hb, SH, 1.55, z0, z1, 0.14, 0.15, "GlowCyan")
wall_box(hb, SH, 1.35, 0.7, 0.95, 0.04, 0.2, "Metal")
wall_rect(hb, SH, -1.45, 1.45, 1.05, 3.35, 0.32, "Holo")
for i, h in enumerate((0.35, 0.6, 0.45, 0.85, 0.7, 1.05)):
    u = -1.2 + i * 0.16
    wall_rect(hb, SH, u, u + 0.1, 1.3, 1.3 + h, 0.33, "GlowCyan")
wall_annulus(hb, SH, 0.75, 2.35, 0.48, 0.52, 0.33, "GlowPurple")
wall_annulus(hb, SH, 0.75, 2.35, 0.3, 0.315, 0.33, "GlowCyan")
for k, w in enumerate((1.0, 0.7, 0.85)):
    z = 3.05 - k * 0.12
    wall_rect(hb, SH, -1.2, -1.2 + w, z, z + 0.025, 0.33, "GlowCyan")
wall_rect(hb, SH, -1.2, -0.3, 2.62, 2.65, 0.33, "GlowOrange")
hb.finish(shell_coll)

# ---- server racks (right wall, back bay)
rb = Builder("Racks")
for k in range(3):
    s = s_of_y_right(3.75 - k * 1.05)
    wall_box(rb, s, 0.5, 0.0, 2.7, 0.02, 0.62, "Graphite")
    wall_box(rb, s, 0.44, 0.15, 2.55, 0.62, 0.64, "Seam", top=False)
    wall_box(rb, s, 0.5, 2.7, 2.74, 0.02, 0.66, "Metal")
    for row in range(14):
        z = 0.3 + row * 0.16
        mat = "GlowOrange" if (row * 7 + k * 3) % 11 == 0 else (
            "GlowPurple" if (row + k) % 6 == 0 else "GlowCyan")
        wall_box(rb, s - 0.18, 0.16, z, z + 0.018, 0.64, 0.65, mat, top=False)
        wall_box(rb, s + 0.28, 0.03, z, z + 0.018, 0.64, 0.65, "GlowCyan", top=False)
rb.finish(shell_coll)

# ---- pillars
pb = Builder("Pillars")
for sx in (-1, 1):
    cx, cy = sx * 8.75, -4.45
    cylinder(pb, cx, cy, 0.46, 0.42, 0.0, 0.2, "Graphite")
    cylinder(pb, cx, cy, 0.3, 0.3, 0.2, 4.3, "Metal", cap=False)
    for z in (0.32, 3.6):
        cylinder(pb, cx, cy, 0.315, 0.315, z, z + 0.05, "GlowCyan")
    cylinder(pb, cx, cy, 0.31, 0.31, 1.2, 3.3, "Graphite")
    for k in range(4):
        a = math.pi / 4 + k * math.pi / 2
        c = (cx + 0.31 * math.cos(a), cy + 0.31 * math.sin(a))
        obox(pb, c, (-math.sin(a), math.cos(a)), (math.cos(a), math.sin(a)), 0.012, 0.01, 1.25, 3.25, "GlowCyan")
    cylinder(pb, cx, cy, 0.42, 0.46, 4.1, 4.3, "Metal")
pb.finish(shell_coll)

# ---- plants in sci-fi planters
plb = Builder("Plants")


def plant(cx, cy, seed):
    cylinder(plb, cx, cy, 0.24, 0.36, 0.0, 0.58, "Pot", seg=32, cap=False)
    cylinder(plb, cx, cy, 0.36, 0.37, 0.58, 0.62, "Pot", seg=32)
    cylinder(plb, cx, cy, 0.37, 0.372, 0.47, 0.5, "GlowCyan", seg=32, cap=False)
    disc(plb, cx, cy, 0.34, 0.6, "Graphite", seg=24)
    n, segs = 16, 7
    for i in range(n):
        a = 2 * math.pi * i / n + seed * 0.37 + 0.2 * math.sin(i * 2.3 + seed)
        length = 0.75 + 0.35 * ((i * 5 + seed) % 7) / 6
        lift = 0.55 + 0.4 * ((i * 3 + seed) % 5) / 4      # how upright the leaf starts
        d = Vector((math.cos(a), math.sin(a), 0))
        side = Vector((-math.sin(a), math.cos(a), 0))
        c = Vector((cx, cy, 0.6)) + d * 0.04
        left, right = [], []
        for k in range(segs + 1):
            t = k / segs
            w = 0.07 * math.sin(math.pi * min(1.0, 0.15 + t * 0.95)) + 0.004
            left.append(c - side * w)
            right.append(c + side * w + Vector((0, 0, 0.012 * (1 - t))))
            pitch = lift * (1 - t) * 1.3 + 0.15 - 0.9 * t * t   # upright, then arching over
            c = c + (d * math.cos(pitch) + Vector((0, 0, math.sin(pitch)))) * (length / segs)
        plb.grid([left, right], "Leaf", smooth=True)


plant(-8.85, 5.7, 1)
plant(8.85, 5.7, 2)
plant(-9.35, -3.3, 3)
plant(9.35, -1.0, 4)
plb.finish(shell_coll)


# ---------------------------------------------------------------- export
def export(coll, name):
    bpy.ops.object.select_all(action="DESELECT")
    for ob in coll.objects:
        ob.select_set(True)
    path = os.path.join(OUT_DIR, name)
    bpy.ops.export_scene.gltf(
        filepath=path,
        export_format="GLB",
        use_selection=True,
        export_apply=True,
        export_yup=True,
        export_lights=False,
        export_cameras=False,
        export_animations=False,
    )
    tris = sum(sum(len(p.vertices) - 2 for p in ob.data.polygons) for ob in coll.objects)
    print(f"ENV: wrote {path}, {tris} triangles")


os.makedirs(OUT_DIR, exist_ok=True)
export(floor_coll, "floor.glb")
export(shell_coll, "shell.glb")
