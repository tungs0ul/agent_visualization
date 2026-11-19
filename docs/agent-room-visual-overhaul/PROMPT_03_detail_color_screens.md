# PROMPT 03: Detail, colour variety and screen art

> For the coding agent. This continues PROMPT_02. Step 1 (the look pass) worked: **all 7 tonal metrics now match
> the target.** But the image still looks bland. `tools/look_metrics.py` now measures the remaining gaps too:

| Metric | Build | Target | What it means |
|---|---|---|---|
| edge_density | 0.137 | 0.219 | About 40% less visible detail than the target |
| flat_tiles | 32% | 18% | Large featureless areas: walls, floor, ceiling, desk sides |
| warm_accent_fraction | 0.000 | 0.002 | **No amber or orange anywhere** |
| violet_magenta_fraction | 0.000 | 0.003 | **No violet or magenta anywhere** |
| white_hardware_fraction | 0.010 | 0.031 | Desks, robot and pots don't read as white. Everything is tinted the same blue. |

**The core problem: one hue covers everything.** Walls, floor, desks, frames and lights are all the same blue, so nothing separates.
In the target:

- **Surfaces are neutral:** cool white hardware, silver and charcoal metal, navy floor.
- **Colour comes from light and emissives.** That's mostly cyan and blue, plus small, deliberate accents in amber, orange, magenta and violet.
- **Screens are illustrations, not text.**

Run `look_metrics.py` after every task here. All 12 metrics must pass at the end, **and** the side-by-side has to read as the same class of image.

Order of work: **A → B → C → D → E**. The new desk model (PROMPT_02 step 2) is part of C and is the single biggest detail gain. Do it now.

---

## A. Colour separation (materials and lights)

1. **Hardware must read white.**
   - Desk shells, robot, pots and the cylinder rims use `#D9E2EE` at roughness 0.2 with clearcoat. Check that their base_color isn't blue-tinted.
   - Add a **soft near-white top light**: a large `RectLight` or a low-intensity `DirectionalLight` from above, colour `#E8F0FF`, so the white surfaces get a bright neutral top face.
   - Shift the window key light from `#9CC2FF` towards `#B8D4FF`.
   - Blue should come from emissives, screens and rim light, not from every light.
2. **Metal is silver or charcoal, not blue.**
   - Window mullions, pillars, bezels and frames: `#9AA6B8` silver (metallic 0.9, roughness 0.3) or `#2A3140` charcoal.
   - Let them reflect the cyan strips. That reflection is what makes them look blue in the target.
3. **Add the accent budget.** Each accent is small, but together they must reach the metric targets.

| Element | Accent |
|---|---|
| Desk end-cap light slits (both ends, all 8 desks) | amber `#FF9A3C`, T2 |
| Pad rim accent arc (front left, about 25°) | amber, T2. Must be clearly visible; currently faint |
| Desk buttons (3–5 per desk) | amber, pink `#FF4FD8`, white |
| Core dais | middle ring amber, inner ring magenta→violet gradient (T2–T3) |
| Orrery | one orange "sun" blip, plus teal and cyan blips |
| Walls | a few short violet `#8A5CFF` strips in the wall bands (crop 13, left side of the target) |
| Server racks | LEDs about 70% cyan/blue, 12% amber, 10% violet, 8% green |
| Screens | each screen has one accent colour in its art (section B) |

4. **Remove the round blue blobs on the floor**: soft circles in front of every pad and three big ones in the foreground.
   They're light hot-spots or leftover halo quads.
   - If they're lights: raise them, increase the light radius, or swap to RectLights facing the desk.
   - If they're quads: delete them.
   - Floor highlights should be thin reflections of lines, not blobs.

## B. Screen art (the "cool" factor; crops 07, 09, 15 and the annotated target)

Look at the target desks. **Each screen is a glowing illustration with one big focal graphic**, not lines of text:
- a wireframe cargo box
- a nebula with a bright pink-white core
- a line chart with an orange path
- crew silhouettes
- a gear or reactor schematic
- a calendar with checkmarks
- a bar chart

The background is dark, the focal element is bright, and there are small labels and gauges around it.

1. **Build a content-card atlas with your Blender render script.** Model simple emissive wireframe or holographic subjects and render them as 512×256 (main) and 160×240 (side) PNGs on a dark navy background, or with transparency.
   - Use Eevee with bloom (or Cycles), plus a blur/glow pass. Save to `assets/textures/holo_cards/`.
   - Subjects per tool category, **at least 3 variants each**, so 8 desks never look identical:

| Category | Focal graphic ideas | Accent |
|---|---|---|
| search | radar or magnifier over a scatter of points, a highlighted cluster | amber highlight |
| read | an open document with scanning beam lines, a page-stack wireframe | violet |
| edit / code | code blocks with a diff (green/red-ish bars), a curly-brace wireframe | magenta |
| terminal | a terminal window with glowing prompt lines, a progress bar | amber |
| web | a globe wireframe with arcs between points | orange arcs |
| plan / todo | a calendar or checklist with checkmarks, a timeline | green checks |
| thinking | a nebula or neural swirl with a bright core | pink-white core |
| delegate | a node graph branching out | violet nodes |
| idle | a slowly rotating emblem, dim | none |

   - Side panels: vertical gauges, lists, mini bar charts, waveforms.
2. **Composite the real data on top.** At runtime, overlay small real text: the tool name, the target file or query (truncated) and the elapsed time. Either render-to-texture UI over the card, or text in the hologram shader's second layer.
   The art comes from the card, and the facts come from the data.
3. **3D holograms above active desks** (crop 07: the cargo box rises out of the screen). For the active tool category, float a small wireframe hologram mesh 10–20 cm above the main screen. Use HologramMaterial in 3D mode, with a slow spin and a scanline.
   This is what makes the target's desks feel alive. Show it on active desks only.
4. **Contrast inside each screen:** background `#06122C`; the focal graphic at T1–T2 so it blooms slightly; the border at T2. Keep scanlines and flicker subtle (FIX_01 limits).

## C. Hero model detail (PROMPT_02 steps 2–6, done properly)

- **Desk console:** build it exactly per PROMPT_02 step 2, replacing the office desks with drawers.
  Include every listed greeble: keyboard key grid, touch pad, buttons, tablet, front-lip LED, underglow, projector bar and end-cap slits. **That's where the target's detail density comes from.**
- **Desk pad:** add a ring of 24 tiny inset lights around the top surface (alternating dim and bright), 4 bolt details, and segment cut lines into the pad top.
- **Core dais:** add panel cuts on each tier, small vents and 3 floating partial arcs. The rings must be thick neon tubes, not flat lines.
- **Orrery:** the 12-segment armoured bezel with amber indicator lights (PROMPT_02 step 6). Replace the grey disc.

Preview every model with your Blender render script next to the matching crop before integrating it.

## D. Room detail (the big flat walls, floor and ceiling)

1. **Walls: a modular kit** (`walls_windows.py`). Every 2.4 m bay has three bands:
   - **Lower band** (0–0.9 m): a kick plate, a vent grille and a cyan LED strip.
   - **Middle band** (0.9–2.4 m): two panels with grooves, one small inset wall screen or status light cluster per 2 bays, and stencilled labels as decals.
   - **Upper band:** an angled panel with a light cove.
   - Pilasters between bays: silver, with a vertical light strip and bolt details.
   - **No wall area larger than about 1.5 × 1.5 m without some detail.**
2. **Floor:**
   - 1.2 m panels with real seams.
   - A decal set: hazard chevrons near the walls (amber, low saturation, small), zone numbers, thin arrows along the traces, and tiny hex-grid patches.
   - Per-panel roughness variation.
   - The traces sit in grooves.
3. **Ceiling:** coffered panels with rectangular light panels, plus cable trays and vent strips between them. It's visible when the camera tilts, so it must not be a flat slab.
4. **Props along the walls:** wall consoles with small screens, crates or equipment cases, cable runs and extra plants. They fill the empty space between the walls and the outer desks.
5. **Trim sheet and decal atlas:** a 2k trim sheet (seams, vents, bolts, grooves, stencils) and a 2k decal atlas, shared across the room, so detail is cheap.

## E. Exterior (quick wins)

- **Planet:** an ico-sphere or cube-sphere so the pole pinch at the top right goes away. Add the atmosphere rim shell and night lights (PROMPT_02 step 8).
- **Ringed planet:** make it dim and rim-lit, or replace it with the small moon from the target. The current flat cartoon ring breaks the style.
- **Sky:** navy nebula cubemap, never black.

---

## Report after each letter (A–E)

- Hero screenshot and the 3 orbit shots.
- Model previews next to the reference crops (C and D).
- The atlas contact sheet (B).
- The full `look_metrics.py` output (12 metrics).
- The top 5 remaining differences.
- FPS.

**Done when** all 12 metrics pass and a side-by-side with the target reads as the same kind of image at a glance.
