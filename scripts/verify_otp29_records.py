#!/usr/bin/env python3
"""Differential native-record algebra checks; uses existing binaries and OTP29."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]


def cases():
    scalar = lambda kind: {"kind": kind}
    atom = lambda name: {"kind": "atoms", "values": [name]}
    variable = {"kind": "var", "id": {"atom": "V"}}
    any_t, integer, none = map(scalar, ("any", "integer", "none"))
    def record(name="r", fields=None, module="m"):
        identity = None if name is None else {"module": module, "name": name}
        return {"kind": "record", "name": identity, "fields": [
            {"key": key, "presence": "missing"} if value is None else
            {"key": key, "presence": "present", "value": value}
            for key, value in (fields or {}).items()]}
    def native_atom(name):
        return {"term": "atom", "value": name}
    plain = [record(None), record(), record(fields={"x": integer}),
             record(fields={"x": atom("ok")}), record(fields={"x": None}),
             record(fields={"x": any_t, "y": integer}), record("other"),
             record("r", {"x": integer}, "n"), record(None, {"x": integer})]
    nested = record(fields={"child": record(fields={"x": variable}), "absent": None})
    record_set = {"kind": "record_set", "values": [plain[2], plain[6]]}
    opaque = {"kind": "nominal", "name": {"module": "m", "name": "hidden", "arity": 0, "opacity": "opaque"}, "inner": integer}
    all_types = plain + [nested, record_set, record(fields={"x": opaque}),
                        {"kind": "tuple", "elements": [nested, variable]},
                        {"kind": "proper_list", "elements": nested, "nonempty": False},
                        {"kind": "union", "values": [plain[2], atom("ok")]},
                        {"kind": "map", "entries": [], "default_key": atom("key"), "default_value": nested}]
    out = []
    def add(op, a, **kw):
        out.append(dict(op=op, a=a, native_output=True, **kw))
    for a in plain:
        for b in plain + [any_t, none, integer, scalar("opaque")]:
            for op in ("join", "meet", "subtract", "subtype", "native_order"):
                add(op, a, b=b)
    for a in all_types:
        for op in ("canonical", "structural", "has_var", "collect_var_names", "substitute_all_variables", "widen_to_number", "abstract_records", "type_to_string"):
            add(op, a)
        for depth in (0, 1, 2, 3, 8):
            add("limit", a, depth=depth)
        add("substitute", a, bindings=[{"id": {"atom": "V"}, "type": integer}])
        for args in ([], [native_atom("r")], [native_atom("wrong")], [native_atom("m"), native_atom("r")], [native_atom("n"), native_atom("r")], [{"term": "integer", "value": "2"}]):
            add("record_test", a, args=args)
        for key in ("x", "missing"):
            add("record_get", a, key=key)
        for selector in (None, "nil", {"module": "m", "name": "r"}):
            add("record_replace", a, selector=selector, updates=[{"key": "x", "value": integer}, {"key": "new", "value": atom("ok")}])
        for b in (plain[2], plain[4], record(fields={"x": opaque})):
            add("opacity_conflict", a, b=b, module="outside")
    for a in (record_set, {"kind": "record_set", "values": []}, {"kind": "record_set", "values": [plain[2]]}):
        for b in plain:
            for op in ("join", "meet", "subtract"):
                add(op, a, b=b)
    add("subtract", record_set, b=record_set)  # Upstream recursion is bounded by the oracle timeout.
    add("unify", record(fields={"x": variable}), b=record("other", {"different": integer}))
    add("unify", record(fields={"x": variable, "y": variable}), b=record(fields={"x": integer, "y": atom("ok")}))
    for a in plain:
        for b in plain:
            add("native_record_field_diffs", a, b=b)
    for name in ({"term": "nil"}, native_atom("name"), {"term": "tuple", "values": [native_atom("m"), native_atom("r")]}, {"term": "integer", "value": "42"}):
        add("record_constructor", any_t, name=name, fields=[{"key": native_atom("x"), "value": integer}, {"key": native_atom("x"), "value": atom("last")}, {"key": {"term": "integer", "value": "7"}, "value": any_t}])
    add("canonical", {"kind": "union_raw", "values": [none] * 8 + [plain[2]]})
    add("subtract", any_t, b=plain[2])
    return out


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--runtime", type=Path, default=ROOT / ".cache/otp-29.1.2/runtime")
    parser.add_argument("--elixir", type=Path, default=Path("elixir"))
    parser.add_argument("--out", required=True, type=Path)
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ, PATH=str(args.runtime.resolve() / "bin") + os.pathsep + os.environ["PATH"], ERL_FLAGS="+S 1:1", ERL_CRASH_DUMP=str((args.out / "erl_crash.dump").resolve()))
    release = subprocess.check_output([str(args.runtime / "bin/erl"), "-noshell", "-eval", 'io:format("~s",[erlang:system_info(otp_release)]),halt().'], env=env, text=True)
    if release != "29":
        raise RuntimeError(f"expected OTP29, got {release!r}")
    commands = args.out / "commands.jsonl"
    results = args.out / "results.jsonl"
    commands.write_text("".join(json.dumps(case, separators=(",", ":")) + "\n" for case in cases()))
    requested_binary = args.binary.resolve()
    binary = (args.out / "type_probe_otp29.snapshot").resolve()
    payload = requested_binary.read_bytes()
    temporary = binary.with_suffix(".tmp")
    temporary.write_bytes(payload)
    temporary.chmod(requested_binary.stat().st_mode)
    temporary.replace(binary)
    before = hashlib.sha256(payload).hexdigest()
    sources = [ROOT / "src" / name for name in (
        "lattice.rs", "type_api.rs", "native_order.rs",
        "runtime_term.rs", "presentation.rs", "bin/type_probe.rs")]
    sources.extend(sorted((ROOT / "src/lattice").glob("*.rs")))
    sources.extend(ROOT / "scripts" / name for name in
                   ("verify_otp29_records.py", "type_codec.exs", "type_oracle.exs"))
    source_hashes = {str(path.relative_to(ROOT)): hashlib.sha256(path.read_bytes()).hexdigest() for path in sources}
    with commands.open() as source, results.open("w") as output:
        subprocess.run([str(binary)], stdin=source, stdout=output, check=True, env=env, timeout=60)
    subprocess.run([str(args.elixir), str(ROOT / "scripts/type_oracle.exs"), str(commands), str(results)], check=True, env=env, timeout=120)
    report = json.loads(Path(str(results) + ".comparison.json").read_text())
    if report["runtime"]["otp"] != "29" or report["passed"] != len(cases()) or report["unsupported"]:
        raise RuntimeError("incomplete native-record differential gate")
    if hashlib.sha256(binary.read_bytes()).hexdigest() != before:
        raise RuntimeError("binary changed during verification")
    if source_hashes != {str(path.relative_to(ROOT)): hashlib.sha256(path.read_bytes()).hexdigest() for path in sources}:
        raise RuntimeError("algebra source changed during verification")
    report.update(binary_sha256=before, binary_snapshot=str(binary), requested_binary=str(requested_binary),
                  source_sha256=source_hashes, verify_mode=env.get("RUST_DIALYZER_VERIFY_FASTPATH", ""), complete_otp29_support=False)
    (args.out / "report.json").write_text(json.dumps(report, indent=2) + "\n")


if __name__ == "__main__":
    main()
