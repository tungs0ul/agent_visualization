"""Render preview PNGs of robot.glb (imports the exported file, so it checks the export).

    Blender -b --factory-startup --python tools/blender/robot_preview.py -- OUT_DIR [Anim:frame[:yaw] ...]
"""
import math
import os
import sys

import bpy
from mathutils import Vector

REPO = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
GLB = os.path.join(REPO, "assets", "models", "robot", "robot.glb")
args = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
out_dir = args[0] if args else "/tmp"
shots = args[1:] or ["Idle:0", "Walk:5"]

for ob in list(bpy.data.objects):
    bpy.data.objects.remove(ob, do_unlink=True)
bpy.ops.import_scene.gltf(filepath=GLB)
rig = next(o for o in bpy.data.objects if o.type == "ARMATURE")
scene = bpy.context.scene
scene.render.engine = "BLENDER_EEVEE"
scene.render.resolution_x = scene.render.resolution_y = 600
scene.render.film_transparent = False
scene.view_settings.view_transform = "AgX"

world = bpy.data.worlds.new("bg")
world.use_nodes = True
world.node_tree.nodes["Background"].inputs["Color"].default_value = (0.035, 0.03, 0.09, 1)
world.node_tree.nodes["Background"].inputs["Strength"].default_value = 1.0
scene.world = world


def light(name, kind, loc, energy, color=(1, 1, 1), size=1.0):
    ld = bpy.data.lights.new(name, kind)
    ld.energy = energy
    ld.color = color
    if kind == "AREA":
        ld.size = size
    ob = bpy.data.objects.new(name, ld)
    scene.collection.objects.link(ob)
    ob.location = loc
    d = Vector((0, 0, 0.5)) - Vector(loc)
    ob.rotation_euler = d.to_track_quat("-Z", "Y").to_euler()


light("key", "AREA", (1.2, -1.6, 2.0), 120, size=1.5)
light("fill", "AREA", (-1.6, -1.0, 1.0), 40, (0.7, 0.75, 1.0), size=2)
light("rim", "AREA", (0.0, 1.8, 1.6), 80, (0.6, 0.5, 1.0), size=1.0)

cam_data = bpy.data.cameras.new("cam")
cam_data.lens = 50
cam = bpy.data.objects.new("cam", cam_data)
scene.collection.objects.link(cam)
scene.camera = cam

for shot in shots:
    parts = shot.split(":")
    name, frame = parts[0], int(parts[1])
    yaw = math.radians(float(parts[2])) if len(parts) > 2 else math.radians(-20)
    act = bpy.data.actions[name]
    rig.animation_data.action = act
    if act.slots:
        rig.animation_data.action_slot = act.slots[0]
    for t in rig.animation_data.nla_tracks:
        t.mute = True
    scene.frame_set(frame)
    dist = 1.9
    target = Vector((0, 0, 0.5))
    cam.location = target + Vector((math.sin(yaw) * dist, -math.cos(yaw) * dist, 0.75))
    cam.rotation_euler = (target - cam.location).to_track_quat("-Z", "Y").to_euler()
    scene.render.filepath = os.path.join(out_dir, f"robot-{name.lower()}-{frame}.png")
    bpy.ops.render.render(write_still=True)
    print("PREVIEW:", scene.render.filepath)
