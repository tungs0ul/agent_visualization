# Agent Room: Visual Overhaul Brief

> **For the coding agent.** Read this whole file before you change anything. Then work through it
> phase by phase (section 13). After each phase, stop and report back (section 17).

---

## 0. Role, mission, ground rules

You are acting as a **senior technical artist and Bevy rendering engineer**. The app is a real-time
**visualisation of what an LLM agent is doing**. It is built with **Bevy** (Rust), and its 3D assets
come from **Blender** as glTF. The current build works, but it looks flat, washed out, grey and cheap.

**Mission:** make the app look as close as possible to the concept art in
`reference/target_reference.png`. Keep every existing feature working, and keep every visual element
driven by **real agent data**.

Ground rules:

1. **Match the mood, lighting, palette, materials, detail density and HUD style. Don't aim for a pixel-perfect copy.**
   The target is AI-generated concept art, so 100% isn't possible in real time. About 85% is, and most of
   that comes from lighting, value structure, emissives, post-processing and the HUD.
2. **Don't copy AI artefacts or fake data.** Garbled icons (the "Tabs" box), invented metrics ("Confidence 98%")
   and lorem-ipsum labels must not ship. Every number, label and log line comes from the app's real
   agent state. If a datum doesn't exist yet, hide the widget or swap in a real metric (section 10.5).
3. **Don't break features.** Settings, History, Chat, Changes, Commands, Replay, Today/stats, the Activity log,
   New session and the Auto-accept indicator must all stay reachable and keep working.
4. **Measure, don't guess.** Use the screenshot and metrics loop in section 14 after every meaningful change.
5. **Data-drive all tunables.** Keep exposure, bloom, light intensities, emissive tiers and colours in one
   hot-reloadable config file (section 14.1). Visual quality comes from iteration, and every recompile slows that down.
6. **Check the Bevy version first** (`Cargo.toml`). Component names and shader syntax differ between versions.
   Section 5.1 lists the differences that matter. When you're unsure of an API, check docs.rs for *that* version.
   Don't write code from memory of a different version.

---

## 1. Inputs in this folder

| File | What it is |
|---|---|
| `reference/target_reference.png` | **The target.** 1470×1070, aspect 1.37. This is the same aspect as the current app screenshots. |
| `reference/target_reference_annotated.png` | The target with 20 numbered callouts. This brief refers to them as ⟨1⟩–⟨20⟩. |
| `reference/crops/*.png` | 2× zoomed details of the target: HUD panels, core, desk pods, orrery, floor, windows, racks. **Open the relevant crop before you build each element.** |
| `reference/current_app_1.png`, `current_app_2.png` | The current build. |
| `reference/palette.png` | The colour palette sampled from the target (also listed in section 3.3). |
| `tools/look_metrics.py` | Compares a screenshot's tonal and colour statistics with the target and prints concrete fixes. |

---

## 2. Gap analysis: why the current build looks cheap

These numbers were measured with `tools/look_metrics.py`. The current build's figures are for `current_app_1.png`.

| Metric | Current | Target | Meaning |
|---|---|---|---|
| Median luminance | **0.34** | **0.20** | Mid-tones are about 1.7× too bright. Everything sits in a milky mid-grey. |
| Pixels darker than L 0.15 | **10%** | **36%** | There's almost no true darkness, so there's no contrast for glow to read against. |
| Mean saturation | **0.44** | **0.74** | Greyish slate instead of saturated navy. |
| Saturated glow pixels | **0.7%** | **10.3%** | The target has about 14× more saturated glow. Current emissives are nearly white and weak. |
| Average dark colour | `#1D2C40` grey slate | `#0A1D3D` saturated navy | |
| 95th-percentile luminance | 0.77 | 0.77 | Highlights are fine. **The problem is the shadows and mid-tones, not the highlights.** |

What you can see in the images:

- **Lighting is flat.** Ambient light is high, there's no key or rim light, there are hardly any shadows, and there's no local light pooling around glowing objects.
- **Emissives are pale and white-ish.** Strips, rings and screens are whitish-cyan with a weak halo. The target uses *saturated* cyan and blue emissives that are bright enough for the tonemapper to burn the core to white while the bloom halo stays coloured.
- **There are no reflections.** The target's floor is dark, glossy metal that reflects every glow line ⟨17⟩. The current floor is matte grey-blue.
- **The desks are generic office desks** with drawer pedestals. The target uses sculpted, glossy white sci-fi consoles with bevelled edges, LED trim and holographic multi-screens ⟨9⟩.
- **The screens look like pale paper sheets.** They're flat, unlit, light cyan rectangles with floating "document" cards. The target screens are emissive holograms with bright borders, scanlines and real content.
- **The core is weak.** The glass cylinder is barely visible, the robot is lit flat, and the rings are thin and white. The target has a fresnel-glowing hologram cylinder, a 3D hologram above the robot and thick neon rings in cyan, amber and magenta ⟨7⟩⟨8⟩.
- **The radar is a grey plastic disc.** The target has a glowing holographic orrery with orbits, blips and a sweep ⟨13⟩.
- **The space outside is cartoonish.** The Earth texture pinches at the UV-sphere pole, the planet has no atmosphere rim or night side, there's one purple nebula blob, a big flat white "moon" disc, and the sky is black. The target sky is deep blue with nebulae, the planet has a strong atmospheric limb glow, and there are ships and asteroids ⟨15⟩⟨16⟩.
- **The composition wastes the frame.** About 20% of the frame is empty ceiling or floor, and the room doesn't fill the width.
- **The HUD is minimal and generic.** Borders are thin and don't glow, there's no hierarchy, and status, metrics and quick actions are missing.
  **Missing-glyph boxes (tofu) appear before "Command", "Replay" and "Activity".** These are icons the font doesn't contain.
- There's an unexplained **yellow/gold bar on the floor** at the bottom right. Find out what it is. If it's a debug leftover, remove it. If it's meant as a marking, turn it into a subtle decal.

---

## 3. Target look: style bible

### 3.1 Mood

A dark, premium sci-fi command deck at night. Deep saturated navy surfaces, white glossy hardware, and
a panoramic window onto a blue planet. **Light comes from the technology itself:** neon rings, holograms,
LED strips and screens, with a cool moonlight-blue key light from the window. Every hard edge catches a highlight,
and every glow line reflects in the polished floor. The room feels calm and alive, because something
always moves a little, while agent events trigger crisp, readable reactions.

### 3.2 Value structure (the most important rule)

- **About 70% of the frame is dark** (navy, L < 0.25): floor, walls, ceiling, window frames, sky.
- **About 20% is lit mid-tone**: white desk shells and robot (lit, so they read bright), steel frames, the planet.
- **About 10% is saturated glow**: lines, rings, screens, LEDs, the atmosphere rim. Glow is *reserved*. It belongs only on edges, lines, screens and indicators, never on large surfaces.
- Target statistics: median luminance ≈ 0.20, 36% of pixels below L 0.15, mean saturation ≈ 0.74 (section 14).

### 3.3 Palette (sRGB hex, sampled and normalised from the target)

**Base and scene**

| Name | Hex | Use |
|---|---|---|
| void navy | `#020A1E` | deepest shadows, space |
| deep navy | `#06122C` | ceiling, recesses, panel seams |
| navy | `#0A1D3D` | average dark tone of the whole image |
| floor metal | `#0E1A33` | floor panels (base_color) |
| wall panel | `#122241` | lower walls, benches |
| steel blue | `#2F4571` | lit metal frames, bezels |
| desk shell white | `#D9E2EE` | console shells, robot, pots |

**Glow** (about 96% of all glow pixels are azure or blue)

| Name | Hex | Use |
|---|---|---|
| cyan | `#38C8FF` | primary glow: rings, strips, traces |
| azure | `#2E8BFF` | secondary glow: screens, radar |
| electric blue | `#1E5BFF` | deep glow, hologram shadows |
| ice / hot core | `#DFF6FF` | only where the tonemapper burns out; **never use as an emissive base colour** |
| holo screen | `#4B8DF0` | screen content average |

**Accents** (no more than 4% of glow in total, and **at most one accent colour per object**)

| Name | Hex | Use |
|---|---|---|
| amber | `#FF9A3C` | arc segments on desk pads, desk end-cap slits, dais middle ring |
| orange-red | `#FF6A3D` | orrery "sun" blip, warnings in 3D |
| magenta | `#FF4FD8` | inner dais ring gradient |
| violet | `#8A5CFF` | the faint hexagon floor line, some LEDs |
| teal | `#3EE0C8` | rare LEDs, data blips |

**HUD**

| Name | Value |
|---|---|
| panel fill | `#04122E` at 80% opacity (top) to `#020B1F` at 88% (bottom) |
| panel border | `#3AA6FF` |
| text primary / secondary / muted | `#EAF4FF` / `#9DB4D6` / `#5F7AA0` |
| success / warning / danger | `#22E07A` / `#FFB547` / `#FF5A6E` |

**Colour rules**

- An emissive *colour* is always a saturated hue from the glow or accent list. **Brightness comes from intensity, not from whitening the colour.** The tonemapper (TonyMcMapface or AgX) turns the core white by itself and leaves the halo coloured. This one rule fixes most of the "pale glow" look.
- Never use neutral grey as a base colour. Dark surfaces are navy-tinted, and light surfaces are cool white.
- Convert hex to linear before scaling. Don't scale alpha:

```rust
fn emissive(hex: &str, nits: f32) -> LinearRgba {
    let c: LinearRgba = Srgba::hex(hex).unwrap().into();
    LinearRgba::rgb(c.red * nits, c.green * nits, c.blue * nits)
}
```

### 3.4 Emissive luminance ladder

`StandardMaterial::emissive` is in nits (cd/m²) and is scaled by camera exposure. These starting values
assume the default exposure (`EV100 ≈ 9.7`). If you change EV100 by +1, double them. **Tune live** (section 14.1).

| Tier | Starting nits | Applies to |
|---|---|---|
| T0 dim | 150–400 | idle or unassigned desk rings, faint floor grid lines, violet hexagon line |
| T1 screen | 600–1,500 | holo screen content, wall display, orrery surface |
| T2 trim | 2,000–4,000 | LED strips, desk pad rims, window-frame strips, rack LEDs |
| T3 hero | 5,000–10,000 | core dais rings, spine trace, robot eyes, hologram edges, packet pulses |
| T4 flash | 15,000+ (brief) | event flashes only: packet arrival, desk boot, task done |

A strip should read as a white-hot 1–2 px core with a saturated cyan halo 6–20 px wide at 1470 px width.
If the whole strip looks white, the colour is too desaturated or bloom is too weak, so don't just add intensity.
If there's no white core, raise the tier.

### 3.5 Material library (StandardMaterial starting values)

| Surface | base_color | metallic | perceptual_roughness | Extras |
|---|---|---|---|---|
| Floor panels | `#0E1A33` | 0.75 | 0.30 (vary 0.24–0.40 per panel via a texture) | clearcoat 0.4, clearcoat_perceptual_roughness 0.12; anisotropic texture filtering x16 |
| Floor seams / grooves | `#060C1A` | 0.6 | 0.6 | |
| Lower wall panels | `#122241` | 0.5 | 0.45 | trim-sheet normal map (panel lines, vents, bolts) |
| Window frames / mullions | `#1A2640` | 0.85 | 0.28 | bevelled; inner edges carry T2 cyan strips |
| Ceiling | `#0B1428` | 0.4 | 0.6 | emissive light panels (T2, `#DFF6FF` tinted towards cyan) |
| Desk shell (white) | `#D9E2EE` | 0.0 | 0.22 | clearcoat 0.8, clearcoat roughness 0.08 (ceramic or automotive white) |
| Desk top inset | `#0A1222` | 0.2 | 0.16 | reads as black glass |
| Desk pad top | `#0D182E` | 0.7 | 0.35 | |
| Robot body | `#EEF3FA` | 0.0 | 0.20 | clearcoat 1.0 |
| Robot visor | `#02040A` | 0.0 | 0.04 | reflectance 0.6, eyes are a separate T3 emissive |
| Server rack body | `#05080F` | 0.6 | 0.18 | |
| Orrery / radar bezel | `#2A3550` | 0.9 | 0.28 | small amber indicator lights |
| Plant leaves | `#1F5F3A` | 0.0 | 0.5 | double-sided, diffuse_transmission 0.3 |
| Pots | `#E6ECF5` | 0.0 | 0.25 | thin cyan T2 ring at the rim |

### 3.6 Shape language and detail density

- **Bevel every hard edge** (2–8 mm). Unbevelled CG edges are the biggest single "cheap" tell after lighting. Bevelled edges catch specular highlights and outline objects in light.
- **Hardware is rounded and sculpted** (console end-caps, stepped dais, pill-shaped pads). **Lines are crisp and thin** (traces, borders, rings).
- **Use three levels of detail on every hero object:** primary form, secondary panel splits and grooves, and tertiary greebles (LEDs, buttons, vents, screws, labels). The target is dense with tiny lights and details. Small orange, pink and white buttons on the desks add a lot.
- **Repeat consistent elements:** every desk pod is identical hardware, varied only by screen content and state.

### 3.7 Motion language

- **Ambient motion is always on, slow and subtle:** ring segments rotate at 0.05–0.25 rad/s, the hologram flickers lightly, scanlines scroll, the radar sweeps every 6 s, rack LEDs twinkle, dust motes drift, the planet and clouds rotate, the robot bobs 2 cm every 3 s, and idle glows breathe ±10% over 3–4 s.
- **Event motion is fast and readable:** 100–600 ms with ease-out curves. Packets travel along traces, desks boot, flashes fire on completion and labels pop in.
- Nothing jitters at high frequency except deliberate hologram glitches (rare, under 80 ms).

---

## 4. Camera and composition

Look at the target. It's a **symmetrical high three-quarter view**. A vertical axis runs through the window centre, the hologram, the robot, the spine trace and the orrery. The room fills the entire width.

**Framing targets** (percentage of frame height from the top, at the 1.37 aspect):

| Element | Target |
|---|---|
| Top of window band / ceiling header strip | 8–12% |
| Bottom of windows / back ledge with plants | 30–34% |
| Robot head | 36–40% |
| Bottom edge of the core dais | 50–53% |
| Orrery centre | 69–73% |
| Front desk pads | partly under the bottom HUD (82% and below) |

- Side walls leave the frame at the left and right edges. There's no empty ceiling band taller than about 8% and no empty floor band at the bottom.
- **Starting point** (core at the origin, 1 unit = 1 m): camera at `(0, 11, 14)` looking at `(0, 0, 2.2)`, vertical FOV ≈ 45°. Adjust position and FOV until the framing targets are met.
- **The hero camera is the default view.** If the app allows orbiting, clamp it: yaw ±30°, pitch 35–60°, zoom 0.8–1.25×, with smooth damping. Optionally ease back to the hero view after 20 s of no input.
- Expose a hotkey (e.g. `F12`) and a CLI flag (e.g. `--hero-screenshot <path>`) that snap to the hero view and capture a screenshot (section 14).

---

## 5. Rendering pipeline (Bevy)

### 5.1 Version check (do this first)

Read `Cargo.toml` and note the Bevy version in your Phase 0 report. Differences that matter:

| Topic | Notes |
|---|---|
| HDR | 0.17+: `Hdr` marker component (Bloom requires it automatically). ≤0.16: `Camera { hdr: true }`. |
| Render target | 0.18+: `RenderTarget` is its own camera component. Before that it's a field on `Camera`. |
| Vignette, LensDistortion, ContactShadows, RectLight (area lights), parallax-corrected cubemaps | 0.19+. RectLight needs the `area_light_luts` cargo feature. On older versions, build a vignette with a full-screen UI radial gradient and use PointLights instead of RectLights. |
| Text | 0.19 moved text to Parley. `TextFont` gained `weight`, and `LetterSpacing` was added. 0.20 added `em`/`rem` units and changed the default font size to `rem(1)`. |
| Shaders | **0.20 moved to WESL.** Files that use imports or preprocessor directives become `.wesl` with `import bevy_pbr::...;` syntax and `constants::MATERIAL_BIND_GROUP`. Plain WGSL without directives still works. ≤0.19 uses `.wgsl` with `#import`. Write custom shaders in the dialect your version uses. |
| Solari (path tracing) | 0.20 runs it on macOS Metal, but there's **no denoiser on Metal**. Don't use it as the main renderer. Use the rasterised pipeline below. |
| UI gradients | `BackgroundGradient` / `BorderGradient` with Linear, Radial and Conic gradients (0.17+). `BoxShadow` (0.15+) is the outer glow. Check where `border_radius` lives in your version (Node field or component). |

Don't upgrade Bevy as part of this work unless I approve it. If the current version lacks a feature,
use the documented fallback.

### 5.2 Main camera components (starting values)

- `Camera3d`, `Hdr` (or `hdr: true`). **Default: forward rendering with `Msaa::Sample4`.** `Msaa::Off` is only for the High preset (deferred, TAA). See 5.5. The SSAO, SSR, TAA and contact-shadow items below are **High preset only**.
- `Tonemapping::TonyMcMapface`. Compare it with `Tonemapping::AgX` side by side and keep whichever keeps the cyan halos more saturated.
- `DebandDither::Enabled`. **This is important:** large dark navy gradients band without it.
- `Exposure { ev100: 9.7 }`. Tune between 9.2 and 10.5.
- `Bloom`: start from `Bloom::NATURAL`, then set intensity ≈ 0.28, low_frequency_boost ≈ 0.6, low_frequency_boost_curvature ≈ 0.9, high_pass_frequency ≈ 0.9.
  If glows look like fog instead of neon, try a tighter look: `prefilter.threshold` 0.9–1.2, `threshold_softness` 0.4, `BloomCompositeMode::Additive`, intensity 0.12–0.18.
  The target has both a tight halo around lines and a wide, soft haze around the core and planet.
- `ColorGrading`: global `post_saturation` 1.10–1.18, `temperature` −0.05 (slightly cool), midtones `contrast` ≈ 1.06, shadows `saturation` ≈ 1.15, highlights `saturation` ≈ 0.95 so hot cores stay clean.
- `ScreenSpaceAmbientOcclusion` (High quality). It grounds desks and pads on the floor.
- `ScreenSpaceReflections`. It requires `DepthPrepass` + `DeferredPrepass`, so it's deferred only, and StandardMaterial must use the deferred path (`DefaultOpaqueRendererMethod::deferred()`). Make sure the floor's roughness (about 0.3) is below the SSR roughness threshold. Custom opaque materials must either support the deferred path or explicitly opt into forward rendering. Additive and transparent materials (holograms, glass, atmosphere) always render forward, which is fine.
- Anti-aliasing: `TemporalAntiAliasing` + `ContrastAdaptiveSharpening` (strength ≈ 0.3). If holograms or screens ghost under TAA, switch to `Smaa` (High).
- Optional: `ChromaticAberration` at a very low intensity (barely visible, edges only). `Vignette` (0.19+), corners about 25–35% darker, colour `#000610`.
- `ContactShadows` (0.19+) on the camera, with `contact_shadows_enabled` on the key light.
- **Don't use `DistanceFog`.** It would also fog the planets and sky. For atmosphere, use the optional room-bounded volumetric haze in 5.4.
- Anisotropic filtering: configure `ImagePlugin`'s default sampler with `anisotropy_clamp: 16` and linear min, mag and mip filters. Without it, the floor texture blurs at this grazing angle.

### 5.3 Lighting rig (starting values at EV100 9.7; units are lux and lumens)

First, **lower the global ambient light drastically**: brightness about 15–40 with colour `#1A3A7A`. This is
`AmbientLight` or `GlobalAmbientLight` depending on your version. Do not raise ambient to "see things".
Add practical lights instead.

| Light | Type | Colour | Intensity | Notes |
|---|---|---|---|---|
| Window key | `DirectionalLight` | `#9CC2FF` | 1,200–2,500 lx | Comes from behind and above the back window, aimed at the camera side, so desks and the robot get **rim light**. Shadows on. 3 cascades, first far bound about 8 m, max about 40 m, `DirectionalLightShadowMap` 4096. |
| Core glow | `PointLight` inside the cylinder at robot chest height | `#39C6FF` | 60k–150k lm, range 8 m, radius 0.6 | No shadows. Lights the robot and dais in cyan. |
| Core top-down | `SpotLight` over the robot | `#CFE6FF` | 30k–60k lm, inner 15°, outer 30° | Shadows on. Gives a crisp highlight on the robot's head. |
| Desk screen wash ×8 | `RectLight` sized to each main holo screen, facing the operator side (0.19+). Fallback: `PointLight`, range 3.5 m, 8k–20k lm | `#3D9BFF` | tune | No shadows. Makes screens visibly light the desk and keyboard. Scale with desk state (off when the desk is unassigned). |
| Orrery glow | `PointLight` 0.5 m above the table | `#2E8BFF` | 30k–50k lm, range 5 m | |
| Rack wash | `RectLight` or `PointLight` in front of the racks | `#38C8FF` | low | Gives the cyan reflection on the floor in front of the racks. |
| Ceiling and window strips | `RectLight`s along the strips (0.19+), or baked lightmaps | cool white-cyan | low | |

Only the key light and the core spot cast shadows. Everything else is shadowless, so the forward/clustered cost stays low.

### 5.4 Reflections, environment and GI

1. **Space environment.** Use a high-resolution space cubemap (section 7.10) as the `Skybox`, with brightness tuned so the sky's average reads about `#0F2950`, never pure black. Use the same cubemap for a dim `EnvironmentMapLight`. On 0.17+, `GeneratedEnvironmentMapLight` builds the filtered maps at runtime. This gives metals a faint blue sheen.
2. **Interior reflection probe (high impact).** In Blender, render a cubemap from about 1.2 m above the room centre *with all emissive strips lit*. Use it as a `LightProbe` + `EnvironmentMapLight` covering the room volume. On 0.19+ it's parallax-corrected automatically. The polished floor and white shells will then reflect the room's glow lines even where SSR has no screen data. SSR adds dynamic reflections on top.
3. **Baked lightmaps for static geometry (high impact, Phase 8).** Bake diffuse direct and indirect lighting (colour off, lighting only) in Blender Cycles for the room shell, floor and static props. Emissive strips then cast real soft light on floors and walls. Store the lightmaps on UV channel 2, export as HDR or KTX2 (rgba16f), and attach Bevy's `Lightmap` component to the matching mesh entities after scene spawn (match by node name). Tune `StandardMaterial::lightmap_exposure`. Don't double-light: lights that are baked shouldn't also be dynamic on the same surfaces, apart from the key light's specular.
4. **Optional, High preset only:** a subtle room-bounded `VolumetricFog` + `FogVolume` (a box around the core, low density, blue tint), with `VolumetricLight` on the core spot. This gives a soft haze and light shafts around the hologram.

### 5.5 Quality presets (revised after FIX 01: temporal effects caused speckle and flicker)

| Preset | Rendering | AA | SSAO | SSR | Contact shadows | Volumetric haze | Reflections | Bloom |
|---|---|---|---|---|---|---|---|---|
| Low | forward | MSAA 4× | off | off | off | off | baked probe | on |
| **Medium (default)** | forward | MSAA 4× (+ SMAA if needed) | off | off | off | off | baked probe | on |
| High (opt-in, continuous rendering only) | deferred | TAA (no CAS) | High | on | on | optional | probe + SSR | on |

**Rule:** any effect that relies on temporal accumulation (SSAO noise, SSR, contact shadows, temporal shadow filtering, TAA) only goes in High.
Add these effects one at a time, and each one must pass a stability test: two screenshots 0.5 s apart, no speckle at 2× zoom.
In every preset, the app must render continuously while the window is focused.

Expose the preset in Settings and persist it.

---

## 6. Scene layout (top view, approximate; core at the origin, +Z towards the camera, metres)

Keep the existing slot logic if it's there. Match these proportions.

```
                       back window wall  z ≈ -5.5   (ledge with consoles + 2 plants at z ≈ -4.8)
   wall display                                                        server racks (4-5)
   x≈-10.5                [Desk L-back]   (CORE r≈2.6)   [Desk R-back]           x≈10.5
                           (-4.6,-1.8)       (0,0)        (4.6,-1.8)
     [Desk L-outer]  [Desk L-inner]                  [Desk R-inner]  [Desk R-outer]
       (-7.6, 1.4)     (-3.4, 1.6)        │ spine      (3.4, 1.6)      (7.6, 1.4)
                                          │
              [Desk L-front]        (ORRERY r≈1.5)        [Desk R-front]
               (-5.6, 5.0)            (0, 5.4)              (5.6, 5.0)
                                    "New session" pill above it
                                     camera ≈ (0, 11, 14)
```

- Desks face **+Z** (keyboard and operator side towards the camera, screens at the back edge facing the camera). Each one is yawed slightly towards the central axis: outer ±18°, inner ±8°, back ±10°, front ±12°.
- **Floor traces** ⟨12⟩: a straight spine from the core to the orrery (T3). A trace from the core to each desk pad, routed with 45° bends (T2 when idle, used for packets). A faint violet hexagon outline around the central zone (T0). Short connectors from the orrery to the front desk pads.
- **Back ledge** ⟨20⟩: a low bench under the windows with small console screens, LED rows and two potted plants, one either side of the core.

---

## 7. Asset specifications (Blender modelling + Bevy setup)

**General approach for assets:**

- **Generate hard-surface assets procedurally with Blender Python scripts.** Commit them under `blender/scripts/` and run them headless with `blender --background --python blender/scripts/<asset>.py -- --out assets/models/<asset>.glb`. This applies to pads, rings, dais, traces, racks, panels, frames, desk shells and bezels. The assets are then reproducible and easy to tweak. If the Blender CLI isn't available in your environment, write the script anyway, tell me, and build simple stand-in geometry in Bevy (tori, annuli, extruded paths) so you can keep going.
- Keep or improve organic assets that already exist (robot, plants). Free CC0 sources: Poly Haven (HDRIs, textures, models), ambientCG (textures), Kenney and Quaternius (props). Earth textures: NASA Visible Earth / Blue Marble.
- Follow the pipeline rules in section 12 (naming, extras, UVs, bevels, export).

### 7.1 Room shell ⟨17⟩⟨20⟩

- **Floor:** large panels (about 1.2 m) with 4–6 mm chamfered seams (real geometry or a trim-sheet normal map). Roughness varies per panel. Use the floor material from 3.5. Add subtle decals or markings (tiny hex patterns, thin arrows, zone numbers) at low contrast.
  **This is the most important surface. It must be dark and glossy, and it must reflect.**
- **Walls:** stepped panels with recessed horizontal light strips (cyan T2, plus a few violet T0 accents as in the target). Add vertical pilasters between window bays, each with a thin vertical strip.
- **Windows:** a panoramic curved band of 6–8 tall panes behind the core and wrapping the back corners. Use bevelled dark mullions (`#1A2640`) with T2 cyan strips on their inner edges and a bright header strip above. Glass is invisible, or an extremely subtle reflective layer at alpha below 0.05.
- **Ceiling:** only its front edge is visible. Rectangular emissive light panels plus a header strip.
- **Back ledge:** a bench with small console screens (T1), rows of tiny LEDs, and two plants in white pots.

### 7.2 Desk pad ⟨11⟩ (crop 08)

- A dark disc platform, Ø 2.8–3.2 m and 8 cm tall, with a slightly recessed inner disc.
- **Outer rim:** a thick neon tube (3–4 cm cross-section) in cyan, T2 when active and T0 when unassigned.
- **Inner ring:** a thin 1–2 cm T1 circle around the inner disc.
- **Accent:** 1–2 amber arc segments (20–30° each) on the front-left of the rim.
- Give the rim tube a UV with **U running around the ring and V across it**, so the energy-ring shader (8.2) can animate dashes, sweeps and the accent arc procedurally.

### 7.3 Desk console ⟨9⟩ (crops 07, 08, 15). Replace the office desks completely.

- About 2.2 m wide, 0.9 m deep and 0.78 m tall. **A sculpted white shell with rounded pill-shaped end-caps** (radius about 0.35 m). The top surface is an inset of dark glass (`#0A1222`), framed by the white shell lip.
- **Base:** two rounded pedestal blocks with panel splits and a vent pattern (**no drawers**), plus a recessed dark underside with a cyan underglow strip (T2) that lights the pad.
- **Lights:** a thin cyan LED line along the front lip (T2), vertical amber light slits on both end-caps (T2 amber), and a holo-projector bar along the back edge (T2 cyan) that the screens float above.
- **Greebles:** a low-profile dark keyboard with faint per-key emissive (T0–T1 texture), a touch pad, 3–5 tiny buttons (amber, pink, white at T2), a small tablet, and a small plant on some desks.
- Bevel everything: Bevel modifier 4–8 mm, 2–3 segments, harden normals, plus Weighted Normal. About 25k triangles maximum.
- Give each desk an empty `ANCHOR_Label` at about 2.1 m height above the screen centre, for the world-anchored status label (10.6).

### 7.4 Holographic screens (crops 07, 08, 09, 15)

- Each desk has **one curved main screen** (about 1.1 × 0.55 m, curve radius about 1.5 m) and **two side panels** (about 0.35 × 0.5 m) angled 25–30° inwards. They float 5–10 cm above the projector bar. **There are no physical monitor bezels.**
- Use the HologramMaterial (8.1): emissive content at T1, a brighter border frame, corner brackets, scanlines, very light flicker, a slight chromatic fringe and additive blending.
- **Content must reflect the real agent state** of that desk's sub-agent (section 11). Two ways to do it:
  - (a) A texture atlas of hologram "content cards" per tool category (search, read, edit/code, terminal, web, plan/todo, thinking, idle), animated with UV scroll and fades. Start with this.
  - (b) Later: render-to-texture UI per desk (a small camera rendering a UI tree into an `Image`, at about 512×256 per screen), showing the real tool name, target file or query, a progress bar, the last output lines and a mini chart.
- Add an optional faint light cone from the projector bar to the screen bottom (an additive gradient quad, very subtle).

### 7.5 Core ⟨7⟩⟨8⟩ (crop 06)

- **Dais:** a 3-step circular platform (Ø about 5.2 / 3.6 / 2.2 m). Each step edge has a neon ring: **outer cyan (T3), middle amber (T2), inner magenta-to-violet gradient (T2)**, plus a cyan disc glow under the robot's feet. Ring segments are dashed and counter-rotate slowly (8.2).
- **Glass cylinder:** radius about 1.3 m, height about 2.6 m, with bright rings at the top and bottom (T3). Use the FresnelGlass material (8.4): an almost invisible centre, glowing cyan edges, faint vertical grid lines, a slow upward scan band, double-sided and additive.
- **Hologram:** a horizontal halo disc at the cylinder's top, with a slowly rotating wireframe or ghost hologram mesh above the robot's head (HologramMaterial in 3D mode). **The hologram shows what the main agent is doing**: a globe for web, a magnifier for search, a document for read/edit, a terminal for bash, a checklist for plan, a neural or orbit swirl for thinking, and a slow idle emblem otherwise. Cross-fade on change (300 ms dissolve plus a glitch).
- **Robot:** keep the model. Upgrade its materials (3.5): glossy white clearcoat, black glass visor, T3 cyan eyes and chest core. Animation: a 2 cm idle bob every 3 s, **the head turns towards the desk that's currently active**, and the eyes change colour with state (section 11).
- **Secondary consoles** behind the robot: keep them, using dark materials and small screens.

### 7.6 Holo orrery / radar table ⟨13⟩ (crop 10). Replace the grey disc.

- A chunky circular bezel (Ø about 3 m, `#2A3550` metal) with armoured segments, notches and small amber indicator lights. Inside is a dark blue bowl.
- **Surface (RadarMaterial, 8.5):** concentric orbit rings, radial spokes every 30°, a bright centre star, 3–8 orbiting blips (orange and cyan), and a rotating conic sweep with a 6 s period. All additive at T1, with blips at T2–T3.
- **Data mapping:** each active session or sub-agent is a blip on its own orbit. A new session spawns a blip with a ripple.
- The **"New session" pill** floats above it (a world-anchored UI pill, 10.6).

### 7.7 Floor traces ⟨12⟩ (crop 11)

- Separate thin strip meshes (2–4 cm wide), slightly inset into a groove or 1 mm above the floor. U runs along the length, and the total length is stored as a uniform or in vertex data so pulses move at constant metres per second.
- Use the FlowTrace material (8.3): a base glow (T0–T2 by importance), a faint idle data trickle, and **event packets** (T3–T4 gaussian pulses) that travel from the core to a desk when work is delegated, and back when results return.

### 7.8 Server racks ⟨14⟩ (crop 14)

- 4–5 tall racks against the right wall. Glossy black body, bevelled. Front panels hold **rows of small horizontal LED bars**. Use LedPanel (8.6): mostly cyan and blue, with about 10% amber, 8% violet and 5% green. They twinkle per LED, and **the twinkle rate scales with token throughput**.

### 7.9 Props

- **Plants:** darker green, double-sided leaves with slight translucency, and white glossy pots with a cyan rim ring. A rim light from the window key separates them.
- **Left wall display:** a large emissive screen (T1) showing a real chart, e.g. actions over time from "Today". Use render-to-texture if available, otherwise a texture.

### 7.10 Exterior space ⟨15⟩⟨16⟩ (crops 12, 13)

All exterior objects are real 3D objects placed far away (100–500 m), so orbiting the camera produces parallax.

- **Sky cubemap:** bake it in Blender as 6 faces at 2048² or larger, rendered with a 90° FOV camera, from a procedural world shader. Stars: two layers of Voronoi or noise with varied sizes and brightness. Nebulae: fbm noise clouds in deep blue `#123A8A`, cyan-blue and violet `#4A2A8A`, low intensity.
  Load the result as a stacked strip reinterpreted as a cube texture, or as KTX2. **The sky must be navy with structure, not black.**
- **Earth-like planet (right):** a **cube-sphere or ico-sphere, never a UV sphere**, so the pole doesn't pinch.
  Day albedo, normal or bump, and a night city-lights emissive map that is masked to the dark side by N·L. Add a separate cloud shell at 1.01× radius (alpha, rotating slightly faster) and an **atmosphere shell** at 1.03–1.05× radius (AtmosphereMaterial, 8.7) with a strong blue-white limb glow.
  Light it from a "sun" direction behind and to the left, so there's a visible terminator. Rotation is very slow.
- **Secondary bodies** (crop 13 and the top of the target): a small Earth-like planet top-left with a lit limb, and a dim, half-shadowed moon top centre-right. Keep the current ringed planet only if it's restyled to match (dim, rim-lit, alpha-textured ring), and don't let it compete with the main planet.
- **Sun or bright star:** a small, very bright emissive disc (T4) that blooms. Replace the current large flat white disc.
- **Optional energy arcs:** thin, branching electric-blue lightning arcs inside the nebula (crop 13). Use additive line meshes that flash on rarely (every 8–20 s, 150 ms), or bake them into the sky if they're static.
- **Asteroid field:** 3–5 rock meshes, 150–300 instances, dark and rim-lit by the sun, tumbling slowly.
- **Ships:** 1–2 small ships with emissive engine glow and nav lights, drifting slowly along a spline in front of the planet ⟨15⟩.

---

## 8. Custom materials and shaders

Implement them as `Material` or `ExtendedMaterial<StandardMaterial, _>` in the shader dialect of your
Bevy version (5.1). All animated parameters read `globals.time`. Expose every parameter in the look config (14.1).

### 8.1 HologramMaterial (screens and 3D holograms)

Uniforms: `color` (linear), `intensity`, `content` texture + `uv_rect`, `scan_density`, `scan_speed`,
`flicker`, `edge_width`, `edge_boost`, `fresnel_power`, `chroma_offset`, `mode` (0 = screen, 1 = 3D ghost), `fade`.

```text
content  = sample(content, uv) (r and b sampled with ±chroma_offset in u)
scan     = 0.82 + 0.18 * sin((uv.y * scan_density + t * scan_speed) * TAU)
flick    = 1 - flicker * step(0.97, hash(floor(t * 14)))    // rare dips
edge     = 1 - smoothstep(0, edge_width, min(min(uv.x, 1-uv.x), min(uv.y, 1-uv.y)))
fres     = pow(1 - saturate(dot(N, V)), fresnel_power)       // mode 1 only
rgb      = (content * color * scan + color * (edge * edge_boost + fres)) * intensity * flick * fade
```

Use `AlphaMode::Add`, unlit, double-sided, and no depth write. Add a soft bottom fade near the projector bar.

### 8.2 EnergyRingMaterial (desk pad rims, dais rings, cylinder rims)

Uniforms: `color_a`, `color_b` (gradient along U), `accent_color` + `accent_range` (u0, u1), `dash_count`,
`dash_duty`, `speed`, `intensity`, `pulse` (0–1, driven by state), `sweep_pos`.
The tube profile across V is `pow(1 - abs(v * 2 - 1), 1.5)`, which gives a bright core and soft edges.
Dashes come from `fract(u * dash_count - t * speed) < dash_duty`. The sweep is a bright gaussian at
`sweep_pos`, used for the desk "boot" animation. Opaque emissive is fine. Leave the halo to bloom.

### 8.3 FlowTraceMaterial (floor traces)

Uniforms: `base_color`, `base_intensity`, `length_m`, `trickle_speed`, and an array of up to 4 packets
`(pos_m, width_m, intensity, color_index)`. The CPU animates the packets from events.
`glow = base + trickle * step(0.96, fract(u_m * 0.8 - t * trickle_speed)) + Σ packet_i * exp(-((u_m - pos_i) / width_i)²)`.

### 8.4 FresnelGlassMaterial (core cylinder)

`alpha = base_alpha (≈0.04) + pow(1 - |N·V|, 3) * edge_alpha (≈0.6)`. Add faint vertical grid lines
(`smoothstep` on `fract(u * 48)`), a horizontal band moving up every 4 s, and emissive along the fresnel term (T2).
Use alpha or additive blending, double-sided, with no depth write.

### 8.5 RadarMaterial (orrery surface)

Work in polar coordinates from centred UVs. Orbit rings at configured radii (thin `smoothstep` lines),
spokes every 30°, a radial gradient (brighter centre), a conic sweep (`exp(-angle_behind_sweep * k)`, 6 s period),
and up to 8 blips from a uniform array `(radius, angle, size, color)` with glow falloff. Use additive blending
over the dark opaque bowl mesh.

### 8.6 LedPanelMaterial (racks, ledge consoles, keyboard keys)

Use a mask texture: R = LED shape, G = palette index, B = random seed. The colour comes from a small palette
uniform. `on = step(0.35, fract(sin(seed * 977 + floor(t * rate * (0.5 + seed))) * 43758.5))`, and
brightness is `mix(0.35, 1.0, on)`. The `rate` uniform is driven by agent throughput.

### 8.7 AtmosphereMaterial (planet shell)

Additive and unlit. `rim = pow(1 - saturate(dot(N, V)), 3.0)`, `lit = saturate(dot(N, L) * 0.6 + 0.5)`,
`color = mix(#2E7BFF, #CFEFFF, rim) * rim * lit * intensity`.
Planet night lights: an `ExtendedMaterial` on the surface that adds `emissive_tex * (1 - saturate(dot(N, L) * 4 + 0.2))`.

---

## 9. Particles and VFX

Use `bevy_hanabi` if a version compatible with your Bevy version exists. If it doesn't, use a small
instanced-quad system that's animated entirely in the vertex shader (no CPU simulation).

- **Room dust motes:** about 300 tiny additive sprites drifting slowly. They're faintly visible near light sources.
- **Core updraft:** particles rising inside the glass cylinder, with the rate scaled by "thinking" intensity.
- **Packet arrival burst:** about 20 sparks where a packet reaches a desk pad.
- **Desk boot:** a ring sweep (8.2 `sweep_pos` 0→1 over 0.6 s), then the screens flicker on (3 quick fades), then the label pops in.
- **Task done:** a green-white T4 flash ring expanding from the pad, then it settles to idle.

---

## 10. HUD (bevy_ui)

### 10.1 Fix first

- **Missing glyph boxes (☐) before "Command", "Replay" and "Activity":** the UI font has no glyph for those characters.
  **Never use emoji or Unicode symbols for icons.** Add an icon font (Lucide, Phosphor or Material Symbols; all permissive licences) and render icons as text spans with that font. Alternatively, use an SVG→PNG icon atlas. Audit every string for symbols.
- **Fonts:** UI font **Barlow** (Regular 400, Medium 500, SemiBold 600) or **Exo 2** if you prefer the current feel. Monospace **JetBrains Mono** for timestamps, tool names and paths. Bundle them in `assets/fonts/`.
  Don't use font sizes below 12 px at the design scale.
- **Scale:** design the HUD on a **1470 × 1070 canvas** (the reference's size) and set `UiScale = window_height / 1070`, clamped to 0.75–1.6. All pixel values below are on that canvas.

### 10.2 Design tokens

| Token | Value |
|---|---|
| Panel background | `BackgroundGradient` linear, top-to-bottom: `#04122E` α 0.80 → `#020B1F` α 0.88 |
| Panel border | 1.5 px `#3AA6FF` at α 0.85 |
| Panel glow | `BoxShadow`: colour `#1E90FF` α 0.45, blur 14 px, spread 0, offset 0. Optional second shadow: black α 0.5, blur 24, offset y 6 |
| Inner top highlight | 1 px line, `#A0DCFF` α 0.22, inset 1 px from the top border |
| Radius | cards 14, sub-cards 10, buttons 10, pills 999 |
| Padding | cards 14–16, rows 6–8 gap |
| Text sizes | title 26 / 300–400 weight; panel title 15 / 600; body 13 / 400; secondary 12; stat value 22 / 500; mono 12.5 |
| Colours | as in 3.3: primary `#EAF4FF`, secondary `#9DB4D6`, muted `#5F7AA0`, accent text `#4CC9FF` |
| Status dot | 8 px circle in the status colour, plus a `BoxShadow` in the same colour (blur 8) |
| Divider | 1 px `#50A0FF` α 0.30 |

### 10.3 Component library (build these once and reuse them)

`Panel(title?, icon?, badge?)`, `Pill(icon, text, variant)`, `IconButton(icon, label, variant)`,
`StatusDot(state)`, `MetricRow(icon, label, value)`, `Sparkline(values)` (vertical bars 3 px wide with 2 px gaps,
gradient `#1C6DD0` → `#5FE3FF`), `ProgressRing(fraction, color)` (a `ConicGradient` with hard stops plus an
inner masking circle), `TabBar(tabs)`, `LogRow(time, kind, text)`, `StatCard(title, value, icon/ring)`,
`WorldLabel(icon, text, state)`.

Interaction states: on hover, the border goes to `#59C8FF` and the glow α to 0.7, over 120 ms. On press,
the fill darkens 10%. On focus, there's a 2 px outline. Disabled is 45% opacity.

### 10.4 Layout map (design canvas 1470 × 1070; ⟨n⟩ from the annotated image)

| ⟨n⟩ | Panel | Rect (x0–x1, y0–y1) | Anchor |
|---|---|---|---|
| ⟨1⟩ | Room title card | 30–245, 12–92 | top-left |
| ⟨2⟩ | Agent Activity (was "Today") | 20–303, 105–278 | top-left |
| — | "+ New Agent" pill (only if the app supports spawning) | 15–120, 308–348 | top-left |
| ⟨3⟩ | Main agent status card | 566–905, 12–92 | top-centre |
| ⟨4⟩ | Agent count / runtime card | 1225–1440, 12–92 | top-right |
| ⟨5⟩ | Quick Actions | 1295–1455, 115–297 | top-right |
| ⟨6⟩ | Command panel (prompt + tabs) | 495–992, 155–277 | top-centre |
| ⟨18⟩⟨19⟩ | Live Activity Log + stat cards | 18–1455, 875–1032, with a glowing cyan line under its bottom edge | bottom, full width |

The HUD must not cover the robot (36–40%) or the orrery (69–73%). If the Command panel overlaps the
hologram at some window sizes, let it collapse to a single line plus the tab bar.

### 10.5 Panel contents and data bindings

Map today's features onto the new layout. **All values must come from real state.**

| Panel | Content |
|---|---|
| ⟨1⟩ Title | Room or project name (26 px) and subtitle with a status dot ("● HQ of <project>" or the working directory). |
| ⟨2⟩ Agent Activity | Header "Agent Activity" with a **Live** badge (green dot) when streaming. A sparkline of actions per minute over the last N minutes. Rows: Actions/min, Tokens in / out, Uptime (or session duration). This replaces the old "Today" label; keep its existing data and add what's available. |
| ⟨3⟩ Main agent | A lightning icon, then "<model> · main agent · <state>" (idle, thinking, running <tool>, waiting for approval, error), with the model name at 24 px and the rest at 18 px. Second row: a status chip ("All done", "Working…" with a spinner), a vertical divider, then **context usage** if the app has it ("Context 62% of 200K" with a small progress ring that goes amber above 80% and red above 95%). See crop 02. |
| ⟨4⟩ Agent count | "<n> agents" plus a state chip ("✓ Active" in green or "Idle" in muted). Second row: "<elapsed> · <out tokens> out tokens". Show the **Auto-accept ON** indicator here as an amber pill. |
| ⟨5⟩ Quick Actions | Only actions the app really supports, e.g. Pause/Resume agent, View plan/todos, Changes (diff), Replay, Export. Each is an `IconButton` with a 28 px circular icon badge. |
| ⟨6⟩ Command | Icon and title "Command", then the **current user prompt** (2 lines max, ellipsised, click to expand). The tab bar: Settings, History, Chat, Changes, Commands, Replay, each with an icon. The active tab gets an underline glow. |
| ⟨18⟩ Live Activity Log | The existing Activity log, restyled: a vertical timeline with coloured nodes (agent message = cyan, tool call = green, tool result = muted, error = red, approval needed = amber). Each row shows a mono timestamp, then "Agent:" in accent or "Tool:" in white with a green dot, then the tool name in mono `#3EE0A0`, then a short result summary. The newest entry slides in (150 ms). "View all →" opens the full log. |
| ⟨19⟩ Stat cards (4) | **Tasks** done/total from the agent's todo list, with a green progress ring. **Elapsed** for the current turn (replaces the fake "Time Est." unless a real estimate exists). **Context** % (or tokens/s; **not** "Confidence"). **Next action**: the next pending todo item, with →. |
| "New session" | A world-anchored pill above the orrery (10.6) that triggers the existing New session action. |

### 10.6 World-anchored labels ⟨10⟩

- Each frame, project the `ANCHOR_Label` world position with `Camera::world_to_viewport` and position an absolute UI node at that point. Centre it horizontally, with its bottom edge 6 px above the anchor.
- Style: a pill with radius 10, height about 32, the panel fill at α 0.85, a 1 px border at α 0.7, a glow blur of 10, a 22 px circular icon badge, 13 px text, and a maximum width of about 190 px with an ellipsis.
- Text is the sub-agent's current activity in plain words, e.g. "Reading src/app.rs…", "Running cargo test…", "Searching 'auth'…". The icon is set by tool category.
- Fade in and scale from 0.92 when a desk becomes active (180 ms). Fade out 8 s after the desk goes idle. Hide the label when its anchor is behind the camera or off-screen.

### 10.7 UI motion

Panels fade and slide 6 px on first appearance (staggered 40 ms). Number changes tween (200 ms).
The Live badge dot pulses (1.6 s). There are no constant fast animations in the HUD.

---

## 11. Agent state → visual mapping (this makes it a visualisation, not wallpaper)

| Event or state | Visual response |
|---|---|
| Main agent **idle** | Dais rings rotate slowly at T2, the hologram shows a slow idle emblem, the robot's eyes are cyan at 60% and breathing, dust drifts. |
| **Thinking / streaming** | Ring speed ×3, inner ring brightens to T3, the core updraft runs, the hologram swirls, the robot's eyes blink in a pattern, ⟨3⟩ shows "thinking", and the Live badge is on. |
| **Tool call (main agent)** | The hologram morphs to the tool's category emblem. A packet travels along the spine to the orrery for web or search tools, or pulses on the dais for local tools. A log row is added. |
| **Sub-agent spawned** | The next free desk boots (9). Its label shows the task description. A packet goes from the core to that desk. A new blip appears on the orrery. The robot's head turns towards it. |
| **Sub-agent tool call** | The desk's screens switch to that tool's content card. The label updates. The LED rate on the racks goes up. |
| **Result returned** | A packet travels from the desk back to the core, with a short flash on arrival at the core. |
| **Awaiting approval** | Amber pulse on the core's middle ring and on the relevant desk rim. An amber log node. ⟨3⟩ says "waiting for approval". |
| **Error** | A brief magenta/red glitch on the hologram or desk screens (2 × 60 ms) and a red log node. Never a full-screen flash. |
| **Task done** | A green-white T4 ring flash, then idle. The todo counter animates. |
| **Desk idle** for more than 30 s | The rim fades to T0, the screens fade to 20%, and the label fades out. |

Tool category → icon, emblem and content card. Keep accent colours rare; most of these stay in the blue family.

| Category | Tools | Icon |
|---|---|---|
| Search | Grep, Glob, search tools | magnifier |
| Read | file reads | document |
| Edit | Edit, Write | code brackets |
| Terminal | Bash | terminal |
| Web | WebFetch, WebSearch | globe |
| Plan | todo, plan | checklist |
| Delegate | Task, sub-agent | branching nodes |
| Other | anything else | gear |

---

## 12. Blender → Bevy pipeline rules

- **Units and orientation:** 1 unit = 1 m. Apply all transforms before export. Object origin at floor centre. Export glTF binary (`.glb`) with +Y up (the default).
- **Export settings:** Apply Modifiers ✓, UVs ✓, Normals ✓, Tangents ✓ (normal maps), **Custom Properties ✓** (become `GltfExtras`), Materials ✓, **no Draco compression** (Bevy can't load it). Emission strength above 1 exports as `KHR_materials_emissive_strength`, which Bevy reads.
- **Naming:** meshes `SM_<Asset>_<Part>`, materials `M_<Name>`, anchors as empties (`ANCHOR_Label`, `SLOT_Desk_0..7`, `ANCHOR_CoreLight`, …).
- **Material override convention** (this is how Blender assets get Bevy custom shaders): a material named `M_Emit_<Tier>_<Color>` (e.g. `M_Emit_T2_Cyan`) is replaced at load time by a StandardMaterial using that tier and palette colour from the look config. `M_Holo_*` becomes HologramMaterial, `M_Ring_*` becomes EnergyRingMaterial, `M_Trace_*` becomes FlowTraceMaterial, `M_Glass_*` becomes FresnelGlassMaterial, and `M_Led_*` becomes LedPanelMaterial. Read extras for parameters (e.g. `ring_accent_range = "0.55,0.62"`). Run the replacement in an observer or system after scene instancing.
- **UVs:** UV0 for textures. **UV1 for lightmaps** (non-overlapping, with a margin) on all static, lightmapped meshes. Rings and traces need meaningful U and V (sections 7.2 and 7.7).
- **Bevels:** Bevel modifier (Angle limit 30°, 4–8 mm, 2–3 segments, Harden Normals) plus Weighted Normal (Keep Sharp) on all hard-surface parts.
- **Texel density:** 512–1024 px/m on hero props, 256 px/m plus a tiled detail normal on the floor and walls. Use a shared 2k trim sheet (seams, vents, bolts, grooves) for the shell.
- **Textures:** KTX2 with full mip chains (BC7 or ASTC; enable Bevy's `ktx2` and `zstd` features). Albedo and emissive are sRGB. Normal, metallic-roughness and masks are linear.
- **Budgets:** room shell ≤150k triangles, desk ≤25k (instanced ×8), core ≤60k, robot ≤30k, exterior ≤80k plus instancing. Merge static meshes per material. Reuse mesh and material handles so Bevy batches them.
- **Separate files:** `room_shell.glb`, `desk_pod.glb` (console, pad and screens as children), `core.glb`, `orrery.glb`, `racks.glb`, `props.glb`, `exterior_*.glb`.

---

## 13. Implementation plan (in order of impact per effort)

Each phase ends with the verification loop (section 14) and a report (section 17). **Don't start the
next phase until I've reviewed the report**, unless I tell you to continue on your own.

### Phase 0: Audit and tooling (no visual changes yet)

- Report the Bevy version, the camera setup, every light, the ambient level, tonemapping, bloom settings, how materials are created, how glTF is loaded, how the UI is structured, and the agent-state data model.
- Find the tofu glyphs and the yellow floor bar.
- Add the look config with hot reload, an inspector toggle (F1, e.g. `bevy-inspector-egui` if it's compatible with your version), the hero camera and screenshot hotkey and CLI flag, an FPS and frame-time overlay, and a **demo/replay mode that feeds a scripted sequence of agent events** so every state in section 11 can be triggered on demand.
- Capture baseline screenshots and metrics.

### Phase 1: Value and colour (largest single win)

- Kill the ambient light. Darken all base colours to the palette.
- Set the emissive colours to saturated hues with the tier ladder.
- Set up tonemapping, exposure, bloom, colour grading, deband and vignette.
- Remove white emissives.
- **Pass when** at least 5 of the 7 metrics are within tolerance and nothing looks milky grey.

### Phase 2: Lighting and reflections

- Switch to deferred. Add SSAO, SSR, TAA + CAS and contact shadows.
- Build the lighting rig (5.3) and the space skybox + environment light. A temporary sky is fine until Phase 6.
- Bake the interior reflection probe.
- Make the floor material glossy.
- **Pass when** the floor visibly reflects glow lines, desks and robot have rim light, and there's light pooling around the core, the orrery and the screens.

### Phase 3: HUD rebuild

- Fix fonts and icons. Build the tokens and component library, then the full layout (10.4) with real data bindings (10.5) and world labels (10.6).
- **Pass when** there's no tofu, every panel matches the crops' styling, and every number is real.

### Phase 4: Hero assets

- New desk console, desk pad, core dais, cylinder, hologram mount, orrery bezel and room shell details. Make them procedural via bpy scripts where possible, bevelled, and using the material override convention.
- **Pass when** side-by-side crops (e.g. crop 07 against the same region of your screenshot) read as the same class of object.

### Phase 5: Shaders

- HologramMaterial, EnergyRing, FlowTrace, FresnelGlass, Radar, LedPanel and Atmosphere, all wired to the look config.

### Phase 6: Exterior

- Sky cubemap bake, planet with clouds, atmosphere and night lights, gas giant, sun, asteroids and ships.

### Phase 7: Motion and state mapping

- Implement section 11 completely, plus the particles (section 9). Verify every state in demo mode, and record a short clip if possible.

### Phase 8: Polish and performance

- Lightmap bake for the shell, decals, optional volumetric haze, quality presets (5.5), the performance pass (section 15) and the final screenshot set.

---

## 14. Verification loop (after every meaningful change)

### 14.1 Look config

Create `assets/look.ron`, or an equivalent file, holding **every** tunable value: exposure, tonemapper, bloom, grading,
ambient, every light (colour and intensity), emissive tier nits, palette hex values, shader parameters, the camera
hero transform and FOV, and HUD tokens. Reload it while the app runs (watch the file's modification time in debug
builds, or use asset hot reloading), and expose it in the inspector. Never hard-code a look value in a system.

### 14.2 Screenshot and compare

1. Run the app in demo mode with a representative state: the main agent thinking, 5 desks active, a few log rows, and Auto-accept on.
2. Capture the hero screenshot with `--hero-screenshot screenshots/phaseN.png`, or press F12.
3. Run the metrics from inside this folder (e.g. `docs/agent-room-visual-overhaul/`), passing the screenshot's absolute path:
   `python3 tools/look_metrics.py <repo>/screenshots/phaseN.png --reference reference/target_reference.png --side-by-side <repo>/screenshots/phaseN_compare.png`
   The script needs `pillow` and `numpy`. The exit code is 0 when all metrics are within tolerance.
4. **Open the side-by-side image and the relevant crops and look at them yourself.** List the 5 biggest remaining visual differences in order of impact. Fix the top ones, then repeat.

### 14.3 Self-review checklist

- [ ] The floor is dark navy and glossy, and reflects glow lines and white desks.
- [ ] Glow lines have white-hot cores with saturated cyan halos. No pale or whitish strips.
- [ ] About 70% of the frame is dark, and glow is limited to lines, screens and indicators.
- [ ] Every hard edge is bevelled and catches a highlight. The rim light from the window is visible on desks and the robot.
- [ ] Desks read as sculpted white sci-fi consoles with LED trim. There are no office drawers left.
- [ ] Screens are emissive holograms with borders, scanlines and real, state-driven content.
- [ ] The core has a visible fresnel cylinder, a hologram, and thick cyan, amber and magenta rings.
- [ ] The orrery is a glowing holographic table, not a grey disc.
- [ ] The sky is navy with nebulae. The planet has an atmosphere rim, a terminator and night lights. There's no pole pinch.
- [ ] The HUD matches the crops: glowing borders, translucent navy fill, consistent radii and hierarchy. No tofu. Real data only.
- [ ] Framing matches section 4.
- [ ] Something subtle moves everywhere, and every agent event produces a visible reaction within 100 ms.
- [ ] 60 fps on the Medium preset on the target machine.

---

## 15. Performance budget

Target **60 fps at the native window resolution on Apple Silicon** (M1 Pro class or better) on the Medium preset.
Approximate GPU budget: opaque and deferred lighting 4 ms, SSAO 1 ms, SSR 1.5 ms, bloom 0.7 ms, TAA and CAS 0.5 ms,
transparents and holograms 1 ms, UI 0.5 ms, particles 0.3 ms, volumetrics 2 ms (High only).

- Two shadow-casting lights at most. Instance repeated objects. Merge static meshes. No per-frame material allocation (mutate uniforms instead).
- Limit render-to-texture screens to the active desks, at 15–30 Hz.
- Since this is an always-on companion app, offer a power saver: 30 fps when the window is unfocused, and 10 fps when nothing has changed for 60 s. Ambient animation may slow down then.

---

## 16. Do and don't

**Do**

- Use saturated emissive colours, and set brightness through intensity.
- Make the darks navy, not grey.
- Bevel everything.
- Reflect glow in the floor.
- Use practical lights near emissives.
- Use real data everywhere.
- Use an icon font.
- Measure after each change.
- Keep the look config as the single source of truth.

**Don't**

- Raise ambient to brighten the scene.
- Use white as an emissive colour.
- Make large surfaces glow.
- Use UV spheres for planets.
- Draw HUD text as 3D geometry.
- Use emoji or symbol characters as icons.
- Invent metrics.
- Add fast constant animation.
- Upgrade Bevy or add heavy dependencies without asking.
- Remove existing features or shortcuts.
- Commit huge binaries without checking whether the repo uses Git LFS.

---

## 17. Report format (end of every phase)

1. **What changed:** bullet list, with the files touched.
2. **Screenshots:** the hero shot path, the side-by-side path, and crops of the elements you changed next to the reference crops.
3. **Metrics table** from `look_metrics.py`, compared with the previous phase.
4. **Top 5 remaining differences** from the target, in order of impact, with your plan for each.
5. **Performance:** average and 1%-low FPS and frame time on the Medium preset.
6. **Open questions or decisions** that need my input (e.g. a Bevy upgrade, a missing data source, an asset licence).
