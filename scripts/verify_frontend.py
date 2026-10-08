#!/usr/bin/env python3
import argparse
from collections import Counter
import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('--out',default='.cache/core')
    parser.add_argument('--core-filename',default='core.json')
    args=parser.parse_args()
    out=ROOT/args.out
    out.mkdir(parents=True,exist_ok=True)
    source_files=list((ROOT/'src').rglob('*.rs'))+[ROOT/'scripts/core_export.exs',ROOT/'scripts/core_corpus.exs',ROOT/'scripts/verify_core.exs',Path(__file__).resolve(),ROOT/'Cargo.lock',ROOT/'Cargo.toml']
    corpus=json.loads((ROOT/'.cache/corpus/report.json').read_text())
    beam_files=[]
    for case in corpus['cases']:
        oracle=json.loads(Path(case['oracle_file']).read_text())
        beam_files.extend((Path(oracle['program_json']).parent/'beams').glob('*.beam'))
    source_files.extend(beam_files)
    before={str(p):digest(p) for p in source_files}

    subprocess.run(["elixir", "scripts/verify_core.exs"], cwd=ROOT, check=True)
    subprocess.run(
        [
            "elixir",
            "scripts/core_corpus.exs",
            ".cache/corpus/report.json",
            str(out/"report.json"),
            args.core_filename,
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
            "core_inspect",
            "--bin",
            "program_inspect",
            "--bin",
            "forms_inspect",
        ],
        cwd=ROOT,
        check=True,
    )
    exported = json.loads((out / "report.json").read_text())
    inventory = json.loads((ROOT / "corpus/inventory.json").read_text())
    expected = {case["id"] for case in inventory["semantic_cases"]}
    actual = [case["id"] for case in exported["cases"]]
    if set(actual) != expected or len(actual) != len(expected):
        raise RuntimeError("frontend corpus identities differ from complete inventory")
    kinds = Counter()
    results = []
    program_results = []
    forms_results = []
    modules = nodes = terms = 0
    for case in exported["cases"]:
        path = Path(case["core_json"])
        result = subprocess.run(
            [ROOT / "target/release/core_inspect", "--inspect-core", path],
            cwd=ROOT,
            capture_output=True,
            text=True,
            check=True,
        )
        inspection = json.loads(result.stdout)
        kinds.update(inspection["kinds"])
        modules += inspection["module_count"]
        nodes += inspection["node_count"]
        terms += inspection["term_count"]
        indexed = subprocess.run(
            [ROOT / "target/release/program_inspect", path],
            cwd=ROOT,
            capture_output=True,
            text=True,
            check=True,
        )
        program_results.append(
            {
                "id": case["id"],
                "core_json": str(path),
                "sha256": digest(path),
                "inspection": json.loads(indexed.stdout),
            }
        )
        declared = subprocess.run(
            [ROOT / "target/release/forms_inspect", path],
            cwd=ROOT,
            capture_output=True,
            text=True,
            check=True,
        )
        forms_results.append(
            {
                "id": case["id"],
                "core_json": str(path),
                "sha256": digest(path),
                "inspection": json.loads(declared.stdout),
            }
        )
        results.append(
            {
                "id": case["id"],
                "status": "decoded",
                "core_json": str(path),
                "sha256": digest(path),
                "inspection": inspection,
            }
        )
    report = {
        "version": 1,
        "scope": "lossless compiler interchange",
        "complete": True,
        "analyzer_parity": False,
        "profile": exported["profile"],
        "case_count": len(results),
        "module_count": modules,
        "node_count": nodes,
        "term_count": terms,
        "kinds": dict(kinds),
        "implementation_sha256": {
            str(path.relative_to(ROOT)): digest(path)
            for path in [
                ROOT / "src/core.rs",
                ROOT / "scripts/core_export.exs",
                ROOT / "scripts/verify_core.exs",
            ]
        },
        "cases": results,
    }
    (out / "rust-report.json").write_text(
        json.dumps(report, indent=2) + "\n"
    )
    program_report = {
        "version": 1,
        "complete": True,
        "case_count": len(program_results),
        "analyzer_parity": False,
        "implementation_sha256": {
            str(path.relative_to(ROOT)): digest(path)
            for path in [
                ROOT / "src/program.rs",
                ROOT / "src/program_metadata.rs",
                ROOT / "src/schedule.rs",
            ]
        },
        "cases": program_results,
    }
    (out / "program-report.json").write_text(
        json.dumps(program_report, indent=2) + "\n"
    )
    forms_report = {
        "version": 1,
        "complete": True,
        "case_count": len(forms_results),
        "analyzer_parity": False,
        "scope": "typed declaration grammar",
        "implementation_sha256": {
            str(path.relative_to(ROOT)): digest(path)
            for path in [ROOT / "src/forms.rs"]
        },
        "cases": forms_results,
    }
    (out / "forms-report.json").write_text(
        json.dumps(forms_report, indent=2) + "\n"
    )
    assert before=={str(p):digest(p) for p in source_files},'frontend inputs or sources changed'
    report['implementation_sha256']=before
    (out/'rust-report.json').write_text(json.dumps(report,indent=2)+'\n')
    print(
        f"PASS: {len(results)} cases, {modules} modules, {nodes} Core nodes, {terms} terms decoded"
    )


if __name__ == "__main__":
    main()
