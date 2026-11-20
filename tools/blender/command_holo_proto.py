"""Prototype of the main agent's big hologram that replaces its console, for the
user's OK before porting.

    STATIONS_OUT=DIR Blender -b --factory-startup --python tools/blender/stations.py -- main_desk
    Blender -b --factory-startup --python tools/blender/command_holo_proto.py -- DIR/main_desk.glb OUT.png [close|room]

The dais without its console, the robot on its pad facing the viewer, and behind it a
big curved hologram wall: dark see-through glass with glowing edges, corner brackets
and scanlines, a header, the task in large text, the current step, a turning emblem
of rings, tool chips, and light beams rising to it from projectors on the dais.
"""
import math
import os
import sys

import bpy
from mathutils import Vector

REPO = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
args = sys.argv[sys.argv.index("--") + 1:]
DESK, OUT = args[0], args[1]
VIEW = args[2] if len(args) > 2 else "close"
TASK = os.environ.get("HOLO_TASK", "Refactor the parser and add tests")
STEP = os.environ.get("HOLO_STEP", "Searching for fn parse")

sc = bpy.context.scene
for ob in list(bpy.data.objects):
    bpy.data.objects.remove(ob, do_unlink=True)
sc.render.engine = "BLENDER_EEVEE"
sc.render.resolution_x, sc.render.resolution_y = 1400, 900
sc.view_settings.view_transform = "AgX"
w = bpy.data.worlds.new("bg")
w.use_nodes = True
w.node_tree.nodes["Background"].inputs["Color"].default_value = (0.006, 0.012, 0.035, 1)
sc.world = w
ng = bpy.data.node_groups.new("comp", "CompositorNodeTree")
ng.interface.new_socket("Image", in_out="OUTPUT", socket_type="NodeSocketColor")
rl, gl, o = ng.nodes.new("CompositorNodeRLayers"), ng.nodes.new("CompositorNodeGlare"), ng.nodes.new("NodeGroupOutput")
gl.inputs["Type"].default_value = "Bloom"
gl.inputs["Threshold"].default_value = 0.8
gl.inputs["Strength"].default_value = 0.8
gl.inputs["Size"].default_value = 0.6
ng.links.new(rl.outputs["Image"], gl.inputs["Image"])
ng.links.new(gl.outputs["Image"], o.inputs[0])
sc.compositing_node_group = ng

bpy.ops.import_scene.gltf(filepath=DESK)
before = set(bpy.data.objects)
bpy.ops.import_scene.gltf(filepath=f"{REPO}/assets/models/robot/robot.glb")
for ob in set(bpy.data.objects) - before:
    if ob.parent is None:
        ob.location = (0, 0, 0.36)
        ob.scale = (1.6,) * 3
fl = bpy.data.meshes.new("f")
fl.from_pydata([(-12, -12, 0), (12, -12, 0), (12, 12, 0), (-12, 12, 0)], [], [(0, 1, 2, 3)])
fm = bpy.data.materials.new("f")
fm.use_nodes = True
fm.node_tree.nodes["Principled BSDF"].inputs["Base Color"].default_value = (0.012, 0.02, 0.045, 1)
fm.node_tree.nodes["Principled BSDF"].inputs["Roughness"].default_value = 0.3
fl.materials.append(fm)
sc.collection.objects.link(bpy.data.objects.new("f", fl))


def light(loc, energy, col, size=3.0, target=(0, 0, 1.0)):
    ld = bpy.data.lights.new("l", "AREA")
    ld.energy, ld.color, ld.size = energy, col, size
    ob = bpy.data.objects.new("l", ld)
    sc.collection.objects.link(ob)
    ob.location = loc
    ob.rotation_euler = (Vector(target) - Vector(loc)).to_track_quat("-Z", "Y").to_euler()


light((0, -4, 6), 500, (0.85, 0.92, 1.0), 5)
light((-5, 1, 3), 150, (0.3, 0.7, 1.0))
light((5, 2, 3), 150, (0.6, 0.4, 1.0))


def emit(name, color, strength, alpha=None):
    mt = bpy.data.materials.new(name)
    mt.use_nodes = True
    nt = mt.node_tree
    for n in list(nt.nodes):
        nt.nodes.remove(n)
    out, em, tr = nt.nodes.new("ShaderNodeOutputMaterial"), nt.nodes.new("ShaderNodeEmission"), nt.nodes.new("ShaderNodeBsdfTransparent")
    em.inputs["Color"].default_value = (*color, 1)
    em.inputs["Strength"].default_value = strength
    if alpha is None:
        mix = nt.nodes.new("ShaderNodeAddShader")
        nt.links.new(tr.outputs[0], mix.inputs[0])
        nt.links.new(em.outputs[0], mix.inputs[1])
    else:
        mix = nt.nodes.new("ShaderNodeMixShader")
        mix.inputs[0].default_value = alpha
        nt.links.new(tr.outputs[0], mix.inputs[1])
        nt.links.new(em.outputs[0], mix.inputs[2])
    nt.links.new(mix.outputs[0], out.inputs["Surface"])
    mt.surface_render_method = "BLENDED"
    return mt


CYAN, BLUE, AMBER = (0.05, 0.75, 1.0), (0.05, 0.3, 1.0), (1.0, 0.45, 0.08)
M_GLASS = emit("glass", (0.01, 0.06, 0.2), 1.0, alpha=0.55)
M_EDGE = emit("edge", CYAN, 9.0)
M_LINE = emit("line", CYAN, 5.0)
M_DIM = emit("dim", (0.05, 0.45, 1.0), 2.0)
M_TEXT = emit("text", (0.75, 0.95, 1.0), 7.0)
M_TEXT2 = emit("text2", CYAN, 5.0)
M_AMBER = emit("amber", AMBER, 7.0)
M_BEAM = emit("beam", (0.1, 0.5, 1.0), 0.6)
M_SCAN = emit("scan", (0.05, 0.4, 1.0), 0.7)

# ---------------------------------------------------------------- the curved wall
R, A0, A1 = 1.75, -52.0, 52.0       # radius round the dais centre and the arc (deg; 0 = straight back)
Z0, Z1 = 1.3, 3.0                  # bottom and top of the glass
TOP = 0.36


def on_arc(a_deg, z, r=R):
    a = math.radians(a_deg)
    return Vector((r * math.sin(a), r * math.cos(a), z))


def arc_mesh(name, r, a0, a1, z0, z1, mat, segs=64):
    me = bpy.data.meshes.new(name)
    verts, faces = [], []
    for i in range(segs + 1):
        a = a0 + (a1 - a0) * i / segs
        verts += [tuple(on_arc(a, z0, r)), tuple(on_arc(a, z1, r))]
        if i:
            faces.append((2 * i - 2, 2 * i - 1, 2 * i + 1, 2 * i))
    me.from_pydata(verts, [], faces)
    ob = bpy.data.objects.new(name, me)
    sc.collection.objects.link(ob)
    ob.data.materials.append(mat)
    return ob


arc_mesh("glass", R, A0, A1, Z0, Z1, M_GLASS)
for z in (Z0, Z1):
    arc_mesh("edge", R - 0.005, A0, A1, z - 0.006, z + 0.006, M_LINE, 96)
for a in (A0, A1):
    arc_mesh("side", R - 0.005, a - 0.3, a + 0.3, Z0, Z1, M_LINE, 2)
# corner brackets
for a, s in ((A0, 1), (A1, -1)):
    for z, t in ((Z0, 1), (Z1, -1)):
        arc_mesh("br", R - 0.01, min(a, a + s * 5), max(a, a + s * 5), z - 0.014, z + 0.014, M_EDGE, 8)
        arc_mesh("br", R - 0.01, a - 0.6, a + 0.6, min(z, z + t * 0.16), max(z, z + t * 0.16), M_EDGE, 2)
# scanlines and a header rule
for k in range(1, 28):
    z = Z0 + (Z1 - Z0) * k / 28
    arc_mesh("scan", R - 0.004, A0 + 1, A1 - 1, z - 0.001, z + 0.001, M_SCAN if k % 7 else M_DIM, 64)
arc_mesh("rule", R - 0.008, A0 + 3, A1 - 3, Z1 - 0.2, Z1 - 0.196, M_LINE, 64)


def text(body, size, a_deg, z, mat, align="CENTER", extrude=0.0):
    cu = bpy.data.curves.new("t", "FONT")
    cu.body = body
    cu.size = size
    cu.align_x = align
    cu.align_y = "CENTER"
    ob = bpy.data.objects.new("t", cu)
    sc.collection.objects.link(ob)
    p = on_arc(a_deg, z, R - 0.03)
    ob.location = p
    # face the dais centre (the viewer side), upright
    ob.rotation_euler = (math.radians(90), 0, math.radians(-a_deg))
    ob.data.materials.append(mat)
    return ob


text("CLAUDE  ·  MAIN AGENT  ·  OPUS", 0.075, 30, Z1 - 0.12, M_TEXT2)
text("● WORKING", 0.075, -40, Z1 - 0.12, M_AMBER)
text(TASK, 0.2, 0, 2.42, M_TEXT)
text("▸ " + STEP, 0.12, 0, 2.1, M_TEXT2)
# tool chips along the bottom
for i, (label, mat) in enumerate((("Grep", M_LINE), ("Read", M_LINE), ("Edit", M_AMBER), ("Bash", M_LINE), ("Task ×3", M_LINE))):
    a = -30 + i * 15
    arc_mesh("chip", R - 0.02, a - 6, a + 6, 1.6, 1.62, mat, 6)
    arc_mesh("chip", R - 0.02, a - 6, a + 6, 1.76, 1.78, mat, 6)
    text(label, 0.07, a, 1.69, mat)

# emblem: turning rings, a core, left of the text
ec = on_arc(-38, 2.22, R - 0.25)
for r, mat, rot in ((0.26, M_EDGE, (90, 0, 0)), (0.22, M_LINE, (70, 25, 0)), (0.18, M_DIM, (110, -20, 30))):
    bpy.ops.mesh.primitive_torus_add(major_radius=r, minor_radius=0.006, major_segments=96, minor_segments=8)
    t = bpy.context.object
    t.location = ec
    t.rotation_euler = [math.radians(v) for v in rot]
    t.rotation_euler.z += math.radians(38)
    t.data.materials.append(mat)
bpy.ops.mesh.primitive_ico_sphere_add(radius=0.06, subdivisions=3)
c = bpy.context.object
c.location = ec
c.data.materials.append(emit("core", (1.0, 0.55, 0.2), 8.0))
# a mini bar chart on the right
for i in range(10):
    h = 0.06 + 0.25 * abs(math.sin(i * 1.7))
    a = 30 + i * 1.6
    arc_mesh("bar", R - 0.02, a, a + 0.9, 1.88, 1.88 + h, M_LINE if i % 4 else M_AMBER, 2)

# ---------------------------------------------------------------- projectors and beams
for a in (-40, 0, 40):
    p = on_arc(a, TOP, 1.3)
    bpy.ops.mesh.primitive_cylinder_add(vertices=24, radius=0.07, depth=0.05, location=(p.x, p.y, TOP + 0.025))
    bpy.context.object.data.materials.append(emit("proj", (0.03, 0.05, 0.08), 1.0, alpha=1.0))
    bpy.ops.mesh.primitive_cylinder_add(vertices=24, radius=0.045, depth=0.01, location=(p.x, p.y, TOP + 0.055))
    bpy.context.object.data.materials.append(M_EDGE)
    # a soft beam fanning up to the glass
    me = bpy.data.meshes.new("beam")
    top_l, top_r = on_arc(a - 16, Z0, R - 0.01), on_arc(a + 16, Z0, R - 0.01)
    me.from_pydata([(p.x - 0.04, p.y, TOP + 0.06), (p.x + 0.04, p.y, TOP + 0.06), tuple(top_r), tuple(top_l)], [], [(0, 1, 2, 3)])
    ob = bpy.data.objects.new("beam", me)
    sc.collection.objects.link(ob)
    ob.data.materials.append(M_BEAM)
# light from the wall onto the dais
light(tuple(on_arc(0, 2.1, 1.2)), 120, (0.15, 0.55, 1.0), 2.5, target=(0, -0.4, 0.4))

cam = bpy.data.objects.new("c", bpy.data.cameras.new("c"))
sc.collection.objects.link(cam)
sc.camera = cam
if VIEW == "room":
    cam.data.lens = 30
    cam.location = (0, -10.5, 8.5)
    target = Vector((0, 0.4, 1.0))
else:
    cam.data.lens = 32
    cam.location = (0, -5.2, 3.6)
    target = Vector((0, 0.6, 1.6))
cam.rotation_euler = (target - cam.location).to_track_quat("-Z", "Y").to_euler()
sc.render.filepath = OUT
bpy.ops.render.render(write_still=True)
