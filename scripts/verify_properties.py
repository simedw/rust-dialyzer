#!/usr/bin/env python3
"""Run the original OTP PropEr generators and property contracts against Rust."""
import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / ".cache/properties"


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


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
        ROOT / "scripts/type_wire.exs",
        ROOT / "scripts/verify_properties.exs",
        ROOT / "scripts/verify_properties.py",
        ROOT / "scripts/build_property_dependency.py",
        ROOT / "corpus/sources.json",
    ]
    initial_hashes = {str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest() for p in files}

    subprocess.run(
        ["python3", "scripts/build_property_dependency.py"], cwd=ROOT, check=True
    )
    subprocess.run(
        ["cargo", "build", "--release", "--offline", "--bin", "type_probe"],
        cwd=ROOT,
        check=True,
    )
    proper = ROOT / ".cache/upstream/proper"
    otp = ROOT / ".cache/upstream/otp"
    subprocess.run(
        [
            "elixir",
            "scripts/verify_properties.exs",
            str(proper),
            str(otp),
            str(OUT),
            str(ROOT / "target/release/type_probe"),
            "5000",
        ],
        cwd=ROOT,
        check=True,
    )
    path = OUT / "report.json"
    report = json.loads(path.read_text())
    expected = {
        "absorption",
        "associativity",
        "commutativity",
        "idempotence",
        "identity",
        "limit",
    }
    actual = report["properties"]
    report["full_domain_properties_verified"] = (
        len(actual) == len(expected)
        and {p["property"] for p in actual} == expected
        and all(
            p["requested"] == p["passed"] == 5000 and not p["failures"] for p in actual
        )
        and report["unsupported"] == 0
    )

    if initial_hashes != {str(p.relative_to(ROOT)): digest(p) for p in files}:
        raise RuntimeError("property implementation changed during verification")
    report["implementation_sha256"] = initial_hashes
    report["upstream_generator_sha256"] = digest(
        otp / "lib/dialyzer/test/property_test/erl_types_prop.erl"
    )
    report["commands_sha256"] = digest(OUT / "commands.jsonl")
    report["results_sha256"] = digest(OUT / "results.jsonl")
    path.write_text(json.dumps(report, indent=2) + "\n")
    print(
        json.dumps(
            {
                "properties": len(actual),
                "checks": sum(p["passed"] for p in actual),
                "complete": report["full_domain_properties_verified"],
            }
        )
    )
    if not report["full_domain_properties_verified"]:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
