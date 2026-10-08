#!/usr/bin/env python3
"""Find every module that makes whole-program native analysis abort.

Usage: analysis_loop.py NATIVE_BIN OUT_DIR FRONTEND_INVENTORY [--exclude M ...]

Starts from the modules that passed the frontend in FRONTEND_INVENTORY
(inventory.json from inventory.py) and that appear in OUT_DIR's request. Each
round analyzes the whole set; when analysis aborts, the module named by the
error (or by the last resource-limit trace on stderr) is removed and the round
repeats. Stops when analysis passes or the failure cannot be attributed.
Writes OUT_DIR/analysis-loop.json.
"""

import argparse
import json
import os
import re
import subprocess
import time
from pathlib import Path


def attribute(detail, stderr, names):
    m = re.search(r"module (\S+) (?:node|call|unknown)", detail)
    if m:
        return m.group(1)
    events = [json.loads(line.split(" ", 1)[1]) for line in stderr.splitlines()
              if line.startswith("RUST_DIALYZER_GUARD ") and "limit" in line]
    if events and "module_id" in events[-1]:
        return names[events[-1]["module_id"]]
    return None


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("native", type=Path)
    ap.add_argument("out", type=Path)
    ap.add_argument("frontend", type=Path)
    ap.add_argument("--exclude", nargs="*", default=[])
    ap.add_argument("--rounds", type=int, default=30)
    args = ap.parse_args()
    request = json.loads((args.out / "request.jsonl").read_text())
    allowed = set(request["beam_paths"])
    inventory = json.loads(args.frontend.read_text())
    beams = [m["beam"] for m in inventory["modules"] if m["status"] == "ok" and m["beam"] in allowed]
    by_name = {Path(b).stem: b for b in beams}
    removed = [dict(module=name, detail="excluded") for name in args.exclude]
    for name in args.exclude:
        beams.remove(by_name[name])

    for round_ in range(args.rounds):
        # The analyzer sorts modules by name; module ids index that order.
        names = sorted(Path(b).stem for b in beams)
        started = time.time()
        p = subprocess.run(
            [args.native, "--inventory", args.out / "prior-library.json",
             args.out / "closure-profile.json"],
            input=json.dumps(dict(request, beam_paths=beams)) + "\n",
            capture_output=True, text=True, env=dict(os.environ, RUST_DIALYZER_TIMINGS="0"))
        try:
            analysis = json.loads(p.stdout.strip().splitlines()[-1])["analysis"]
        except (ValueError, IndexError, KeyError):
            analysis = dict(status="crash", code=p.returncode, detail=p.stderr[-300:])
        detail = str(analysis.get("detail", ""))
        print(round_, len(beams), f"{time.time() - started:.0f}s", analysis.get("status"),
              detail[:240], analysis.get("public_warning_count", ""), flush=True)
        if analysis.get("status") == "ok":
            break
        culprit = attribute(detail, p.stderr, names)
        if culprit not in by_name or by_name[culprit] not in beams:
            print("cannot attribute failure; stopping")
            break
        removed.append(dict(module=culprit, detail=detail))
        beams.remove(by_name[culprit])
        save(args.out, removed, analysis, beams)

    save(args.out, removed, analysis, beams)


def save(out, removed, analysis, beams):
    """Written after every round so progress survives interruption."""
    (out / "analysis-loop.json").write_text(
        json.dumps(dict(removed=removed, last=analysis, modules=len(beams)), indent=1) + "\n")


if __name__ == "__main__":
    main()
