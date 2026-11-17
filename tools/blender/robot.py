"""Build the Agent Room robot: a small white chibi robot with a dark visor.

Run headless from the repo root (Blender 5.2):

    /Applications/Blender.app/Contents/MacOS/Blender -b --factory-startup \
        --python tools/blender/robot.py

Writes assets/models/robot/robot.glb with one mesh (rigidly skinned to a small
armature), five materials (Shell, Visor, Eyes, Accent, Joint) and eight looping
animations (Idle, Walk, Type, Read, Carry, Handover, Wave, Think).

Modelled facing Blender -Y with feet at the origin, so in glTF (Y up) the robot
stands on y=0 and faces +Z. "Accent" is the material the app recolours per agent.
"""

import math
import os
import sys

import bmesh
import bpy
from mathutils import Euler, Matrix, Quaternion, Vector

FPS = 24
REPO = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
OUT = os.path.join(REPO, "assets", "models", "robot", "robot.glb")
if "--" in sys.argv:
    args = sys.argv[sys.argv.index("--") + 1:]
    if args:
        OUT = os.path.abspath(args[0])

# ---------------------------------------------------------------- scene reset
for ob in list(bpy.data.objects):
    bpy.data.objects.remove(ob, do_unlink=True)
for coll in (bpy.data.meshes, bpy.data.materials, bpy.data.armatures, bpy.data.actions):
    for item in list(coll):
        coll.remove(item)
scene = bpy.context.scene
scene.render.fps = FPS
scene.render.fps_base = 1.0

# ---------------------------------------------------------------- materials
def make_material(name, color, rough, metal=0.0, emit=None, strength=0.0):
    mat = bpy.data.materials.new(name)
    mat.use_nodes = True
    bsdf = mat.node_tree.nodes.get("Principled BSDF")
    bsdf.inputs["Base Color"].default_value = (*color, 1.0)
    bsdf.inputs["Roughness"].default_value = rough
    bsdf.inputs["Metallic"].default_value = metal
    if emit is not None:
        bsdf.inputs["Emission Color"].default_value = (*emit, 1.0)
        bsdf.inputs["Emission Strength"].default_value = strength
    return mat


MATS = [
    make_material("Shell", (0.92, 0.92, 0.93), 0.28),
    make_material("Visor", (0.012, 0.016, 0.035), 0.06),
    make_material("Eyes", (0.05, 0.65, 1.0), 0.3, emit=(0.05, 0.65, 1.0), strength=4.0),
    make_material("Accent", (0.05, 0.75, 1.0), 0.3, emit=(0.05, 0.75, 1.0), strength=4.0),
    make_material("Joint", (0.55, 0.57, 0.6), 0.45),
]
MAT = {m.name: i for i, m in enumerate(MATS)}

BONES = ["root", "body", "head", "arm.L", "arm.R", "hand.L", "hand.R", "leg.L", "leg.R"]
GROUP = {name: i for i, name in enumerate(BONES)}

# ---------------------------------------------------------------- geometry helpers
bm = bmesh.new()
deform = bm.verts.layers.deform.verify()


def add_faces(verts, faces, mat, bone):
    """Append raw verts/faces to the combined bmesh with one material and bone."""
    bv = [bm.verts.new(v) for v in verts]
    for v in bv:
        v[deform][GROUP[bone]] = 1.0
    for f in faces:
        face = bm.faces.new([bv[i] for i in f])
        face.material_index = MAT[mat]
        face.smooth = True


def ellipsoid(center, radii, mat, bone, seg=24, rings=12, rot=None):
    verts, faces = [], []
    verts.append((0, 0, -1))
    for r in range(1, rings):
        el = -math.pi / 2 + math.pi * r / rings
        for s in range(seg):
            az = 2 * math.pi * s / seg
            verts.append((math.cos(el) * math.cos(az), math.cos(el) * math.sin(az), math.sin(el)))
    verts.append((0, 0, 1))
    top = len(verts) - 1
    for s in range(seg):
        faces.append((0, 1 + (s + 1) % seg, 1 + s))
    for r in range(rings - 2):
        a, b = 1 + r * seg, 1 + (r + 1) * seg
        for s in range(seg):
            s2 = (s + 1) % seg
            faces.append((a + s, a + s2, b + s2, b + s))
    last = 1 + (rings - 2) * seg
    for s in range(seg):
        faces.append((last + s, last + (s + 1) % seg, top))
    m = rot.to_matrix() if rot else Matrix.Identity(3)
    out = []
    for x, y, z in verts:
        v = m @ Vector((x * radii[0], y * radii[1], z * radii[2]))
        out.append(Vector(center) + v)
    add_faces(out, faces, mat, bone)


def torus_x(center, major, minor, mat, bone, seg=32, ring=10):
    """Torus whose axis is the X axis (for the ear rings)."""
    verts, faces = [], []
    for i in range(seg):
        a = 2 * math.pi * i / seg
        for j in range(ring):
            b = 2 * math.pi * j / ring
            rr = major + minor * math.cos(b)
            verts.append(Vector(center) + Vector((minor * math.sin(b), rr * math.cos(a), rr * math.sin(a))))
    for i in range(seg):
        for j in range(ring):
            i2, j2 = (i + 1) % seg, (j + 1) % ring
            faces.append((i * ring + j, i2 * ring + j, i2 * ring + j2, i * ring + j2))
    add_faces(verts, faces, mat, bone)


def cylinder_z(center, radius, height, mat, bone, seg=12):
    cx, cy, cz = center
    verts = []
    for k in (0, 1):
        z = cz + (k - 0.5) * height
        for s in range(seg):
            a = 2 * math.pi * s / seg
            verts.append((cx + radius * math.cos(a), cy + radius * math.sin(a), z))
    faces = [(s, (s + 1) % seg, seg + (s + 1) % seg, seg + s) for s in range(seg)]
    add_faces(verts, faces, mat, bone)


def surface_patch(center, radii, az0, el0, half_az, half_el, offset, mat, bone,
                  n=14, round_=0.75, bulge=0.0):
    """A rounded-rectangle patch lying on an ellipsoid surface, pushed out by offset.

    Angles in degrees; az measured from the front (-Y) toward +X, el upward.
    `bulge` adds extra outward offset toward the patch centre (a lens shape).
    """
    a, b, c = radii
    verts = []
    for i in range(n + 1):
        for j in range(n + 1):
            u = -1 + 2 * i / n
            v = -1 + 2 * j / n
            # square -> disk, blended for a rounded rectangle
            du = u * math.sqrt(max(0.0, 1 - v * v / 2))
            dv = v * math.sqrt(max(0.0, 1 - u * u / 2))
            pu = u + (du - u) * round_
            pv = v + (dv - v) * round_
            az = math.radians(az0 + pu * half_az)
            el = math.radians(el0 + pv * half_el)
            d = Vector((math.sin(az) * math.cos(el), -math.cos(az) * math.cos(el), math.sin(el)))
            t = 1.0 / math.sqrt((d.x / a) ** 2 + (d.y / b) ** 2 + (d.z / c) ** 2)
            p = d * t
            nrm = Vector((p.x / a ** 2, p.y / b ** 2, p.z / c ** 2)).normalized()
            w = max(0.0, 1 - (pu * pu + pv * pv) / 2)
            verts.append(Vector(center) + p + nrm * (offset + bulge * w))
    faces = []
    for i in range(n):
        for j in range(n):
            k = i * (n + 1) + j
            faces.append((k, k + n + 1, k + n + 2, k + 1))
    add_faces(verts, faces, mat, bone)


# ---------------------------------------------------------------- the robot
HEAD_C = (0.0, 0.0, 0.70)
HEAD_R = (0.27, 0.245, 0.23)
BODY_C = (0.0, 0.0, 0.33)
BODY_R = (0.165, 0.15, 0.18)

# head
ellipsoid(HEAD_C, HEAD_R, "Shell", "head", seg=48, rings=24)
surface_patch(HEAD_C, HEAD_R, 0, -3, 58, 29, 0.004, "Visor", "head", n=22, round_=0.8, bulge=0.006)
for side in (-1, 1):
    surface_patch(HEAD_C, HEAD_R, side * 19, 1, 8.5, 11.5, 0.0135, "Eyes", "head", n=8, round_=0.9, bulge=0.002)
    # ear pods: white puck, accent ring, white cap
    ex = side * (HEAD_R[0] + 0.012)
    ellipsoid((ex, 0.0, HEAD_C[2] + 0.005), (0.04, 0.088, 0.088), "Shell", "head", seg=24, rings=12)
    torus_x((side * (HEAD_R[0] + 0.036), 0.0, HEAD_C[2] + 0.005), 0.058, 0.011, "Accent", "head")
    ellipsoid((side * (HEAD_R[0] + 0.040), 0.0, HEAD_C[2] + 0.005), (0.047, 0.047, 0.012), "Joint", "head",
              seg=20, rings=8, rot=Euler((0, math.radians(90), 0)))
# antenna
cylinder_z((0, 0, 0.955), 0.008, 0.06, "Joint", "head")
ellipsoid((0, 0, 0.99), (0.022, 0.022, 0.022), "Accent", "head", seg=12, rings=6)
# neck
ellipsoid((0, 0, 0.49), (0.075, 0.075, 0.04), "Joint", "body", seg=20, rings=8)

# body
ellipsoid(BODY_C, BODY_R, "Shell", "body", seg=32, rings=16)
surface_patch(BODY_C, BODY_R, 0, 4, 32, 26, 0.003, "Joint", "body", n=12, round_=0.8)
surface_patch(BODY_C, BODY_R, 0, 10, 8, 8, 0.008, "Accent", "body", n=6, round_=1.0, bulge=0.003)
surface_patch(BODY_C, BODY_R, -9, -6, 3.5, 2.5, 0.006, "Shell", "body", n=4, round_=1.0)
surface_patch(BODY_C, BODY_R, 9, -6, 3.5, 2.5, 0.006, "Shell", "body", n=4, round_=1.0)

# arms (shoulder ball, upper arm, hand)
for side, s in (("L", 1), ("R", -1)):
    ellipsoid((s * 0.16, 0, 0.415), (0.042, 0.042, 0.042), "Joint", f"arm.{side}", seg=16, rings=8)
    ellipsoid((s * 0.19, 0, 0.335), (0.038, 0.038, 0.075), "Shell", f"arm.{side}",
              seg=16, rings=10, rot=Euler((0, math.radians(s * 18), 0)))
    ellipsoid((s * 0.212, 0, 0.245), (0.048, 0.046, 0.05), "Shell", f"hand.{side}", seg=16, rings=10)
    ellipsoid((s * 0.207, 0, 0.274), (0.034, 0.034, 0.012), "Joint", f"hand.{side}", seg=14, rings=6)

# legs and feet
for side, s in (("L", 1), ("R", -1)):
    ellipsoid((s * 0.075, 0, 0.13), (0.04, 0.04, 0.07), "Joint", f"leg.{side}", seg=14, rings=8)
    ellipsoid((s * 0.08, -0.02, 0.045), (0.062, 0.088, 0.045), "Shell", f"leg.{side}", seg=20, rings=10)

mesh = bpy.data.meshes.new("RobotMesh")
bm.to_mesh(mesh)
bm.free()
for m in MATS:
    mesh.materials.append(m)
robot = bpy.data.objects.new("Robot", mesh)
scene.collection.objects.link(robot)
for name in BONES:
    robot.vertex_groups.new(name=name)

# ---------------------------------------------------------------- armature
arm_data = bpy.data.armatures.new("RobotRig")
rig = bpy.data.objects.new("RobotRig", arm_data)
scene.collection.objects.link(rig)
bpy.context.view_layer.objects.active = rig
bpy.ops.object.mode_set(mode="EDIT")
eb = arm_data.edit_bones


def bone(name, head, tail, parent=None):
    b = eb.new(name)
    b.head, b.tail = head, tail
    b.roll = 0.0
    if parent:
        b.parent = eb[parent]
    return b


bone("root", (0, 0, 0), (0, 0, 0.1))
bone("body", (0, 0, 0.2), (0, 0, 0.47), "root")
bone("head", (0, 0, 0.49), (0, 0, 0.8), "body")
for side, s in (("L", 1), ("R", -1)):
    bone(f"arm.{side}", (s * 0.16, 0, 0.415), (s * 0.205, 0, 0.27), "body")
    bone(f"hand.{side}", (s * 0.205, 0, 0.27), (s * 0.215, 0, 0.2), f"arm.{side}")
    bone(f"leg.{side}", (s * 0.075, 0, 0.19), (s * 0.075, 0, 0.03), "root")
bpy.ops.object.mode_set(mode="OBJECT")

robot.parent = rig
mod = robot.modifiers.new("Armature", "ARMATURE")
mod.object = rig

# ---------------------------------------------------------------- animation
REST = {b.name: b.matrix_local.to_3x3() for b in arm_data.bones}
for pb in rig.pose.bones:
    pb.rotation_mode = "QUATERNION"

if rig.animation_data is None:
    rig.animation_data_create()


def tau(f, n, cycles=1.0, phase=0.0):
    return 2 * math.pi * (cycles * f / n + phase)


def bake_action(name, frames, pose_fn):
    """pose_fn(f) -> {bone: (euler_deg_xyz in character space, loc in character space)}.

    Character space: +X = robot's left, -Y = forward, +Z = up. A rotation is about
    the bone's head, in the parent's (rest-aligned) frame.
    """
    act = bpy.data.actions.new(name)
    act.use_fake_user = True
    rig.animation_data.action = act
    prev = {}
    for f in range(frames + 1):
        pose = pose_fn(f % frames)  # last frame == first frame: clean loop
        for bname in BONES:
            pb = rig.pose.bones[bname]
            rot, loc = pose.get(bname, ((0, 0, 0), (0, 0, 0)))
            R = Euler([math.radians(x) for x in rot], "XYZ").to_matrix()
            M = REST[bname]
            q = (M.inverted() @ R @ M).to_quaternion()
            if bname in prev:
                q.make_compatible(prev[bname])
            prev[bname] = q
            pb.rotation_quaternion = q
            pb.location = M.inverted() @ Vector(loc)
            pb.keyframe_insert("rotation_quaternion", frame=f)
            pb.keyframe_insert("location", frame=f)
    act.use_frame_range = True
    act.frame_start, act.frame_end = 0, frames
    act.use_cyclic = True
    track = rig.animation_data.nla_tracks.new()
    track.name = name
    strip = track.strips.new(name, 0, act)
    strip.name = name
    rig.animation_data.action = None
    return act


def walk_legs(f, n, stride=24.0, lift=0.012):
    t = tau(f, n)
    sw = math.sin(t)
    return {
        "leg.L": ((-stride * sw, 0, 0), (0, 0, lift * max(0.0, math.sin(t)))),
        "leg.R": ((stride * sw, 0, 0), (0, 0, lift * max(0.0, -math.sin(t)))),
    }, t


def idle(f):
    n, t = 48, tau(f, 48)
    bob = 0.008 * math.sin(t)
    return {
        "body": ((1.5 * math.sin(t), 0, 0), (0, 0, bob)),
        "head": ((-2.0 * math.sin(t + 0.6), 4.0 * math.sin(t * 1 + 1.2), 3.0 * math.sin(t)), (0, 0, 0)),
        "arm.L": ((0, -3 - 2 * math.sin(t), 0), (0, 0, 0)),
        "arm.R": ((0, 3 + 2 * math.sin(t), 0), (0, 0, 0)),
    }


def walk(f):
    n = 20
    pose, t = walk_legs(f, n, stride=26)
    bob = 0.018 * abs(math.sin(t))
    sw = math.sin(t)
    pose.update({
        "root": ((0, 0, 0), (0, 0, bob)),
        "body": ((3, -7 * sw, 4 * sw), (0, 0, 0)),
        "head": ((-2, 5 * sw, -3 * sw), (0, 0, 0)),
        "arm.L": ((28 * sw, -8, 0), (0, 0, 0)),
        "arm.R": ((-28 * sw, 8, 0), (0, 0, 0)),
    })
    return pose


def type_(f):
    n, t = 24, tau(f, 24)
    return {
        "body": ((6, 0, 0), (0, 0, 0.003 * math.sin(2 * t))),
        "head": ((12 + 1.5 * math.sin(t), 0, 2 * math.sin(t)), (0, 0, 0)),
        "arm.L": ((-62 + 7 * math.sin(2 * t), 0, -14), (0, 0, 0)),
        "arm.R": ((-62 + 7 * math.sin(2 * t + math.pi * 0.8), 0, 14), (0, 0, 0)),
        "hand.L": ((-10 + 10 * math.sin(2 * t + 0.5), 0, 0), (0, 0, 0)),
        "hand.R": ((-10 + 10 * math.sin(2 * t + math.pi * 0.8 + 0.5), 0, 0), (0, 0, 0)),
    }


def read(f):
    n, t = 48, tau(f, 48)
    nod = 3.0 * max(0.0, math.sin(2 * t))
    return {
        "body": ((2, 0, 0), (0, 0, 0.005 * math.sin(t))),
        "head": ((16 + nod, 0, 4 * math.sin(t)), (0, 0, 0)),
        "arm.L": ((-95 + 2 * math.sin(t), 0, -24), (0, 0, 0)),
        "arm.R": ((-95 + 2 * math.sin(t), 0, 24), (0, 0, 0)),
        "hand.L": ((-25, 0, 0), (0, 0, 0)),
        "hand.R": ((-25, 0, 0), (0, 0, 0)),
    }


def carry(f):
    n = 20
    pose, t = walk_legs(f, n, stride=18)
    sw = math.sin(t)
    pose.update({
        "root": ((0, 0, 0), (0, 0, 0.012 * abs(sw))),
        "body": ((-3, -5 * sw, 2 * sw), (0, 0, 0)),
        "head": ((-1, 3 * sw, 0), (0, 0, 0)),
        "arm.L": ((-72 + 3 * sw, 0, -18), (0, 0, 0)),
        "arm.R": ((-72 - 3 * sw, 0, 18), (0, 0, 0)),
        "hand.L": ((-15, 0, 0), (0, 0, 0)),
        "hand.R": ((-15, 0, 0), (0, 0, 0)),
    })
    return pose


def handover(f):
    n = 29
    k = 0.5 - 0.5 * math.cos(tau(f, n))  # 0 -> 1 -> 0
    k = k * k * (3 - 2 * k)
    return {
        "body": ((8 * k, 0, 0), (0, 0, 0)),
        "head": ((6 * k, 0, 0), (0, 0, 0)),
        "arm.L": ((-80 * k, 0, -15 * k), (0, 0, 0)),
        "arm.R": ((-80 * k, 0, 15 * k), (0, 0, 0)),
        "hand.L": ((-15 * k, 0, 0), (0, 0, 0)),
        "hand.R": ((-15 * k, 0, 0), (0, 0, 0)),
    }


def wave(f):
    n, t = 29, tau(f, 29)
    w = math.sin(2 * t)
    return {
        "body": ((0, -3, 3 * math.sin(t)), (0, 0, 0.005 * math.sin(2 * t))),
        "head": ((-4, 8 + 2 * w, 6), (0, 0, 0)),
        "arm.R": ((-30, 140 + 16 * w, 0), (0, 0, 0)),
        "hand.R": ((-20, 18 * w, 0), (0, 0, 0)),
        "arm.L": ((0, -4, 0), (0, 0, 0)),
    }


def think(f):
    n, t = 48, tau(f, 48)
    return {
        "body": ((3, 0, 4), (0, 0, 0.004 * math.sin(t))),
        "head": ((-8 + 2 * math.sin(t), -12 + 3 * math.sin(t), -8 + 4 * math.sin(t)), (0, 0, 0)),
        "arm.R": ((-118, 0, 35), (0, 0, 0)),
        "hand.R": ((-30 + 6 * math.sin(2 * t), 0, 0), (0, 0, 0)),
        "arm.L": ((-35, 0, -35), (0, 0, 0)),
        "hand.L": ((-20, 0, 0), (0, 0, 0)),
    }


ANIMS = [
    ("Idle", 48, idle),
    ("Walk", 20, walk),
    ("Type", 24, type_),
    ("Read", 48, read),
    ("Carry", 20, carry),
    ("Handover", 29, handover),
    ("Wave", 29, wave),
    ("Think", 48, think),
]
for name, frames, fn in ANIMS:
    bake_action(name, frames, fn)

for pb in rig.pose.bones:
    pb.rotation_quaternion = Quaternion()
    pb.location = Vector()
scene.frame_start, scene.frame_end = 0, 48

# ---------------------------------------------------------------- export
os.makedirs(os.path.dirname(OUT), exist_ok=True)
bpy.ops.export_scene.gltf(
    filepath=OUT,
    export_format="GLB",
    export_apply=True,
    export_animations=True,
    export_animation_mode="ACTIONS",
    export_nla_strips=True,
    export_force_sampling=True,
    export_frame_range=False,
    export_anim_slide_to_zero=False,
    export_optimize_animation_size=True,
    export_yup=True,
    export_lights=False,
    export_cameras=False,
)
tris = sum(len(p.vertices) - 2 for p in mesh.polygons)
print(f"ROBOT: wrote {OUT}, {tris} triangles")
