# FIX 01: Noise, flicker, blur and framing (do this before anything else)

> For the coding agent. This overrides PROMPT.md sections 5.2 and 5.5 until every check at the bottom passes.
> The latest build is darker, which was the goal, but it's now **unusable**. Every surface has per-pixel
> speckle, the image flickers like an old TV, 3D edges are both pixelated and soft, and the framing is worse.
> Fix stability first. Add effects back only once the image is clean.

## What's wrong (measured on the latest screenshot)

| Issue | Evidence | Most likely cause |
|---|---|---|
| Per-pixel "sand" noise on walls, floor and ceiling | Visible in every dark area at 2× zoom | Effects that rely on **temporal accumulation** are on (SSAO, SSR, contact shadows, temporal shadow filtering), but **TAA isn't resolving them**. TAA could be missing, missing its motion-vector or depth prepass, or not getting continuous frames. |
| Flicker / "1990 TV" | Noise pattern changes each frame | Same as above, plus TAA jitter without a valid history. Also check whether hologram flicker and scanlines are too strong. **If the app uses `WinitSettings::desktop_app()` or a reactive update mode, TAA never converges**, because frames are only rendered on events. |
| Pixelated *and* blurry 3D | Jagged edges with soft, noisy interiors | `Msaa::Off` (required by deferred rendering) with no working AA. CAS sharpening amplifies the noise. Possibly the 3D view renders below the window's physical resolution (Retina scale factor). |
| Too little glow | Glow pixels 3.8% (target 10.3%); 95th-percentile luminance 0.64 (target 0.77) | Phase 1 darkened everything but didn't raise the emissive tiers. Strips and screens are still faint. |
| Too much black | 47% of pixels are dark (target 36%) | Ambient light was cut, but no practical lights were added. |
| Dollhouse framing | Sky and black space visible **above the walls**, room fills about 75% of the width, big empty bands top and bottom | The camera moved back and up. There's no ceiling or roof, so you see over the walls. |
| Command panel over the core | It covers the hologram and window | Breaks PROMPT.md section 10.4, which says the HUD must not cover the core. |
| Yellow floor bar still there | Bottom right | Not investigated. |
| Chat and Replay lost their icons | Tofu was "fixed" by deleting the icon | They need icon-font glyphs. |

## Step 1: Make the image clean and stable (no exceptions)

1. **Render continuously while the window is visible.** Use `WinitSettings { focused_mode: UpdateMode::Continuous, unfocused_mode: <reactive, e.g. 10 fps> }` or the equivalent for your version. Log the current mode at startup and report it.
2. **Switch to a non-temporal baseline (the new default "Medium" preset):**
   - Use **forward rendering**. Remove `DefaultOpaqueRendererMethod::deferred()`, `DeferredPrepass` and `ScreenSpaceReflections`.
   - Remove `ScreenSpaceAmbientOcclusion`, `ContactShadows` and `TemporalAntiAliasing`. Remove `ContrastAdaptiveSharpening`.
   - Set `ShadowFilteringMethod::Gaussian` instead of Temporal.
   - Use **`Msaa::Sample4`**. Add `Smaa` (High) only if edges still alias.
   - Keep `Hdr`, `Tonemapping`, `Bloom`, `ColorGrading`, `DebandDither::Enabled` and `Vignette`. None of these are temporal.
   - Turn hologram flicker down to at most a rare 3% dip (none on large surfaces). Set scanline amplitude to at most 0.1. No dithering or noise patterns in any custom shader.
3. **Check the resolution.** Log the window's logical size, its physical size and the scale factor. Confirm the 3D camera renders at full physical resolution: no viewport scaling, and no render target smaller than the window. On Retina the physical size must be 2× the logical size.
4. **Check textures.** All textures must have mipmaps (KTX2 with a mip chain, or generated). Use linear filtering and `anisotropy_clamp: 16`. A normal map without mips at a grazing angle gives exactly this kind of sparkle.
5. Take a hero screenshot, plus **two more captured 0.5 s apart** without moving the camera.
   **Pass when** both are free of speckle at 2× zoom and the difference between them is only from intended animation. Report the zoomed crops.

## Step 2: Put light back the right way

- **Raise the emissive tiers** (PROMPT.md 3.4) until each strip has a white core with a cyan halo. Check that emissive colours are the saturated palette hexes, not whitened ones.
- **Add the practical lights from PROMPT.md 5.3**: the core point light, the core spot, the orrery light and the desk screen lights. If your Bevy version has no `RectLight`, use PointLights.
- **Floor reflections without SSR:** bake the interior reflection probe (PROMPT.md 5.4, item 2) and use it as a `LightProbe` + `EnvironmentMapLight`, with floor roughness about 0.3 and metallic about 0.75. This is stable and costs almost nothing.
- **Pass when** the metrics show glow fraction ≥ 7%, dark fraction 30–42% and 95th-percentile luminance ≥ 0.72.

## Step 3: Fix the framing

- **Close the room.** Add the ceiling (or a roof cap plus a header band) so nothing outside the walls is ever visible. The only view to space is through the windows.
- Re-apply PROMPT.md section 4: camera starting at `(0, 11, 14)` looking at `(0, 0, 2.2)` with a vertical FOV of about 45°. The room fills the full width, the window band starts at 8–12% from the top, the robot's head sits at 36–40%, and the orrery at 69–73%.
- Move the Command panel back to its layout slot (PROMPT.md 10.4, top centre at y 155–277 on the 1470×1070 canvas, below the status card). **It must not overlap the cylinder or the robot.** If it would, collapse it to one line.

## Step 4: Small leftovers

- Find the yellow floor bar's source: entity name, mesh and material. Report it, then remove it or turn it into a subtle decal.
- Give Chat and Replay real icons from the icon font.

## Effects can come back later, one at a time, only in the "High" preset

Re-add SSAO, then SSR (deferred), then TAA, **one per step**, each with the two-screenshot stability test from Step 1.5.
Revert any effect that adds speckle or flicker. Never ship temporal effects in a preset that doesn't render continuously.

## Report

For each step:

- Before and after hero screenshots, plus 2× crops of a wall, the floor and the core.
- The metrics table.
- The update mode, the physical and logical resolution, the MSAA and AA settings, and the list of active camera effects.
- FPS.

Stop after Step 4 and wait for review. **Don't start Phase 4 (new desks and screens) yet.**
