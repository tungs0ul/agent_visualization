"""Prototype of a crew desk's hologram screen, rendered in Blender for the user's OK
before anything is ported to the app (target crop 07).

    /Applications/Blender.app/Contents/MacOS/Blender -b --factory-startup \
        --python tools/blender/holo_screen_proto.py -- OUT.png [close|room]

Imports the desk (assets/models/stations/desk_console.glb) and builds the main screen
on it at `screens::SCREENS`: dark see-through glass with a fresnel edge glow and
corner brackets, a header plate, glowing UI bars, a 3D wireframe crate standing out
of the glass with translucent blue faces, a soft light beam rising from the projector
bar, a scan band, floating motes and blue light spilling onto the desk.
"""
import math
import os
import random
import sys

import bpy
from mathutils import Vector

REPO = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
args = sys.argv[sys.argv.index("--") + 1:]
OUT = args[0]
VIEW = args[1] if len(args) > 1 else "close"
rng = random.Random(7)

sc = bpy.context.scene
for ob in list(bpy.data.objects):
    bpy.data.objects.remove(ob, do_unlink=True)
sc.render.engine = "BLENDER_EEVEE"
sc.render.resolution_x, sc.render.resolution_y = 1400, 1000
sc.view_settings.view_transform = "AgX"
try:
    sc.view_settings.look = "AgX - Punchy"
except Exception:
    pass
w = bpy.data.worlds.new("bg")
w.use_nodes = True
w.node_tree.nodes["Background"].inputs["Color"].default_value = (0.006, 0.012, 0.035, 1)
sc.world = w
ng = bpy.data.node_groups.new("comp", "CompositorNodeTree")
ng.interface.new_socket("Image", in_out="OUTPUT", socket_type="NodeSocketColor")
rl = ng.nodes.new("CompositorNodeRLayers")
gl = ng.nodes.new("CompositorNodeGlare")
gl.inputs["Type"].default_value = "Bloom"
gl.inputs["Threshold"].default_value = 0.8
gl.inputs["Strength"].default_value = 0.9
gl.inputs["Size"].default_value = 0.6
o = ng.nodes.new("NodeGroupOutput")
ng.links.new(rl.outputs["Image"], gl.inputs["Image"])
ng.links.new(gl.outputs["Image"], o.inputs[0])
sc.compositing_node_group = ng

bpy.ops.import_scene.gltf(filepath=f"{REPO}/assets/models/stations/desk_console.glb")
if VIEW == "room":
    for x in (-3.2, 3.2):
        bpy.ops.import_scene.gltf(filepath=f"{REPO}/assets/models/stations/desk_console.glb")
        for ob in bpy.context.selected_objects:
            if ob.parent is None:
                ob.location.x += x

fl = bpy.data.meshes.new("f")
fl.from_pydata([(-12, -12, 0), (12, -12, 0), (12, 12, 0), (-12, 12, 0)], [], [(0, 1, 2, 3)])
m = bpy.data.materials.new("f")
m.use_nodes = True
b = m.node_tree.nodes["Principled BSDF"]
b.inputs["Base Color"].default_value = (0.012, 0.02, 0.045, 1)
b.inputs["Roughness"].default_value = 0.25
b.inputs["Metallic"].default_value = 0.6
fl.materials.append(m)
sc.collection.objects.link(bpy.data.objects.new("f", fl))


def light(kind, loc, energy, col, size=2.0, target=(0, 0, 0.8), spot=None):
    ld = bpy.data.lights.new("l", kind)
    ld.energy = energy
    ld.color = col
    if kind == "AREA":
        ld.size = size
    if spot:
        ld.spot_size = spot
    ob = bpy.data.objects.new("l", ld)
    sc.collection.objects.link(ob)
    ob.location = loc
    ob.rotation_euler = (Vector(target) - Vector(loc)).to_track_quat("-Z", "Y").to_euler()
    return ob


light("AREA", (0, -2.5, 4.5), 260, (0.85, 0.92, 1.0), size=4)
light("AREA", (-4, 1, 2.5), 90, (0.3, 0.7, 1.0), size=3)
light("AREA", (4, 2, 3), 90, (0.6, 0.4, 1.0), size=3)


# ---------------------------------------------------------------- materials
def emit(name, color, strength, alpha=None):
    """Emission, optionally see-through: Add(Transparent, Emission) when alpha is None
    (pure light), else Mix(Transparent, Emission) by alpha."""
    mt = bpy.data.materials.new(name)
    mt.use_nodes = True
    nt = mt.node_tree
    for n in list(nt.nodes):
        nt.nodes.remove(n)
    out = nt.nodes.new("ShaderNodeOutputMaterial")
    em = nt.nodes.new("ShaderNodeEmission")
    em.inputs["Color"].default_value = (*color, 1)
    em.inputs["Strength"].default_value = strength
    tr = nt.nodes.new("ShaderNodeBsdfTransparent")
    if alpha is None:
        add = nt.nodes.new("ShaderNodeAddShader")
        nt.links.new(tr.outputs[0], add.inputs[0])
        nt.links.new(em.outputs[0], add.inputs[1])
        nt.links.new(add.outputs[0], out.inputs["Surface"])
    else:
        mix = nt.nodes.new("ShaderNodeMixShader")
        mix.inputs[0].default_value = alpha
        nt.links.new(tr.outputs[0], mix.inputs[1])
        nt.links.new(em.outputs[0], mix.inputs[2])
        nt.links.new(mix.outputs[0], out.inputs["Surface"])
    try:
        mt.surface_render_method = "BLENDED"
    except Exception:
        pass
    return mt, nt


def glass_material():
    """Dark navy glass, see-through, with a fresnel-like glow toward its edges (from
    the UV distance to the border) and faint horizontal scanlines."""
    mt, nt = emit("glass", (0.01, 0.05, 0.16), 1.0, alpha=0.78)
    em = [n for n in nt.nodes if n.type == "EMISSION"][0]
    tc = nt.nodes.new("ShaderNodeTexCoord")
    sep = nt.nodes.new("ShaderNodeSeparateXYZ")
    nt.links.new(tc.outputs["UV"], sep.inputs[0])
    # distance to the nearest edge: min(u, 1-u, v, 1-v)
    def m(op, a, b=None, v=None):
        n = nt.nodes.new("ShaderNodeMath")
        n.operation = op
        nt.links.new(a, n.inputs[0])
        if b is not None:
            nt.links.new(b, n.inputs[1])
        elif v is not None:
            n.inputs[1].default_value = v
        return n.outputs[0]
    u, v = sep.outputs["X"], sep.outputs["Y"]
    du = m("MINIMUM", u, m("SUBTRACT", m("MULTIPLY", u, v=-1.0), v=-1.0))
    dv = m("MINIMUM", v, m("SUBTRACT", m("MULTIPLY", v, v=-1.0), v=-1.0))
    d = m("MINIMUM", du, dv)
    edge = m("POWER", m("SUBTRACT", m("MULTIPLY", m("MINIMUM", m("DIVIDE", d, v=0.08), v=1.0), v=-1.0), v=-1.0), v=3.0)
    # scanlines: 0.85..1 every 1/90 of the height
    wave = nt.nodes.new("ShaderNodeTexWave")
    wave.wave_type = "BANDS"
    wave.bands_direction = "Y"
    wave.inputs["Scale"].default_value = 45.0
    nt.links.new(tc.outputs["UV"], wave.inputs["Vector"])
    scan = m("ADD", m("MULTIPLY", wave.outputs["Fac"], v=0.25), v=0.75)
    col = nt.nodes.new("ShaderNodeMix")
    col.data_type = "RGBA"
    col.inputs["A"].default_value = (0.003, 0.014, 0.05, 1)
    col.inputs["B"].default_value = (0.1, 0.55, 1.0, 1)
    nt.links.new(edge, col.inputs["Factor"])
    mul = nt.nodes.new("ShaderNodeMix")
    mul.data_type = "RGBA"
    mul.blend_type = "MULTIPLY"
    mul.inputs["Factor"].default_value = 1.0
    nt.links.new(col.outputs["Result"], mul.inputs["A"])
    nt.links.new(scan, mul.inputs["B"])
    nt.links.new(mul.outputs["Result"], em.inputs["Color"])
    em.inputs["Strength"].default_value = 1.0
    return mt


CYAN = (0.05, 0.75, 1.0)
BLUE = (0.05, 0.3, 1.0)
WHITE = (0.75, 0.92, 1.0)
AMBER = (1.0, 0.45, 0.08)
M_GLASS = glass_material()
M_EDGE = emit("edge", CYAN, 9.0)[0]
M_LINE = emit("line", CYAN, 6.0)[0]
M_LINE_DIM = emit("line_dim", (0.05, 0.45, 1.0), 2.5)[0]
M_WHITE = emit("white", WHITE, 8.0)[0]
M_AMBER = emit("amber", AMBER, 7.0)[0]
M_FACE = emit("face", BLUE, 3.5, alpha=0.5)[0]
M_BEAM = None


def plane(name, center, size, mat, rot):
    bpy.ops.mesh.primitive_plane_add(size=1)
    p = bpy.context.object
    p.name = name
    p.scale = (size[0], size[1], 1)
    p.rotation_euler = rot
    p.location = center
    p.data.materials.append(mat)
    return p


# ---------------------------------------------------------------- the screen
# screens::SCREENS main: glTF (0, 1.21, -0.335), 1.5 x 0.75, tilt -6 -> Blender
SW, SH, TILT = 1.5, 0.75, -6.0
CEN = Vector((0, 0.335, 1.21))
ROT = (math.radians(90 + TILT), 0, 0)
basis = None


def on_screen(x, y, z=0.0):
    """A point in the screen's frame: x right, y up, z toward the viewer (metres)."""
    t = math.radians(TILT)
    right = Vector((1, 0, 0))
    up = Vector((0, math.sin(t) * -1, math.cos(t)))
    up = Vector((0, -math.sin(-t), math.cos(-t)))
    front = right.cross(up) * -1
    return CEN + right * x + up * y + front * z


def strip(a, b, z, width, mat, depth=0.004):
    """A glowing bar between two screen points."""
    pa, pb = on_screen(*a, z), on_screen(*b, z)
    mid = (pa + pb) / 2
    d = pb - pa
    bpy.ops.mesh.primitive_cube_add(size=1)
    c = bpy.context.object
    c.scale = (d.length / 2 + width / 2, width / 2, depth / 2)
    c.rotation_mode = "QUATERNION"
    up = on_screen(0, 1) - on_screen(0, 0)
    front = (on_screen(0, 0, 1) - on_screen(0, 0)).normalized()
    xa = d.normalized()
    ya = front.cross(xa).normalized()
    from mathutils import Matrix
    c.rotation_quaternion = Matrix((xa, ya, front)).transposed().to_quaternion()
    c.location = mid
    c.data.materials.append(mat)
    return c


glass = plane("glass", on_screen(0, 0, 0.0), (SW, SH), M_GLASS, ROT)
# edge frame: thin bright lines, brighter corner brackets
hw, hh = SW / 2, SH / 2
for a, b in (((-hw, hh), (hw, hh)), ((-hw, -hh), (hw, -hh)), ((-hw, -hh), (-hw, hh)), ((hw, -hh), (hw, hh))):
    strip(a, b, 0.002, 0.003, M_LINE_DIM)
for sx in (-1, 1):
    for sy in (-1, 1):
        cx, cy = sx * hw, sy * hh
        strip((cx, cy), (cx - sx * 0.09, cy), 0.004, 0.008, M_EDGE)
        strip((cx, cy), (cx, cy - sy * 0.07), 0.004, 0.008, M_EDGE)
# header: title bar, words, status pill
strip((-hw + 0.05, hh - 0.05), (-hw + 0.38, hh - 0.05), 0.003, 0.012, M_WHITE)
x = -hw + 0.05
while x < -hw + 0.5:
    wlen = rng.uniform(0.02, 0.06)
    strip((x, hh - 0.08), (x + wlen, hh - 0.08), 0.003, 0.005, M_LINE_DIM)
    x += wlen + 0.012
strip((hw - 0.2, hh - 0.05), (hw - 0.07, hh - 0.05), 0.003, 0.018, M_AMBER)
strip((-hw + 0.03, hh - 0.11), (hw - 0.03, hh - 0.11), 0.002, 0.002, M_LINE_DIM)
# side readouts: rows of bars with small dots
for side in (-1, 1):
    x0 = side * (hw - 0.05) - (0.22 if side > 0 else 0)
    for k in range(6):
        y = hh - 0.18 - k * 0.055
        L = rng.uniform(0.08, 0.2)
        strip((x0, y), (x0 + L, y), 0.003, 0.008, M_AMBER if k == 0 else M_LINE)
        strip((x0, y - 0.018), (x0 + L * 0.6, y - 0.018), 0.003, 0.004, M_LINE_DIM)
# bottom: tick scale and a mini bar chart
for i in range(40):
    xx = -hw + 0.06 + i * (SW - 0.12) / 39
    strip((xx, -hh + 0.04), (xx, -hh + 0.04 + (0.03 if i % 5 == 0 else 0.015)), 0.003, 0.003, M_LINE_DIM)
for i in range(12):
    xx = hw - 0.42 + i * 0.03
    strip((xx, -hh + 0.09), (xx, -hh + 0.09 + rng.uniform(0.02, 0.1)), 0.003, 0.016, M_LINE)

# ---------------------------------------------------------------- the 3D crate
crate_c = on_screen(0, -0.02, 0.09)          # stands out of the glass toward the viewer
cw, cd, ch = 0.4, 0.28, 0.26
UPV = (on_screen(0, 1) - on_screen(0, 0)).normalized()


def crate_box(scale, mat, wire=None, offset=(0, 0, 0)):
    """A box in the crate's frame (real size: scale applied), optionally as glowing
    edges only (wireframe of `wire` metres)."""
    bpy.ops.mesh.primitive_cube_add(size=1)
    c = bpy.context.object
    c.scale = scale
    c.location = offset
    bpy.ops.object.transform_apply(location=True, scale=True)
    c.data.materials.append(mat)
    if wire:
        mm = c.modifiers.new("w", "WIREFRAME")
        mm.thickness = wire
        mm.use_even_offset = False
    return c


parts = [
    crate_box((cw, cd, ch), M_FACE),
    crate_box((cw, cd, ch), M_EDGE, wire=0.007),
    # straps round the body, a lid seam and a label plate
    crate_box((0.035, cd + 0.006, ch + 0.006), M_WHITE, wire=0.004, offset=(-0.11, 0, 0)),
    crate_box((0.035, cd + 0.006, ch + 0.006), M_WHITE, wire=0.004, offset=(0.11, 0, 0)),
    crate_box((cw + 0.006, cd + 0.006, 0.02), M_LINE, wire=0.003, offset=(0, 0, ch * 0.32)),
    crate_box((0.1, 0.004, 0.05), M_AMBER, offset=(0, -cd / 2 - 0.004, -0.04)),
]
pivot = bpy.data.objects.new("crate", None)
sc.collection.objects.link(pivot)
for prt in parts:
    prt.parent = pivot
pivot.location = crate_c
# turned 35 deg about the screen's up axis, leaning back with the screen
pivot.rotation_mode = "ZYX"   # spin about the vertical first, then lean toward the viewer
pivot.rotation_euler = (math.radians(TILT + 16), 0, math.radians(35))

# platform rings under the crate, on the glass plane
for r, mat, wd in ((0.27, M_EDGE, 0.006), (0.34, M_LINE_DIM, 0.003), (0.2, M_LINE, 0.003)):
    bpy.ops.mesh.primitive_torus_add(major_radius=r, minor_radius=wd, major_segments=96, minor_segments=6)
    t = bpy.context.object
    t.scale = (1, 0.3, 1)
    t.rotation_euler = ROT
    t.location = on_screen(0, -0.2, 0.03)
    t.data.materials.append(mat)
# motes floating round the crate
for i in range(40):
    bpy.ops.mesh.primitive_ico_sphere_add(radius=rng.uniform(0.003, 0.007), subdivisions=1)
    p = bpy.context.object
    p.location = on_screen(rng.uniform(-0.35, 0.35), rng.uniform(-0.25, 0.25), rng.uniform(0.0, 0.15))
    p.data.materials.append(M_WHITE if i % 4 == 0 else M_LINE)

# ---------------------------------------------------------------- beam and scan band
def gradient_plane(name, center, size, rot, color, strength, fade_up=True):
    mt, nt = emit(name, color, strength)
    em = [n for n in nt.nodes if n.type == "EMISSION"][0]
    tc = nt.nodes.new("ShaderNodeTexCoord")
    sep = nt.nodes.new("ShaderNodeSeparateXYZ")
    nt.links.new(tc.outputs["UV"], sep.inputs[0])
    mr = nt.nodes.new("ShaderNodeMapRange")
    mr.inputs["From Min"].default_value, mr.inputs["From Max"].default_value = (0.0, 1.0) if fade_up else (1.0, 0.0)
    mr.inputs["To Min"].default_value, mr.inputs["To Max"].default_value = strength, 0.0
    nt.links.new(sep.outputs["Y"], mr.inputs["Value"])
    pw = nt.nodes.new("ShaderNodeMath")
    pw.operation = "POWER"
    nt.links.new(mr.outputs["Result"], pw.inputs[0])
    pw.inputs[1].default_value = 1.0
    # soft at the sides
    sx = nt.nodes.new("ShaderNodeMath")
    sx.operation = "PINGPONG"
    sx.inputs[1].default_value = 0.5
    nt.links.new(sep.outputs["X"], sx.inputs[0])
    sm = nt.nodes.new("ShaderNodeMapRange")
    sm.inputs["From Min"].default_value, sm.inputs["From Max"].default_value = 0.0, 0.12
    nt.links.new(sx.outputs[0], sm.inputs["Value"])
    mul = nt.nodes.new("ShaderNodeMath")
    mul.operation = "MULTIPLY"
    nt.links.new(pw.outputs[0], mul.inputs[0])
    nt.links.new(sm.outputs["Result"], mul.inputs[1])
    nt.links.new(mul.outputs[0], em.inputs["Strength"])
    return plane(name, center, size, mt, rot)


# beam: from the projector bar up the face of the glass, fading upward
gradient_plane("beam", on_screen(0, -hh + 0.12, 0.012), (SW * 0.98, 0.26), ROT, (0.1, 0.55, 1.0), 1.4)
# a brighter scan band a third of the way up
gradient_plane("scan", on_screen(0, 0.16, 0.006), (SW * 0.96, 0.02), ROT, (0.3, 0.8, 1.0), 0.5)
# blue light from the screen onto the desk top
light("AREA", tuple(on_screen(0, -0.1, 0.15)), 40, (0.15, 0.55, 1.0), size=1.4, target=(0, -0.2, 0.78))

# ---------------------------------------------------------------- camera
cd_ = bpy.data.cameras.new("c")
cam = bpy.data.objects.new("c", cd_)
sc.collection.objects.link(cam)
sc.camera = cam
if VIEW == "room":
    cd_.lens = 30
    cam.location = (0.0, -7.5, 6.0)
    target = Vector((0, 0.2, 0.7))
else:
    cd_.lens = 38
    cam.location = (0.35, -2.7, 2.05)
    target = Vector((0, 0.2, 1.0))
cam.rotation_euler = (target - cam.location).to_track_quat("-Z", "Y").to_euler()
sc.render.filepath = OUT
bpy.ops.render.render(write_still=True)
