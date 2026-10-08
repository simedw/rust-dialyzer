#!/usr/bin/env python3
"""Run core_cache_analyze once and check it against an OTP oracle.

Usage: check.py BINARY MANIFEST ORACLE_DIR OUT_DIR [--options OPTIONS.json] [--env K=V ...]

ORACLE_DIR comes from full_oracle.exs (otp-warnings.json and
otp-full-signatures.json). Prints wall time, analysis time, instructions
retired, and whether warnings and signatures match exactly. Exits 1 on any
mismatch or failure.
"""

import argparse
import json
import os
import re
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
PRIOR = REPO / ".cache/phase0/rust-cone/out/prior-library.json"
OPTIONS = REPO / ".cache/phase0/core-cache/options-unknown.json"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("binary", type=Path)
    ap.add_argument("manifest", type=Path)
    ap.add_argument("oracle", type=Path)
    ap.add_argument("out", type=Path)
    ap.add_argument("--options", type=Path, default=OPTIONS)
    ap.add_argument("--env", nargs="*", default=[])
    args = ap.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ, **dict(kv.split("=", 1) for kv in args.env))
    result_path, stderr_path = args.out / "out.json", args.out / "stderr"
    with open(result_path, "w") as out, open(stderr_path, "w") as err:
        code = subprocess.run(["/usr/bin/time", "-l", args.binary, PRIOR, args.manifest, args.options],
                              stdout=out, stderr=err, env=env).returncode
    stderr = stderr_path.read_text()
    stat = lambda pat: (m := re.search(pat, stderr)) and m.group(1)
    result = json.loads(result_path.read_text() or "{}")
    summary = {
        "status": result.get("status", f"exit {code}"),
        "wall_s": float(stat(r"([\d.]+) real") or 0),
        "load_s": round(result.get("load_seconds", 0), 2),
        "analysis_s": round(result.get("analysis_seconds", 0), 2),
        "instructions_T": round(int(stat(r"(\d+)\s+instructions retired") or 0) / 1e12, 3),
        "peak_GB": round(int(stat(r"(\d+)\s+peak memory footprint") or 0) / 1e9, 2),
    }
    ok = summary["status"] == "ok"
    if ok:
        (args.out / "sigs.jsonl").write_text(json.dumps({"signatures": result["signatures"]}) + "\n")
        w = subprocess.run([sys.executable, HERE / "compare_warnings.py",
                            args.oracle / "otp-warnings.json", result_path],
                           capture_output=True, text=True)
        s = subprocess.run([sys.executable, REPO / "reports/real-projects/fixes/inferred-signatures/compare.py",
                            args.out / "sigs.jsonl", args.oracle / "otp-full-signatures.json",
                            "--out", args.out / "sig-comparison.json"], capture_output=True, text=True)
        sig = json.loads(s.stdout.strip().splitlines()[-1])
        summary["warnings"] = w.stdout.splitlines()[0]
        summary["signatures"] = f'{sig["exact_matches"]}/{sig["otp_signature_count"]} exact, ' \
                                f'{sig["missing_count"]} missing, {sig["extra_count"]} extra'
        ok = w.returncode == 0 and sig["complete_exact_parity"]
    else:
        summary["detail"] = str(result.get("detail", stderr[-400:]))[:400]
    summary["parity"] = ok
    print(json.dumps(summary, indent=1))
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
