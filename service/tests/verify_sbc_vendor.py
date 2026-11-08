#!/usr/bin/env python3
"""Offline fixed-vendor/provenance verification; no build/network/SDK access."""
import hashlib
import json
from pathlib import Path

root = Path(__file__).resolve().parent.parent / "vendor" / "bluedroid-sbc"
manifest = json.loads((root / "provenance.json").read_text())
assert manifest["commit"] == "eb0bb8b5ea6d234ccb940313b47f7a5c3b4e20ec"
assert manifest["local_modifications"] == []
assert len(manifest["files"]) == 12
expected = set()
for item in manifest["files"]:
    path = root / item["path"]
    assert path.parent in (root / "include", root / "srce")
    data = path.read_bytes()
    assert hashlib.sha256(data).hexdigest() == item["sha256"], path
    assert b'Licensed under the Apache License, Version 2.0' in data, path
    expected.add(path)
assert expected == set(root.glob("include/*.h")) | set(root.glob("srce/*.c"))
license_text = (root / "LICENSE-APACHE-2.0").read_text()
assert "Version 2.0, January 2004" in license_text
assert "END OF TERMS AND CONDITIONS" in license_text
print("PASS: 12 fixed SBC vendor sources and Apache-2.0 notices")
