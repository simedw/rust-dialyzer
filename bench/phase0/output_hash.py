#!/usr/bin/env python3
"""Print a SHA-1 of each core_cache_analyze JSON output with timing fields removed.

Two runs with identical warnings and signatures print the same hash, so this
checks determinism and "byte-identical output" across changes.
Phoenix scenario reference hash: eadaeb93e4b89908ec1bb33c8f53ba01502d5e16
"""
import hashlib
import json
import sys


def strip(o):
    if isinstance(o, dict):
        return {k: strip(v) for k, v in o.items()
                if not k.endswith('_seconds') and 'tim' not in k.lower() and not k.endswith('_ms')}
    if isinstance(o, list):
        return [strip(x) for x in o]
    return o


for f in sys.argv[1:]:
    print(hashlib.sha1(json.dumps(strip(json.load(open(f))), sort_keys=True).encode()).hexdigest(), f)
