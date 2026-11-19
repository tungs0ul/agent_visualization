"""Render the planets seen through the bridge window as sprites (Cycles, once).

    /Applications/Blender.app/Contents/MacOS/Blender -b --factory-startup \
        --python tools/blender/planets.py [-- name ...]

Writes assets/textures/space/<name>.png, 1024x1024, premultiplied RGBA: colour is
what the planet and its glow add to the sky, alpha is how much the body hides of
what's behind it. The atmosphere is painted in 2D over the render (`atmosphere`). The app draws each
as a camera-facing quad with premultiplied
blending (`space.rs`), 2.6 planet radii wide (the camera's ortho scale here).

- ocean: the target's big planet (crop 12): a blue world, blue-grey land, white cloud
  streaks, sparse lights on the night side and a bright white-cyan limb, lit from
  above. The app shows only its upper part through the window.
- terra: the target's small planet (crop 13): blue oceans, tan continents, light
  clouds, lit from the upper right with a blue rim.
"""

import math
import os
import sys

import bpy
from mathutils import Vector

REPO = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
OUT = os.path.join(REPO, "assets", "textures", "space")
ARGS = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
SIZE = 2048
FRAME = 2.6                       # ortho scale: the sprite is 2.6 radii wide


def reset():
    bpy.ops.wm.read_factory_settings(use_empty=True)
    sc = bpy.context.scene
    sc.render.engine = "CYCLES"
    sc.cycles.samples = 96
    sc.cycles.use_denoising = True
    sc.render.resolution_x = sc.render.resolution_y = SIZE
    sc.view_settings.view_transform = "Standard"
    sc.render.image_settings.file_format = "PNG"
    sc.render.image_settings.color_mode = "RGBA"
    world = bpy.data.worlds.new("w")
    world.use_nodes = True
    world.node_tree.nodes["Background"].inputs["Color"].default_value = (0, 0, 0, 1)
    sc.world = world
    cam = bpy.data.objects.new("cam", bpy.data.cameras.new("cam"))
    cam.data.type = "ORTHO"
    cam.data.ortho_scale = FRAME
    cam.location = (0, -10, 0)
    cam.rotation_euler = (math.radians(90), 0, 0)
    sc.collection.objects.link(cam)
    sc.camera = cam
    return sc


def sun(direction, energy, color=(1, 1, 1)):
    light = bpy.data.objects.new("sun", bpy.data.lights.new("sun", "SUN"))
    light.data.energy = energy
    light.data.color = color
    light.data.angle = math.radians(1.0)
    light.rotation_euler = Vector(direction).to_track_quat("Z", "Y").to_euler()
    bpy.context.scene.collection.objects.link(light)


def sphere(name, radius, material):
    bpy.ops.mesh.primitive_uv_sphere_add(segments=128, ring_count=64, radius=radius)
    ob = bpy.context.object
    ob.name = name
    bpy.ops.object.shade_smooth()
    ob.data.materials.append(material)
    return ob


class Nodes:
    """Tiny helper over a material's node tree."""

    def __init__(self, name):
        self.mat = bpy.data.materials.new(name)
        self.mat.use_nodes = True
        self.t = self.mat.node_tree
        for n in list(self.t.nodes):
            self.t.nodes.remove(n)
        self.out = self.t.nodes.new("ShaderNodeOutputMaterial")

    def n(self, kind, **inputs):
        node = self.t.nodes.new(kind)
        for key, value in inputs.items():
            node.inputs[key].default_value = value
        return node

    def link(self, a, b):
        self.t.links.new(a, b)

    def ramp(self, src, stops, mode="LINEAR"):
        r = self.t.nodes.new("ShaderNodeValToRGB")
        r.color_ramp.interpolation = mode
        el = r.color_ramp.elements
        while len(el) > 1:
            el.remove(el[-1])
        el[0].position, el[0].color = stops[0][0], (*stops[0][1], 1) if len(stops[0][1]) == 3 else stops[0][1]
        for pos, col in stops[1:]:
            e = el.new(pos)
            e.color = (*col, 1) if len(col) == 3 else col
        self.link(src, r.inputs["Fac"])
        return r

    def noise(self, scale, detail, rough, distort=0.0, w=0.0, stretch=None):
        coord = self.t.nodes.new("ShaderNodeTexCoord")
        nz = self.n("ShaderNodeTexNoise")
        if stretch is not None:
            mp = self.t.nodes.new("ShaderNodeMapping")
            mp.inputs["Scale"].default_value = stretch
            self.link(coord.outputs["Object"], mp.inputs["Vector"])
            nz.noise_dimensions = "4D"
            for k, v in (("Scale", scale), ("Detail", detail), ("Roughness", rough), ("Distortion", distort), ("W", w)):
                nz.inputs[k].default_value = v
            self.link(mp.outputs["Vector"], nz.inputs["Vector"])
            return nz
        nz.noise_dimensions = "4D"
        nz.inputs["Scale"].default_value = scale
        nz.inputs["Detail"].default_value = detail
        nz.inputs["Roughness"].default_value = rough
        nz.inputs["Distortion"].default_value = distort
        nz.inputs["W"].default_value = w
        self.link(coord.outputs["Object"], nz.inputs["Vector"])
        return nz

    def sunside(self, direction):
        """0 on the night side, 1 on the day side, soft across the terminator."""
        geo = self.t.nodes.new("ShaderNodeNewGeometry")
        dot = self.t.nodes.new("ShaderNodeVectorMath")
        dot.operation = "DOT_PRODUCT"
        dot.inputs[1].default_value = Vector(direction).normalized()
        self.link(geo.outputs["Normal"], dot.inputs[0])
        mr = self.n("ShaderNodeMapRange")
        mr.inputs["From Min"].default_value = -0.25
        mr.inputs["From Max"].default_value = 0.35
        self.link(dot.outputs["Value"], mr.inputs["Value"])
        return mr.outputs["Result"]

    def glow_shell(self, color, peak, strength, day=None, night=0.25):
        """Additive glow on a shell: brightest where the view grazes the planet's limb
        (`peak` = the Layer Weight facing value there), fading inward and outward."""
        lw = self.n("ShaderNodeLayerWeight", Blend=0.5)
        r = self.ramp(lw.outputs["Facing"], [(0.0, (0, 0, 0)), (peak * 0.7, (0.12, 0.12, 0.12)), (peak, (1, 1, 1)),
                                             (peak + (1 - peak) * 0.45, (0.08, 0.08, 0.08)), (1.0, (0, 0, 0))])
        mask = r.outputs["Color"]
        if day is not None:
            mul = self.n("ShaderNodeMath")
            mul.operation = "MULTIPLY_ADD"
            mul.inputs[1].default_value = 1 - night
            mul.inputs[2].default_value = night
            self.link(self.sunside(day), mul.inputs[0])
            m2 = self.n("ShaderNodeMath")
            m2.operation = "MULTIPLY"
            bw = self.n("ShaderNodeRGBToBW")
            self.link(mask, bw.inputs["Color"])
            self.link(bw.outputs["Val"], m2.inputs[0])
            self.link(mul.outputs["Value"], m2.inputs[1])
            mask = m2.outputs["Value"]
        em = self.n("ShaderNodeEmission", Color=(*color, 1))
        mul = self.n("ShaderNodeMath")
        mul.operation = "MULTIPLY"
        mul.inputs[1].default_value = strength
        self.link(mask, mul.inputs[0])
        self.link(mul.outputs["Value"], em.inputs["Strength"])
        tr = self.n("ShaderNodeBsdfTransparent")
        add = self.n("ShaderNodeAddShader")
        self.link(tr.outputs[0], add.inputs[0])
        self.link(em.outputs[0], add.inputs[1])
        self.link(add.outputs[0], self.out.inputs["Surface"])
        return self.mat


DAY = (-0.3, -0.5, 0.85)         # big planet: sun above and in front
TERRA_SUN = (0.8, -0.2, 0.55)     # small planet: sun upper right


def world(sun_dir, land_stops, cloud_cut, cloud_stretch, lights, ocean_rough=0.45):
    """Surface sphere (land and ocean colours from `land_stops`), a cloud sphere, the
    sun and a faint blue fill."""
    s = Nodes("surface")
    land = s.noise(1.8, 14, 0.62, distort=0.3, w=3.1)
    col = s.ramp(land.outputs["Fac"], land_stops)
    is_land = s.ramp(land.outputs["Fac"], [(0.53, (0, 0, 0)), (0.56, (1, 1, 1))])
    bsdf = s.n("ShaderNodeBsdfPrincipled")
    s.link(col.outputs["Color"], bsdf.inputs["Base Color"])
    rough = s.n("ShaderNodeMapRange")
    rough.inputs["To Min"].default_value, rough.inputs["To Max"].default_value = ocean_rough, 0.9
    bw = s.n("ShaderNodeRGBToBW")
    s.link(is_land.outputs["Color"], bw.inputs["Color"])
    s.link(bw.outputs["Val"], rough.inputs["Value"])
    s.link(rough.outputs["Result"], bsdf.inputs["Roughness"])
    if lights:
        city = s.noise(70, 4, 0.5, w=7.0)
        spots = s.ramp(city.outputs["Fac"], [(0.64, (0, 0, 0)), (0.72, (0.6, 0.6, 0.6))])
        night = s.n("ShaderNodeMath")
        night.operation = "SUBTRACT"
        night.inputs[0].default_value = 1.0
        s.link(s.sunside(sun_dir), night.inputs[1])
        m1, m2 = s.n("ShaderNodeMath"), s.n("ShaderNodeMath")
        m1.operation = m2.operation = "MULTIPLY"
        sb = s.n("ShaderNodeRGBToBW")
        s.link(spots.outputs["Color"], sb.inputs["Color"])
        s.link(sb.outputs["Val"], m1.inputs[0])
        s.link(bw.outputs["Val"], m1.inputs[1])
        s.link(m1.outputs["Value"], m2.inputs[0])
        s.link(night.outputs["Value"], m2.inputs[1])
        bsdf.inputs["Emission Color"].default_value = (0.75, 0.88, 1.0, 1)
        s.link(m2.outputs["Value"], bsdf.inputs["Emission Strength"])
    s.link(bsdf.outputs[0], s.out.inputs["Surface"])
    sphere("planet", 1.0, s.mat)

    c = Nodes("clouds")
    cn = c.noise(3.0, 15, 0.64, distort=0.6, w=11.0, stretch=cloud_stretch)
    cm = c.ramp(cn.outputs["Fac"], [(cloud_cut, (0, 0, 0)), (cloud_cut + 0.15, (1, 1, 1))])
    cb = c.n("ShaderNodeBsdfPrincipled", Roughness=0.9)
    cb.inputs["Base Color"].default_value = (0.95, 0.97, 1.0, 1)
    tr = c.n("ShaderNodeBsdfTransparent")
    mix = c.n("ShaderNodeMixShader")
    cbw = c.n("ShaderNodeRGBToBW")
    c.link(cm.outputs["Color"], cbw.inputs["Color"])
    c.link(cbw.outputs["Val"], mix.inputs[0])
    c.link(tr.outputs[0], mix.inputs[1])
    c.link(cb.outputs[0], mix.inputs[2])
    c.link(mix.outputs[0], c.out.inputs["Surface"])
    sphere("clouds", 1.008, c.mat)
    sun(sun_dir, 4.5, (1.0, 0.98, 0.95))
    sun((0.3, -0.8, -0.4), 0.12, (0.35, 0.55, 1.0))


def ocean():
    # all blues, as in the target: deep ocean, bright shallows, blue-grey land
    world(DAY, [
        (0.0, (0.002, 0.012, 0.07)), (0.47, (0.006, 0.04, 0.2)), (0.53, (0.02, 0.15, 0.45)),
        (0.545, (0.06, 0.14, 0.28)), (0.65, (0.1, 0.2, 0.34)), (1.0, (0.22, 0.3, 0.42))],
        cloud_cut=0.5, cloud_stretch=(1.0, 1.0, 3.5), lights=True, ocean_rough=0.6)


def terra():
    world(TERRA_SUN, [
        (0.0, (0.004, 0.02, 0.1)), (0.5, (0.01, 0.07, 0.25)), (0.535, (0.03, 0.18, 0.4)),
        (0.545, (0.35, 0.22, 0.1)), (0.66, (0.55, 0.38, 0.2)), (1.0, (0.7, 0.58, 0.4))],
        cloud_cut=0.56, cloud_stretch=(1.0, 1.0, 2.0), lights=False, ocean_rough=0.65)


# The atmosphere, painted in 2D over the render: a thin bright limb and a soft halo
# fading outward, strongest towards the sun. (colour, sun direction, limb, halo)
AIR = {
    "ocean": ((0.45, 0.85, 1.0), DAY, 3.2, 0.9),
    "terra": ((0.4, 0.75, 1.0), TERRA_SUN, 1.6, 0.35),
}


def atmosphere(px, name):
    import numpy as np
    color, toward, limb, halo = AIR[name]
    ys, xs = np.mgrid[0:SIZE, 0:SIZE]
    # image rows run top-down in the render, but Blender's pixel buffer is bottom-up
    u = (xs + 0.5) / SIZE * FRAME - FRAME / 2
    v = (ys + 0.5) / SIZE * FRAME - FRAME / 2
    r = np.sqrt(u * u + v * v)
    sun2 = np.array([toward[0], toward[2]])
    sun2 = sun2 / np.linalg.norm(sun2)
    facing = (u * sun2[0] + v * sun2[1]) / np.maximum(r, 1e-6)
    day = 0.25 + 0.75 * np.clip(facing * 0.5 + 0.5, 0, 1) ** 1.6
    inside = np.where(r < 1, np.clip((r - 0.82) / 0.18, 0, 1) ** 3, 0.0)
    outside = np.where(r >= 1, np.exp(-(r - 1) / 0.02) + 0.35 * np.exp(-(r - 1) / 0.12), 0.0)
    glow = (inside * limb * 0.6 + outside * (limb * 0.6 + halo)) * day
    glow = glow.ravel()
    for i in range(3):
        px[:, i] += glow * color[i]
    return px


def render(name, build):
    """Render twice (colour on black, then coverage on transparent) and pack both into
    one premultiplied RGBA PNG."""
    import numpy as np
    sc = reset()
    build()
    tmp = os.path.join(OUT, f".{name}")
    sc.render.film_transparent = False
    sc.render.filepath = tmp + "_rgb.png"
    bpy.ops.render.render(write_still=True)
    # coverage: glow shells off, so only the body (and clouds over it) hide the sky
    sc.render.film_transparent = True
    sc.cycles.samples = 16
    sc.render.filepath = tmp + "_a.png"
    bpy.ops.render.render(write_still=True)
    rgb = bpy.data.images.load(tmp + "_rgb.png")
    cov = bpy.data.images.load(tmp + "_a.png")
    px = np.array(rgb.pixels[:]).reshape(-1, 4)
    a = np.array(cov.pixels[:]).reshape(-1, 4)[:, 3]
    px[:, 3] = np.maximum(a, 0.0)
    px = atmosphere(px, name)
    px[:, :3] = np.clip(px[:, :3], 0, 1)
    out = bpy.data.images.new(name, SIZE, SIZE, alpha=True)
    out.pixels = px.ravel().tolist()
    out.filepath_raw = os.path.join(OUT, f"{name}.png")
    out.file_format = "PNG"
    out.save()
    for f in (tmp + "_rgb.png", tmp + "_a.png"):
        os.remove(f)
    print("PLANET: wrote", out.filepath_raw)


os.makedirs(OUT, exist_ok=True)
for name, build in (("ocean", ocean), ("terra", terra)):
    if not ARGS or name in ARGS:
        render(name, build)
