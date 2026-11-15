#!/usr/bin/env python3
"""Pack basso's native bundle into target/basso-<version>.hnmod for a release.

Same layout as horizon-mods' native/tools/native.py `package`: horizon-native.json, then the bundle's .rs and .toml
files under mixins/basso/ (horizon-mixin.toml, the helpers in mixins/basso/, and the basso-engine crate), plus the
licence as mixins/basso/LICENSE. Deterministic: fixed timestamps and sorted entries, so the same source gives the
same SHA-256.

    python3 scripts/package.py
"""
import hashlib
import json
import sys
import tomllib
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
GAME = "26.3"
# The horizon-mixins commit the launcher built and tested basso with (native-mods.json of the last basso build).
MIXINS = "5a64af938ec86b00d53d16a408aabc0038fb3e57"


def manifest(mixin):
    horizon = mixin["compatibility"]["horizon_revision"]
    if len(horizon) != 40:
        sys.exit(f"horizon_revision must be a full commit, got {horizon!r}")
    # basso hooks raw targets (turn_player, chat_tree_for, world_primitives), so it pins its commit: no "hooks".
    return {"schema": 1, "id": mixin["mod"]["id"], "version": mixin["mod"]["version"], "game": GAME,
            "horizon": horizon, "mixins": MIXINS}


def files(mod_id, mixin):
    """Bundle path -> bytes: the manifest, the helpers and modules, and each crate's sources."""
    roots = [ROOT / "mixins" / mod_id] + [ROOT / c["path"] for c in mixin.get("crate", [])]
    out = {f"mixins/{mod_id}/horizon-mixin.toml": (ROOT / "horizon-mixin.toml").read_bytes()}
    for root in roots:
        for path in sorted(root.rglob("*")):
            relative = path.relative_to(ROOT)
            if "target" in relative.parts or not path.is_file():
                continue
            if path.suffix in (".rs", ".toml") or "data" in relative.parts:
                out[f"mixins/{mod_id}/{relative.as_posix()}"] = path.read_bytes()
    out[f"mixins/{mod_id}/LICENSE"] = (ROOT / "LICENSE.md").read_bytes()
    return out


def main():
    mixin = tomllib.loads((ROOT / "horizon-mixin.toml").read_text())
    meta = manifest(mixin)
    entries = {"horizon-native.json": (json.dumps(meta, indent=2, sort_keys=True) + "\n").encode()}
    entries.update(files(meta["id"], mixin))
    out = ROOT / "target" / f"{meta['id']}-{meta['version']}.hnmod"
    out.parent.mkdir(exist_ok=True)
    with zipfile.ZipFile(out, "w", compression=zipfile.ZIP_DEFLATED) as archive:
        for name, content in sorted(entries.items()):
            info = zipfile.ZipInfo(name, (2026, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            info.external_attr = 0o100644 << 16
            info.create_system = 3
            archive.writestr(info, content)
    print(json.dumps({"file": str(out), "files": len(entries), "size": out.stat().st_size,
                      "sha256": hashlib.sha256(out.read_bytes()).hexdigest()}, indent=2))


if __name__ == "__main__":
    main()
