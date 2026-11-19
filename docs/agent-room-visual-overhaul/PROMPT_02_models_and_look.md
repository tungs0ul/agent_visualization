# PROMPT 02: Rebuild the models (Blender) and finish the look (Bevy)

> For the coding agent. This continues PROMPT.md and FIX_01. **Path A is decided:** the scene stays fully
> real-time 3D, with free **rotate and zoom**. All models come from Blender through the project's
> **existing Blender script pipeline**. Extend that pipeline; don't invent a parallel one.
> Work step by step. Report after each step (PROMPT.md section 17) and wait for review.

---

## 0. Context and quality bar

The image is now clean and stable (FIX_01). What still makes it look cheap is **content**. Measured on the latest
screenshot, against `reference/target_reference.png`:

- **Too bright and milky again.** Median luminance is 0.26 (target 0.20). Only 20% of pixels are dark (target 36%). Glow pixels are 5.7% (target 10.3%).
- **Screens** are blown-out white slabs with floating "paper sheet" cards.
- **Desks** are office desks with drawers.
- **Radar** is a grey mechanical disc.
- **Floor** is light, matte grey-blue with no reflections. Rings are wide, pale halos instead of thin, intense tubes.
- **Space outside:** the moon is a giant flat white disc, the planet has no atmosphere, and the sky is black.
- The camera is slightly too close: windows are cut at the top and the orrery sits at about 85% of the frame height.
- A stray dark blue bar lies on the floor at the bottom right (formerly yellow).

**Because the user can orbit and zoom, every model must hold up from every angle and up close.**

- Model all sides (backs, undersides, insides of rings). No front-only facades.
- Hero detail must still read at 2× zoom.
- Give the camera rig limits so it can never see outside the room shell (section 9).

**Quality gate for every asset:** before integrating an asset, render preview images with the project's Blender render
script (front three-quarter, side and close-up), using a neutral dark studio rig. Put them next to the matching
crop in `reference/crops/`. Only integrate once it reads as the same *class* of object. Include the previews in your report.

---

## 1. Blender pipeline rules (apply to every asset below)

- **One parametric script per asset** in the existing Blender scripts folder, e.g. `desk_console.py`, `desk_pad.py`, `core_dais.py`, `holo_cylinder.py`, `orrery.py`, `floor.py`, `walls_windows.py`, `ceiling.py`, `back_ledge.py`, `server_rack.py`, `planet.py`.
  Dimensions and colours sit at the top as named constants or CLI args. Each script builds the asset, applies modifiers, sets up materials, and exports a `.glb` into `assets/models/`. All of it runs headless: `blender -b -P <script> -- --out <path>`.
- **Units and orientation:** 1 unit = 1 m. Origin at floor centre. Transforms applied. +Y up on export. Tangents ✓, Custom Properties ✓, Apply Modifiers ✓, **no Draco**.
- **Bevel every hard edge.** Bevel modifier, angle limit 30°, 4–8 mm (2–3 mm on small parts), 2–3 segments, Harden Normals. Then a Weighted Normal modifier (Keep Sharp). **This is non-negotiable.** It's what makes edges catch light.
- **Real geometry for panel splits.** Grooves 3–5 mm wide and 3 mm deep where the reference shows panel lines. Use inset + extrude-inward or boolean cuts, applied and cleaned.
- **Emissive parts are separate mesh pieces** with their own material, so Bevy can drive them individually.
  Use the naming convention from PROMPT.md section 12: `M_Emit_T2_Cyan`, `M_Ring_*`, `M_Holo_*`, `M_Led_*`, `M_Glass_*`.
- **UVs:** UV0 is clean and unwrapped. Rings and strips have **U along the length and V across**, normalised. Static room parts get **UV1 for lightmaps**.
- **Empties** for runtime hooks: `ANCHOR_Label` (above a desk's screens), `ANCHOR_ScreenMain`, `ANCHOR_ScreenL`, `ANCHOR_ScreenR`, `ANCHOR_Light`, `SLOT_Desk_0..7`.
- **Budgets:** desk ≤ 25k triangles (instanced ×8), core ≤ 60k, orrery ≤ 20k, room shell ≤ 150k.
- **LOD:** if zooming out far makes small greebles shimmer, export a simplified LOD1 and switch on distance. Otherwise skip LODs.

---

## 2. Step 1: Look pass (Bevy, no new models yet)

Fix the value balance first, so new models get judged under the right light.

1. **Darken the large surfaces:** floor `#0E1A33`, walls `#122241`, ceiling `#0B1428` (PROMPT.md 3.5).
   Lower the environment and ambient light until the dark fraction is 30–40%. Don't touch emissives to do this.
2. **Make the floor glossy:** metallic 0.75, roughness about 0.30 (varied per panel), clearcoat 0.4.
   Reflections come from the baked interior reflection probe (FIX_01 step 2), not from SSR.
3. **Rings and strips:** thinner and more intense. Use saturated `#38C8FF` at the T2/T3 tiers.
   **Remove any wide additive "halo plane" on the floor around the rings.** Bloom makes the halo. A pad rim should read as a thin bright tube with a white core.
4. **Screens:** stop the clipping. The screen background is dark navy and only the content glows. They'll be replaced in step 3.
5. **Camera:** pull back and up a little until the framing targets from PROMPT.md section 4 hold. The whole window band is visible, the robot's head sits at 36–40% and the orrery at 69–73%.
6. **Find and remove the stray bar** on the floor (bottom right). Report its entity and mesh name.
7. **Pass when:** median luminance is 0.18–0.23, dark fraction 30–40%, and the floor visibly reflects the rings.

---

## 3. Step 2: Desk console. Replace the office desks completely (crops 07, 08, 15)

**Shape, top view:** a wide "stadium" (a rectangle with semicircular ends), 2.2 m × 0.85 m.
The front edge (operator side, facing +Z) is slightly concave, a 3 m-radius arc, so it hugs the operator.

**Build recipe** (`desk_console.py`):

1. **Shell:** draw the stadium outline as a curve, convert it to mesh and extrude it 0.10 m. That makes the tabletop slab, with its top at 0.78 m.
   Inset the top face by 0.05 m and push it down 6 mm to form a **dark glass top inset** (`M_DeskTop_Glass`, `#0A1222`, roughness 0.16). The rim stays the white shell (`M_DeskShell_White`).
2. **End caps:** at each rounded end, a vertical rounded "pod" 0.30 m wide, running from the floor to the tabletop. Its outer face is curved to follow the stadium end.
   Cut a **vertical light slit** in each end cap: 0.25 m × 0.02 m, recessed, with a separate emissive piece (`M_Emit_T2_Amber`).
3. **Pedestal:** a single centre pedestal block, 1.2 m wide, 0.45 m deep and 0.68 m tall, set back 0.15 m from the front edge.
   It has rounded vertical edges (radius 0.06 m), **two horizontal panel grooves** and a small vent grille (an array of slots).
   **No drawers.** Shell material, plus a dark recessed band at the bottom (kick plate, `#0A1222`).
4. **Underside:** recessed and dark. Add a **cyan underglow strip** following the stadium outline 0.03 m inset (`M_Emit_T2_Cyan`), facing down. It lights the pad.
5. **Front lip LED:** a thin strip (0.006 m tall) along the concave front edge, just under the lip (`M_Emit_T2_Cyan`).
6. **Holo-projector bar:** along the back edge, 1.6 m long, 0.05 m deep and 0.03 m tall. Dark metal body with an emissive top face (`M_Emit_T3_Cyan`). This is what the screens appear to project from.
7. **Greebles on the top inset:**
   - **Keyboard:** a low dark slab, 0.45 × 0.15 m, with a key grid from an array of 0.014 m keycaps. Keycaps use `M_Led_Keys` (a faint per-key glow).
   - **Touch pad:** 0.12 × 0.09 m, dark glass with a thin cyan outline.
   - **Buttons:** 3–5 round 0.012 m buttons in amber, pink and white (`M_Emit_T2_*`).
   - **Tablet:** a small slab at a slight angle.
   - Optionally a small plant on some desks (reuse the existing plant).
8. **Bevel and weighted normals everything** (section 1). Shell material: `#D9E2EE`, roughness 0.22, clearcoat 0.8, clearcoat roughness 0.08.
9. **Empties:**
   - `ANCHOR_ScreenMain`: 0.95 m high, centred over the projector bar.
   - `ANCHOR_ScreenL` and `ANCHOR_ScreenR`: at the bar's ends, rotated 28° inwards.
   - `ANCHOR_Label`: 2.1 m high.
   - `ANCHOR_Light`: just in front of the main screen, for the desk light.

Remove the old desk model, the drawers and the floating "paper/document" cards from all 8 slots. Keep the existing slot positions and yaw values (PROMPT.md section 6).

---

## 4. Step 3: Holographic screens (crops 07, 09, 15)

- **Geometry** (in `desk_console.py` as children, or a separate `holo_screens.glb`):
  - Main screen: 1.1 × 0.55 m, curved (Simple Deform Bend, radius about 1.5 m).
  - Two side panels: 0.32 × 0.48 m.
  - **No bezels.** Each screen is a plane plus a separate thin border frame mesh (a 0.008 m wide outline with 0.05 m corner brackets).
  - Materials `M_Holo_Screen` and `M_Holo_Border`.
- **Shader:** HologramMaterial from PROMPT.md 8.1, with these settings:
  - **Dark translucent navy background** (`#06122C` at α 0.55), not white. Content glows at T1, the border at T2.
  - Scanline amplitude at most 0.1. Very rare flicker. Faint bottom fade towards the projector bar.
- **Content:** build a texture atlas (2048², 4×4 cells) of hologram "content cards", one or two per tool category: search, read, edit/code, terminal, web, plan/todo, thinking, idle.
  - Generate the cards programmatically: a Python + Pillow script, or render-to-texture UI in Bevy.
  - Style: dark background, cyan and blue line charts, bar charts, code lines, node graphs, a starfield or radial graphic, and small orange or violet accents (see the crops).
  - Side panels use narrow cards: lists, gauges, vertical bars.
  - Animate them with slow UV scroll and value jitter. Swap cards with a 250 ms dissolve when the desk's tool category changes (PROMPT.md section 11).
- **Lighting:** one `RectLight` (fallback `PointLight`, range 3.5 m) at `ANCHOR_Light`, colour `#3D9BFF`, so the screens tint the desk top and keyboard blue. Turn it off when the desk is idle.

---

## 5. Step 4: Desk pad (crop 08)

`desk_pad.py`:

- A disc, Ø 3.0 m and 0.08 m tall, with a 0.02 m chamfered top edge. Top material `#0D182E`, metallic 0.7, roughness 0.35.
- An inner recessed disc, Ø 1.6 m and 3 mm down, with a thin `M_Ring_Inner` circle on its edge (a tube with a 0.01 m cross-section).
- **The outer rim is a separate tube**: a torus with major radius 1.48 m and a 0.035 m cross-section, sitting in a groove (`M_Ring_Rim`, U around the ring and V across).
  The EnergyRing shader (PROMPT.md 8.2) draws dashes, the boot sweep and an **amber accent arc** (about 25° at the front left).
- Pad and console are one prefab (`desk_pod.glb`), with the console as a child.

---

## 6. Step 5: Core (crop 06)

`core_dais.py`:

- **Three stepped tiers**: Ø 5.2 / 3.6 / 2.2 m, each 0.12 m tall, with bevelled edges and dark metal tops.
- **Each step edge carries a neon tube:**
  - Outer: `M_Ring_CoreOuter`, cyan T3, 0.04 m cross-section.
  - Middle: `M_Ring_CoreMid`, amber T2.
  - Inner: `M_Ring_CoreInner`, a magenta-to-violet gradient along U at T2.
  - Plus a cyan glow disc under the robot.
- **Segmented arcs:** add 3 partial-arc pieces (90–140° each) floating 5 mm above the outer tier. They rotate slowly at runtime (child entities).

`holo_cylinder.py`:

- An open cylinder, radius 1.3 m and height 2.6 m, with 64 segments and **U around and V up**. Material `M_Glass_Cylinder` (FresnelGlass, PROMPT.md 8.4).
- Separate rim tubes at the top and bottom (`M_Ring_CylRim`, T3).
- A horizontal **halo disc** at the top: an annulus from 1.0 to 1.3 m (`M_Holo_Halo`).

**Hologram above the robot:** a set of simple wireframe emblem meshes, one per state (globe, magnifier, document, terminal, checklist, orbit swirl, idle emblem).
Generate them in Blender: use a Wireframe modifier on low-poly shapes, then export. Use HologramMaterial in 3D mode with a slow spin, cross-faded on state change.

**Robot:** keep the model and upgrade its materials only (PROMPT.md 3.5). Glossy white clearcoat, black glass visor, T3 eyes and chest light.

---

## 7. Step 6: Holo orrery (crop 10). Replace the grey disc.

`orrery.py`:

- **Bezel ring:** outer Ø 3.0 m, inner Ø 2.5 m, 0.18 m tall. Built from **12 armoured segments** with 0.02 m gaps between them, alternating heights (0.18 / 0.15 m).
  Add small notch details and 4 tiny amber indicator lights. Material `#2A3550`, metallic 0.9, roughness 0.28.
- **Bowl:** a shallow concave dish inside the bezel, dark `#04203F`.
- **Surface:** a flat disc 1 cm above the bowl with `M_Holo_Orrery` (RadarMaterial, PROMPT.md 8.5).
- **Rim light:** a thin cyan inner rim tube on top of the bezel (T2).
- **Light:** a `PointLight` 0.5 m above the surface, `#2E8BFF`.
- **Data:** blips are the active sessions and sub-agents. Keep the "New session" pill above it.

---

## 8. Step 7: Room shell (crops 11, 13, 14 and the annotated target)

- **`floor.py`:**
  - 1.2 m panels with real 4 mm chamfered seams.
  - Per-panel roughness variation, via a vertex colour or a small roughness texture.
  - Floor material from step 1. UV1 for lightmaps.
- **Floor traces:**
  - Thin strips (0.03 m wide) set in 3 mm grooves.
  - A spine from the core to the orrery.
  - 45°-bent paths from the core to each pad.
  - A violet hexagon outline around the centre.
  - Material `M_Trace_*` (FlowTrace, PROMPT.md 8.3).
  - Delete any old wide glowing floor lines.
- **`walls_windows.py`:**
  - Bevelled dark mullions with inner-edge cyan strips.
  - A bright header strip above the windows.
  - Stepped lower wall panels with recessed horizontal strips (cyan, plus a few violet).
  - Pilasters with vertical strips.
- **`ceiling.py`:**
  - A closed ceiling with rectangular emissive light panels and a header strip.
  - **Required** so orbiting never shows outside the room.
- **`back_ledge.py`:**
  - A bench under the windows with small console screens (`M_Holo_*`) and LED rows (`M_Led_*`).
  - Keep the two plants.
- **`server_rack.py`:**
  - Glossy black racks, bevelled.
  - Front panels with rows of LED bar slots (`M_Led_Rack`, LedPanel shader, PROMPT.md 8.6).
  - 4–5 instances on the right wall.
- **Left wall display:** an emissive screen with HologramMaterial and real "Today" chart content.

---

## 9. Step 8: Space outside and camera rig (crops 12, 13)

- **`planet.py`:**
  - The surface is an **ico-sphere or cube-sphere**, never a UV sphere.
  - A cloud shell at 1.01× radius and an atmosphere shell at 1.04× radius (AtmosphereMaterial, PROMPT.md 8.7).
  - Day texture plus a night-lights emissive texture (NASA Blue Marble / Black Marble, public domain).
  - Lit from behind and to the left so there's a visible terminator.
- **Moon:** replace the giant white disc with a **small, dim, half-shadowed cratered moon**.
- **Sun:** a small, very bright emissive disc (T4) that blooms.
- **Sky:** bake a navy nebula cubemap in Blender (6 faces at 2048²; procedural noise nebula in `#123A8A` and `#4A2A8A`, plus two star layers). Never pure black.
- **Asteroids:** an instanced asteroid field.
- **Ships:** 1–2 small ships with engine glow, drifting slowly.
- **Camera rig (orbit and zoom):**
  - Orbit around the core. Yaw is unlimited, or ±60° if the back of the room isn't detailed.
  - Pitch 25–65°. Zoom distance 6–22 m, with smooth damping.
  - **Collision clamp** so the camera never leaves the room shell or clips through the ceiling.
  - A "home" key returns to the hero view.
  - At close zoom, labels must not overlap: hide those whose anchors are within 40 px of each other, and keep the active one.

---

## 10. Order, checks and report

**Order:** step 1 → step 2 (+3 together, desks and screens are one pod) → step 4 → step 5 → step 6 → step 7 → step 8.
The HUD rebuild (PROMPT.md section 10, Phase 3) can run in parallel or right after step 3.

**After each step:**

1. Blender previews of new assets next to the reference crops.
2. In-engine hero screenshot, plus **3 orbit shots**: rotated left 40°, rotated right 40°, and zoomed in on one desk.
3. `tools/look_metrics.py` on the hero shot.
4. The stability check: two shots 0.5 s apart, no speckle at 2× zoom.
5. The top 5 remaining differences from the target, and FPS.

**Don't:**

- Model with drawers, paper or document cards, white emissive screens, or wide halo planes on the floor.
- Make front-only facades.
- Use UV spheres for planets.
- Re-enable SSAO, SSR or TAA (FIX_01 rule).
- Invent HUD data.
