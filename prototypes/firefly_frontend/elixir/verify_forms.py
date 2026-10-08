#!/usr/bin/env python3
"""Compare native forms against independently captured Elixir compiler forms."""
import argparse
import hashlib
import json
import pathlib
import subprocess

HERE = pathlib.Path(__file__).resolve().parent
parser = argparse.ArgumentParser()
parser.add_argument("inputs", type=pathlib.Path)
parser.add_argument("--output", type=pathlib.Path, default=HERE / "forms-verification.json")
args = parser.parse_args()
output_dir = HERE / "generated"
output_dir.mkdir(exist_ok=True)
results = []
for case in sorted(args.inputs.iterdir()):
    metadata = case / "metadata.etf"
    oracle = case / "oracle-source-forms.etf"
    if not metadata.exists() or not oracle.exists():
        continue
    native = output_dir / (case.name + ".etf")
    command = [str(HERE / "target/debug/translate-elixir"), str(metadata), str(native)]
    result = subprocess.run(command, capture_output=True, text=True, timeout=30)
    if result.returncode:
        results.append({"module": case.name, "equal": False, "error": result.stderr.strip()})
        continue
    compare = subprocess.run([str(HERE / "target/debug/compare-forms"), str(native), str(oracle)],
                             capture_output=True, text=True, timeout=30)
    results.append({"module": case.name, "equal": compare.returncode == 0,
                    "detail": compare.stdout.strip(),
                    "metadata_sha256": hashlib.sha256(metadata.read_bytes()).hexdigest(),
                    "native_forms_sha256": hashlib.sha256(native.read_bytes()).hexdigest(),
                    "oracle_forms_sha256": hashlib.sha256(oracle.read_bytes()).hexdigest()})
report = {"modules": len(results), "equal": sum(row["equal"] for row in results), "results": results}
args.output.write_text(json.dumps(report, indent=2) + "\n")
print(json.dumps({"modules": report["modules"], "equal": report["equal"]}))
raise SystemExit(0 if all(row["equal"] for row in results) else 1)
