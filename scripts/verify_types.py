#!/usr/bin/env python3
"""Run every shared type-family and solver-record differential gate."""
import hashlib
import json
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / ".cache/types"


def main():
    files = [
        ROOT / "src/lattice.rs",
        ROOT / "src/type_api.rs",
        ROOT / "src/native_order.rs",
        ROOT / "src/runtime_term.rs",
        ROOT / "src/presentation.rs",
        ROOT / "src/presentation_form.rs",
        ROOT / "Cargo.toml",
        ROOT / "Cargo.lock",
        ROOT / "src/bin/type_probe.rs",
        ROOT / "scripts/type_codec.exs",
        ROOT / "scripts/type_oracle.exs",
        ROOT / "scripts/verify_types.py",
        ROOT / "corpus/type_operations.json",
        *sorted((ROOT / "scripts/type_probes").glob("*.py")),
    ]
    initial_hashes = {str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest() for p in files}

    OUT.mkdir(parents=True, exist_ok=True)
    subprocess.run(
        ["cargo", "build", "--release", "--offline", "--bin", "type_probe"],
        cwd=ROOT,
        check=True,
    )
    gates = [
        ("probe", "results", 35908),
        ("family_probe", "family_results", 35673),
        ("callable_probe", "callable_results", 85548),
        ("map_probe", "map_results", 84590),
        ("nominal_probe", "nominal_results", 38072),
        ("raw_probe", "raw_results", 17088),
        ("subst_probe", "subst_results", 12128),
        ("unify_probe", "unify_results", 13744),
        ("api_probe", "api_results", 179007),
        ("map_candidate_probe", "map_candidate_results", 25008),
        ("list_api_probe", "list_api_results", 13706),
        ("runtime_probe", "runtime_results", 13482),
        ("merge_api_probe", "merge_api_results", 28098),
        ("presentation_probe", "presentation_results", 37796),
        ("form_presentation_probe", "form_presentation_results", 28778),
        ("unification_limit_probe", "unification_limit_results", 5000),
    ]
    reports = []
    for name, output, count in gates:
        subprocess.run(
            [sys.executable, str(ROOT / "scripts/type_probes" / (name + ".py"))],
            cwd=ROOT,
            check=True,
        )
        report = json.loads((OUT / (output + ".jsonl.comparison.json")).read_text())
        if report["commands"] != count:
            raise RuntimeError(
                f'{name}: unexpected denominator {report["commands"]} != {count}'
            )
        reports.append({"id": name, **report})

    report = {
        "version": 2,
        "complete_domain": False,
        "unsupported_families": [],
        "unsupported_operations": json.loads(
            (ROOT / "corpus/type_operations.json").read_text()
        )["unsupported_operations"],
        "passed": sum(r["passed"] for r in reports),
        "commands": sum(r["commands"] for r in reports),
        "failures": [f for r in reports for f in r["failures"]],
        "native_values": sum(r["native_values"] for r in reports),
        "native_errors": sum(sum(r["native_error_matches"].values()) for r in reports),
        "native_timeouts": sum(len(r["native_timeouts"]) for r in reports),
        "implementation_sha256": initial_hashes,
        "gates": reports,
    }
    if initial_hashes != {str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest() for p in files}:
        raise RuntimeError("type implementation changed during verification")
    (OUT / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    print(
        json.dumps(
            {
                k: report[k]
                for k in (
                    "commands",
                    "passed",
                    "native_values",
                    "native_errors",
                    "native_timeouts",
                )
            }
        )
    )


if __name__ == "__main__":
    main()
