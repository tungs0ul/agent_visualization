"""Render preview PNGs of research.glb (imports the exported file, so it checks it).

    Blender -b --factory-startup --python tools/blender/research_preview.py -- OUT_DIR

Writes OUT_DIR/research.png (the app's three-quarter view, robot at its work spot),
OUT_DIR/research-front.png (straight on, eye level, so the holograms read),
OUT_DIR/research-empty.png (no robot, closer, so the console reads) and
OUT_DIR/research-beside.png (the research station next to the bookshelf, to check
they read as two different things).

The robot stands at the app's work spot (0, 0.043, 0.85) at scale 1.3, facing +Z
(the camera), as `world3d::Location::work_spot` places it.
"""
import math
import os
import sys

import bpy
from mathutils import Vector

REPO = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
DIR = os.path.join(REPO, "assets", "models", "stations")
GLB = os.path.join(DIR, "research.glb")
ROBOT = os.path.join(REPO, "assets", "models", "robot", "robot.glb")
args = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
out_dir = args[0] if args else "/tmp"

SPOT = (0.0, 0.043, 0.85)   # glTF metres, from world3d::Location::work_spot
SCALE = 1.3                 # the app's scale for a sub-agent robot

scene = bpy.context.scene
scene.render.engine = "BLENDER_EEVEE"
scene.render.resolution_x, scene.render.resolution_y = 960, 720
scene.view_settings.view_transform = "AgX"
world = bpy.data.worlds.new("bg")
world.use_nodes = True
world.node_tree.nodes["Background"].inputs["Color"].default_value = (0.01, 0.014, 0.04, 1)
world.node_tree.nodes["Background"].inputs["Strength"].default_value = 1.0
scene.world = world

# bloom, so the emissive strips read as they do under Bevy's bloom
try:
    ng = bpy.data.node_groups.new("comp", "CompositorNodeTree")
    ng.interface.new_socket("Image", in_out="OUTPUT", socket_type="NodeSocketColor")
    rl = ng.nodes.new("CompositorNodeRLayers")
    gl = ng.nodes.new("CompositorNodeGlare")
    gl.inputs["Type"].default_value = "Bloom"
    gl.inputs["Threshold"].default_value = 1.0
    gl.inputs["Strength"].default_value = 0.5
    gl.inputs["Size"].default_value = 0.5
    out = ng.nodes.new("NodeGroupOutput")
    ng.links.new(rl.outputs["Image"], gl.inputs["Image"])
    ng.links.new(gl.outputs["Image"], out.inputs[0])
    scene.compositing_node_group = ng
except Exception as exc:  # preview still works without bloom
    print("no bloom:", exc)


def clear():
    for ob in list(bpy.data.objects):
        bpy.data.objects.remove(ob, do_unlink=True)


def import_glb(path, loc=(0, 0, 0), yaw=0.0, scale=1.0):
    """loc is in glTF metres (x, y up, z towards the camera); yaw 0 faces +Z."""
    before = set(bpy.data.objects)
    bpy.ops.import_scene.gltf(filepath=path)
    new = [o for o in bpy.data.objects if o not in before]
    for o in new:
        if o.parent is None:
            o.location = Vector((loc[0], -loc[2], loc[1]))
            o.rotation_mode = "XYZ"
            o.rotation_euler = (0, 0, math.radians(yaw))
            o.scale = (scale, scale, scale)
    return new


def light(name, kind, loc, energy, color=(1, 1, 1), size=1.0, target=(0, 0, 1.0)):
    ld = bpy.data.lights.new(name, kind)
    ld.energy = energy
    ld.color = color
    if kind == "AREA":
        ld.size = size
    ob = bpy.data.objects.new(name, ld)
    scene.collection.objects.link(ob)
    ob.location = loc
    ob.rotation_euler = (Vector(target) - Vector(loc)).to_track_quat("-Z", "Y").to_euler()


def floor():
    fl = bpy.data.meshes.new("floor")
    s = 14
    fl.from_pydata([(-s, -s, 0), (s, -s, 0), (s, s, 0), (-s, s, 0)], [], [(0, 1, 2, 3)])
    m = bpy.data.materials.new("floor")
    m.use_nodes = True
    b = m.node_tree.nodes["Principled BSDF"]
    b.inputs["Base Color"].default_value = (0.02, 0.025, 0.05, 1)
    b.inputs["Roughness"].default_value = 0.25
    fl.materials.append(m)
    scene.collection.objects.link(bpy.data.objects.new("floor", fl))


def camera(yaw, pitch, dist, target, lens=42):
    cam_data = bpy.data.cameras.new("cam")
    cam_data.lens = lens
    cam = bpy.data.objects.new("cam", cam_data)
    scene.collection.objects.link(cam)
    scene.camera = cam
    yaw, pitch = math.radians(yaw), math.radians(pitch)
    target = Vector(target)
    cam.location = target + dist * Vector((math.sin(yaw) * math.cos(pitch) * -1,
                                           -math.cos(yaw) * math.cos(pitch), math.sin(pitch)))
    cam.rotation_euler = (target - cam.location).to_track_quat("-Z", "Y").to_euler()


def lights():
    light("key", "AREA", (3.0, -4.4, 5.0), 420, size=4)
    light("cyan", "AREA", (-4.6, -1.8, 2.2), 150, (0.3, 0.8, 1.0), size=4)
    light("warm", "AREA", (2.4, 3.8, 3.2), 200, (1.0, 0.6, 0.3), size=4)
    light("fill", "AREA", (0, -3.0, 1.4), 70, (0.5, 0.7, 1.0), size=3)


def shot(name, yaw, pitch, dist, target, robot=True, lens=42):
    clear()
    floor()
    import_glb(GLB)
    if robot:
        import_glb(ROBOT, SPOT, 0, SCALE)
    lights()
    camera(yaw, pitch, dist, target, lens)
    scene.render.filepath = os.path.join(out_dir, name)
    bpy.ops.render.render(write_still=True)
    print("PREVIEW:", scene.render.filepath)


shot("research.png", -22, 24, 5.6, (0, 0.3, 1.1))
shot("research-front.png", 0, 10, 5.2, (0, 0.3, 1.2))
shot("research-empty.png", -30, 20, 4.4, (0, 0.3, 1.05), robot=False)

# the two research-ish stations side by side: bookshelf left, research right
clear()
floor()
import_glb(GLB, (2.6, 0, 0))
import_glb(ROBOT, (2.6 + SPOT[0], SPOT[1], SPOT[2]), 0, SCALE)
import_glb(os.path.join(DIR, "bookshelf.glb"), (-2.9, 0, 0))
import_glb(ROBOT, (-2.9, 0.033, 1.3), 180, SCALE)
lights()
camera(-4, 18, 11.0, (0, 1.0, 0.4), 38)
scene.render.filepath = os.path.join(out_dir, "research-beside.png")
bpy.ops.render.render(write_still=True)
print("PREVIEW:", scene.render.filepath)
