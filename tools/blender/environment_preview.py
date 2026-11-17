"""Render Eevee previews of the environment GLBs from the app camera (imports the exports).

    Blender -b --factory-startup --python tools/blender/environment_preview.py -- OUT_DIR [app|close|side ...]

Also drops in the robot and plain placeholder discs/columns at the workstation spots
so the free spots and the scale can be judged.
"""
import math
import os
import sys

import bpy
from mathutils import Vector

REPO = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
ENV = os.path.join(REPO, "assets", "models", "environment")
args = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
out_dir = args[0] if args else "/tmp"
shots = args[1:] or ["app", "close"]

for ob in list(bpy.data.objects):
    bpy.data.objects.remove(ob, do_unlink=True)
for name in ("floor.glb", "shell.glb"):
    bpy.ops.import_scene.gltf(filepath=os.path.join(ENV, name))
robot = os.path.join(REPO, "assets", "models", "robot", "robot.glb")
for pos in ((1.6, -2.2), (-2.8, 1.6)):
    before = set(bpy.data.objects)
    bpy.ops.import_scene.gltf(filepath=robot)
    for ob in set(bpy.data.objects) - before:
        if ob.parent is None:
            ob.location = (pos[0], pos[1], 0)
            ob.scale = (1.6,) * 3

scene = bpy.context.scene
scene.render.engine = "BLENDER_EEVEE"
scene.render.resolution_x, scene.render.resolution_y = 1280, 800
scene.view_settings.view_transform = "AgX"
scene.view_settings.look = "AgX - Punchy" if "AgX - Punchy" in [
    i.identifier for i in scene.view_settings.bl_rna.properties["look"].enum_items] else "None"
try:
    scene.eevee.use_raytracing = True
except AttributeError:
    pass

world = bpy.data.worlds.new("bg")
world.use_nodes = True
bg = world.node_tree.nodes["Background"]
bg.inputs["Color"].default_value = (0.012, 0.016, 0.045, 1)
bg.inputs["Strength"].default_value = 1.0
scene.world = world

# a far planet glow and a few stars behind the window, like the app draws
mat = bpy.data.materials.new("planet")
mat.use_nodes = True
b = mat.node_tree.nodes["Principled BSDF"]
b.inputs["Base Color"].default_value = (0.1, 0.2, 0.5, 1)
b.inputs["Emission Color"].default_value = (0.25, 0.4, 1.0, 1)
b.inputs["Emission Strength"].default_value = 0.6
bpy.ops.mesh.primitive_uv_sphere_add(radius=9, location=(10, 40, 0), segments=48, ring_count=24)
bpy.context.object.data.materials.append(mat)


def light(name, kind, loc, energy, color=(1, 1, 1), size=1.0, target=(0, 0, 0)):
    ld = bpy.data.lights.new(name, kind)
    ld.energy = energy
    ld.color = color
    if kind == "AREA":
        ld.size = size
    else:
        ld.shadow_soft_size = size
    ob = bpy.data.objects.new(name, ld)
    scene.collection.objects.link(ob)
    ob.location = loc
    d = Vector(target) - Vector(loc)
    ob.rotation_euler = d.to_track_quat("-Z", "Y").to_euler()


light("key", "AREA", (3, -6, 9), 1400, (0.85, 0.9, 1.0), size=8)
light("fill", "AREA", (-6, 2, 7), 500, (0.45, 0.4, 1.0), size=6)
light("sun", "SUN", (0, 0, 10), 0.6, (0.6, 0.7, 1.0), target=(0.3, 0.6, 0))
for loc, col, e in (((-6, 4, 3), (0.3, 0.6, 1.0), 400), ((6, 4, 3), (0.6, 0.3, 1.0), 400),
                    ((0, 0, 3), (0.3, 0.8, 1.0), 300), ((-8, -3, 2.5), (1.0, 0.5, 0.2), 150),
                    ((8, -2, 2.5), (0.3, 0.7, 1.0), 250)):
    light("pt", "POINT", loc, e, col, size=0.5)

# placeholders at the workstation spots (not part of the export)
ph = bpy.data.materials.new("placeholder")
ph.use_nodes = True
ph.node_tree.nodes["Principled BSDF"].inputs["Base Color"].default_value = (0.5, 0.5, 0.55, 1)
for (x, y), h in (((0, 0), 0.35),):
    bpy.ops.mesh.primitive_cylinder_add(radius=2.2, depth=h, location=(x, y, h / 2), vertices=64)
    bpy.context.object.data.materials.append(ph)

# bloom-ish glare in the compositor (best effort; API moved around in 4.x/5.x)
try:
    tree = bpy.data.node_groups.new("comp", "CompositorNodeTree")
    scene.compositing_node_group = tree
    rl = tree.nodes.new("CompositorNodeRLayers")
    glare = tree.nodes.new("CompositorNodeGlare")
    out = tree.nodes.new("NodeGroupOutput")
    tree.interface.new_socket("Image", in_out="OUTPUT", socket_type="NodeSocketColor")
    for key, val in (("Type", "Bloom"), ("Quality", "High"), ("Threshold", 1.0), ("Size", 0.6),
                     ("Strength", 0.6)):
        if key in glare.inputs:
            try:
                glare.inputs[key].default_value = val
            except Exception as e:  # noqa: BLE001
                print("glare", key, e)
    tree.links.new(rl.outputs["Image"], glare.inputs["Image"])
    tree.links.new(glare.outputs["Image"], out.inputs[0])
except Exception as e:  # noqa: BLE001
    print("PREVIEW: no glare:", e)

cam_data = bpy.data.cameras.new("cam")
cam_data.sensor_fit = "VERTICAL"
cam = bpy.data.objects.new("cam", cam_data)
scene.collection.objects.link(cam)
scene.camera = cam

VIEWS = {
    # app camera: Bevy (0, 9, 10.5) -> (0, 0, 0.2), 45 deg vertical fov
    "app": ((0, -10.5, 9), (0, -0.2, 0), 45),
    "close": ((-5.5, -3.5, 3.2), (2.0, 4.5, 1.4), 60),
    "side": ((7.5, -5.5, 4.0), (-4.0, 4.0, 1.0), 60),
    "back": ((0, -4.0, 2.2), (0, 6.9, 2.0), 70),
}
for shot in shots:
    loc, look, fov = VIEWS[shot]
    cam.location = loc
    cam.rotation_euler = (Vector(look) - Vector(loc)).to_track_quat("-Z", "Y").to_euler()
    cam_data.angle = math.radians(fov)
    scene.render.filepath = os.path.join(out_dir, f"env-{shot}.png")
    bpy.ops.render.render(write_still=True)
    print("PREVIEW:", scene.render.filepath)
