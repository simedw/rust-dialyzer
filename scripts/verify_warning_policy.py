#!/usr/bin/env python3
"""Compare warning attributes and return classifications with the native source."""
import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / ".cache/warning_policy"


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def compare(paths, native, flag, name, rejected=False):
    data = "".join(str(p) + "\n" for p in paths)
    commands = {
        "native": ["elixir", str(ROOT / "scripts" / native)],
        "rust": [str(ROOT / "target/release/warning_policy_probe"), *flag],
    }
    outputs = {}
    for implementation, command in commands.items():
        result = subprocess.run(command, input=data, text=True, capture_output=True, check=True)
        target = OUT / f"{name}.{implementation}.jsonl"
        target.write_text(result.stdout)
        outputs[implementation] = [json.loads(row) for row in result.stdout.splitlines()]
        if len(outputs[implementation]) != len(paths):
            raise RuntimeError(f"{name}: {implementation} result denominator differs")
    failures = []
    for path, native, rust in zip(paths, outputs["native"], outputs["rust"]):
        same_rejection = rejected and "Err" in native and "Err" in rust
        if native != rust and not same_rejection:
            failures.append({"path": str(path), "native": native, "rust": rust})
    (OUT / f"{name}.failures.json").write_text(json.dumps(failures, indent=2))
    summary = {"cases": len(paths), "failures": len(failures)}
    if flag:
        summary["functions"] = sum(len(m["functions"]) for row in outputs["native"] for m in row)
    if failures:
        raise RuntimeError(f"{name}: {len(failures)} native differences")
    return summary


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    subprocess.run(["cargo", "build", "--release", "--offline", "--bin", "warning_policy_probe"], cwd=ROOT, check=True)
    cases = json.loads((ROOT / ".cache/core/report.json").read_text())["cases"]
    paths = [Path(c["core_json"]) for c in cases]
    expected = {c["id"] for c in json.loads((ROOT / "corpus/inventory.json").read_text())["semantic_cases"]}
    if len(cases) != len(expected) or {c["id"] for c in cases} != expected:
        raise RuntimeError("warning policy corpus is incomplete")
    subprocess.run(["python3", "scripts/warning_policy_fixtures.py"], cwd=ROOT, check=True)
    synthetic = [Path(p) for p in (OUT / "synthetic-paths.txt").read_text().splitlines()]
    if len(synthetic) != 893:
        raise RuntimeError("warning policy synthetic denominator changed")
    results = {
        "attributes": compare(paths, "warning_policy_oracle.exs", [], "attributes", rejected=True),
        "synthetic": compare(synthetic, "warning_policy_oracle.exs", [], "synthetic", rejected=True),
        "returns": compare(paths, "return_oracle.exs", ["--returns"], "returns"),
    }
    files = [
        ROOT / "Cargo.toml", ROOT / "Cargo.lock",
        ROOT / "src/core.rs", ROOT / "src/lattice.rs", ROOT / "src/type_api.rs",
        ROOT / "src/native_order.rs", ROOT / "src/runtime_term.rs",
        ROOT / "src/warnings.rs", ROOT / "src/warning_policy.rs", ROOT / "src/returns.rs",
        ROOT / "src/bin/warning_policy_probe.rs", ROOT / "corpus/warnings.json", ROOT / "corpus/bifs.json",
        ROOT / "scripts/core_export.exs", ROOT / "scripts/warning_policy_oracle.exs",
        ROOT / "scripts/return_oracle.exs", ROOT / "scripts/warning_policy_fixtures.py", Path(__file__),
        *sorted((ROOT / "src/bifs").glob("*.rs")),
    ]
    report = {
        "complete_policy": True,
        "complete_diagnostics": False,
        "exception_payloads_verified": False,
        "results": results,
        "cases": [{"id": c["id"], "core_json": c["core_json"], "sha256": digest(Path(c["core_json"]))} for c in cases],
        "implementation_sha256": {str(p.relative_to(ROOT)): digest(p) for p in files},
    }
    (OUT / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(results))


if __name__ == "__main__":
    main()
