#!/usr/bin/env python3
"""Run the native analyzer on each module of a prepared request by itself.

Usage: probe_modules.py NATIVE_BIN OUT_DIR [--jobs N] [--timeout S]

OUT_DIR must contain request.jsonl, prior-library.json and closure-profile.json
from the real-project harness. Writes probe.json with one record per module:
ok, error (analyzer reported an error), panic (non-zero exit without a result)
or timeout. Calls into other target modules are unknown in a one-module run, so
this finds frontend and analyzer failures, not cross-module behaviour.
"""

import argparse
import json
import os
import subprocess
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path


def probe(native, out, request, beam, timeout):
    single = dict(request, beam_paths=[beam])
    env = dict(os.environ, RUST_DIALYZER_TIMINGS="0")
    try:
        p = subprocess.run(
            [native, out / "prior-library.json", out / "closure-profile.json"],
            input=json.dumps(single) + "\n", capture_output=True, text=True,
            timeout=timeout, env=env)
    except subprocess.TimeoutExpired:
        return dict(beam=beam, status="timeout")
    last = p.stdout.strip().splitlines()[-1:] or [""]
    try:
        result = json.loads(last[0])
    except ValueError:
        result = None
    if p.returncode == 0 and result and "error" not in result:
        return dict(beam=beam, status="ok")
    if result and "error" in result:
        return dict(beam=beam, status="error", detail=result["error"][:300])
    panic = next((l for l in p.stderr.splitlines() if l.strip() and "panicked" not in l), "")
    return dict(beam=beam, status="panic", code=p.returncode, detail=panic[:300])


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("native", type=Path)
    ap.add_argument("out", type=Path)
    ap.add_argument("--jobs", type=int, default=8)
    ap.add_argument("--timeout", type=int, default=300)
    args = ap.parse_args()
    request = json.loads((args.out / "request.jsonl").read_text())
    with ThreadPoolExecutor(args.jobs) as pool:
        results = list(pool.map(
            lambda b: probe(args.native, args.out, request, b, args.timeout),
            request["beam_paths"]))
    (args.out / "probe.json").write_text(json.dumps(results, indent=1) + "\n")
    counts = {}
    for r in results:
        counts[r["status"]] = counts.get(r["status"], 0) + 1
    print(json.dumps(counts))
    for r in results:
        if r["status"] != "ok":
            print(r["status"], Path(r["beam"]).name, r.get("detail", ""))


if __name__ == "__main__":
    main()
