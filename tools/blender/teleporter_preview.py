"""Render preview PNGs of teleporter.glb (imports the exported file, so it checks it).

    Blender -b --factory-startup --python tools/blender/teleporter_preview.py -- OUT_DIR

Writes OUT_DIR/teleporter.png (hero three-quarter view, robot standing in the beam),
OUT_DIR/teleporter-empty.png (no robot, beam off, so the deck grid reads) and
OUT_DIR/teleporter-top.png (steep view of the rings and the ramp).
"""
import math
import os
import sys

import bpy
from mathutils import Vector

REPO = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
GLB = os.path.join(REPO, "assets", "models", "stations", "teleporter.glb")
ROBOT = os.path.join(REPO, "assets", "models", "robot", "robot.glb")
args = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
out_dir = args[0] if args else "/tmp"

DECK_Z = 0.22  # walkable top of the pad, glTF y

scene = bpy.context.scene
scene.render.engine = "BLENDER_EEVEE"
scene.render.resolution_x, scene.render.resolution_y = 900, 700
scene.view_settings.view_transform = "AgX"
world = bpy.data.worlds.new("bg")
world.use_nodes = True
world.node_tree.nodes["Background"].inputs["Color"].default_value = (0.01, 0.014, 0.04, 1)
world.node_tree.nodes["Background"].inputs["Strength"].default_value = 1.0
scene.world = world

# bloom, so the Energy strips and the beam read as they do in Bevy
try:
    ng = bpy.data.node_groups.new("comp", "CompositorNodeTree")
    ng.interface.new_socket("Image", in_out="OUTPUT", socket_type="NodeSocketColor")
    rl = ng.nodes.new("CompositorNodeRLayers")
    gl = ng.nodes.new("CompositorNodeGlare")
    gl.inputs["Type"].default_value = "Bloom"
    gl.inputs["Threshold"].default_value = 1.0
    gl.inputs["Strength"].default_value = 0.4
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
    before = set(bpy.data.objects)
    bpy.ops.import_scene.gltf(filepath=path)
    new = [o for o in bpy.data.objects if o not in before]
    for o in new:
        if o.parent is None:
            o.location = Vector(loc)
            o.rotation_mode = "XYZ"
            o.rotation_euler = (0, 0, math.radians(yaw))
            o.scale = (scale, scale, scale)
    return new


def light(name, kind, loc, energy, color=(1, 1, 1), size=1.0, target=(0, 0, 0.8)):
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
    fo = bpy.data.objects.new("floor", fl)
    scene.collection.objects.link(fo)


def camera(yaw, pitch, dist, target_z):
    cam_data = bpy.data.cameras.new("cam")
    cam_data.lens = 42
    cam = bpy.data.objects.new("cam", cam_data)
    scene.collection.objects.link(cam)
    scene.camera = cam
    yaw, pitch = math.radians(yaw), math.radians(pitch)
    target = Vector((0, 0, target_z))
    cam.location = target + dist * Vector((math.sin(yaw) * math.cos(pitch) * -1,
                                           -math.cos(yaw) * math.cos(pitch), math.sin(pitch)))
    cam.rotation_euler = (target - cam.location).to_track_quat("-Z", "Y").to_euler()


def shot(name, yaw, pitch, dist, target_z, robot=True, beam=True):
    clear()
    floor()
    import_glb(GLB)
    if not beam:
        for ob in list(bpy.data.objects):
            if ob.name.startswith("Beam"):
                bpy.data.objects.remove(ob, do_unlink=True)
    if robot:
        # glTF (x, y, z) -> Blender (x, -z, y); yaw 0 = facing +Z, where the ramp is
        import_glb(ROBOT, (0, 0, DECK_Z), 0, 1.6)
    light("key", "AREA", (3.2, -4.6, 5.0), 420, size=4)
    light("cyan", "AREA", (-4.6, -1.8, 2.2), 180, (0.3, 0.8, 1.0), size=4)
    light("purple", "AREA", (2.0, 4.4, 3.4), 240, (0.6, 0.35, 1.0), size=4)
    light("fill", "AREA", (0, -2.6, 1.2), 60, (0.5, 0.7, 1.0), size=3)
    camera(yaw, pitch, dist, target_z)
    scene.render.filepath = os.path.join(out_dir, name)
    bpy.ops.render.render(write_still=True)
    print("PREVIEW:", scene.render.filepath)


shot("teleporter.png", -24, 15, 7.6, 1.5)
shot("teleporter-empty.png", -24, 26, 5.0, 0.5, robot=False, beam=False)
shot("teleporter-top.png", 8, 52, 4.6, 0.35, robot=False)
