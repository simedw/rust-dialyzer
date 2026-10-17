#!/usr/bin/env python3
"""Compare Rust Core output with each installed OTP compiler.

Example: python3 scripts/verify_otp_frontend.py --otp ~/.asdf/installs/erlang/28.3 \
    --otp .cache/otp-29.1.2/runtime --compiler target/release/beam_core_compile

This checks complete raw and cleaned Core terms. It does not certify Dialyzer
analysis compatibility; run the type, constraint, and warning gates separately.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys


ROOT = Path(__file__).resolve().parents[1]
CAPTURE = r'''
[Beam, Out] = init:get_plain_arguments(),
Opts = [no_copt,to_core,binary,return_errors,no_inline,strict_record_tests,
        dialyzer,no_spawn_compiler_process],
{ok,{Mod,[{debug_info,{debug_info_v1,Backend,Data}}]}} =
    beam_lib:chunks(Beam,[debug_info]),
{ok,Raw} = Backend:debug_info(core_v1,Mod,Data,Opts),
Clean = dialyzer_clean_core:clean(Raw),
ok = file:write_file(filename:join(Out,"expected.raw.etf"),term_to_binary(Raw)),
ok = file:write_file(filename:join(Out,"expected.clean.etf"),term_to_binary(Clean)),
halt().
'''
COMPARE = r'''
[Expected,Actual] = init:get_plain_arguments(),
{ok,E} = file:read_file(Expected), {ok,A} = file:read_file(Actual),
case binary_to_term(E) =:= binary_to_term(A) of
    true -> halt(0);
    false -> io:format("Core terms differ~n"), halt(1)
end.
'''
METADATA = r'''
ok = application:load(dialyzer),
{ok,Props} = application:get_all_key(dialyzer),
io:format("~s|~s|~s|~s",[erlang:system_info(otp_release),
    erlang:system_info(version),proplists:get_value(vsn,Props),
    code:lib_dir(dialyzer)]),halt().
'''


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def run(command, env, log):
    result = subprocess.run(
        list(map(str, command)), cwd=ROOT, env=env, capture_output=True, timeout=120
    )
    log.write_bytes(result.stdout + result.stderr)
    return result.returncode


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--otp", type=Path, action="append", required=True)
    parser.add_argument("--compiler", type=Path, required=True)
    parser.add_argument("--fixture", type=Path, action="append")
    parser.add_argument("--output", type=Path, default=ROOT / ".cache/otp-frontend")
    args = parser.parse_args()
    compiler = args.compiler.resolve(strict=True)
    fixtures = args.fixture or [
        ROOT / "tests/fixtures/beam_core_port/aliases/native_aliases.erl",
        ROOT / "tests/fixtures/beam_core_port/aliases/native_impossible_aliases.erl",
    ]
    fixtures = [p.resolve(strict=True) for p in fixtures]
    expected_hash = digest(compiler)
    args.output.mkdir(parents=True, exist_ok=True)
    report = {"compiler": str(compiler), "compiler_sha256": expected_hash, "versions": []}
    for index, install in enumerate(args.otp):
        install = install.resolve(strict=True)
        erl, erlc = install / "bin/erl", install / "bin/erlc"
        env = dict(os.environ, PATH=str(install / "bin") + os.pathsep + os.environ["PATH"])
        env["ERL_FLAGS"] = "+S 2:2"
        metadata = subprocess.check_output(
            [str(erl), "-noshell", "-eval", METADATA], env=env, timeout=30, text=True
        ).strip().split("|")
        major, erts, dialyzer, lib = metadata
        if major not in {"28", "29"}:
            parser.error(f"OTP {major} is outside the supported test scope (28 and 29)")
        directory = args.output / f"{index}-otp-{major}"
        directory.mkdir(exist_ok=True)
        row = {"install": str(install), "otp": major, "erts": erts, "dialyzer": dialyzer,
               "runtime_sha256": {name: digest(Path(lib) / "ebin" / (name + ".beam"))
                                  for name in ["dialyzer_typesig", "dialyzer_contracts"]},
               "cases": []}
        for fixture in fixtures:
            case_dir = directory / fixture.stem
            case_dir.mkdir(exist_ok=True)
            case = {"source": str(fixture), "source_sha256": digest(fixture), "stages": {}}
            command = [erlc, "+debug_info", "-o", case_dir, fixture]
            case["compile_exit"] = run(command, env, case_dir / "compile.log")
            if case["compile_exit"] == 0:
                beam = case_dir / (fixture.stem + ".beam")
                case["oracle_exit"] = run(
                    [erl, "-noshell", "-eval", CAPTURE, "-extra", beam, case_dir],
                    env, case_dir / "oracle.log",
                )
                if case["oracle_exit"] == 0:
                    for mode, name in [("core", "raw"), ("clean", "clean")]:
                        actual = case_dir / f"actual.{name}.etf"
                        code = run([compiler, mode, beam, actual], env, case_dir / f"{name}.log")
                        case["stages"][name] = code == 0 and run(
                            [erl, "-noshell", "-eval", COMPARE, "-extra",
                             case_dir / f"expected.{name}.etf", actual],
                            env, case_dir / f"{name}.comparison.log",
                        ) == 0
            case["passed"] = case["stages"] == {"raw": True, "clean": True}
            row["cases"].append(case)
            print(f"OTP {major}: {fixture.name}: {'PASS' if case['passed'] else 'FAIL'}", flush=True)
        report["versions"].append(row)
        report["passed"] = all(c["passed"] for v in report["versions"] for c in v["cases"])
        (args.output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    if digest(compiler) != expected_hash:
        raise RuntimeError("compiler changed during verification")
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    sys.exit(main())
