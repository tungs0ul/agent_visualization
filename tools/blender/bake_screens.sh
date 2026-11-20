#!/bin/sh
# Bakes every crew-desk screen subject into assets/textures/screens/<subject>.png:
# an atlas of 8 frames of 512x256 (4 columns, 2 rows) that the app plays forth and
# back while the desk works (`screens.rs`). Run from the repo root.
set -eu
blender=${BLENDER:-/Applications/Blender.app/Contents/MacOS/Blender}
frames=8
tmp=$(mktemp -d)
mkdir -p assets/textures/screens
for subject in crate rocket planet sun station ship blackhole galaxy; do
  i=0
  while [ $i -lt $frames ]; do
    HOLO_FRAMES=$frames HOLO_FRAME=$i "$blender" -b --factory-startup --python tools/blender/holo_screen_proto.py -- \
      "$tmp/$subject-$i.png" bake "$subject" >/dev/null
    i=$((i + 1))
  done
  "$blender" -b --factory-startup --python tools/blender/atlas.py -- "$PWD/assets/textures/screens/$subject.png" 4 \
    $(i=0; while [ $i -lt $frames ]; do printf '%s ' "$tmp/$subject-$i.png"; i=$((i + 1)); done) >/dev/null
  echo "baked $subject"
done
rm -rf "$tmp"
