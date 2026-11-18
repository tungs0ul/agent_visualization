"""Build the Agent Room teleportation platform as a GLB.

Run headless from the repo root (Blender 5.2):

    /Applications/Blender.app/Contents/MacOS/Blender -b --factory-startup \
        --python tools/blender/teleporter.py

Writes assets/models/stations/teleporter.glb.

Conventions follow stations.py: metres, modelled Z-up facing Blender -Y, so in
glTF (Y up) the model stands on y=0 with its front at +Z. Origin = floor centre
of the pad. The walkable deck is a disc of radius 1.10 m at y = 0.22; the tiers are
7 cm steps all round, with a wider stair at the front (+Z) to walk off.

Unlike the stations, the parts the app animates stay separate glTF nodes:

    Platform      static dais, ramp, grid deck, base machinery, overhead emitter
    Ring0/1/2     segmented rings, origin on the centre axis, spin about Y
    Beam          light column, radius 0.35 m, 3 m tall, own material to fade
    Orbiters      empty on the centre axis
      Orbit0..5   small gems, own origins, so they orbit (parent) and tumble

Materials: Panel (white ceramic), PanelDark (graphite), Trim (dark titanium),
Energy (every glowing strip, recoloured per agent type by the app) and Beam
(the column alone, alpha 0.3 BLEND so the app can fade it).
"""

import math
import os
import sys
from contextlib import contextmanager

import bmesh
import bpy
from mathutils import Euler, Vector

REPO = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
OUT_DIR = os.path.join(REPO, "assets", "models", "stations")
NAME = "teleporter"
rad = math.radians

DECK_R = 1.10          # walkable top radius
DECK_Z = 0.22          # walkable top height
BEAM_R = 0.35
BEAM_TOP = DECK_Z + 3.0
EMIT_Z = 3.04          # overhead emitter ring, where the column ends


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
MAT_SPECS = {
    "Panel": dict(color=(0.85, 0.86, 0.88), rough=0.32),
    "PanelDark": dict(color=(0.025, 0.035, 0.06), rough=0.12, metal=0.2),
    "Trim": dict(color=(0.3, 0.33, 0.38), rough=0.3, metal=0.9),
    "Energy": dict(color=CYAN, rough=0.3, emit=CYAN, strength=6.0),
    "Beam": dict(color=(0.35, 0.85, 1.0), rough=0.2, emit=(0.1, 0.75, 1.0), strength=2.2, alpha=0.09),
}


def mat(name):
    return bpy.data.materials.get(name) or make_material(name, **MAT_SPECS[name])


# ---------------------------------------------------------------- grouping
# Each group becomes one glTF node: a merged mesh, or an empty when it has no
# parts of its own. `origin` is the node's translation; its parts are written
# relative to it, so the app can rotate the node in place.
GROUPS = []          # [{name, parent, origin, parts}]
STACK = []           # transform frames inside the current group


def new_group(name, parent=None, origin=(0, 0, 0)):
    GROUPS.append(dict(name=name, parent=parent, origin=origin, parts=[]))
    return GROUPS[-1]


@contextmanager
def group(name, parent=None, origin=(0, 0, 0)):
    g = new_group(name, parent, origin)
    STACK.append(g)
    try:
        yield g
    finally:
        STACK.pop()


@contextmanager
def at(loc=(0, 0, 0), rot=(0, 0, 0)):
    """Local frame for the parts made inside the block (rot in degrees, XYZ)."""
    e = bpy.data.objects.new("grp", None)
    bpy.context.scene.collection.objects.link(e)
    e.location = loc
    e.rotation_euler = Euler([rad(a) for a in rot])
    prev = STACK[-1].get("frame")
    if prev is not None:
        e.parent = prev
    STACK[-1]["frame"] = e
    try:
        yield e
    finally:
        STACK[-1]["frame"] = prev


def part(bm, material, loc=(0, 0, 0), rot=(0, 0, 0), bevel=0.0, segs=2, angle=35):
    me = bpy.data.meshes.new("part")
    bm.to_mesh(me)
    bm.free()
    ob = bpy.data.objects.new("part", me)
    bpy.context.scene.collection.objects.link(ob)
    ob.location = loc
    ob.rotation_euler = Euler([rad(a) for a in rot])
    frame = STACK[-1].get("frame")
    if frame is not None:
        ob.parent = frame
    ob["mat"] = material
    if bevel > 0:
        m = ob.modifiers.new("bevel", "BEVEL")
        m.width = bevel
        m.segments = segs
        m.limit_method = "ANGLE"
        m.angle_limit = rad(angle)
    STACK[-1]["parts"].append(ob)
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


def tube(center, r0, r1, z0, z1, material, segs=32, rot=(0, 0, 0)):
    """Open-ended shell, for the light column."""
    return part(revolve([(r0, z0), (r1, z1)], segs, closed=False), material, center, rot)


def gem(center, r, material, rot=(0, 0, 0)):
    bm = bmesh.new()
    bmesh.ops.create_icosphere(bm, subdivisions=0, radius=r)
    return part(bm, material, center, rot)


def radial(a, r, z):
    """Point on the circle at angle a (0 = +Y back, 180 = -Y front)."""
    return (r * math.sin(rad(a)), r * math.cos(rad(a)), z)


# ---------------------------------------------------------------- the platform
def build_dais():
    """Three sunken tiers with cyan rim light, read as embedded in the floor."""
    tiers = ((1.45, -0.12, 0.08), (1.28, 0.08, 0.15), (DECK_R, 0.15, DECK_Z))
    for r, z0, z1 in tiers:
        disc((0, 0, 0), r, z0, z1, "Panel", segs=48, bevel=0.016, bsegs=1)
        zm = (max(z0, 0.0) + z1) / 2
        ring((0, 0, 0), r - 0.008, r + 0.005, zm - 0.009, zm + 0.009, "Energy", segs=40)
    # graphite band and vertical fins on the lowest riser: machinery, not a plain wall
    ring((0, 0, 0), 1.41, 1.432, -0.02, 0.055, "PanelDark", segs=48)
    for k in range(24):
        a = 7.5 + 15 * k
        box(radial(a, 1.437, 0.018), (0.022, 0.03, 0.055), "Trim", rot=(0, 0, -a))
    # tread seams, so the steps read as machined plate
    for k in range(8):
        a = 22.5 + 45 * k
        for r0, r1, z in ((1.30, 1.43, 0.08), (1.12, 1.26, 0.15)):
            rm = (r0 + r1) / 2
            box(radial(a, rm, z), (0.012, r1 - r0, 0.008), "PanelDark", rot=(0, 0, -a))


def build_deck():
    """Dark glossy deck with a glowing concentric grid where the robot lands."""
    disc((0, 0, 0), 1.04, DECK_Z, DECK_Z + 0.004, "PanelDark", segs=48)
    for r in (0.20, 0.36, 0.52):
        ring((0, 0, 0), r - 0.008, r + 0.008, DECK_Z + 0.002, DECK_Z + 0.008, "Energy", segs=32)
    for k in range(12):
        a = 15 + 30 * k
        box(radial(a, 0.36, DECK_Z + 0.005), (0.01, 0.42, 0.006), "Energy", rot=(0, 0, -a))
    disc((0, 0, 0), 0.085, DECK_Z + 0.002, DECK_Z + 0.01, "Energy", segs=16)
    # collar where the column leaves the deck
    ring((0, 0, 0), BEAM_R + 0.01, BEAM_R + 0.07, DECK_Z + 0.002, DECK_Z + 0.022, "Trim", segs=32, bevel=0.005,
         bsegs=1)
    ring((0, 0, 0), BEAM_R + 0.012, BEAM_R + 0.028, DECK_Z + 0.02, DECK_Z + 0.03, "Energy", segs=32)


def build_exit():
    """The front sector (glTF +Z) lit as the way off: dark treads, glowing step noses.

    The three tiers are 7 cm steps all round, so a robot can leave in any direction;
    lighting one sector keeps the silhouette a clean circle and still says where.
    """
    span, front = 19.0, 180.0
    a0, a1 = front - span, front + span
    for r, z in ((1.45, 0.08), (1.28, 0.15), (DECK_R, DECK_Z)):
        ring((0, 0, 0), r + 0.001, r + 0.016, z - 0.028, z - 0.008, "Energy", segs=10, a0=a0, a1=a1)
        ring((0, 0, 0), r - 0.012, r + 0.004, z - 0.075, z - 0.055, "Trim", segs=10, a0=a0, a1=a1)
    for r0, r1, z in ((1.30, 1.43, 0.08), (1.12, 1.26, 0.15)):
        ring((0, 0, 0), r0, r1, z + 0.001, z + 0.007, "PanelDark", segs=10, a0=a0 + 2, a1=a1 - 2)
        for s in (-1, 1):
            a = front + s * (span - 3)
            ring((0, 0, 0), r0, r1, z + 0.002, z + 0.009, "Energy", segs=2, a0=a - 1.1, a1=a + 1.1)


def build_base_machinery():
    """Clamps, emitter pods, vents and conduits round the base: mechanical, not busy."""
    for k in range(6):  # clamps straddling the lowest riser
        a = 30 + 60 * k
        with at(radial(a, 1.44, 0.0), (0, 0, -a)):
            box((0, 0, 0.045), (0.16, 0.1, 0.09), "Trim", bevel=0.009, segs=1)
            box((0, -0.045, 0.045), (0.07, 0.02, 0.028), "Energy")
            box((0, 0.02, 0.105), (0.1, 0.07, 0.022), "PanelDark")
    for k in range(4):  # emitter buttresses on the middle tier, between the clamps
        a = 45 + 90 * k
        with at(radial(a, 1.20, 0.15), (0, 0, -a)):
            box((0, 0, 0.05), (0.2, 0.16, 0.1), "Trim", bevel=0.012, segs=1)
            box((0, 0, 0.115), (0.15, 0.12, 0.035), "PanelDark", bevel=0.008, segs=1)
            box((0, -0.082, 0.055), (0.1, 0.012, 0.04), "Energy")
            box((0, 0.082, 0.055), (0.1, 0.012, 0.04), "Energy")
    for k in range(8):  # vents in the lowest tread
        a = 22.5 + 45 * k
        box(radial(a, 1.37, 0.082), (0.05, 0.1, 0.006), "PanelDark", rot=(0, 0, -a))
    for k in range(4):  # conduits climbing the risers
        a = 90 * k + 15
        with at((0, 0, 0), (0, 0, -a)):
            box((0, 1.355, 0.115), (0.05, 0.16, 0.05), "Trim", bevel=0.008)
            box((0, 1.19, 0.185), (0.05, 0.16, 0.05), "Trim", bevel=0.008)


def build_emitter():
    """Light overhead ring the column passes through."""
    with at((0, 0, EMIT_Z)):
        ring((0, 0, 0), 0.62, 0.80, 0.0, 0.085, "Trim", segs=40, bevel=0.014, bsegs=1)
        ring((0, 0, 0), 0.605, 0.625, 0.015, 0.07, "Energy", segs=40)
        ring((0, 0, 0), 0.80, 0.83, 0.03, 0.05, "Energy", segs=40)
        for k in range(4):
            a = 45 + 90 * k
            box(radial(a, 0.76, 0.095), (0.1, 0.1, 0.03), "PanelDark", rot=(0, 0, -a))
            box(radial(a, 0.57, 0.012), (0.07, 0.07, 0.03), "Energy", rot=(0, 0, -a))


# ---------------------------------------------------------------- moving parts
def build_rings():
    """Three segmented rings, each its own node with its origin on the centre axis."""
    specs = (
        ("Ring0", 0.62, 0.73, 0.238, 0.268, 5, 50, 0.0),
        ("Ring1", 0.80, 0.90, 0.230, 0.254, 3, 96, 24.0),
        ("Ring2", 0.96, 1.04, 0.224, 0.246, 6, 38, 12.0),
    )
    for name, r0, r1, z0, z1, n, span, phase in specs:
        with group(name, parent="Platform"):
            for k in range(n):
                a = phase + 360 * k / n
                segs = max(3, round(span / 9))
                ring((0, 0, 0), r0, r1, z0, z1, "Trim", segs=segs, a0=a - span / 2, a1=a + span / 2,
                     bevel=0.005, bsegs=1)
                ring((0, 0, 0), r0 + 0.016, r0 + 0.044, z1 - 0.006, z1 + 0.004, "Energy", segs=segs,
                     a0=a - span / 2 + 3, a1=a + span / 2 - 3)
                box(radial(a, (r0 + r1) / 2, z1 + 0.012), ((r1 - r0) * 0.5, 0.03, 0.03), "PanelDark",
                    rot=(0, 0, -a))


def build_beam():
    """Vertical column: a double shell so it reads as volume without a shader."""
    with group("Beam", parent="Platform"):
        tube((0, 0, 0), BEAM_R, BEAM_R * 0.86, DECK_Z, BEAM_TOP, "Beam", segs=32)
        tube((0, 0, 0), BEAM_R * 0.3, BEAM_R * 0.18, DECK_Z, BEAM_TOP, "Beam", segs=20)


def build_orbiters():
    """Six gems under one node, so the app spins the set and tumbles each one."""
    new_group("Orbiters", parent="Platform")
    spots = ((0, 0.86, 0.62), (60, 0.70, 1.28), (120, 0.92, 1.92),
             (180, 0.74, 0.95), (240, 0.88, 2.24), (300, 0.66, 1.60))
    for i, (a, r, z) in enumerate(spots):
        c = radial(a, r, z)
        # built in platform space; finish() writes the verts relative to `origin`
        with group(f"Orbit{i}", parent="Orbiters", origin=c):
            if i % 2:
                gem(c, 0.07, "Energy", rot=(20 * i, 15 * i, 0))
            else:
                box(c, (0.085, 0.085, 0.085), "Energy", rot=(45, 0, 35 + 20 * i))


# ---------------------------------------------------------------- merge + export
def finish():
    bpy.context.view_layer.update()
    dg = bpy.context.evaluated_depsgraph_get()
    built = {}
    total = 0
    for g in GROUPS:
        verts, faces, mat_idx, names = [], [], [], []
        for ob in g["parts"]:
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
        if not faces:
            ob = bpy.data.objects.new(g["name"], None)   # pure transform node
        else:
            o = g["origin"]
            mesh = bpy.data.meshes.new(g["name"])
            mesh.from_pydata([v - Vector(o) for v in verts], [], faces)
            mesh.validate()
            for n in names:
                mesh.materials.append(mat(n))
            mesh.polygons.foreach_set("material_index", mat_idx)
            mesh.shade_smooth()
            mesh.set_sharp_from_angle(angle=rad(36))
            ob = bpy.data.objects.new(g["name"], mesh)
            tris = sum(len(p.vertices) - 2 for p in mesh.polygons)
            total += tris
            print(f"  {g['name']:<10} {tris:>6} tris  materials {names}")
        bpy.context.scene.collection.objects.link(ob)
        ob.location = g["origin"]
        built[g["name"]] = ob
    for g in GROUPS:                      # scene graph after every node exists
        if g["parent"]:
            built[g["name"]].parent = built[g["parent"]]
    for ob in list(bpy.data.objects):     # drop the build frames
        if ob.name.startswith("grp") or ob.name.startswith("part"):
            bpy.data.objects.remove(ob, do_unlink=True)
    path = os.path.join(OUT_DIR, f"{NAME}.glb")
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
    print(f"TELEPORTER: wrote {path}, {total} triangles")


for mname, spec in MAT_SPECS.items():
    make_material(mname, **spec)
for ob in list(bpy.data.objects):
    bpy.data.objects.remove(ob, do_unlink=True)

with group("Platform"):
    build_dais()
    build_deck()
    build_exit()
    build_base_machinery()
    build_emitter()
build_rings()
build_beam()
build_orbiters()
finish()
