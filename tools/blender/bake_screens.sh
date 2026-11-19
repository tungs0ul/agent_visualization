#!/bin/sh
# Bakes every crew-desk screen subject into assets/textures/screens/<subject>.png.
# Run from the repo root.
set -eu
blender=${BLENDER:-/Applications/Blender.app/Contents/MacOS/Blender}
mkdir -p assets/textures/screens
for subject in crate rocket planet sun station ship blackhole; do
  "$blender" -b --factory-startup --python tools/blender/holo_screen_proto.py -- \
    "$PWD/assets/textures/screens/$subject.png" bake "$subject" >/dev/null
  echo "baked $subject"
done
