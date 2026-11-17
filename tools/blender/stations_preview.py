"""Render preview PNGs of the station GLBs (imports the exported files, so it checks them).

    Blender -b --factory-startup --python tools/blender/stations_preview.py -- OUT_DIR [name ...]

Each station is shown on a dark floor with the robot (robot.glb, scaled 1.6) at its
work spot, from ~35 degrees above the front. Writes OUT_DIR/station-<name>.png.
The holograms are rendered together as OUT_DIR/station-holograms.png.
"""
import math
import os
import sys

import bpy
from mathutils import Vector

REPO = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
DIR = os.path.join(REPO, "assets", "models", "stations")
ROBOT = os.path.join(REPO, "assets", "models", "robot", "robot.glb")
args = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
out_dir = args[0] if args else "/tmp"
names = args[1:] or ["main_desk", "bookshelf", "file_reading", "lab", "writing_desk", "plant",
                     "wall_panel", "pillar", "document", "holograms"]

# robot work spot per station, glTF (x, y, z) -> Blender (x, -z, y); yaw 180 = facing the station
SPOTS = {
    "main_desk": ((0.0, 0.35, 0.0), 180),
    "bookshelf": ((0.0, 0.033, 1.3), 180),
    "file_reading": ((0.0, 0.043, 0.85), 180),
    "lab": ((0.0, 0.053, 1.0), 180),
    "writing_desk": ((0.0, 0.043, 0.85), 180),
}
HOLOS = ["holo_document", "holo_editor", "holo_terminal", "holo_cards", "holo_nodes", "holo_capsule"]

scene = bpy.context.scene
scene.render.engine = "BLENDER_EEVEE"
scene.render.resolution_x, scene.render.resolution_y = 800, 600
scene.view_settings.view_transform = "AgX"
world = bpy.data.worlds.new("bg")
world.use_nodes = True
world.node_tree.nodes["Background"].inputs["Color"].default_value = (0.01, 0.014, 0.04, 1)
world.node_tree.nodes["Background"].inputs["Strength"].default_value = 1.0
scene.world = world

# bloom so the glow strips read
try:
    ng = bpy.data.node_groups.new("comp", "CompositorNodeTree")
    ng.interface.new_socket("Image", in_out="OUTPUT", socket_type="NodeSocketColor")
    rl = ng.nodes.new("CompositorNodeRLayers")
    gl = ng.nodes.new("CompositorNodeGlare")
    gl.inputs["Type"].default_value = "Bloom"
    gl.inputs["Threshold"].default_value = 1.0
    gl.inputs["Strength"].default_value = 0.6
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


def setup(extent, target_z):
    fl = bpy.data.meshes.new("floor")
    s = 12
    fl.from_pydata([(-s, -s, 0), (s, -s, 0), (s, s, 0), (-s, s, 0)], [], [(0, 1, 2, 3)])
    m = bpy.data.materials.new("floor")
    m.use_nodes = True
    b = m.node_tree.nodes["Principled BSDF"]
    b.inputs["Base Color"].default_value = (0.02, 0.025, 0.05, 1)
    b.inputs["Roughness"].default_value = 0.25
    fl.materials.append(m)
    fo = bpy.data.objects.new("floor", fl)
    scene.collection.objects.link(fo)
    light("key", "AREA", (extent * 0.8, -extent * 1.2, extent * 1.5), 75 * extent * extent, size=extent)
    light("cyan", "AREA", (-extent * 1.3, -extent * 0.5, extent * 0.6), 30 * extent * extent, (0.3, 0.8, 1.0),
          size=extent)
    light("purple", "AREA", (extent * 0.5, extent * 1.2, extent), 38 * extent * extent, (0.6, 0.35, 1.0), size=extent)
    cam_data = bpy.data.cameras.new("cam")
    cam_data.lens = 40
    cam = bpy.data.objects.new("cam", cam_data)
    scene.collection.objects.link(cam)
    scene.camera = cam
    yaw, pitch = math.radians(-22), math.radians(35)
    dist = extent * 2.3
    target = Vector((0, 0, target_z))
    cam.location = target + dist * Vector((math.sin(yaw) * math.cos(pitch) * -1, -math.cos(yaw) * math.cos(pitch),
                                           math.sin(pitch)))
    cam.rotation_euler = (target - cam.location).to_track_quat("-Z", "Y").to_euler()


for name in names:
    clear()
    if name == "holograms":
        for i, h in enumerate(HOLOS):
            x = (i % 3 - 1) * 1.05
            import_glb(os.path.join(DIR, f"{h}.glb"), (x, (i // 3) * 1.2, (i // 3) * 0.9))
        setup(2.4, 0.7)
    else:
        objs = import_glb(os.path.join(DIR, f"{name}.glb"))
        lo = Vector((1e9,) * 3)
        hi = Vector((-1e9,) * 3)
        bpy.context.view_layer.update()
        for o in objs:
            if o.type == "MESH":
                for c in o.bound_box:
                    w = o.matrix_world @ Vector(c)
                    lo = Vector(map(min, lo, w))
                    hi = Vector(map(max, hi, w))
        extent = max(hi.x - lo.x, hi.y - lo.y, hi.z - lo.z)
        if name in SPOTS:
            (gx, gy, gz), yaw = SPOTS[name]
            import_glb(ROBOT, (gx, -gz, gy), yaw, 1.6)
        setup(max(extent, 0.8), (lo.z + hi.z) / 2 * 0.8)
    scene.render.filepath = os.path.join(out_dir, f"station-{name}.png")
    bpy.ops.render.render(write_still=True)
    print("PREVIEW:", scene.render.filepath)
