# Agent Room visual overhaul

The user's brief "Agent Room: Visual Overhaul Brief" (2026-11-19) drives PLAN.md M30.
It was pasted into a session; this file keeps what later sessions need from it. Ask
the user for the full text if a detail is missing here.

- `reference/target_reference.png`: the target concept art (1288×938, aspect 1.37).
  The brief also names an annotated copy with callouts ⟨1⟩–⟨20⟩, crops and a palette
  image; those were not provided.
- `tools/look_metrics.py`: written here (the brief's version was not provided). No
  dependencies; run `python3 -I tools/look_metrics.py <shot.png>`.
- `reference/` and `baseline/` are local only (git-ignored, large PNGs). The reference
  pack is in ~/Downloads/agent-room-visual-overhaul; copy it here to run the metrics.
- `baseline/phase0_hero.png`: the build before the overhaul (commit bf522aa).

## Ground rules (brief section 0)

Match mood, lighting, palette, materials, detail density and HUD style (about 85%,
not pixel-perfect). No AI artefacts or invented data: every number, label and log
line comes from real agent state; hide a widget whose data doesn't exist (no
"Confidence 98%", no fake "Time Est."). Keep every feature reachable. Measure after
each change. Keep every look value in one hot-reloadable config. Check the Bevy
version's API (0.20.1 here) before writing code. No Bevy upgrade or heavy dependency
without asking.

## Targets (brief sections 2 and 3)

- Value structure: about 70% of the frame dark navy (L < 0.25), 20% lit mid-tones
  (white desk shells, robot, steel, planet), 10% saturated glow on lines, screens and
  indicators only. Median luminance ≈ 0.20, 36% of pixels below L 0.15, mean
  saturation ≈ 0.74.
- Palette: void navy #020A1E, deep navy #06122C, navy #0A1D3D, floor #0E1A33, wall
  #122241, steel #2F4571, desk shell white #D9E2EE; glow cyan #38C8FF, azure #2E8BFF,
  electric blue #1E5BFF, holo #4B8DF0 (ice #DFF6FF only where the tonemapper burns
  out); accents ≤4% of glow: amber #FF9A3C, orange-red #FF6A3D, magenta #FF4FD8,
  violet #8A5CFF, teal #3EE0C8. HUD: fill #04122E α0.80→#020B1F α0.88, border
  #3AA6FF, text #EAF4FF / #9DB4D6 / #5F7AA0, success #22E07A, warning #FFB547,
  danger #FF5A6E.
- Emissives: saturated hue, brightness from intensity; never white or grey base
  colours. Tiers in nits: T0 150–400, T1 600–1500, T2 2000–4000, T3 5000–10000,
  T4 15000+ (flashes only).
- Materials: floor #0E1A33 metallic 0.75 roughness 0.30 with clearcoat; desk shells
  white #D9E2EE clearcoat 0.8; robot white clearcoat, black visor; walls #122241;
  frames #1A2640 with cyan strips.
- Bevel every hard edge (2–8 mm); three levels of detail on hero objects.
- Motion: slow ambient motion always on (brief 3.7); events 100–600 ms.

## Phases (brief section 13); stop and report after each

0. Audit and tooling: look config with hot reload, inspector, hero camera and
   screenshot hotkey/flag, FPS overlay, demo mode; baseline metrics.
1. Value and colour: kill ambient, palette base colours, saturated emissive tiers,
   tonemapping, exposure, bloom, colour grading, deband, vignette.
2. Lighting and reflections: deferred, SSAO, SSR, TAA+CAS, contact shadows, lighting
   rig (window key, core glow, core spot, desk screen washes, orrery, racks), space
   skybox and environment light, interior reflection probe, glossy floor.
3. HUD rebuild: icon font (no emoji/symbol glyphs), Barlow and JetBrains Mono,
   tokens, components, the layout of the reference (title, Agent Activity with a real
   sparkline, main agent card, agent count, Quick Actions, Command panel, Live
   Activity Log with real stat cards: Tasks from todos, Elapsed, Context %, Next
   action), world-anchored desk labels.
4. Hero assets in Blender scripts: sculpted white desk consoles and pads, core dais,
   cylinder, hologram mount, orrery, room shell details.
5. Shaders: Hologram, EnergyRing, FlowTrace, FresnelGlass, Radar, LedPanel, Atmosphere.
6. Exterior: sky cubemap, cube-sphere planet with clouds, atmosphere and night side,
   gas giant, sun, asteroids, ships.
7. Motion and agent-state mapping (brief section 11), particles.
8. Polish and performance: lightmaps, decals, presets, 60 fps on Medium.

Report after each phase: what changed (files), screenshot paths, metrics table
against the previous phase, top 5 remaining differences, FPS, open questions.
