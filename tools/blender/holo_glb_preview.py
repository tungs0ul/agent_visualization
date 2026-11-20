"""Renders the exported desk-hologram models (assets/models/holo/*.glb) in a row, with
the app's hologram shader rebuilt in Blender (`assets/shaders/holo.wesl`): emissive
light mixed toward the rim colour by (1 - |n.v|^k)^e, added as light or solid. A check
of the export when the app can't be screenshotted.

    /Applications/Blender.app/Contents/MacOS/Blender -b --factory-startup \
        --python tools/blender/holo_glb_preview.py -- OUT.png [subject ...]
"""
import math
import os
import sys

import bpy
from mathutils import Vector

REPO = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
args = sys.argv[sys.argv.index("--") + 1:]
OUT = args[0]
NAMES = args[1:] or ["rocket", "planet", "sun", "station", "ship", "blackhole", "crate", "galaxy"]
FACING = ("sun", "blackhole", "galaxy")
GAIN = 0.55        # holo.rs GAIN

sc = bpy.context.scene
for ob in list(bpy.data.objects):
    bpy.data.objects.remove(ob, do_unlink=True)
sc.render.engine = "BLENDER_EEVEE"
sc.render.resolution_x, sc.render.resolution_y = 1800, 520
sc.view_settings.view_transform = "AgX"
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
gl.inputs["Strength"].default_value = 0.7
gl.inputs["Size"].default_value = 0.6
o = ng.nodes.new("NodeGroupOutput")
ng.links.new(rl.outputs["Image"], gl.inputs["Image"])
ng.links.new(gl.outputs["Image"], o.inputs[0])
sc.compositing_node_group = ng

# the hero camera's angle: 38 degrees down from the front
cam_data = bpy.data.cameras.new("c")
cam_data.type = "ORTHO"
cam_data.ortho_scale = 1.25 * len(NAMES)
cam = bpy.data.objects.new("c", cam_data)
sc.collection.objects.link(cam)
sc.camera = cam
down = math.radians(38)
view_dir = Vector((0, math.cos(down), -math.sin(down)))         # Blender: -Y is front
centre_x = 1.25 * (len(NAMES) - 1) / 2
cam.location = Vector((centre_x, 0, 0)) - view_dir * 20
cam.rotation_euler = view_dir.to_track_quat("-Z", "Y").to_euler()


def holo_material(mt):
    """The app's shader as Blender nodes, from the glTF material and its extras."""
    bsdf = mt.node_tree.nodes.get("Principled BSDF")
    if bsdf is None:
        return
    base = list(bsdf.inputs["Emission Color"].default_value)[:3]
    strength = bsdf.inputs["Emission Strength"].default_value
    tex = next((n for n in mt.node_tree.nodes if n.type == "TEX_IMAGE"), None)
    rim = mt.get("holo_rim")
    solid = bool(mt.get("holo_solid", False))
    nt = mt.node_tree
    for n in list(nt.nodes):
        if tex is None or n.name != tex.name:
            nt.nodes.remove(n)
    out = nt.nodes.new("ShaderNodeOutputMaterial")
    em = nt.nodes.new("ShaderNodeEmission")
    em.inputs["Strength"].default_value = GAIN
    b = [c * strength for c in base]
    r = list(rim[:3]) if rim else b
    k, e = (rim[3], rim[4]) if rim else (1.0, 1.0)
    # f = (1 - |n.v|^k)^e from the Layer Weight's Facing (blend 0.5: 1 - |n.v|)
    lw = nt.nodes.new("ShaderNodeLayerWeight")
    lw.inputs["Blend"].default_value = 0.5
    one_minus = nt.nodes.new("ShaderNodeMath")
    one_minus.operation = "SUBTRACT"
    one_minus.inputs[0].default_value = 1.0
    nt.links.new(lw.outputs["Facing"], one_minus.inputs[1])
    pk = nt.nodes.new("ShaderNodeMath")
    pk.operation = "POWER"
    pk.inputs[1].default_value = k
    nt.links.new(one_minus.outputs[0], pk.inputs[0])
    inv = nt.nodes.new("ShaderNodeMath")
    inv.operation = "SUBTRACT"
    inv.inputs[0].default_value = 1.0
    nt.links.new(pk.outputs[0], inv.inputs[1])
    pe = nt.nodes.new("ShaderNodeMath")
    pe.operation = "POWER"
    pe.inputs[1].default_value = e
    nt.links.new(inv.outputs[0], pe.inputs[0])
    mix = nt.nodes.new("ShaderNodeMix")
    mix.data_type = "RGBA"
    mix.inputs[6].default_value = (*b, 1)
    mix.inputs[7].default_value = (*r, 1)
    nt.links.new(pe.outputs[0], mix.inputs["Factor"])
    col = mix.outputs[2]
    if tex is not None:
        mul = nt.nodes.new("ShaderNodeMix")
        mul.data_type = "RGBA"
        mul.blend_type = "MULTIPLY"
        mul.inputs["Factor"].default_value = 1.0
        nt.links.new(col, mul.inputs[6])
        nt.links.new(tex.outputs[0], mul.inputs[7])
        col = mul.outputs[2]
    nt.links.new(col, em.inputs["Color"])
    if solid:
        nt.links.new(em.outputs[0], out.inputs["Surface"])
        mt.surface_render_method = "DITHERED"
    else:
        tr = nt.nodes.new("ShaderNodeBsdfTransparent")
        add = nt.nodes.new("ShaderNodeAddShader")
        nt.links.new(tr.outputs[0], add.inputs[0])
        nt.links.new(em.outputs[0], add.inputs[1])
        nt.links.new(add.outputs[0], out.inputs["Surface"])
        mt.surface_render_method = "BLENDED"


done = set()
for i, name in enumerate(NAMES):
    before = set(bpy.data.objects)
    bpy.ops.import_scene.gltf(filepath=f"{REPO}/assets/models/holo/{name}.glb")
    new = [ob for ob in bpy.data.objects if ob not in before]
    root = bpy.data.objects.new(f"root_{name}", None)
    sc.collection.objects.link(root)
    for ob in new:
        if ob.parent is None:
            ob.parent = root
        for slot in getattr(ob, "material_slots", []):
            if slot.material and slot.material.name not in done:
                holo_material(slot.material)
                done.add(slot.material.name)
    root.location = (1.25 * i, 0, 0)
    if name in FACING:
        # facing subjects look at the camera (holo.rs); glTF +Z is Blender -Y
        root.rotation_mode = "QUATERNION"
        root.rotation_quaternion = (-view_dir).to_track_quat("-Y", "Z")
    else:
        root.rotation_euler = (0, 0, math.radians(30))
sc.render.filepath = OUT
bpy.ops.render.render(write_still=True)
print("PREVIEW:", OUT)
