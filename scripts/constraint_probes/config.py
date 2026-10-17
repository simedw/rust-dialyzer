"""Shared runtime selection for the constraint differential gates."""
import argparse
import os
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
PREFIX = "RUST_DIALYZER_CONSTRAINTS_"


def parser():
    result = argparse.ArgumentParser()
    result.add_argument("--otp", choices=("28", "29"), default=os.getenv(PREFIX + "OTP", "28"))
    result.add_argument("--binary", type=Path, default=os.getenv(PREFIX + "BINARY"))
    result.add_argument("--otp-install", type=Path, default=os.getenv(PREFIX + "INSTALL"))
    result.add_argument("--otp-source", type=Path, default=os.getenv(PREFIX + "SOURCE"))
    result.add_argument("--out", type=Path, default=os.getenv(PREFIX + "OUT"))
    return result


def configure(args=None):
    args = parser().parse_args() if args is None else args
    args.binary_explicit = args.binary is not None
    args.binary = (args.binary or ROOT / "target/release/constraints_probe").resolve()
    args.out = (args.out or ROOT / ".cache/constraints").resolve()
    if args.otp == "29" and (args.otp_install is None or args.otp_source is None):
        raise ValueError("OTP29 requires --otp-install and matching --otp-source")
    args.otp_install = args.otp_install.resolve() if args.otp_install else None
    args.otp_source = (args.otp_source or ROOT / ".cache/upstream/otp").resolve()
    args.source_file = args.otp_source / "lib/dialyzer/src/dialyzer_typesig.erl"
    if args.otp == "29" and (args.otp_install is None or not args.source_file.is_file()):
        raise ValueError("OTP29 requires --otp-install and matching --otp-source")
    args.env = os.environ.copy()
    args.env.update(ASDF_ERLANG_VERSION="28.3", ASDF_ELIXIR_VERSION="1.19.4-otp-28")
    if args.otp_install:
        args.env["PATH"] = str(args.otp_install / "bin") + os.pathsep + args.env.get("PATH", "")
    values = {"OTP": args.otp, "BINARY": args.binary, "OUT": args.out, "SOURCE": args.otp_source}
    if args.otp_install:
        values["INSTALL"] = args.otp_install
    args.env.update({PREFIX + key: str(value) for key, value in values.items()})
    args.env[PREFIX + "SOURCE_FILE"] = str(args.source_file)
    args.out.mkdir(parents=True, exist_ok=True)
    return args
