#!/usr/bin/env python3
"""Replay frozen original numeric operations and complete modules in Root."""
import hashlib
import itertools
import json
import pathlib
import subprocess

ROOT = pathlib.Path(__file__).resolve().parents[2]
OUT = ROOT / ".cache/numeric_bifs"
OUT.mkdir(parents=True, exist_ok=True)
SHIFT = pathlib.Path("/private/tmp/rust-dialyzer-arithmetic-limit-checkpoint-1")
WIDEN = pathlib.Path("/private/tmp/rust-dialyzer-widen-number-checkpoint-1")


def sha(path):
    return hashlib.sha256(pathlib.Path(path).read_bytes()).hexdigest()


def main():
    checkpoints = {}
    for base in (SHIFT, WIDEN):
        manifest = json.loads((base / "checkpoint.json").read_text())
        for name, digest in manifest.get("files", manifest).items():
            path = base / name
            if isinstance(digest, str) and path.is_file():
                assert sha(path) == digest, path
                checkpoints[str(path)] = digest
    library = SHIFT / "library.json"
    profile = ROOT / ".cache/analysis-profile.json"
    jobs = [
        ("shift", SHIFT / "shift-native.jsonl", "arithmetic_limit_probe", []),
        ("widen", WIDEN / "proof/native.jsonl", "widen_number_probe", []),
        ("limit", SHIFT / "limit-module-native.jsonl", "whole_module_probe", [library, profile]),
        ("lists_key", WIDEN / "lists_key_bug_embedded.jsonl", "whole_module_probe", [library, profile]),
    ]
    paths = list((ROOT / "src").rglob("*.rs")) + [
        ROOT / "Cargo.toml", ROOT / "Cargo.lock", pathlib.Path(__file__), library, profile
    ]
    paths += [ROOT / "target/release" / name for _, _, name, _ in jobs]
    before = {str(path): sha(path) for path in paths}
    counts = {}
    for name, native, binary, args in jobs:
        output = OUT / (name + ".jsonl")
        with native.open("rb") as source, output.open("wb") as target:
            subprocess.run([str(ROOT / "target/release" / binary), *map(str, args)],
                           stdin=source, stdout=target, cwd=ROOT, check=True)
        count = 0
        with native.open() as source, output.open() as target:
            for expected, actual in itertools.zip_longest(source, target):
                assert expected is not None and actual is not None, name
                expected, actual = json.loads(expected), json.loads(actual)
                assert expected["id"] == actual["id"], name
                assert actual["matched"] if name == "shift" else expected["native"] == actual.get("native"), (name, actual)
                count += 1
        counts[name] = count
    assert counts == {"shift": 478, "widen": 4263, "limit": 5, "lists_key": 1}, counts
    assert before == {str(path): sha(path) for path in paths}
    assert checkpoints == {path: sha(path) for path in checkpoints}
    report = {"matches": counts, "source_sha256": before,
              "checkpoint_sha256": checkpoints,
              "outputs": {path.name: sha(path) for path in OUT.glob("*.jsonl")},
              "full_inference": False}
    (OUT / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    print(counts)


if __name__ == "__main__":
    main()
