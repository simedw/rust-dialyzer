#!/usr/bin/env python3
"""Compare the shared Core engine across the entire pinned semantic corpus."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
from dependency_cache import export as export_dependency

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / ".cache/engine"


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--select", help="regular expression selecting corpus identities"
    )
    parser.add_argument("--refresh-native", action="store_true")
    args = parser.parse_args()
    inventory = json.loads((ROOT / "corpus/inventory.json").read_text())
    indexed = {c["id"]: c for c in inventory["semantic_cases"]}
    core_cases = json.loads((ROOT / ".cache/core/report.json").read_text())["cases"]
    old = {
        c["id"]: c
        for c in json.loads((ROOT / ".cache/corpus/report.json").read_text())["cases"]
    }
    OUT.mkdir(parents=True, exist_ok=True)
    manifest = []
    for c in core_cases:
        if args.select and not re.search(args.select, c["id"]):
            continue
        original = indexed[c["id"]]
        oracle = json.loads(Path(old[c["id"]]["oracle_file"]).read_text())
        record = {
            "id": c["id"],
            "kind": original["kind"],
            "core_json": c["core_json"],
            "beams_dir": oracle["beams_dir"],
            "slug": re.sub(r"[^a-zA-Z0-9_.-]", "_", c["id"]),
            "base_plt": str(ROOT / ".cache/corpus/otp/base.plt")
            if original["kind"] == "otp"
            else oracle["plt"],
            "warning_options": original.get("warning_options", []),
        }
        base_key = digest(Path(record["base_plt"]))
        record["dependency_json"] = str(
            ROOT / ".cache/dependencies" / f"base-{base_key}.json"
        )
        if original.get("suite_dir"):
            record["suite_dir"] = str(ROOT / ".cache/upstream" / original["suite_dir"])
        directory = OUT / record["slug"]
        directory.mkdir(parents=True, exist_ok=True)
        record.update(
            native_json=str(directory / "native.json"),
            rust_json=str(directory / "rust.json"),
        )
        manifest.append(record)
    manifest_path = OUT / "manifest.json"
    manifest_path.write_text(json.dumps(manifest, indent=2))
    if args.refresh_native:
        subprocess.run(
            ["elixir", "scripts/corpus_facts.exs", str(manifest_path), str(OUT)],
            cwd=ROOT,
            check=True,
        )
    for base, dependency in sorted(
        {(r["base_plt"], r["dependency_json"]) for r in manifest}
    ):
        export_dependency(base, dependency)
    files = [
        *sorted((ROOT / "src").glob("*.rs")),
        *sorted((ROOT / "src/bifs").glob("*.rs")),
        ROOT / "src/bin/core_analyze.rs",
        ROOT / "Cargo.toml",
        ROOT / "Cargo.lock",
        ROOT / "corpus/bifs.json",
        ROOT / "scripts/verify_corpus_engine.py",
        ROOT / "scripts/corpus_facts.exs",
        ROOT / "scripts/compare_corpus_facts.exs",
        ROOT / "scripts/type_codec.exs",
        ROOT / "scripts/core_export.exs",
        ROOT / "scripts/native_types.exs",
        ROOT / "scripts/dependency_cache.py",
    ]
    implementation = {str(p.relative_to(ROOT)): digest(p) for p in files}
    subprocess.run(
        ["cargo", "build", "--release", "--offline", "--bin", "core_analyze"],
        cwd=ROOT,
        check=True,
    )
    cases = []
    compared = []
    for record in manifest:
        try:
            process = subprocess.run(
                [
                    ROOT / "target/release/core_analyze",
                    record["core_json"],
                    record["dependency_json"],
                ],
                cwd=ROOT,
                capture_output=True,
                text=True,
                timeout=60,
            )
            if process.returncode:
                rust = {"status": "failure", "error": process.stderr.strip()}
            else:
                rust = json.loads(process.stdout)
        except subprocess.TimeoutExpired:
            rust = {"status": "timeout"}
        Path(record["rust_json"]).write_text(json.dumps(rust, indent=2))
        case = {
            "id": record["id"],
            "core_json": record["core_json"],
            "sha256": digest(Path(record["core_json"])),
            "rust_status": rust["status"],
            "error": rust.get("error"),
            "native_json": record["native_json"],
            "rust_json": record["rust_json"],
            "dependency_json": record["dependency_json"],
            "dependency_sha256": digest(Path(record["dependency_json"])),
        }
        if Path(record["native_json"]).exists():
            case["native_sha256"] = digest(Path(record["native_json"]))
            compared.append(record)
        else:
            case["native_status"] = "not_generated"
        cases.append(case)
    compare_manifest = OUT / "comparison-manifest.json"
    compare_manifest.write_text(json.dumps(compared))
    native_comparison = OUT / "comparison.json"
    subprocess.run(
        [
            "elixir",
            "scripts/compare_corpus_facts.exs",
            str(compare_manifest),
            str(native_comparison),
        ],
        cwd=ROOT,
        check=True,
    )
    comparisons = {c["id"]: c for c in json.loads(native_comparison.read_text())}
    for c in cases:
        c.update(comparisons.get(c["id"], {}))
    if implementation != {str(p.relative_to(ROOT)): digest(p) for p in files}:
        raise RuntimeError("implementation changed during corpus verification")
    report = {
        "version": 1,
        "complete": False,
        "required_cases": len(indexed),
        "details_and_signatures_verified": False,
        "implementation_sha256": implementation,
        "cases": cases,
    }
    (OUT / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    print(
        json.dumps(
            {
                "cases": len(cases),
                "inferred": sum(c["rust_status"] == "inferred" for c in cases),
                "signature_matches": sum(
                    c.get("signature_match", False) for c in cases
                ),
                "warning_category_location_matches": sum(
                    c.get("warning_category_location_match", False) for c in cases
                ),
                "complete": False,
            }
        )
    )


if __name__ == "__main__":
    main()
