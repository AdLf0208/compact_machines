#!/usr/bin/env python3
"""Carry Compact Machines' own assets and models out of the pack jar.

The MIT jar is the evidence and the source of the art: the four blockstates,
the machine block model (three layers, two tint indices), the wall model, the
item models and the textures this slice registers. Nothing else is copied.

One patch: the machine model's tint layer carries NeoForge's
`neoforge_data: {block_light: 2}`, a loader-specific key Horizon does not read;
it is stripped, and the model is otherwise byte-for-byte the jar's.

Run: `python3 tools/carry_content.py` (writes into content/), or
`python3 tools/carry_content.py --check` to prove content/ matches the jar.
"""
import hashlib
import json
import sys
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
JAR = (ROOT.parent / "_shared" / "licence-cache"
       / "compactmachines-neoforge-7.0.81.jar")
OUT = ROOT / "content"

# Files copied as they are (byte for byte).
VERBATIM = [
    "assets/compactmachines/blockstates/machine.json",
    "assets/compactmachines/blockstates/new_machine.json",
    "assets/compactmachines/blockstates/wall.json",
    "assets/compactmachines/blockstates/solid_wall.json",
    "assets/compactmachines/models/block/wall.json",
    "assets/compactmachines/models/item/machine.json",
    "assets/compactmachines/models/item/new_machine.json",
    "assets/compactmachines/models/item/wall.json",
    "assets/compactmachines/models/item/solid_wall.json",
    "assets/compactmachines/models/item/personal_shrinking_device.json",
    "assets/compactmachines/models/item/shrinking_module.json",
    "assets/compactmachines/models/item/enlarging_module.json",
    "assets/compactmachines/textures/block/machine/border.png",
    "assets/compactmachines/textures/block/machine/overlay.png",
    "assets/compactmachines/textures/block/machine/tint.png",
    "assets/compactmachines/textures/block/wall.png",
    "assets/compactmachines/textures/item/personal_shrinking_device.png",
    "assets/compactmachines/textures/item/atom_shrinker.png",
    "assets/compactmachines/textures/item/atom_enlarger.png",
]

# Files copied with a patch, and how they change.
PATCHED = {
    "assets/compactmachines/models/block/machine/machine.json": "strip-neoforge-data",
}


def strip_neoforge_data(value):
    """Removes every NeoForge-only `neoforge_data` key from a model."""
    if isinstance(value, dict):
        return {k: strip_neoforge_data(v) for k, v in value.items() if k != "neoforge_data"}
    if isinstance(value, list):
        return [strip_neoforge_data(v) for v in value]
    return value


def carried():
    """Every file this slice carries: repo-relative name -> bytes."""
    files = {}
    with zipfile.ZipFile(JAR) as jar:
        names = set(jar.namelist())
        missing = [name for name in (*VERBATIM, *PATCHED) if name not in names]
        if missing:
            raise SystemExit(f"the jar does not hold: {missing}")
        for name in VERBATIM:
            files[name] = jar.read(name)
        for name, patch in PATCHED.items():
            raw = jar.read(name)
            if patch == "strip-neoforge-data":
                model = json.loads(raw)
                text = json.dumps(strip_neoforge_data(model), indent=2) + "\n"
                files[name] = text.encode()
            else:
                raise SystemExit(f"unknown patch {patch}")
    return files


def main():
    files = carried()
    if "--check" in sys.argv:
        bad = []
        for name, data in files.items():
            path = OUT / name
            if not path.is_file() or path.read_bytes() != data:
                bad.append(name)
        if bad:
            raise SystemExit(f"content/ differs from the jar carry: {bad}")
        digest = hashlib.sha256(b"".join(files[name] for name in sorted(files))).hexdigest()
        print(f"ok: {len(files)} carried files, carry sha256 {digest}")
        return
    for name, data in files.items():
        path = OUT / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)
    total = sum(len(data) for data in files.values())
    print(f"carried {len(files)} files ({total} B) from {JAR.name}")


if __name__ == "__main__":
    main()
