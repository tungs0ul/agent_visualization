#!/bin/sh
# Exports every crew-desk screen subject as a 3D model, assets/models/holo/<subject>.glb,
# for the big hologram over a desk while a sub-agent works there (`holo.rs`; how the
# materials are carried over: `holo_glb.py`). Run from the repo root; about a minute.
# Subjects: the ones in `screens::SUBJECTS`, or those given as arguments.
set -eu
blender=${BLENDER:-/Applications/Blender.app/Contents/MacOS/Blender}
mkdir -p assets/models/holo
subjects=${*:-"rocket planet sun station ship blackhole crate"}
for subject in $subjects; do
  "$blender" -b --factory-startup --python tools/blender/holo_screen_proto.py -- \
    "$PWD/assets/models/holo/$subject.glb" glb "$subject" 2>/dev/null | grep '^HOLO:' || { echo "failed: $subject" >&2; exit 1; }
done
