"""Build the Agent Room research station as a GLB.

Run headless from the repo root (Blender 5.2):

    /Applications/Blender.app/Contents/MacOS/Blender -b --factory-startup \
        --python tools/blender/research.py

Writes assets/models/stations/research.glb.

Where a robot goes when Claude searches or fetches the web (V2.md phase 2). It
replaces the "File reading" desk at the same spot, so it keeps that station's
footprint and conventions: metres, modelled Z-up facing Blender -Y, so in glTF
(Y up) it stands on y=0 with its front toward +Z. Origin = floor centre. The
robot stands at (0, 0.043, 0.85) facing +Z, so the strip around it stays empty
and everything the station shows sits behind it (z <= 0.5) or above it.

The look: a curved console with a projector drum, two framing pylons, and above
them a knowledge-graph hologram (glowing nodes joined by thin lines) flanked by
a search panel and a linked data panel, with thin light lines running from the
graph to each one. Amber/orange is the station's signature colour (the glow the
app gives it); cyan stays a minor accent.

Materials follow stations.py so Bevy's bloom matches: Panel (white ceramic),
PanelDark (graphite), Trim (dark titanium), GlowOrange/GlowCyan/GlowPurple
(emission strength 6), Holo and HoloAmber (translucent panels, strength 2).
One mesh, one node, no textures.
"""

import math
import os
import random

import bmesh
import bpy
from mathutils import Euler, Vector

REPO = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
OUT_DIR = os.path.join(REPO, "assets", "models", "stations")
NAME = "research"
rad = math.radians

# The console is an arc whose centre of curvature sits under the robot, so it
# wraps around it without ever reaching into the strip the robot stands in.
CY = -1.45             # arc centre, Blender y (the robot stands at y = -0.85)
RIN, ROUT = 1.15, 1.78
SPAN = 36.0            # half-angle of the console sector
TOP = 0.80             # work surface height
PYLON_X = 1.34         # the two framing pylons
PYLON_Y = 0.16
GLOBE = Vector((0.0, 0.14, 1.62))   # knowledge-graph hologram centre


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
AMBER = (1.0, 0.55, 0.12)
PURPLE = (0.55, 0.22, 1.0)
MAT_SPECS = {
    "Panel": dict(color=(0.85, 0.86, 0.88), rough=0.32),
    "PanelDark": dict(color=(0.025, 0.035, 0.06), rough=0.12, metal=0.2),
    "Trim": dict(color=(0.45, 0.48, 0.53), rough=0.35, metal=0.8),
    "GlowOrange": dict(color=ORANGE, rough=0.3, emit=ORANGE, strength=6.0),
    "GlowCyan": dict(color=CYAN, rough=0.3, emit=CYAN, strength=6.0),
    "GlowPurple": dict(color=PURPLE, rough=0.3, emit=PURPLE, strength=6.0),
    "Holo": dict(color=(0.1, 0.75, 1.0), rough=0.2, emit=(0.1, 0.7, 1.0), strength=2.0, alpha=0.35),
    "HoloAmber": dict(color=(1.0, 0.62, 0.2), rough=0.2, emit=AMBER, strength=2.0, alpha=0.35),
}


def mat(name):
    return bpy.data.materials.get(name) or make_material(name, **MAT_SPECS[name])


# ---------------------------------------------------------------- scene / grouping
PARTS = []
STACK = []


def at_frame(loc, rot):
    e = bpy.data.objects.new("grp", None)
    bpy.context.scene.collection.objects.link(e)
    e.location = loc
    e.rotation_euler = Euler([rad(a) for a in rot])
    if STACK:
        e.parent = STACK[-1]
    return e


class at:
    """Local frame for the parts made inside the block (rot in degrees, XYZ)."""

    def __init__(self, loc=(0, 0, 0), rot=(0, 0, 0)):
        self.loc, self.rot = loc, rot

    def __enter__(self):
        STACK.append(at_frame(self.loc, self.rot))
        return STACK[-1]

    def __exit__(self, *exc):
        STACK.pop()
        return False


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


def disc(center, r, z0, z1, material, segs=48, phase=0.0, bevel=0.0, bsegs=2, rot=(0, 0, 0)):
    bm = revolve([(0, z0), (r, z0), (r, z1), (0, z1)], segs, phase=phase)
    return part(bm, material, center, rot, bevel=bevel, segs=bsegs)


def ring(center, r_in, r_out, z0, z1, material, segs=48, a0=0.0, a1=360.0, phase=0.0,
         bevel=0.0, bsegs=2, rot=(0, 0, 0)):
    bm = revolve([(r_in, z0), (r_out, z0), (r_out, z1), (r_in, z1)], segs, a0, a1, phase)
    return part(bm, material, center, rot, bevel, bsegs)


def lathe(center, profile, material, segs=40, bevel=0.0, rot=(0, 0, 0)):
    """Open (r, z) profile from axis to axis, e.g. a drum or a dish."""
    return part(revolve(profile, segs, closed=False), material, center, rot, bevel)


def tube(center, r0, r1, z0, z1, material, segs=28, rot=(0, 0, 0)):
    """Open-ended shell, for the projector cone."""
    return part(revolve([(r0, z0), (r1, z1)], segs, closed=False), material, center, rot)


def sphere(center, r, material, seg=12, rings_=6, scale=(1, 1, 1)):
    bm = bmesh.new()
    bmesh.ops.create_uvsphere(bm, u_segments=seg, v_segments=rings_, radius=r)
    bmesh.ops.scale(bm, vec=scale, verts=bm.verts)
    return part(bm, material, center)


def strand(a, b, material, radius=0.005, sides=5):
    """A straight glowing filament between two points: the graph's edges and links."""
    a, b = Vector(a), Vector(b)
    d = b - a
    L = d.length
    if L < 1e-5:
        return None
    bm = bmesh.new()
    nrm = d.normalized()
    up = Vector((0, 0, 1)) if abs(nrm.z) < 0.9 else Vector((1, 0, 0))
    u = nrm.cross(up).normalized()
    v = nrm.cross(u)
    loops = []
    for p in (a, b):
        loops.append([bm.verts.new(p + radius * (math.cos(2 * math.pi * s / sides) * u
                                                 + math.sin(2 * math.pi * s / sides) * v))
                      for s in range(sides)])
    for s in range(sides):
        bm.faces.new([loops[0][s], loops[0][(s + 1) % sides], loops[1][(s + 1) % sides], loops[1][s]])
    bm.faces.new(list(reversed(loops[0])))
    bm.faces.new(loops[1])
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    return part(bm, material)


# ---------------------------------------------------------------- holo panels
def seg2(p0, p1, th, material, y=-0.008, depth=0.004):
    """A thin glowing bar between two points of the local XZ plane (a line on a holo)."""
    dx, dz = p1[0] - p0[0], p1[1] - p0[1]
    L = math.hypot(dx, dz)
    box(((p0[0] + p1[0]) / 2, y, (p0[1] + p1[1]) / 2), (L + th, depth, th), material,
        rot=(0, -math.degrees(math.atan2(dz, dx)), 0))


def outline(x0, z0, x1, z1, th, material, y=-0.008):
    for p, q in (((x0, z0), (x1, z0)), ((x1, z0), (x1, z1)), ((x1, z1), (x0, z1)), ((x0, z1), (x0, z0))):
        seg2(p, q, th, material, y)


def text_lines(x0, z0, width, n, step, th, material, rng, y=-0.008, min_frac=0.45):
    for i in range(n):
        lw = width * rng.uniform(min_frac, 1.0)
        box((x0 + lw / 2, y, z0 - i * step), (lw, 0.004, th), material)


def magnifier(center, s=0.13, material="GlowOrange"):
    """Search icon as thin emissive outlines in the local XZ plane, facing -Y."""
    th = s * 0.07
    with at(center):
        ring((-s * 0.08, 0, s * 0.08), s * 0.2, s * 0.27, -th / 2, th / 2, material, segs=20, rot=(90, 0, 0))
        seg2((s * 0.1, -s * 0.1), (s * 0.34, -s * 0.34), th * 1.8, material, 0)


def holo_content(kind, w, h, rng, f=-0.008):
    """The 'content' of a holo panel spanning x in +-w/2, z in +-h/2, facing -Y."""
    if kind == "search":
        # query field with a magnifier, then ranked result rows, the top one lit
        outline(-w * 0.44, h * 0.24, w * 0.44, h * 0.42, 0.007, "GlowOrange", f)
        magnifier((w * 0.35, f, h * 0.33), h * 0.15)
        box((-w * 0.2, f, h * 0.33), (w * 0.36, 0.004, 0.012), "GlowOrange")
        for i in range(3):
            z = h * 0.09 - i * h * 0.19
            m = "GlowOrange" if i == 0 else "GlowCyan"
            box((-w * 0.37, f, z), (h * 0.11, 0.004, h * 0.11), m)
            box((-w * 0.04, f, z + h * 0.035), (w * 0.52, 0.004, 0.013), m)
            text_lines(-w * 0.3, z - h * 0.035, w * 0.56, 1, 0, 0.007, "GlowCyan", rng, y=f)
    elif kind == "net":
        # a linked data view: a small node net wired to two value bars
        pts = [(-w * 0.3, h * 0.22), (-w * 0.06, h * 0.33), (w * 0.14, h * 0.17),
               (-w * 0.22, -h * 0.02), (w * 0.02, h * 0.02), (w * 0.3, h * 0.3)]
        for i, j in ((0, 1), (1, 2), (0, 3), (3, 4), (4, 2), (2, 5), (1, 4)):
            seg2(pts[i], pts[j], 0.005, "GlowOrange" if (i + j) % 3 else "GlowCyan", f)
        for i, p in enumerate(pts):
            r = 0.022 if i in (1, 4) else 0.014
            m = ("GlowOrange", "GlowCyan", "GlowOrange", "GlowPurple")[i % 4]
            ring((p[0], f, p[1]), 0.0, r, -0.002, 0.002, m, segs=10, rot=(90, 0, 0))
        seg2((-w * 0.44, -h * 0.2), (w * 0.44, -h * 0.2), 0.005, "GlowCyan", f)
        for i in range(5):
            bh = (0.3 + 0.62 * rng.random()) * h * 0.22
            box((-w * 0.33 + i * w * 0.17, f, -h * 0.2 + bh / 2), (w * 0.08, 0.004, bh),
                "GlowOrange" if i == 2 else "GlowCyan")
    elif kind == "card":
        box((-w * 0.08, f, h * 0.3), (w * 0.6, 0.004, 0.018), "GlowOrange")
        text_lines(-w * 0.4, h * 0.1, w * 0.8, 3, h * 0.2, 0.009, "GlowCyan", rng, y=f)


def holo_screen(center, w, h, yaw=0.0, tilt=-6.0, content="card", frame="GlowOrange", seed=1,
                bg="HoloAmber"):
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
            box((sx * (w / 2 + 0.012), -0.004, -h / 2 + 0.05), (0.008, 0.006, 0.05), "GlowCyan")
        holo_content(content, w, h, rng)


# ---------------------------------------------------------------- the station
def floor_pad(cx, cy, r, h=0.04, segs=44):
    """Lit platform where the robot stands: white rim, dark top, amber rings."""
    disc((cx, cy, 0), r, 0.0, h, "Panel", segs=segs, bevel=0.012, bsegs=1)
    disc((cx, cy, 0), r - 0.1, h, h + 0.003, "PanelDark", segs=segs)
    ring((cx, cy, 0), r - 0.1, r - 0.075, h - 0.002, h + 0.005, "GlowOrange", segs=segs)
    ring((cx, cy, 0), r - 0.012, r + 0.004, h * 0.35, h * 0.65, "GlowOrange", segs=segs)
    ring((cx, cy, 0), r * 0.42, r * 0.42 + 0.015, h, h + 0.005, "GlowCyan", segs=segs)
    for k in range(4):
        a = 45 + 90 * k
        ring((cx, cy, 0), r - 0.2, r - 0.17, h, h + 0.006, "GlowCyan", segs=6, a0=a - 12, a1=a + 12)


def build_console():
    """Curved console: three plinth panels, a worktop, and a clean rear upstand."""
    with at((0, CY, 0.0)):
        ring((0, 0, 0), RIN + 0.10, ROUT - 0.10, 0.0, TOP - 0.06, "PanelDark", segs=24, a0=-SPAN, a1=SPAN)
        # the panel sectors stop short of the graphite band's ends, so no two end
        # caps ever land on the same plane and fight
        for a0, a1 in ((-SPAN + 1.2, -13.0), (-11.0, 11.0), (13.0, SPAN - 1.2)):
            ring((0, 0, 0), RIN + 0.08, ROUT - 0.07, 0.03, TOP - 0.08, "Panel", segs=8, a0=a0, a1=a1,
                 bevel=0.014, bsegs=1)
            am = (a0 + a1) / 2
            ring((0, 0, 0), RIN + 0.066, RIN + 0.082, 0.30, 0.60, "PanelDark", segs=5, a0=am - 8, a1=am + 8)
            ring((0, 0, 0), RIN + 0.056, RIN + 0.07, 0.42, 0.45, "GlowOrange", segs=3, a0=am - 5, a1=am + 5)
        ring((0, 0, 0), RIN + 0.068, RIN + 0.08, 0.055, 0.075, "GlowOrange", segs=22, a0=-35, a1=35)
        # worktop
        ring((0, 0, 0), RIN, ROUT, TOP, TOP + 0.07, "Panel", segs=26, a0=-38, a1=38, bevel=0.022, bsegs=1)
        ring((0, 0, 0), RIN + 0.06, ROUT - 0.14, TOP + 0.07, TOP + 0.076, "PanelDark", segs=26, a0=-35, a1=35)
        ring((0, 0, 0), RIN - 0.004, RIN + 0.008, TOP + 0.018, TOP + 0.038, "GlowOrange", segs=26,
             a0=-37, a1=37)
        # rear upstand: one clean wall with a graphite inlay and an amber light line
        ring((0, 0, 0), ROUT - 0.17, ROUT - 0.03, TOP + 0.05, 1.20, "Panel", segs=26, a0=-34, a1=34,
             bevel=0.018, bsegs=1)
        ring((0, 0, 0), ROUT - 0.182, ROUT - 0.172, TOP + 0.15, 1.11, "PanelDark", segs=26, a0=-31, a1=31)
        ring((0, 0, 0), ROUT - 0.19, ROUT - 0.18, 0.975, 0.995, "GlowOrange", segs=26, a0=-30, a1=30)
        ring((0, 0, 0), ROUT - 0.18, ROUT - 0.02, 1.20, 1.25, "Trim", segs=26, a0=-34.5, a1=34.5,
             bevel=0.012, bsegs=1)


def build_projector():
    """Drum on the worktop whose lens throws the knowledge graph up and forward."""
    with at((0, CY + RIN + 0.30, TOP + 0.076)):
        disc((0, 0, 0), 0.30, 0.0, 0.045, "Trim", segs=24, bevel=0.009, bsegs=1)
        lathe((0, 0, 0), [(0, 0.045), (0.27, 0.045), (0.245, 0.215), (0.20, 0.255), (0, 0.255)], "Panel",
              segs=24, bevel=0.01)
        ring((0, 0, 0), 0.252, 0.266, 0.09, 0.115, "GlowOrange", segs=24)
        ring((0, 0, 0), 0.205, 0.222, 0.248, 0.262, "GlowOrange", segs=24)
        disc((0, 0, 0), 0.18, 0.254, 0.262, "PanelDark", segs=24)
        disc((0, 0, 0), 0.085, 0.26, 0.272, "GlowOrange", segs=16)
        for k in range(3):
            a = 180 + k * 120
            box((0.265 * math.sin(rad(a)), 0.265 * math.cos(rad(a)), 0.13), (0.055, 0.04, 0.17), "Trim",
                rot=(0, 0, -a), bevel=0.008, segs=1)


def build_input_slab():
    """Angled query pad on the left of the worktop: amber keys, a cyan read-out."""
    a = -27.0
    with at(((RIN + 0.27) * math.sin(rad(a)), CY + (RIN + 0.27) * math.cos(rad(a)), TOP + 0.076), (0, 0, -a)):
        with at((0, 0, 0), (-10, 0, 0)):
            box((0, 0, 0.014), (0.44, 0.30, 0.028), "Trim", bevel=0.006)
            box((0, 0, 0.030), (0.39, 0.25, 0.004), "PanelDark")
            rng = random.Random(3)
            for r_ in range(4):
                for c in range(6):
                    m = "GlowOrange" if (r_ + c) % 4 == 0 else ("GlowCyan" if rng.random() < 0.4 else "PanelDark")
                    box((-0.155 + c * 0.062, 0.088 - r_ * 0.058, 0.033), (0.046, 0.042, 0.004), m)
            box((0, -0.125, 0.034), (0.3, 0.02, 0.005), "GlowCyan")


def build_dish():
    """Scanning dish on the right of the worktop: the station fetches the web.

    Solid shell with a softly lit cup and no feed arm, so from the front it reads
    as a dish aimed outward, never as a fan.
    """
    a = 28.0
    with at(((RIN + 0.28) * math.sin(rad(a)), CY + (RIN + 0.28) * math.cos(rad(a)), TOP + 0.076), (0, 0, -a - 16)):
        disc((0, 0, 0), 0.09, 0.0, 0.024, "Trim", segs=16, bevel=0.006, bsegs=1)
        box((0, 0, 0.105), (0.05, 0.05, 0.17), "Trim", bevel=0.008, segs=1)
        box((0, -0.028, 0.165), (0.026, 0.012, 0.024), "GlowCyan")
        with at((0, 0, 0.215), (52, 0, 0)):
            lathe((0, 0, 0), [(0, -0.015), (0.11, 0.02), (0.185, 0.095), (0.215, 0.14)], "Panel", segs=20)
            lathe((0, 0, 0), [(0, 0.012), (0.105, 0.045), (0.175, 0.108), (0.2, 0.142)], "HoloAmber", segs=20)
            ring((0, 0, 0), 0.2, 0.218, 0.134, 0.15, "Trim", segs=20)
            ring((0, 0, 0), 0.185, 0.2, 0.128, 0.138, "GlowOrange", segs=20)
            disc((0, 0, 0), 0.045, 0.018, 0.026, "GlowOrange", segs=12)


def build_pylons():
    """Two slim emitters that frame the holograms and say where they come from."""
    for sx in (-1, 1):
        with at((sx * PYLON_X, PYLON_Y, 0.0)):
            disc((0, 0, 0), 0.155, 0.0, 0.045, "Trim", segs=18, bevel=0.01, bsegs=1)
            box((0, 0, 0.84), (0.12, 0.15, 1.6), "Panel", bevel=0.014, segs=1)
            box((-sx * 0.063, 0, 0.86), (0.012, 0.085, 1.32), "PanelDark")
            box((-sx * 0.069, 0, 0.86), (0.008, 0.045, 1.2), "GlowOrange")
            # emitter head, tipped in towards the holograms it throws
            box((0, 0, 1.68), (0.155, 0.17, 0.1), "Trim", bevel=0.01, segs=1)
            box((-sx * 0.08, 0, 1.68), (0.012, 0.09, 0.05), "GlowOrange")
            box((0, 0, 1.745), (0.1, 0.11, 0.035), "PanelDark", bevel=0.006, segs=1)
            ring((0, 0, 0), 0.07, 0.085, 1.605, 1.618, "GlowCyan", segs=16)


def build_graph():
    """The knowledge graph: glowing nodes joined by thin lines, in a lens-lit cloud."""
    rng = random.Random(17)
    n = 15
    pts = []
    for i in range(n):
        y = 1 - 2 * (i + 0.5) / n
        rr = math.sqrt(max(0.0, 1 - y * y))
        th = i * 2.39996
        p = Vector((math.cos(th) * rr * 1.15, math.sin(th) * rr * 0.72, y * 0.92)) * 0.37 * rng.uniform(0.78, 1.08)
        pts.append(GLOBE + p)
    edges = set()
    for i, p in enumerate(pts):
        for j in sorted(range(n), key=lambda k: (pts[k] - p).length)[1:3]:
            edges.add((min(i, j), max(i, j)))
    for i in (2, 9):
        edges.add((i, i))                       # two spokes to the core, drawn below
    for i, j in sorted(edges):
        if i == j:
            strand(GLOBE, pts[i], "GlowOrange", radius=0.003)
        else:
            strand(pts[i], pts[j], "GlowOrange" if (i + j) % 3 else "GlowCyan", radius=0.0035)
    for i, p in enumerate(pts):
        m = ("GlowOrange", "GlowOrange", "GlowCyan", "GlowOrange", "GlowPurple")[i % 5]
        sphere(p, 0.026 if i % 3 else 0.034, m, seg=8, rings_=4)
    sphere(GLOBE, 0.062, "GlowOrange", seg=12, rings_=6)
    sphere(GLOBE, 0.095, "HoloAmber", seg=12, rings_=6)
    ring(GLOBE, 0.42, 0.428, -0.002, 0.002, "GlowOrange", segs=30, rot=(72, 12, 0))
    ring(GLOBE, 0.36, 0.366, -0.002, 0.002, "GlowCyan", segs=30, a0=0, a1=215, rot=(-64, 26, 0))


def build_holograms():
    """Search results on the left, a linked data view on the right, cards below them."""
    left = (-0.84, 0.1, 1.46)
    right = (0.84, 0.1, 1.46)
    holo_screen(left, 0.62, 0.46, yaw=24, content="search", seed=4)
    holo_screen(right, 0.62, 0.46, yaw=-24, content="net", seed=5)
    for sx in (-1, 1):
        holo_screen((sx * 1.04, -0.26, 0.97), 0.32, 0.23, yaw=-sx * 38, content="card", seed=6 + sx)
    # thin light lines from the graph out to each panel, so the views read as linked
    for p in (left, right):
        sx = 1 if p[0] > 0 else -1
        anchor = Vector((p[0] * 0.5, p[1], p[2] + 0.3))
        strand(GLOBE + Vector((sx * 0.33, 0.0, 0.11)), anchor, "GlowOrange", radius=0.005)
        strand(anchor, (p[0] * 0.86, p[1], p[2] + 0.235), "GlowOrange", radius=0.005)
        strand((p[0] * 0.86, p[1], p[2] - 0.235), (sx * 1.04, -0.26, 1.09), "GlowCyan", radius=0.004)
    # the projection itself: three filaments from the drum's lens into the graph
    lens = Vector((0.0, CY + RIN + 0.30, TOP + 0.076 + 0.272))
    for k in range(3):
        a = 180 + 120 * k
        strand(lens + Vector((0.07 * math.sin(rad(a)), 0.07 * math.cos(rad(a)), 0.0)),
               GLOBE + Vector((0.0, 0.0, -0.1)), "GlowOrange", radius=0.0045)


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
for ob in list(bpy.data.objects):
    bpy.data.objects.remove(ob, do_unlink=True)

floor_pad(0, -0.45, 1.5)
build_console()
build_projector()
build_input_slab()
build_dish()
build_pylons()
build_graph()
build_holograms()
finish(NAME)
