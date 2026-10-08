#!/usr/bin/env python3
"""Exercise native PLT interchange and target-summary exclusion."""
import copy
import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / ".cache/dependencies"


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def inspect(core, artifact, accepted=True):
    result = subprocess.run(
        [ROOT / "target/release/dependencies_inspect", core, artifact],
        cwd=ROOT,
        text=True,
        capture_output=True,
    )
    if (result.returncode == 0) != accepted:
        raise RuntimeError(
            f"unexpected dependency decode result: {result.stdout}{result.stderr}"
        )
    return json.loads(result.stdout) if accepted else result.stderr.strip()


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    oracle = json.loads((ROOT / ".cache/oracle.json").read_text())
    core = OUT / "tiny.core.json"
    subprocess.run(
        [
            "elixir",
            "-r",
            "scripts/core_export.exs",
            "-e",
            "CoreExport.run(Enum.at(System.argv(), 0), Enum.at(System.argv(), 1))",
            "examples/tiny/_build/dev/lib/tiny/ebin",
            str(core),
        ],
        cwd=ROOT,
        check=True,
    )
    subprocess.run(
        [
            "cargo",
            "build",
            "--release",
            "--offline",
            "--bin",
            "dependencies_inspect",
        ],
        cwd=ROOT,
        check=True,
    )
    targets = {m["name"] for m in json.loads(core.read_text())["modules"]}
    results = []
    artifacts = {}
    for kind in ("base", "app"):
        path = OUT / f"tiny-{kind}.json"
        subprocess.run(
            [
                "elixir",
                "scripts/export_dependencies.exs",
                oracle["cache"][f"{kind}_plt"],
                str(path),
            ],
            cwd=ROOT,
            check=True,
        )
        artifact = json.loads(path.read_text())
        artifacts[kind] = artifact
        source = artifact["source"]
        if digest(Path(source["path"])) != source["sha256"]:
            raise RuntimeError("exported PLT fingerprint differs")
        for fingerprint in source["files"]:
            if digest(Path(fingerprint["path"])) != fingerprint["sha256"]:
                raise RuntimeError(
                    f'exported BEAM fingerprint differs: {fingerprint["path"]}'
                )
        decoded = inspect(core, path)
        expected = {
            "summaries": sum(
                s["mfa"]["module"] not in targets for s in artifact["summaries"]
            ),
            "contracts": sum(
                s["mfa"]["module"] not in targets for s in artifact["contracts"]
            ),
            "callbacks": sum(
                s["mfa"]["module"] not in targets for s in artifact["callbacks"]
            ),
            "excluded_target_summaries": sum(
                s["mfa"]["module"] in targets for s in artifact["summaries"]
            ),
            "modules": len(artifact["modules"]),
        }
        if any(decoded[key] != value for key, value in expected.items()):
            raise RuntimeError(f"imported facts differ: {decoded} vs {expected}")
        results.append(
            {
                "id": kind,
                "counts": decoded,
                "artifact": str(path),
                "sha256": digest(path),
                "source_files_verified": len(source["files"]),
            }
        )
    if results[1]["counts"]["excluded_target_summaries"] == 0:
        raise RuntimeError("application PLT did not exercise target-summary exclusion")
    base = artifacts["base"]
    mutations = {
        "schema": lambda a: a.update(version=99),
        "runtime": lambda a: a["profile"].update(erts="different"),
        "source_kind": lambda a: a["source"].update(kind="unknown"),
        "summary_arity": lambda a: a["summaries"][0]["mfa"].update(arity=256),
        "summary_module": lambda a: a["summaries"][0]["mfa"].update(
            module="absent_module"
        ),
        "duplicate_summary": lambda a: a["summaries"].append(
            copy.deepcopy(a["summaries"][0])
        ),
        "contract_arity": lambda a: a["contracts"][0]["mfa"].update(arity=256),
        "duplicate_contract": lambda a: a["contracts"].append(
            copy.deepcopy(a["contracts"][0])
        ),
        "invalid_term_reference": lambda a: a["exported_types"].update(
            root=len(a["exported_types"]["terms"])
        ),
    }
    rejected = []
    mutated = OUT / "invalid.json"
    for name, mutate in mutations.items():
        artifact = copy.deepcopy(base)
        mutate(artifact)
        mutated.write_text(json.dumps(artifact))
        rejected.append({"id": name, "error": inspect(core, mutated, accepted=False)})
    mutated.unlink()
    files = [
        "src/dependencies.rs",
        "src/core.rs",
        "src/bifs/boundary.rs",
        "src/lattice.rs",
        "src/bin/dependencies_inspect.rs",
        "scripts/export_dependencies.exs",
        "scripts/core_export.exs",
        "scripts/native_types.exs",
        "scripts/verify_dependencies.py",
    ]
    report = {
        "version": 1,
        "complete_interchange": True,
        "complete_lifecycle": False,
        "claim": "Native classic PLT metadata and source fingerprints imported; all target-module summaries and contracts excluded; malformed boundary inputs rejected. Invalidation and persisted Rust PLTs remain open.",
        "implementation_sha256": {path: digest(ROOT / path) for path in files},
        "core_json": str(core),
        "core_sha256": digest(core),
        "cases": results,
        "rejections": rejected,
    }
    (OUT / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps({"imported": results, "malformed_inputs_rejected": len(rejected)}))


if __name__ == "__main__":
    main()
