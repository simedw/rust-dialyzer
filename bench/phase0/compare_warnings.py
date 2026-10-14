#!/usr/bin/env python3
"""Compare native analyzer warnings with an OTP oracle run, ignoring order.

Usage: compare_warnings.py OTP_ORACLE_OUTPUT RUST_RESULT.json [DIFF.json]

OTP_ORACLE_OUTPUT is stdout of reports/real-projects/harness/oracle.exs (last
line is the JSON result). RUST_RESULT.json is core_cache_analyze output.
Both carry warning terms in the harness wire format. Exits 0 on equality.
"""

import collections
import gzip
import json
import sys
from functools import lru_cache


def expand(arena):
    """Rebuild a flat term arena as nested terms (same as harness/run.py)."""
    terms = arena["terms"]

    @lru_cache(None)
    def at(index):
        term = terms[index]
        kind = term["kind"]
        if kind == "nil":
            return {"kind": "list", "items": [], "tail": None}
        if kind == "tuple":
            return {"kind": kind, "items": [at(i) for i in term["items"]]}
        if kind == "list":
            items, tail = [at(i) for i in term["items"]], at(term["tail"])
            if tail["kind"] == "list":
                items += tail["items"]
                tail = tail["tail"]
            return {"kind": kind, "items": items, "tail": tail}
        return term

    return at(arena["root"])


def items(term):
    assert term["kind"] == "list", term["kind"]
    return term["items"]


def tag(warning):
    return warning["items"][0].get("value", "?")


def main():
    otp_path, rust_path = sys.argv[1:3]
    def read(path):
        return gzip.open(path, 'rt') if path.endswith('.gz') else open(path)
    with read(otp_path) as stream:
        otp = json.loads(stream.read().strip().splitlines()[-1])
    with read(rust_path) as stream:
        rust = json.load(stream)
    key = lambda w: json.dumps(w, sort_keys=True)
    otp_c = collections.Counter(map(key, items(otp["warnings"])))
    rust_c = collections.Counter(map(key, items(expand(rust["warnings"]))))
    otp_only, rust_only = otp_c - rust_c, rust_c - otp_c
    print(f"otp {sum(otp_c.values())}, rust {sum(rust_c.values())}, "
          f"shared {sum((otp_c & rust_c).values())}, "
          f"otp-only {sum(otp_only.values())}, rust-only {sum(rust_only.values())}")
    for label, diff in (("otp-only", otp_only), ("rust-only", rust_only)):
        tags = collections.Counter(tag(json.loads(k)) for k in diff.elements())
        if tags:
            print(f"  {label} by tag: {dict(tags.most_common())}")
    if len(sys.argv) > 3:
        json.dump({"otp_only": [json.loads(k) for k in otp_only.elements()],
                   "rust_only": [json.loads(k) for k in rust_only.elements()]},
                  open(sys.argv[3], "w"), indent=1)
    sys.exit(0 if not otp_only and not rust_only else 1)


if __name__ == "__main__":
    main()
