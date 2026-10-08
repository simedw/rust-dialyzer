#!/usr/bin/env python3
"""Run the existing analyzer with native-produced Core and frozen oracle context."""
import argparse
import json
from pathlib import Path
import subprocess

p = argparse.ArgumentParser()
p.add_argument("native_core", type=Path)
p.add_argument("--probe", type=Path, required=True)
args = p.parse_args()
here = Path(__file__).resolve().parent
row = json.loads((here / "oracle-row.jsonl").read_text())
native = json.loads(args.native_core.read_text())
assert native["modules"], "native frontend produced no modules"
# Native bodies/forms/attributes come only from this artifact. Profiles are an
# explicit captured test context, never source programs or inferred facts.
native.setdefault("profile", row["core"]["profile"])
native.setdefault("atom_layout", row["core"]["atom_layout"])
native.setdefault("contract_closure_profile", row["core"]["contract_closure_profile"])
row["core"] = native
(here / "native-row.jsonl").write_text(json.dumps(row) + "\n")
result = subprocess.run([str(args.probe.resolve()), str(here / "prior-library.json"),
                         str(here / "closure-profile.json")],
                        input=json.dumps(row) + "\n", text=True,
                        capture_output=True, check=True)
(here / "native-rust-result.jsonl").write_text(result.stdout)
actual = json.loads(result.stdout)
summary = {k: actual.get(k) for k in ("id", "public_warnings_match", "public_warning_count", "error")}
print(json.dumps(summary, indent=2))
assert actual.get("public_warnings_match") is True, summary
assert actual.get("public_warning_count") == row["warning_count"], summary
