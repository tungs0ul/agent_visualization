"""Prototype of a crew desk's hologram screen, rendered in Blender for the user's OK
before anything is ported to the app (target crop 07).

    /Applications/Blender.app/Contents/MacOS/Blender -b --factory-startup \
        --python tools/blender/holo_screen_proto.py -- OUT.png [close|room|screen|bake] [subject]

`bake` writes what the app shows: the screen's light only (subject, UI, beam) seen
straight on through an orthographic camera, on black, no bloom (the app adds its
own), 1024x512 for the 1.5 x 0.75 m screen. The app draws it added over its dark
glass (`screens.rs`). Bake all with `tools/blender/bake_screens.sh`.

Imports the desk (assets/models/stations/desk_console.glb) and builds the main screen
on it at `screens::SCREENS`: dark see-through glass with a fresnel edge glow and
corner brackets, a header plate, glowing UI bars, a 3D hologram of a starship subject
(crate, rocket, planet, sun, station, ship, black hole) standing out of the glass, a soft light beam rising from the projector
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
VIEW = args[1] if len(args) > 1 else "close"     # close | room | screen | bake
SUBJECT = args[2] if len(args) > 2 else "crate"   # crate rocket planet sun station ship blackhole
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
    front = right.cross(up)
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

# ---------------------------------------------------------------- the 3D subject
crate_c = on_screen(0, -0.02, 0.09)          # stands out of the glass toward the viewer
M_ORANGE = emit("orange", (1.0, 0.35, 0.03), 9.0)[0]
M_HOT = emit("hot", (1.0, 0.75, 0.4), 14.0)[0]
M_SUNFACE = emit("sunface", (1.0, 0.4, 0.05), 5.0, alpha=0.8)[0]
M_PINK = emit("pink", (1.0, 0.1, 0.7), 7.0)[0]
M_VIOLET = emit("violet", (0.35, 0.1, 1.0), 6.0)[0]
M_BLACK = emit("black", (0, 0, 0), 0.0, alpha=1.0)[0]
M_BLACK.surface_render_method = "DITHERED"   # solid: hides what is behind it
pivot = bpy.data.objects.new("subject", None)
sc.collection.objects.link(pivot)


def part(op, mat, loc=(0, 0, 0), scale=(1, 1, 1), rot=(0, 0, 0), wire=None, **kw):
    """A primitive in the subject's frame (x right, y depth, z up, metres): real size
    (scale applied), optionally as glowing edges only (a wireframe `wire` thick)."""
    op(**kw)
    c = bpy.context.object
    c.scale = scale
    c.rotation_euler = [math.radians(a) for a in rot]
    c.location = loc
    bpy.ops.object.transform_apply(location=True, rotation=True, scale=True)
    c.data.materials.append(mat)
    if wire:
        mm = c.modifiers.new("w", "WIREFRAME")
        mm.thickness = wire
        mm.use_even_offset = False
    c.parent = pivot
    return c


def both(op, loc=(0, 0, 0), scale=(1, 1, 1), rot=(0, 0, 0), face=None, edge=None, wire=0.005, **kw):
    part(op, face or M_FACE, loc, scale, rot, **kw)
    part(op, edge or M_EDGE, loc, scale, rot, wire=wire, **kw)


cube = bpy.ops.mesh.primitive_cube_add
cyl = bpy.ops.mesh.primitive_cylinder_add
cone = bpy.ops.mesh.primitive_cone_add
uvs = bpy.ops.mesh.primitive_uv_sphere_add
tor = bpy.ops.mesh.primitive_torus_add
ico = bpy.ops.mesh.primitive_ico_sphere_add


def crate():
    cw, cd, ch = 0.4, 0.28, 0.26
    both(cube, scale=(cw, cd, ch), size=1, wire=0.007)
    for x in (-0.11, 0.11):
        part(cube, M_WHITE, (x, 0, 0), (0.035, cd + 0.006, ch + 0.006), size=1, wire=0.004)
    part(cube, M_LINE, (0, 0, ch * 0.32), (cw + 0.006, cd + 0.006, 0.02), size=1, wire=0.003)
    part(cube, M_AMBER, (0, -cd / 2 - 0.004, -0.04), (0.1, 0.004, 0.05), size=1)
    return (TILT + 16, 35)


def rocket():
    # upright, nose up, three fins, a porthole ring, an orange exhaust plume
    # body and nose: smooth translucent faces, no wireframe (its vertical lines looked
    # busy, the user said); rings mark the base, a band and the nose seam instead
    part(cyl, M_FACE, (0, 0, 0.0), vertices=48, radius=0.06, depth=0.3)
    part(cone, M_FACE, (0, 0, 0.2), vertices=48, radius1=0.06, radius2=0.0, depth=0.1)
    for z in (-0.15, 0.15):
        part(tor, M_EDGE, (0, 0, z), major_radius=0.06, minor_radius=0.0025, major_segments=48)
    # three swept triangular fins, translucent with glowing outlines
    for k in range(3):
        a = k * 120 + 90
        for mat, wire in ((M_FACE, None), (M_EDGE, 0.003)):
            me = bpy.data.meshes.new("fin")
            t = 0.003
            pts = [(0.058, -0.07), (0.058, -0.15), (0.118, -0.185), (0.1, -0.12)]
            verts = [(x, y, z) for y in (-t, t) for x, z in pts]
            me.from_pydata(verts, [], [(0, 1, 2, 3), (7, 6, 5, 4), (0, 4, 5, 1), (1, 5, 6, 2), (2, 6, 7, 3), (3, 7, 4, 0)])
            ob = bpy.data.objects.new("fin", me)
            sc.collection.objects.link(ob)
            ob.rotation_euler = (0, 0, math.radians(a))
            ob.data.materials.append(mat)
            if wire:
                mm = ob.modifiers.new("w", "WIREFRAME")
                mm.thickness = wire
            ob.parent = pivot
    part(tor, M_WHITE, (0, 0, 0.07), rot=(90, 0, 0), major_radius=0.022, minor_radius=0.003)
    for z in (-0.06, 0.12):
        part(tor, M_LINE, (0, 0, z), major_radius=0.061, minor_radius=0.002, major_segments=32)
    part(cone, M_ORANGE, (0, 0, -0.2), radius1=0.0, radius2=0.045, depth=0.1, vertices=16)
    part(cone, M_HOT, (0, 0, -0.18), radius1=0.0, radius2=0.025, depth=0.06, vertices=12)
    return (TILT + 10, 25)


def radial_material(name, stops, strength, inner=0.0, outer=1.0, rings=0.0):
    """Light by distance from the object's centre (object space, metres): `stops` is
    a colour ramp over inner..outer, its alpha the brightness; optional fine rings
    (streaks in an accretion disc). Pure light, see-through elsewhere."""
    mt, nt = emit(name, (1, 1, 1), strength)
    em = [n for n in nt.nodes if n.type == "EMISSION"][0]
    tc = nt.nodes.new("ShaderNodeTexCoord")
    ln = nt.nodes.new("ShaderNodeVectorMath")
    ln.operation = "LENGTH"
    nt.links.new(tc.outputs["Object"], ln.inputs[0])
    mr = nt.nodes.new("ShaderNodeMapRange")
    mr.inputs["From Min"].default_value, mr.inputs["From Max"].default_value = inner, outer
    nt.links.new(ln.outputs["Value"], mr.inputs["Value"])
    rp = nt.nodes.new("ShaderNodeValToRGB")
    el = rp.color_ramp.elements
    el[0].position, el[0].color = stops[0]
    el[1].position, el[1].color = stops[-1]
    for pos, col in stops[1:-1]:
        e = el.new(pos)
        e.color = col
    nt.links.new(mr.outputs["Result"], rp.inputs["Fac"])
    nt.links.new(rp.outputs["Color"], em.inputs["Color"])
    amp = rp.outputs["Alpha"]
    if rings:
        wv = nt.nodes.new("ShaderNodeTexWave")
        wv.wave_type = "RINGS"
        wv.inputs["Scale"].default_value = rings
        wv.inputs["Distortion"].default_value = 1.0
        wv.inputs["Detail"].default_value = 4.0
        nt.links.new(tc.outputs["Object"], wv.inputs["Vector"])
        mx = nt.nodes.new("ShaderNodeMapRange")
        mx.inputs["To Min"].default_value = 0.55
        nt.links.new(wv.outputs["Fac"], mx.inputs["Value"])
        m2 = nt.nodes.new("ShaderNodeMath")
        m2.operation = "MULTIPLY"
        nt.links.new(amp, m2.inputs[0])
        nt.links.new(mx.outputs["Result"], m2.inputs[1])
        amp = m2.outputs[0]
    m3 = nt.nodes.new("ShaderNodeMath")
    m3.operation = "MULTIPLY"
    m3.inputs[1].default_value = strength
    nt.links.new(amp, m3.inputs[0])
    nt.links.new(m3.outputs[0], em.inputs["Strength"])
    return mt


def facing_camera(obj):
    """Turn a flat disc (made in XY) to face the viewer: its normal along the screen's front."""
    front = (on_screen(0, 0, 1) - on_screen(0, 0, 0)).normalized()
    obj.rotation_mode = "QUATERNION"
    obj.rotation_quaternion = front.to_track_quat("Z", "Y")


def sun():
    # A swirling plasma ball, darker toward its limb like a real sun, a corona of fine
    # streamers fading outward and two flame arcs on the limb (the user dropped the
    # measuring ring and its ticks).
    mt, nt = emit("photosphere", (1, 0.5, 0.1), 1.5, alpha=1.0)
    mt.surface_render_method = "DITHERED"   # solid: hides the far half of the arcs
    em = [n for n in nt.nodes if n.type == "EMISSION"][0]
    tc = nt.nodes.new("ShaderNodeTexCoord")
    nz = nt.nodes.new("ShaderNodeTexNoise")
    nz.noise_dimensions = "4D"
    for k, v in (("Scale", 9.0), ("Detail", 10.0), ("Roughness", 0.65), ("Distortion", 2.2), ("W", 4.0)):
        nz.inputs[k].default_value = v
    nt.links.new(tc.outputs["Object"], nz.inputs["Vector"])
    rp = nt.nodes.new("ShaderNodeValToRGB")
    el = rp.color_ramp.elements
    el[0].position, el[0].color = 0.3, (0.5, 0.03, 0.0, 1)
    el[1].position, el[1].color = 0.75, (1.0, 0.75, 0.15, 1)
    e = el.new(0.5)
    e.color = (1.0, 0.25, 0.0, 1)
    nt.links.new(nz.outputs["Fac"], rp.inputs["Fac"])
    # limb darkening: dimmer where the surface turns away from the viewer
    lw = nt.nodes.new("ShaderNodeLayerWeight")
    lw.inputs["Blend"].default_value = 0.35
    inv = nt.nodes.new("ShaderNodeMapRange")
    inv.inputs["To Min"].default_value, inv.inputs["To Max"].default_value = 1.0, 0.35
    nt.links.new(lw.outputs["Facing"], inv.inputs["Value"])
    mul = nt.nodes.new("ShaderNodeMix")
    mul.data_type = "RGBA"
    mul.blend_type = "MULTIPLY"
    mul.inputs["Factor"].default_value = 1.0
    nt.links.new(rp.outputs["Color"], mul.inputs["A"])
    nt.links.new(inv.outputs["Result"], mul.inputs["B"])
    nt.links.new(mul.outputs["Result"], em.inputs["Color"])
    part(uvs, mt, segments=64, ring_count=32, radius=0.12)

    # corona: a disc behind the ball, light = radial falloff x streaks by angle
    cm, cn = emit("corona", (1, 0.5, 0.12), 1.0)
    cem = [n for n in cn.nodes if n.type == "EMISSION"][0]
    ctc = cn.nodes.new("ShaderNodeTexCoord")
    sep = cn.nodes.new("ShaderNodeSeparateXYZ")
    cn.links.new(ctc.outputs["Object"], sep.inputs[0])
    ang = cn.nodes.new("ShaderNodeMath")
    ang.operation = "ARCTAN2"
    cn.links.new(sep.outputs["Y"], ang.inputs[0])
    cn.links.new(sep.outputs["X"], ang.inputs[1])
    streak = cn.nodes.new("ShaderNodeTexNoise")
    streak.noise_dimensions = "1D"
    streak.inputs["Scale"].default_value = 3.5
    streak.inputs["Detail"].default_value = 6.0
    cn.links.new(ang.outputs[0], streak.inputs["W"])
    sr = cn.nodes.new("ShaderNodeMapRange")
    sr.inputs["From Min"].default_value, sr.inputs["From Max"].default_value = 0.35, 0.75
    sr.inputs["To Min"].default_value = 0.15
    cn.links.new(streak.outputs["Fac"], sr.inputs["Value"])
    ln = cn.nodes.new("ShaderNodeVectorMath")
    ln.operation = "LENGTH"
    cn.links.new(ctc.outputs["Object"], ln.inputs[0])
    rr = cn.nodes.new("ShaderNodeMapRange")
    rr.inputs["From Min"].default_value, rr.inputs["From Max"].default_value = 0.118, 0.27
    rr.inputs["To Min"].default_value, rr.inputs["To Max"].default_value = 1.0, 0.0
    cn.links.new(ln.outputs["Value"], rr.inputs["Value"])
    pw = cn.nodes.new("ShaderNodeMath")
    pw.operation = "POWER"
    pw.inputs[1].default_value = 2.2
    cn.links.new(rr.outputs["Result"], pw.inputs[0])
    m1 = cn.nodes.new("ShaderNodeMath")
    m1.operation = "MULTIPLY"
    cn.links.new(pw.outputs[0], m1.inputs[0])
    cn.links.new(sr.outputs["Result"], m1.inputs[1])
    m2 = cn.nodes.new("ShaderNodeMath")
    m2.operation = "MULTIPLY"
    m2.inputs[1].default_value = 4.0
    cn.links.new(m1.outputs[0], m2.inputs[0])
    cn.links.new(m2.outputs[0], cem.inputs["Strength"])
    c = part(bpy.ops.mesh.primitive_circle_add, cm, vertices=96, radius=0.27, fill_type="NGON")
    facing_camera(c)
    c.location = (0, 0.03, 0)

    # two big flame arcs, their feet on the limb (a ring just behind the limb, its
    # inner half hidden by the ball)
    M_FLAME = emit("flame", (1.0, 0.22, 0.0), 5.0)[0]
    for ang_deg, r, w in ((58, 0.04, 0.0035), (205, 0.028, 0.003)):
        a = math.radians(ang_deg)
        for rad, mat, ww in ((r, M_FLAME, w),):
            loop = part(tor, mat, major_radius=rad, minor_radius=ww, major_segments=64)
            facing_camera(loop)
            loop.location = (0.125 * math.cos(a), 0.02, 0.125 * math.sin(a))

    return (TILT, 0)


def rim_face(name="rimface", strength=(2.5, 9.0), alpha=(0.45, 0.95), blend=0.25):
    """Smooth translucent blue whose edges glow cyan (by how much the surface turns
    away from the viewer): a hologram outline without wireframe lines."""
    mt, nt = emit(name, BLUE, 1.0, alpha=0.5)
    em = [n for n in nt.nodes if n.type == "EMISSION"][0]
    mix = [n for n in nt.nodes if n.type == "MIX_SHADER"][0]
    lw = nt.nodes.new("ShaderNodeLayerWeight")
    lw.inputs["Blend"].default_value = blend
    pw = nt.nodes.new("ShaderNodeMath")
    pw.operation = "POWER"
    pw.inputs[1].default_value = 2.0
    nt.links.new(lw.outputs["Facing"], pw.inputs[0])
    col = nt.nodes.new("ShaderNodeMix")
    col.data_type = "RGBA"
    col.inputs["A"].default_value = (*BLUE, 1)
    col.inputs["B"].default_value = (0.3, 0.9, 1.0, 1)
    nt.links.new(pw.outputs[0], col.inputs["Factor"])
    nt.links.new(col.outputs["Result"], em.inputs["Color"])
    st = nt.nodes.new("ShaderNodeMapRange")
    st.inputs["To Min"].default_value, st.inputs["To Max"].default_value = strength
    nt.links.new(pw.outputs[0], st.inputs["Value"])
    nt.links.new(st.outputs["Result"], em.inputs["Strength"])
    al = nt.nodes.new("ShaderNodeMapRange")
    al.inputs["To Min"].default_value, al.inputs["To Max"].default_value = alpha
    nt.links.new(pw.outputs[0], al.inputs["Value"])
    nt.links.new(al.outputs["Result"], mix.inputs[0])
    return mt


def flat(points, thick, mat, wire=None, rot_z=0.0, mirror_y=False):
    """A thin flat plate from (x, z) outline points (y is the thickness), e.g. a fin
    or a wing; with `wire`, its glowing outline only."""
    me = bpy.data.meshes.new("plate")
    n = len(points)
    verts = [(x, y, z) for y in (-thick / 2, thick / 2) for x, z in points]
    faces = [tuple(range(n)), tuple(range(2 * n - 1, n - 1, -1))]
    faces += [(i, i + n, (i + 1) % n + n, (i + 1) % n) for i in range(n)]
    me.from_pydata(verts, [], faces)
    ob = bpy.data.objects.new("plate", me)
    sc.collection.objects.link(ob)
    ob.rotation_euler = (0, 0, math.radians(rot_z))
    ob.data.materials.append(mat)
    if wire:
        mm = ob.modifiers.new("w", "WIREFRAME")
        mm.thickness = wire
    ob.parent = pivot
    return ob


PLATFORM = [True]   # a subject with its own base turns the ring platform off


def station():
    # After the user's reference: a round deck with a glowing rim, a domed hub with
    # amber windows and an amber ring at its foot, a ring of modules with small
    # screens round the edge, bridges to the hub, two antenna towers and a cargo arm,
    # seen from above at an angle. Hologram style: smooth glowing surfaces.
    PLATFORM[0] = False
    rim = rim_face("station", strength=(1.0, 7.0), alpha=(0.85, 0.98), blend=0.2)
    M_WIN = emit("window", (1.0, 0.5, 0.1), 6.0)[0]
    M_SCREEN = emit("modscreen", (0.2, 0.75, 1.0), 5.0)[0]
    # deck: a thick disc, a bright rim underneath, an inner deck ring
    part(cyl, rim, (0, 0, -0.02), vertices=96, radius=0.25, depth=0.04)
    part(tor, M_EDGE, (0, 0, -0.04), major_radius=0.25, minor_radius=0.004, major_segments=128)
    part(tor, M_LINE, (0, 0, 0.0), major_radius=0.25, minor_radius=0.0018, major_segments=128)
    part(tor, M_LINE_DIM, (0, 0, 0.001), major_radius=0.16, minor_radius=0.0015, major_segments=128)
    # hub: a short drum and a dome, panel seams, amber windows, an amber ring at its foot
    part(cyl, rim, (0, 0, 0.02), vertices=64, radius=0.115, depth=0.04)
    part(tor, M_ORANGE, (0, 0, 0.003), major_radius=0.122, minor_radius=0.003, major_segments=96)
    dome = part(uvs, rim_face("dome", strength=(1.6, 9.0), alpha=(0.9, 0.98), blend=0.3), (0, 0, 0.04),
                segments=64, ring_count=32, radius=0.105)
    for z in (0.075, 0.11):
        r = math.sqrt(max(0.105 ** 2 - (z - 0.04) ** 2, 0))
        part(tor, M_LINE_DIM, (0, 0, z), major_radius=r, minor_radius=0.0012, major_segments=64)
    for k in range(6):
        a = math.radians(k * 60 + 15)
        part(cube, M_WIN, (0.117 * math.cos(a), 0.117 * math.sin(a), 0.022), (0.03, 0.004, 0.016), (0, 0, k * 60 + 15 + 90), size=1)
    for k in range(3):
        a = math.radians(k * 120 + 40)
        part(cube, M_SCREEN, (0.085 * math.cos(a), 0.085 * math.sin(a), 0.1), (0.03, 0.004, 0.02), (35, 0, k * 120 + 40 + 90), size=1)
    # ring of modules on the deck edge, each with a little screen and a light
    for k in range(9):
        if k == 6:
            continue                         # a gap where the cargo arm leaves
        a = math.radians(k * 40)
        c = (0.205 * math.cos(a), 0.205 * math.sin(a), 0.025)
        rot = (0, 0, k * 40 + 90)
        part(cube, rim, c, (0.075, 0.05, 0.05), rot, size=1)
        part(cube, M_EDGE, c, (0.075, 0.05, 0.05), rot, size=1, wire=0.0015)
        part(cube, M_SCREEN, (0.205 * math.cos(a), 0.205 * math.sin(a), 0.052), (0.045, 0.028, 0.002), rot, size=1)
        part(ico, M_AMBER if k % 2 else M_WHITE, (0.235 * math.cos(a), 0.235 * math.sin(a), 0.03), radius=0.004, subdivisions=1)
    # bridges from the hub to the ring
    for k in (1, 4, 7):
        a = math.radians(k * 40)
        part(cube, rim, (0.16 * math.cos(a), 0.16 * math.sin(a), 0.012), (0.07, 0.022, 0.016), (0, 0, k * 40), size=1)
    # towers: a tall stacked one with an antenna mast, a cluster of masts
    tx, ty = 0.09, 0.17
    for z, r, h in ((0.07, 0.022, 0.08), (0.125, 0.03, 0.03), (0.155, 0.016, 0.04)):
        part(cyl, rim, (tx, ty, z), vertices=24, radius=r, depth=h)
        part(tor, M_EDGE, (tx, ty, z + h / 2), major_radius=r, minor_radius=0.0013, major_segments=24)
    for dx, dy, h in ((0, 0, 0.09), (0.012, 0.004, 0.06)):
        part(cyl, M_LINE, (tx + dx, ty + dy, 0.2 + h / 2), vertices=6, radius=0.0015, depth=h)
    for dx, dy, h in ((-0.13, 0.09, 0.12), (-0.115, 0.1, 0.09), (-0.14, 0.105, 0.15), (-0.125, 0.115, 0.1)):
        part(cyl, M_LINE, (dx, dy, 0.05 + h / 2), vertices=6, radius=0.002, depth=h)
        part(ico, M_WHITE, (dx, dy, 0.05 + h), radius=0.004, subdivisions=1)
    # cargo arm: a long boom out past the deck with lights along it
    a = math.radians(6 * 40)
    for t in (0.2, 0.27, 0.34):
        part(cube, rim, (t * math.cos(a), t * math.sin(a), 0.02), (0.07, 0.045, 0.03), (0, 0, 240), size=1)
        part(cube, M_EDGE, (t * math.cos(a), t * math.sin(a), 0.02), (0.07, 0.045, 0.03), (0, 0, 240), size=1, wire=0.0012)
        part(cube, emit("cargo_green", (0.2, 1.0, 0.4), 5.0)[0] if t == 0.34 else M_WIN,
             ((t) * math.cos(a), (t) * math.sin(a), 0.037), (0.05, 0.006, 0.003), (0, 0, 240), size=1)
    pivot.scale = (1.05, 1.05, 1.05)
    return (TILT + 38, 25)


def iss():
    # A modular orbital station, with the detail that makes real ones read as real:
    # a lattice truss, solar wings of dark cells on a central mast, modules of
    # different sizes with side branches, a capsule docked at the front, ribbed
    # radiators. Mostly dim lines; bright edges only on a few key outlines.
    rim = rim_face("station")
    M_CELL = emit("cells", (0.02, 0.12, 0.45), 1.6, alpha=0.7)[0]
    # main stack along y (front to back): modules of different radii, nodes between
    for y, L, r in ((-0.13, 0.1, 0.028), (0.0, 0.12, 0.034), (0.15, 0.12, 0.03)):
        part(cyl, rim, (0, y, 0), rot=(90, 0, 0), vertices=40, radius=r, depth=L)
        for e in (-1, 1):
            part(tor, M_EDGE, (0, y + e * L / 2, 0), rot=(90, 0, 0), major_radius=r, minor_radius=0.0016, major_segments=40)
        part(tor, M_LINE_DIM, (0, y, 0), rot=(90, 0, 0), major_radius=r + 0.001, minor_radius=0.001, major_segments=40)
    for y in (-0.065, 0.075):
        part(uvs, rim, (0, y, 0), segments=32, ring_count=16, radius=0.04)
    # side branches on the front node, a lab module up on the back node
    for sx in (-1, 1):
        part(cyl, rim, (sx * 0.075, -0.065, 0), rot=(0, 90, 0), vertices=32, radius=0.022, depth=0.07)
        part(tor, M_EDGE, (sx * 0.11, -0.065, 0), rot=(0, 90, 0), major_radius=0.022, minor_radius=0.0015, major_segments=32)
    part(cyl, rim, (0, 0.075, 0.07), vertices=32, radius=0.026, depth=0.08)
    part(tor, M_EDGE, (0, 0.075, 0.11), major_radius=0.026, minor_radius=0.0015, major_segments=32)
    # a capsule docked at the front: a short cylinder and a cone heat shield
    part(cyl, rim, (0, -0.205, 0), rot=(90, 0, 0), vertices=32, radius=0.02, depth=0.04)
    part(cone, rim, (0, -0.24, 0), rot=(90, 0, 0), vertices=32, radius1=0.0, radius2=0.026, depth=0.03)
    # lattice truss across (x): two rails and zigzag struts
    L = 0.66
    for z in (-0.008, 0.008):
        _bar((-L / 2, 0.02, z), (L / 2, 0.02, z), 0.002, M_LINE)
    n = 22
    for i in range(n):
        x0, x1 = -L / 2 + i * L / n, -L / 2 + (i + 1) * L / n
        z0, z1 = (-0.008, 0.008) if i % 2 == 0 else (0.008, -0.008)
        _bar((x0, 0.02, z0), (x1, 0.02, z1), 0.0012, M_LINE_DIM)
    # solar wings: a mast with a blanket of cells either side, two up and two down
    for x in (-0.31, -0.23, 0.23, 0.31):
        for sz in (-1, 1):
            zc = sz * 0.14
            _bar((x, 0.02, sz * 0.012), (x, 0.02, sz * 0.26), 0.0015, M_LINE)
            for side in (-1, 1):
                cx = x + side * 0.016
                part(cube, M_CELL, (cx, 0.02, zc), (0.028, 0.002, 0.23), size=1)
                part(cube, M_LINE, (cx, 0.02, zc), (0.028, 0.002, 0.23), size=1, wire=0.001)
                for k in range(1, 10):
                    part(cube, M_LINE_DIM, (cx, 0.018, zc - 0.115 + k * 0.023), (0.027, 0.001, 0.0008), size=1)
        part(ico, M_AMBER, (x, 0.012, 0), radius=0.006, subdivisions=2)
    # radiators: ribbed white panels hanging below the truss near the middle
    for x in (-0.12, 0.12):
        for k in range(3):
            part(cube, M_WHITE, (x + (k - 1) * 0.022, 0.03, -0.08), (0.018, 0.002, 0.1), rot=(0, 0, 20), size=1, wire=0.0012)
    pivot.scale = (0.8, 0.8, 0.8)
    return (26, 35, -10)


def _bar(a, b, r, mat):
    """A thin glowing rod from a to b (subject frame)."""
    a, b = Vector(a), Vector(b)
    bpy.ops.mesh.primitive_cylinder_add(vertices=8, radius=r, depth=(b - a).length)
    c = bpy.context.object
    c.rotation_mode = "QUATERNION"
    c.rotation_quaternion = (b - a).to_track_quat("Z", "Y")
    c.location = (a + b) / 2
    c.data.materials.append(mat)
    c.parent = pivot
    return c


def ship():
    # a sleek hull, a cockpit, swept wings, a tail fin and two engines glowing cyan;
    # smooth glowing surfaces, outlines only on the flat parts (as the approved rocket)
    rim = rim_face("ship")
    part(uvs, rim, (0.0, 0, 0), (1.0, 0.26, 0.18), segments=48, ring_count=24, radius=0.26)
    part(uvs, M_WHITE, (0.1, 0, 0.035), (1.0, 0.55, 0.4), segments=24, ring_count=12, radius=0.05)
    part(tor, M_EDGE, (-0.12, 0, 0), (1, 1, 1), (0, 90, 0), major_radius=0.05, minor_radius=0.002, major_segments=48)
    for sy in (-1, 1):
        # wings: flat plates in the hull's plane, swept back
        pts = [(0.06, sy * 0.04), (-0.12, sy * 0.04), (-0.17, sy * 0.2), (-0.1, sy * 0.2)]
        for mat, wire in ((M_FACE, None), (M_EDGE, 0.0025)):
            me = bpy.data.meshes.new("wing")
            t = 0.004
            verts = [(x, y, z) for z in (-t / 2, t / 2) for x, y in pts]
            me.from_pydata(verts, [], [(0, 1, 2, 3), (7, 6, 5, 4), (0, 4, 5, 1), (1, 5, 6, 2), (2, 6, 7, 3), (3, 7, 4, 0)])
            ob = bpy.data.objects.new("wing", me)
            sc.collection.objects.link(ob)
            ob.data.materials.append(mat)
            if wire:
                ob.modifiers.new("w", "WIREFRAME").thickness = wire
            ob.parent = pivot
        # engines on the wing roots
        part(cyl, rim, (-0.17, sy * 0.07, 0), rot=(0, 90, 0), vertices=32, radius=0.022, depth=0.09)
        part(tor, M_EDGE, (-0.215, sy * 0.07, 0), rot=(0, 90, 0), major_radius=0.022, minor_radius=0.0025, major_segments=32)
        part(cyl, M_HOT, (-0.217, sy * 0.07, 0), rot=(0, 90, 0), vertices=24, radius=0.017, depth=0.004)
        part(cone, M_LINE, (-0.26, sy * 0.07, 0), rot=(0, -90, 0), vertices=24, radius1=0.017, radius2=0.0, depth=0.08)
    flat([(-0.06, 0.04), (-0.2, 0.04), (-0.22, 0.11), (-0.17, 0.11)], 0.004, M_FACE)
    flat([(-0.06, 0.04), (-0.2, 0.04), (-0.22, 0.11), (-0.17, 0.11)], 0.004, M_EDGE, wire=0.0025)
    return (TILT + 22, -28)


def planet():
    # a ringed planet in the rocket's style: a smooth sphere whose edge glows, a few
    # faint latitude bands, a flat ring (bright inner edge fading out, streaked) tilted
    # toward the viewer, and a small moon (no orbit circle, the user dislikes outer rings)
    rim = rim_face("planet")
    part(uvs, rim, segments=64, ring_count=32, radius=0.15)
    for z in (-0.07, -0.02, 0.05):
        r = math.sqrt(0.15 ** 2 - z ** 2)
        part(tor, M_LINE_DIM, (0, 0, z), major_radius=r, minor_radius=0.0012, major_segments=96)
    stops = [(0.0, (0.5, 0.9, 1.0, 0.0)), (0.08, (0.5, 0.9, 1.0, 0.9)), (0.5, (0.15, 0.55, 1.0, 0.55)),
             (1.0, (0.1, 0.3, 1.0, 0.0))]
    ring = part(bpy.ops.mesh.primitive_circle_add, radial_material("pring", stops, 4.0, inner=0.19, outer=0.31, rings=60.0),
                vertices=128, radius=0.31, fill_type="NGON")
    ring.rotation_euler = (math.radians(14), math.radians(-12), 0)
    part(uvs, rim_face("moonlet"), (0.33, -0.05, 0.11), segments=24, ring_count=12, radius=0.025)
    return (TILT, 15)


def blackhole():
    # the familiar look: a black core, a thin photon ring, a flat accretion disc
    # (white-hot inside, orange, fading out, streaked) seen from a little above, and
    # the disc's far side bent over and under the core by its gravity (no outer ring: the user dropped it)
    stops = [(0.0, (1.0, 0.95, 0.8, 1.0)), (0.15, (1.0, 0.7, 0.3, 0.9)), (0.45, (1.0, 0.38, 0.08, 0.55)),
             (1.0, (0.6, 0.1, 0.05, 0.0))]
    disc_mat = radial_material("disc", stops, 5.0, inner=0.085, outer=0.28, rings=40.0)
    halo_mat = radial_material("halo", stops, 5.0, inner=0.082, outer=0.15, rings=40.0)
    part(uvs, M_BLACK, segments=48, ring_count=24, radius=0.08)
    d = part(bpy.ops.mesh.primitive_circle_add, disc_mat, vertices=96, radius=0.3, fill_type="NGON")
    d.rotation_euler = (math.radians(12), 0, 0)   # nearly edge-on, seen a little from above
    # the lensed far side: a ring facing the viewer just behind the core
    h = part(bpy.ops.mesh.primitive_circle_add, halo_mat, vertices=96, radius=0.16, fill_type="NGON")
    facing_camera(h)
    h.location = (0, 0.02, 0)
    ring = part(tor, M_HOT, (0, -0.002, 0), major_radius=0.083, minor_radius=0.0025, major_segments=96)
    facing_camera(ring)
    return (0, 0)


SUBJECTS = dict(iss=iss, crate=crate, rocket=rocket, planet=planet, sun=sun, station=station, ship=ship, blackhole=blackhole)
angles = SUBJECTS[SUBJECT]()
if os.environ.get("HOLO_ANGLES"):           # try other angles: "lean,turn,roll"
    angles = tuple(float(v) for v in os.environ["HOLO_ANGLES"].split(","))
lean, turn, roll = (tuple(angles) + (0.0,))[:3]
pivot.location = crate_c
pivot.rotation_mode = "ZYX"   # spin about the vertical first, then lean toward the viewer, then roll
pivot.rotation_euler = (math.radians(lean), math.radians(roll), math.radians(turn))

# Extras to try (env HOLO_EXTRAS, comma separated): "earth" puts the curve of a
# glowing planet under the subject instead of the ring platform; "callouts" adds
# leader lines from parts of the subject to small labels at the sides.
EXTRAS = set(filter(None, os.environ.get("HOLO_EXTRAS", "").split(",")))
if "earth" in EXTRAS:
    earth = rim_face("earth", strength=(0.5, 6.0), alpha=(0.25, 0.9), blend=0.12)
    R, cy = 0.9, -1.13                       # its top just above the screen's lower third
    bpy.ops.mesh.primitive_uv_sphere_add(segments=128, ring_count=64, radius=R)
    e = bpy.context.object
    e.location = on_screen(0.12, cy, -0.6)
    e.data.materials.append(earth)
    # the atmosphere: a bright thin horizon and a soft wider glow along the limb
    for rr, wd, mat in ((R + 0.002, 0.003, M_EDGE), (R + 0.012, 0.008, M_LINE_DIM)):
        bpy.ops.mesh.primitive_torus_add(major_radius=rr, minor_radius=wd, major_segments=256, minor_segments=8)
        t = bpy.context.object
        t.rotation_euler = ROT
        t.location = e.location
        t.data.materials.append(mat)
    # city lights and cloud glints on the visible cap, below its rim
    for i in range(90):
        bpy.ops.mesh.primitive_ico_sphere_add(radius=rng.uniform(0.0018, 0.0035), subdivisions=1)
        p = bpy.context.object
        x = rng.uniform(-0.62, 0.68)
        top = cy + math.sqrt(max(R * R - (x - 0.12) ** 2, 0.0))
        p.location = on_screen(x, top - rng.uniform(0.015, 0.1), 0.02)
        p.data.materials.append(M_AMBER if i % 3 == 0 else M_LINE_DIM)
elif PLATFORM[0]:
    # platform rings under the subject, on the glass plane
    for r, mat, wd in ((0.27, M_EDGE, 0.006), (0.34, M_LINE_DIM, 0.003), (0.2, M_LINE, 0.003)):
        bpy.ops.mesh.primitive_torus_add(major_radius=r, minor_radius=wd, major_segments=96, minor_segments=6)
        t = bpy.context.object
        t.scale = (1, 0.3, 1)
        t.rotation_euler = ROT
        t.location = on_screen(0, -0.2, 0.03)
        t.data.materials.append(mat)
if "callouts" in EXTRAS:
    # (point on the subject in screen coords, label side)
    for (px, py), side, ly in (((-0.2, 0.12), -1, 0.2), ((0.02, 0.02), -1, -0.05), ((0.2, -0.1), 1, -0.12), ((0.17, 0.2), 1, 0.22)):
        lx = side * 0.42
        strip((px, py), (lx * 0.85, ly), 0.03, 0.0015, M_LINE_DIM)
        strip((lx * 0.85, ly), (lx, ly), 0.03, 0.0015, M_LINE_DIM)
        bpy.ops.mesh.primitive_ico_sphere_add(radius=0.006, subdivisions=2)
        d = bpy.context.object
        d.location = on_screen(px, py, 0.03)
        d.data.materials.append(M_AMBER)
        x0 = lx if side > 0 else lx - 0.12
        strip((x0, ly + 0.018), (x0 + 0.12, ly + 0.018), 0.03, 0.005, M_WHITE)
        strip((x0, ly - 0.004), (x0 + rng.uniform(0.05, 0.1), ly - 0.004), 0.03, 0.003, M_LINE_DIM)
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
if VIEW == "screen":
    sc.render.resolution_x, sc.render.resolution_y = 900, 520
    cd_.lens = 50
    cam.location = tuple(on_screen(0, 0.02, 2.0))
    target = on_screen(0, 0.0, 0.0)
elif VIEW == "room":
    cd_.lens = 30
    cam.location = (0.0, -7.5, 6.0)
    target = Vector((0, 0.2, 0.7))
else:
    cd_.lens = 38
    cam.location = (0.35, -2.7, 2.05)
    target = Vector((0, 0.2, 1.0))
cam.rotation_euler = (target - cam.location).to_track_quat("-Z", "Y").to_euler()
if VIEW == "bake":
    # light only: no desk, floor, glass or lamps; black behind; linear-ish colours
    for ob in list(sc.objects):
        if ob.type in ("LIGHT",) or ob.name in ("glass", "f") or (ob.type == "MESH" and ob.data.materials
                and ob.data.materials[0] and ob.data.materials[0].name in ("Panel", "PanelDark", "DeskGlass")):
            ob.hide_render = True
    for ob in list(sc.objects):
        if ob.type in ("MESH", "EMPTY") and ob.parent is None and ob.name.startswith(("desk_console", "Scene")):
            ob.hide_render = True
            for ch in ob.children_recursive:
                ch.hide_render = True
    w.node_tree.nodes["Background"].inputs["Color"].default_value = (0, 0, 0, 1)
    sc.compositing_node_group = None
    sc.view_settings.view_transform = "Standard"
    sc.view_settings.look = "None"
    sc.view_settings.exposure = -1.7   # keep peaks under 1 so colours survive; the app adds gain
    sc.render.resolution_x, sc.render.resolution_y = 1024, 512
    sc.render.image_settings.color_mode = "RGB"
    cd_.type = "ORTHO"
    cd_.ortho_scale = SW
    cam.location = tuple(on_screen(0, 0, 2.0))
    cam.rotation_euler = (on_screen(0, 0, 0) - cam.location).to_track_quat("-Z", "Y").to_euler()
sc.render.filepath = OUT
bpy.ops.render.render(write_still=True)
