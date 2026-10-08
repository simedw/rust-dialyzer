#!/usr/bin/env python3
"""Frontend and analysis coverage inventory for a prepared native request.

Usage: inventory.py NATIVE_BIN OUT_DIR [--jobs N] [--timeout S]

OUT_DIR must contain request.jsonl, prior-library.json and closure-profile.json
from the real-project harness. NATIVE_BIN must support `--inventory`.

1. Every module goes through the frontend in its own process, so a crash in one
   module (Firefly has unsafe code) cannot hide the others.
2. The modules that pass are analyzed together in one process.

Writes inventory.json and prints a summary grouped by stage and failure kind.
"""

import argparse
import collections
import json
import os
import re
import signal
import subprocess
import time
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path


def native(args, request, inventory_analysis):
    row = dict(request, inventory_analysis=inventory_analysis)
    env = dict(os.environ, RUST_DIALYZER_TIMINGS="0")
    return subprocess.run(
        [args.native, "--inventory", args.out / "prior-library.json",
         args.out / "closure-profile.json"],
        input=json.dumps(row) + "\n", capture_output=True, text=True,
        timeout=args.timeout, env=env)


def last_json(stdout):
    lines = stdout.strip().splitlines()
    try:
        return json.loads(lines[-1]) if lines else None
    except ValueError:
        return None


def frontend(args, request, beam):
    try:
        p = native(args, dict(request, beam_paths=[beam]), False)
    except subprocess.TimeoutExpired:
        return dict(beam=beam, stage="frontend", status="timeout")
    result = last_json(p.stdout)
    if p.returncode < 0:
        return dict(beam=beam, stage="frontend", status="crash",
                    detail=signal.Signals(-p.returncode).name)
    if not result or "modules" not in result:
        detail = (result or {}).get("error") or p.stderr[-300:]
        return dict(beam=beam, stage="frontend", status="error", detail=detail)
    return result["modules"][0]


def kind(record):
    """A short failure signature for grouping, without paths and numbers."""
    detail = record.get("detail", "")
    detail = re.sub(r"/\S+\.beam:?", "", detail)
    detail = re.sub(r" at \S+:\d+", "", detail)
    detail = re.sub(r"span: [^}]*", "span", detail)
    detail = re.sub(r"\d+", "N", detail)
    return f'{record["stage"]}/{record["status"]}: {detail.strip()[:140]}'


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("native", type=Path)
    ap.add_argument("out", type=Path)
    ap.add_argument("--jobs", type=int, default=8)
    ap.add_argument("--timeout", type=int, default=600)
    args = ap.parse_args()
    request = json.loads((args.out / "request.jsonl").read_text())

    started = time.time()
    with ThreadPoolExecutor(args.jobs) as pool:
        modules = list(pool.map(lambda b: frontend(args, request, b), request["beam_paths"]))
    frontend_seconds = time.time() - started

    passed = [m["beam"] for m in modules if m["status"] == "ok"]
    started = time.time()
    try:
        p = native(args, dict(request, beam_paths=passed), True)
        result = last_json(p.stdout) or {}
        analysis = result.get("analysis") or dict(
            status="crash" if p.returncode < 0 else "error",
            detail=result.get("error") or p.stderr[-500:])
    except subprocess.TimeoutExpired:
        analysis = dict(status="timeout")
    analysis["wall_seconds"] = time.time() - started

    report = dict(modules=modules, analysis=analysis, frontend_wall_seconds=frontend_seconds)
    (args.out / "inventory.json").write_text(json.dumps(report, indent=1) + "\n")

    failures = [m for m in modules if m["status"] != "ok"]
    print(f"{len(modules)} modules: {len(passed)} pass the frontend, "
          f"{len(failures)} fail ({frontend_seconds:.0f} s)")
    for signature, members in sorted(
            collections.Counter(kind(m) for m in failures).items(), key=lambda kv: -kv[1]):
        names = [Path(m["beam"]).stem for m in failures if kind(m) == signature]
        print(f"{members:4}  {signature}\n      {', '.join(names[:8])}"
              + (" …" if len(names) > 8 else ""))
    print("analysis of passing modules:", json.dumps(analysis)[:600])


if __name__ == "__main__":
    main()
