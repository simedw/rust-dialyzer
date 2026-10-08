#!/usr/bin/env python3
"""Compare warning-profile transitions with the native option implementation."""
import hashlib
import itertools
import json
from pathlib import Path
import random
import subprocess

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / ".cache/warnings"


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    subprocess.run(["python3", "scripts/warning_catalogue.py"], cwd=ROOT, check=True)
    subprocess.run(
        ["cargo", "build", "--release", "--offline", "--bin", "warning_probe"],
        cwd=ROOT,
        check=True,
    )
    catalogue = json.loads((ROOT / "corpus/warnings.json").read_text())
    tags = catalogue["tags"]
    options = [r["name"] for r in catalogue["options"]]
    initial = [[], tags, catalogue["default"], tags[::2], tags[1::2]]
    rows = [{"initial": tags, "options": []}] + [
        {"initial": p, "options": list(opts)}
        for p in initial
        for opts in [
            (),
            *[(o,) for o in options],
            *itertools.product(options, repeat=2),
        ]
    ]
    rng = random.Random(28307)
    rows += [
        {
            "initial": rng.sample(tags, rng.randrange(len(tags) + 1)),
            "options": rng.choices(options, k=rng.randrange(50)),
        }
        for _ in range(5000)
    ]
    rows += [{"initial": p, "options": ["invalid_warning_option"]} for p in initial]
    request = OUT / "requests.jsonl"
    request.write_text("".join(json.dumps(r) + "\n" for r in rows))
    results = []
    for name, command in [
        ("native", ["elixir", "scripts/warning_oracle.exs"]),
        ("rust", [str(ROOT / "target/release/warning_probe")]),
    ]:
        path = OUT / (name + ".jsonl")
        with request.open() as inp, path.open("w") as out:
            subprocess.run(command, cwd=ROOT, stdin=inp, stdout=out, check=True)
        result = [json.loads(line) for line in path.read_text().splitlines()]
        if len(result) != len(rows):
            raise RuntimeError("different warning result count")
        results.append(result)
    failures = [
        {"request": r, "native": n, "rust": s}
        for r, n, s in zip(rows, *results)
        if not (n == s or "error" in n and "error" in s)
    ]
    files = [
        ROOT / "src/warnings.rs",
        ROOT / "src/bin/warning_probe.rs",
        ROOT / "corpus/warnings.json",
        ROOT / "scripts/warning_catalogue.py",
        ROOT / "scripts/warning_oracle.exs",
        ROOT / "scripts/verify_warnings.py",
    ]
    report = {
        "version": 1,
        "complete_profiles": not failures,
        "complete_diagnostics": False,
        "commands": len(rows),
        "passed": len(rows) - len(failures),
        "failures": failures,
        "seed": 28307,
        "implementation_sha256": {
            str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest()
            for p in files
        },
    }
    (OUT / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    print(
        json.dumps(
            {
                "commands": len(rows),
                "passed": report["passed"],
                "failures": len(failures),
            }
        )
    )
    if failures:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
