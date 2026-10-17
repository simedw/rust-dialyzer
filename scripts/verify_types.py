#!/usr/bin/env python3
"""Run every shared type-family and solver-record differential gate."""
import argparse
import gzip
import math
import os
import shutil
import hashlib
import json
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / ".cache/types"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--otp", choices=("28", "29"), default="28")
    parser.add_argument("--binary", type=Path)
    parser.add_argument("--otp-install", type=Path)
    parser.add_argument("--out", type=Path, default=OUT)
    parser.add_argument("--compress-jsonl", action="store_true")
    args = parser.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ)
    if args.otp_install:
        env["PATH"] = str(args.otp_install.resolve() / "bin") + os.pathsep + env["PATH"]
    if args.otp == "29":
        env.setdefault("ERL_FLAGS", "+S 1:1")
        elixir_bin = Path.home() / ".asdf/installs/elixir/1.19.4-otp-28/bin"
        if elixir_bin.is_dir():
            env["PATH"] = str(elixir_bin) + os.pathsep + env["PATH"]
    actual_otp = subprocess.check_output(["erl", "-noshell", "-eval", 'io:format("~s",[erlang:system_info(otp_release)]),halt().'], env=env, text=True)
    if actual_otp != args.otp:
        raise RuntimeError(f"requested OTP{args.otp}, runtime is {actual_otp!r}")
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
        *sorted((ROOT / "src/lattice").glob("*.rs")),
    ]
    initial_hashes = {str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest() for p in files}

    if args.binary is None:
        if args.otp != "28":
            raise RuntimeError("OTP29 requires an existing --binary")
        subprocess.run(["cargo", "build", "--release", "--offline", "--bin", "type_probe"], cwd=ROOT, check=True)
        binary = ROOT / "target/release/type_probe"
    else:
        binary = out / "type_probe.snapshot"
        payload = args.binary.resolve().read_bytes()
        temp = binary.with_suffix(".tmp")
        temp.write_bytes(payload)
        temp.chmod(args.binary.stat().st_mode)
        temp.replace(binary)
    binary_hash = hashlib.sha256(binary.read_bytes()).hexdigest()
    env.update(DIALYZER_PROBE_OTP=args.otp, DIALYZER_PROBE_BINARY=str(binary), DIALYZER_PROBE_OUT=str(out))
    if args.otp_install:
        env["DIALYZER_PROBE_OTP_INSTALL"] = str(args.otp_install.resolve())
    arity = 9 if args.otp == "29" else 8
    raw_types = 4 + arity + sum(math.comb(arity, size) for size in (0, 1, 2, arity)) + 14
    raw_count = 4 * raw_types ** 2 + 11 * raw_types
    gates = [
        ("probe", "results", 35908),
        ("family_probe", "family_results", 35673),
        ("callable_probe", "callable_results", 85548),
        ("map_probe", "map_results", 84590),
        ("nominal_probe", "nominal_results", 38072),
        ("raw_probe", "raw_results", raw_count),
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
        print(f"OTP{args.otp} {name}: expecting {count} cases", flush=True)
        try:
            subprocess.run([sys.executable, str(ROOT / "scripts/type_probes" / (name + ".py"))], cwd=ROOT, check=True, env=env)
        finally:
            if args.compress_jsonl:
                for artifact in out.glob("*.jsonl"):
                    with artifact.open("rb") as source, gzip.open(str(artifact) + ".gz", "wb", compresslevel=1) as target:
                        shutil.copyfileobj(source, target)
                    artifact.unlink()
        report = json.loads((out / (output + ".jsonl.comparison.json")).read_text())
        if report["runtime"]["otp"] != args.otp:
            raise RuntimeError(f"{name}: oracle runtime changed")
        if report["commands"] != count:
            raise RuntimeError(
                f'{name}: unexpected denominator {report["commands"]} != {count}'
            )
        reports.append({"id": name, **report})
        (out / "progress.json").write_text(json.dumps({"otp": args.otp, "binary_sha256": binary_hash, "gates": reports}, indent=2) + "\n")

    report = {
        "version": 3,
        "otp": args.otp,
        "binary_sha256": binary_hash,
        "verify_mode": env.get("RUST_DIALYZER_VERIFY_FASTPATH", ""),
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
    if hashlib.sha256(binary.read_bytes()).hexdigest() != binary_hash:
        raise RuntimeError("type probe changed during verification")
    (out / "report.json").write_text(json.dumps(report, indent=2) + "\n")
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
