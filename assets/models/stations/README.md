# Stations (sci-fi workstations, props and activity holograms)

Own work, made procedurally with `tools/blender/stations.py` (no third-party assets).

Rebuild from the repo root (Blender 5.2):

    /Applications/Blender.app/Contents/MacOS/Blender -b --factory-startup \
        --python tools/blender/stations.py            # all files
    ... --python tools/blender/stations.py -- lab plant   # only some

Preview renders (imports the exported GLBs, puts the robot at its work spot):

    /Applications/Blender.app/Contents/MacOS/Blender -b --factory-startup \
        --python tools/blender/stations_preview.py -- OUT_DIR [name ...]

Inspect: `python3 -I tools/blender/inspect_glb.py assets/models/stations/lab.glb`

## Conventions

- Metres, glTF Y up. Origin = floor centre of the station (holograms: bottom centre).
- Front (where the robot works, facing the camera) is +Z. One mesh per file.
- Workstations have no physical monitors: the screens are floating `Holo` panels.
- Shared material names the app may tweak: `Panel`, `PanelDark`, `Trim`, `GlowCyan`,
  `GlowOrange`, `GlowPurple`, `GlowGreen`, `Holo` (alpha 0.35, BLEND; `HoloAmber` in research.glb), `HoloDark`
  (terminal background, alpha 0.6, BLEND), `Glass` (alpha 0.2, BLEND), `Paper`, `Leaf`,
  `Pot`, and `BookBlue/BookTeal/BookRed/BookAmber/BookCream`. Glow strength is 6
  (`Holo` 2).

## Files and robot work spots (glTF metres, robot faces -Z toward the station)

| file | size (x × y × z) | tris | robot spot |
|---|---|---|---|
| main_desk.glb | 4.0 × 2.13 × 4.0 | 23k | (0, 0.35, 0) platform centre; console behind at -Z; inbox tray top at (0.60, 1.18, -0.86) |
| bookshelf.glb | 3.2 × 2.6 × 0.8 alcove (+ floor ring to z 2.0) | 14k | (0, 0.033, 1.3) |
| research.glb | 3.0 × 2.03 × 3.0 | 11.5k | (0, 0.043, 0.85) |
| lab.glb | 3.7 × 2.12 × 3.7 | 24k | (0, 0.053, 1.0) |
| writing_desk.glb | 2.9 × 2.03 × 2.9 | 11k | (0, 0.043, 0.85) |
| plant.glb | 1.6 × 1.17 × 1.25 | 2k | - |
| wall_panel.glb | 3.0 × 3.0 × 0.27, front +Z, window 2.2 × 1.7 at y 0.75-2.45 | 2k | - |
| pillar.glb | 0.67 × 3.0 × 0.67 | 2k | - |
| document.glb / holo_capsule.glb | 0.29 × 0.2 × 0.29 | 1k | - |
| (the big desk holograms are the screen subjects: `assets/models/holo/`, `tools/blender/export_holo_subjects.sh`) | | | |
