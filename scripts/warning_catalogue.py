#!/usr/bin/env python3
"""Extract pinned native warning-profile rules for differential verification."""
import hashlib
import json
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / ".cache/upstream/otp/lib/dialyzer/src"
header = (SOURCE / "dialyzer.hrl").read_text()
names = dict(re.findall(r"-define\((WARN_\w+),\s*(\w+)\)", header))
source = (SOURCE / "dialyzer_options.erl").read_text()
default = source.split("DefaultWarns = [", 1)[1].split("],", 1)[0]
body = source.split("build_warnings([Opt|Opts], Warnings) ->", 1)[1].split(
    "OtherAtom ->", 1
)[0]
branches = re.findall(r"^\s+(\w+) ->(.*?)(?=^\s+\w+ ->|\Z)", body, re.M | re.S)
options = []
for name, branch in branches:
    tags = [names[token] for token in re.findall(r"\?(WARN_\w+)", branch)]
    if "ordsets:del_element" in branch or "ordsets:subtract" in branch:
        action = "disable"
    elif "ordsets:add_element" in branch or "ordsets:union" in branch:
        action = "enable"
    else:
        raise RuntimeError(f"unrecognized warning rule: {name}")
    options.append({"name": name, "action": action, "tags": tags})
if len(options) != 26:
    raise RuntimeError(f"warning grammar changed: {len(options)} options")
artifact = {
    "version": 1,
    "otp_sha": "ab489c0b260c637a9487f8753909ea94dacbbec2",
    "source_sha256": {
        name: hashlib.sha256((SOURCE / name).read_bytes()).hexdigest()
        for name in ["dialyzer_options.erl", "dialyzer.hrl"]
    },
    "tags": sorted(names.values()),
    "default": sorted(names[token] for token in re.findall(r"\?(WARN_\w+)", default)),
    "options": options,
}
(ROOT / "corpus/warnings.json").write_text(json.dumps(artifact, indent=2) + "\n")
print(
    json.dumps(
        {
            "tags": len(artifact["tags"]),
            "options": len(options),
            "default": len(artifact["default"]),
        }
    )
)
