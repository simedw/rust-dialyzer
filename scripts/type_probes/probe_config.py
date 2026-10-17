"""Shared profile and artifact options; generators retain their OTP28 defaults."""
import argparse
import os
from pathlib import Path
import sys


def configuration(workspace):
    parser = argparse.ArgumentParser(add_help=False)
    parser.add_argument("--otp", choices=("28", "29"), default=os.environ.get("DIALYZER_PROBE_OTP", "28"))
    parser.add_argument("--binary", type=Path, default=os.environ.get("DIALYZER_PROBE_BINARY"))
    parser.add_argument("--otp-install", type=Path, default=os.environ.get("DIALYZER_PROBE_OTP_INSTALL"))
    parser.add_argument("--out", type=Path, default=os.environ.get("DIALYZER_PROBE_OUT"))
    args, remaining = parser.parse_known_args()
    sys.argv[1:] = remaining
    args.binary = args.binary or workspace / "target/release/type_probe"
    args.out = args.out or workspace / ".cache/types"
    args.out.mkdir(parents=True, exist_ok=True)
    if args.otp_install:
        os.environ["PATH"] = str(args.otp_install.resolve() / "bin") + os.pathsep + os.environ["PATH"]
    args.union_arity = 9 if args.otp == "29" else 8
    return args


def read_commands(path, config):
    """Read prior gate inputs even when the runner has archived their JSONL."""
    import gzip
    if config.otp == "29" and path.name == "raw_commands.jsonl":
        path = path.with_name("raw_baseline_pool.jsonl")
    if path.exists():
        return path.open()
    return gzip.open(str(path) + ".gz", "rt")
