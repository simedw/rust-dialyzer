#!/usr/bin/env python3
"""Compare two raw dialyxir warning outputs as multisets, ignoring order.

Lines that are not raw warning terms (progress output, timings) are ignored.
Exits 0 when the multisets are equal, 1 otherwise.
"""

import sys
from collections import Counter


def load(path):
    with open(path, encoding="utf-8") as f:
        return Counter(line.rstrip("\n") for line in f if line.startswith("{:warn_"))


def main():
    if len(sys.argv) != 3:
        sys.exit("usage: warnset.py EXPECTED ACTUAL")
    expected, actual = load(sys.argv[1]), load(sys.argv[2])
    missing, extra = expected - actual, actual - expected
    print(f"expected {sum(expected.values())}, actual {sum(actual.values())}, "
          f"missing {sum(missing.values())}, extra {sum(extra.values())}")
    for label, diff in (("missing", missing), ("extra", extra)):
        for line, n in sorted(diff.items()):
            print(f"{label} x{n}: {line[:400]}")
    sys.exit(0 if not missing and not extra else 1)


if __name__ == "__main__":
    main()
