"""Print animations, materials, nodes, triangle count and bounds of a .glb.

    python3 -I tools/blender/inspect_glb.py assets/models/robot/robot.glb
"""
import json
import struct
import sys

path = sys.argv[1] if len(sys.argv) > 1 else "assets/models/robot/robot.glb"
with open(path, "rb") as fh:
    data = fh.read()
magic, version, length = struct.unpack_from("<4sII", data, 0)
assert magic == b"glTF", "not a glb"
clen, ctype = struct.unpack_from("<II", data, 12)
gl = json.loads(data[20:20 + clen])
acc = gl["accessors"]

print("animations:")
for a in gl.get("animations", []):
    times = [acc[s["input"]].get("max", [0])[0] for s in a["samplers"]]
    print(f"  {a['name']:<10} channels={len(a['channels']):<3} duration={max(times):.3f}s")
print("materials:", [m["name"] + ("(" + m["alphaMode"] + ")" if "alphaMode" in m else "") for m in gl.get("materials", [])])
print("nodes:", [n.get("name") for n in gl.get("nodes", [])])
print("skins:", [(s.get("name"), len(s["joints"])) for s in gl.get("skins", [])])
tris = 0
lo, hi = [1e9] * 3, [-1e9] * 3
for m in gl.get("meshes", []):
    for p in m["primitives"]:
        if "indices" in p:
            tris += acc[p["indices"]]["count"] // 3
        pa = acc[p["attributes"]["POSITION"]]
        lo = [min(a, b) for a, b in zip(lo, pa["min"])]
        hi = [max(a, b) for a, b in zip(hi, pa["max"])]
print("triangles:", tris)
print("bbox min:", [round(v, 3) for v in lo], "max:", [round(v, 3) for v in hi])
