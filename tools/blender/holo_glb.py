"""Exports a screen subject built by `holo_screen_proto.py` as a 3D model for the app:
the big hologram over a crew desk while a sub-agent works there (`holo.rs`).

glTF can't carry the prototype's node trees, so each material is rebuilt from its
record in `HOLO` as a plain emissive material, with what the app's hologram shader
needs in the material's extras:

- `holo_rim`: the fresnel edge glow of `rim_face` (smooth translucent blue whose
  edges glow cyan). The app's shader mixes the emissive colour toward the rim
  colour by `(1 - |n.v|^k)^e`, Blender's Layer Weight "Facing" with the same blend.
- `holo_solid`: drawn opaque (the black hole's core, the sun's ball); everything
  else is added as light.
- Radial ramps with streaks (planet ring, accretion disc, the lensed halo), the
  sun's corona and its boiling plasma are baked into small emissive textures here.
- Wireframe modifiers are applied to the geometry.

The parts are joined into one mesh ("body", one primitive per material); for the
subjects in `SPIN` the parts that turn on their own (the sun's ball, the black hole's
disc, the galaxy's stars) form a second node ("spin") that the app turns about its
local Y (glTF up). The model is centred and scaled so its largest side is 1 m.
"""
import math

import bmesh
import bpy
from mathutils import Matrix, Vector, noise

TEX = 256
# Subjects shown facing the camera (the app turns the whole model to the viewer, as
# on the screen): the material of the part that sets the spin axis, and which parts
# turn about it ("*": all but the named ones). Everything else turns as a whole.
SPIN = {
    "sun": ("photosphere", {"photosphere"}),
    "blackhole": ("disc", {"disc"}),
    "galaxy": ("galdisc", {"*", "bulge"}),
}
UV_RADIUS = {}          # material name -> the radius its planar UVs span


def spins(subject, mname):
    if subject not in SPIN:
        return False
    names = SPIN[subject][1]
    return (base_name(mname) not in names) if "*" in names else (base_name(mname) in names)


def export(pivot, holo, out, subject, lean=0.0):
    # Facing subjects keep the screen's lean toward the viewer (the galaxy's slant);
    # turning ones stand upright.
    pivot.location = (0, 0, 0)
    pivot.rotation_euler = (math.radians(lean) if subject in SPIN else 0.0, 0, 0)
    bpy.context.view_layer.update()
    dg = bpy.context.evaluated_depsgraph_get()
    parts = [ob for ob in pivot.children_recursive if ob.type == "MESH" and ob.data.materials]
    for ob in parts:
        if holo.get(ob.data.materials[0].name, {}).get("kind") in ("radial", "corona"):
            planar_uvs(ob)
    body = [ob for ob in parts if not spins(subject, ob.data.materials[0].name)]
    turning = [ob for ob in parts if spins(subject, ob.data.materials[0].name)]
    objects = [new_object("body", join(body, Matrix.Identity(4), dg, holo), Matrix.Identity(4))]
    if turning:
        axis = next(ob for ob in turning if base_name(ob.data.materials[0].name) == SPIN[subject][0])
        frame = axis.matrix_world.copy()
        objects.append(new_object("spin", join(turning, frame.inverted(), dg, holo), frame))

    # centre on the bounding box, largest side 1 m
    lo, hi = Vector((1e9,) * 3), Vector((-1e9,) * 3)
    for ob in objects:
        for v in ob.data.vertices:
            p = ob.matrix_world @ v.co
            lo = Vector(map(min, lo, p))
            hi = Vector(map(max, hi, p))
    size = max(hi - lo)
    norm = Matrix.Scale(1.0 / size, 4) @ Matrix.Translation(-(lo + hi) / 2)
    for ob in objects:
        ob.matrix_world = norm @ ob.matrix_world
        smooth(ob.data)
        if not ob.data.uv_layers:
            ob.data.uv_layers.new(name="UVMap")

    for ob in list(bpy.data.objects):
        if ob not in objects:
            bpy.data.objects.remove(ob, do_unlink=True)
    bpy.ops.export_scene.gltf(
        filepath=out,
        export_format="GLB",
        export_apply=True,
        export_animations=False,
        export_yup=True,
        export_lights=False,
        export_cameras=False,
        export_extras=True,
        export_image_format="AUTO",
    )
    tris = sum(sum(len(p.vertices) - 2 for p in ob.data.polygons) for ob in objects)
    print(f"HOLO: wrote {out}, {tris} triangles, {len(objects)} node(s)")


def join(parts, to_frame, dg, holo):
    """The parts with modifiers applied, in `to_frame`, as one mesh with one material
    slot per material."""
    groups = {}
    for ob in parts:
        groups.setdefault(ob.data.materials[0].name, []).append(ob)
    whole = bmesh.new()
    mesh = bpy.data.meshes.new("holo")
    for i, (mname, obs) in enumerate(groups.items()):
        for ob in obs:
            me = bpy.data.meshes.new_from_object(ob.evaluated_get(dg), depsgraph=dg)
            me.transform(to_frame @ ob.matrix_world)
            before = len(whole.faces)
            whole.from_mesh(me)
            whole.faces.ensure_lookup_table()
            for f in whole.faces[before:]:
                f.material_index = i
            bpy.data.meshes.remove(me)
        mesh.materials.append(material(mname, holo))
    whole.to_mesh(mesh)
    whole.free()
    return mesh


def base_name(name):
    return name.split(".")[0]


def new_object(name, mesh, matrix):
    ob = bpy.data.objects.new(name, mesh)
    bpy.context.scene.collection.objects.link(ob)
    ob.matrix_world = matrix
    return ob


def smooth(mesh):
    """Smooth normals (the fresnel glow needs them) with sharp edges kept."""
    mesh.shade_smooth()
    mesh.set_sharp_from_angle(angle=math.radians(40))


def planar_uvs(ob):
    """UVs from the flat disc's own XY coordinates, 0..1 over its radius."""
    me = ob.data
    r = max(max(abs(v.co.x), abs(v.co.y)) for v in me.vertices) or 1.0
    uv = me.uv_layers.new(name="UVMap") if not me.uv_layers else me.uv_layers[0]
    for loop in me.loops:
        co = me.vertices[loop.vertex_index].co
        uv.data[loop.index].uv = (co.x / (2 * r) + 0.5, co.y / (2 * r) + 0.5)
    UV_RADIUS[me.materials[0].name] = r


# ---------------------------------------------------------------- materials
def material(name, holo):
    spec = dict(holo.get(name, dict(kind="light", color=(0.05, 0.75, 1.0), strength=4.0)))
    if base_name(name) == "black":
        spec = dict(kind="occluder")
    kind = spec["kind"]
    mt = bpy.data.materials.new("holo_" + base_name(name))
    mt.use_nodes = True
    nt = mt.node_tree
    bsdf = nt.nodes["Principled BSDF"]
    bsdf.inputs["Base Color"].default_value = (0, 0, 0, 1)
    bsdf.inputs["Roughness"].default_value = 1.0
    extras = {}
    image = None
    if kind == "light":
        color, strength = spec["color"], spec["strength"]
    elif kind == "occluder":
        color, strength = (0, 0, 0), 1.0
        extras["holo_solid"] = True
    elif kind == "rim":
        # Blender: colour BLUE -> edge by facing^2, strength lo -> hi, alpha lo -> hi;
        # as added light: colour x strength x alpha at both ends.
        (s0, s1), (a0, a1) = spec["strength"], spec["alpha"]
        color, strength = spec["base"], s0 * a0
        edge = [c * s1 * a1 for c in spec["edge"]]
        blend = spec["blend"]
        k = 2 * blend if blend < 0.5 else 0.5 / (1 - blend)
        extras["holo_rim"] = [*edge, k, 2.0]
    elif kind == "radial":
        image = radial_image(name, spec)
        color, strength = (1, 1, 1), spec["strength"]
    elif kind == "corona":
        image = corona_image(name)
        color, strength = (1, 1, 1), spec["strength"]
    elif kind == "plasma":
        image = plasma_image(name)
        color, strength = (1, 1, 1), spec["strength"]
        extras["holo_solid"] = True
        # limb darkening: 1 -> 0.35 by Facing (blend 0.35), linear
        extras["holo_rim"] = [0.35 * strength, 0.35 * strength, 0.35 * strength, 0.7, 1.0]
    else:
        raise ValueError(kind)
    bsdf.inputs["Emission Color"].default_value = (*color, 1)
    bsdf.inputs["Emission Strength"].default_value = strength
    if image is not None:
        tex = nt.nodes.new("ShaderNodeTexImage")
        tex.image = image
        nt.links.new(tex.outputs["Color"], bsdf.inputs["Emission Color"])
    for key, value in extras.items():
        mt[key] = value
    return mt


def new_image(name, w, h, pixel):
    """An sRGB PNG image from `pixel(u, v) -> (r, g, b)` in linear 0..1."""
    img = bpy.data.images.new("holo_" + base_name(name), w, h, alpha=False)
    px = [0.0] * (w * h * 4)
    for y in range(h):
        for x in range(w):
            r, g, b = pixel((x + 0.5) / w, (y + 0.5) / h)
            i = (y * w + x) * 4
            px[i:i + 4] = (min(r, 1.0), min(g, 1.0), min(b, 1.0), 1.0)
    img.pixels = px
    img.file_format = "PNG"
    img.pack()
    return img


def ramp(stops, t):
    t = min(max(t, 0.0), 1.0)
    for (p0, c0), (p1, c1) in zip(stops, stops[1:]):
        if t <= p1:
            f = 0.0 if p1 == p0 else (t - p0) / (p1 - p0)
            return [a + (b - a) * f for a, b in zip(c0, c1)]
    return list(stops[-1][1])


def radial_image(name, spec):
    """`radial_material`: a colour ramp by radius, its alpha the brightness, times
    fine streaky rings (Blender's Rings wave with distortion)."""
    outer_uv = UV_RADIUS[name]
    inner, outer, rings = spec["inner"], spec["outer"], spec["rings"]

    def pixel(u, v):
        x, y = (u - 0.5) * 2 * outer_uv, (v - 0.5) * 2 * outer_uv
        r = math.hypot(x, y)
        if r > outer_uv:
            return (0, 0, 0)
        c = ramp(spec["stops"], (r - inner) / (outer - inner))
        amp = c[3]
        if rings:
            p = Vector((x, y, 0.0)) * rings
            n = r * rings * 20.0 + 1.0 * noise.noise(p * 0.5) * 6.0
            fac = 0.5 + 0.5 * math.sin(n)
            amp *= 0.55 + 0.45 * fac
        return tuple(ch * amp for ch in c[:3])

    return new_image(name, TEX, TEX, pixel)


def corona_image(name):
    """The sun's corona: radial falloff from the limb, times streamers by angle."""
    R = UV_RADIUS[name]

    def pixel(u, v):
        x, y = (u - 0.5) * 2 * R, (v - 0.5) * 2 * R
        r = math.hypot(x, y)
        fall = max(0.0, min(1.0, 1.0 - (r - 0.118) / (R - 0.118))) ** 2.2
        a = math.atan2(y, x)
        s = 0.5 + 0.5 * noise.noise(Vector((math.cos(a) * 3.5, math.sin(a) * 3.5, 0.3)))
        s += 0.25 * noise.noise(Vector((math.cos(a) * 9.0, math.sin(a) * 9.0, 1.7)))
        streak = 0.15 + 0.85 * min(max((s - 0.35) / 0.4, 0.0), 1.0)
        k = fall * streak
        return (1.0 * k, 0.5 * k, 0.12 * k)

    return new_image(name, TEX, TEX, pixel)


def plasma_image(name):
    """The sun's boiling plasma on the sphere's UVs (longitude, latitude): distorted
    fractal noise through the prototype's dark red, orange, yellow ramp."""
    stops = [(0.0, (0.5, 0.03, 0.0, 1)), (0.3, (0.5, 0.03, 0.0, 1)), (0.5, (1.0, 0.25, 0.0, 1)),
             (0.75, (1.0, 0.75, 0.15, 1)), (1.0, (1.0, 0.75, 0.15, 1))]

    def pixel(u, v):
        lon, lat = u * 2 * math.pi, (v - 0.5) * math.pi
        p = Vector((math.cos(lat) * math.cos(lon), math.cos(lat) * math.sin(lon), math.sin(lat))) * 0.12 * 9.0
        warp = Vector((noise.noise(p + Vector((3.1, 0, 0))), noise.noise(p + Vector((0, 5.2, 0))), noise.noise(p + Vector((0, 0, 7.7)))))
        q = p + warp * 2.2 * 0.5
        n, amp, freq, total = 0.0, 1.0, 1.0, 0.0
        for _ in range(5):
            n += noise.noise(q * freq) * amp
            total += amp
            amp *= 0.65
            freq *= 2.0
        fac = 0.5 + 0.5 * n / total * 1.6
        return tuple(ramp(stops, fac)[:3])

    return new_image(name, TEX, TEX // 2, pixel)
