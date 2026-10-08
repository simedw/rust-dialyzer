#!/usr/bin/env python3
import hashlib
import json, os, re, subprocess, sys
from pathlib import Path

root = Path(__file__).resolve().parents[1]
here = root / ".cache/flow"
here.mkdir(parents=True, exist_ok=True)
subprocess.run(
    ["cargo", "build", "--release", "--offline", "--bin", "core_analyze"],
    cwd=root,
    check=True,
)
report = json.loads((root / ".cache/core/report.json").read_text())
selected = []
for c in report["cases"]:
    name = c["id"].split("/")[-1]
    if name not in {"atom_guard", "atom_widen", "compare1", "exhaust_case"}:
        continue
    raw = json.loads(
        subprocess.check_output(
            [str(root / "target/release/core_analyze"), c["core_json"]]
        )
    )
    if raw["status"] != "inferred":
        raise RuntimeError(raw)
    path = here / (name + ".rust.json")
    path.write_text(json.dumps(raw, indent=2))
    selected.append(
        {
            "name": name,
            "id": c["id"],
            "core_json": c["core_json"],
            "sha256": hashlib.sha256(Path(c["core_json"]).read_bytes()).hexdigest(),
            "rust_json": str(path),
            "beams_dir": str(Path(c["core_json"]).parent / "beams"),
            "oracle_json": str(Path(c["core_json"]).parent / "oracle.json"),
        }
    )
assert len(selected) == 4
fixture = root / "examples/structural/engine_structural.erl"
fixture_dir = here / "structural"
fixture_beams = fixture_dir / "beams"
fixture_beams.mkdir(parents=True, exist_ok=True)
subprocess.run(
    ["erlc", "+debug_info", "-o", str(fixture_beams), str(fixture)],
    cwd=root,
    check=True,
)
fixture_core = fixture_dir / "core.json"
subprocess.run(
    ["elixir", "scripts/export_core.exs", str(fixture_beams), str(fixture_core)],
    cwd=root,
    check=True,
)
fixture_rust = fixture_dir / "rust.json"
fixture_rust.write_text(
    subprocess.check_output(
        [str(root / "target/release/core_analyze"), str(fixture_core)], text=True
    )
)
fixture_oracle = fixture_dir / "oracle.json"
fixture_oracle.write_text(
    json.dumps(
        {
            "warnings": [
                {
                    "module": "engine_structural",
                    "function": "bad_dynamic",
                    "arity": 0,
                    "line": 16,
                }
            ]
            * 2
        }
    )
)
selected.append(
    {
        "name": "structural",
        "id": "structural/engine_structural",
        "core_json": str(fixture_core),
        "sha256": hashlib.sha256(fixture_core.read_bytes()).hexdigest(),
        "rust_json": str(fixture_rust),
        "beams_dir": str(fixture_beams),
        "oracle_json": str(fixture_oracle),
    }
)
manifest = here / "verification-manifest.json"
manifest.write_text(json.dumps(selected))
env = os.environ.copy()
env.update(ASDF_ERLANG_VERSION="28.3", ASDF_ELIXIR_VERSION="1.19.4-otp-28")
subprocess.run(
    [
        "elixir",
        str(root / "scripts/verify_flow_native.exs"),
        str(manifest),
        str(here / "native-comparison.json"),
    ],
    cwd=root,
    env=env,
    check=True,
)
comparisons = json.loads((here / "native-comparison.json").read_text())
category = {
    "CallArguments": "call",
    "NoReturn": "no_return",
    "GuardImpossible": "guard_fail",
    "PatternImpossible": "pattern_match",
    "PatternCovered": "pattern_match_cov",
    "ApplyArguments": "fun_app_args",
    "ApplyNonFunction": "fun_app_no_fun",
    "RemoteTarget": "app_call",
    "MapUpdate": "map_construction",
    "BinaryConstruction": "bin_construction",
}
native_tag = {"warn_failing_call": "call", "warn_return_no_exit": "no_return"}
failures = []
for c in comparisons:
    rust = sorted(
        (w["line"], w["column"], category[w["cause"].split(" ")[0]])
        for w in c["rust_warnings"]
    )
    native = []
    for w in c["native_warnings"]:
        detail = re.match(r"\{:([a-z_]+),", w["detail"])
        native.append(
            (
                w["line"],
                w["column"],
                detail.group(1) if detail else native_tag.get(w["tag"], w["tag"]),
            )
        )
    if rust != sorted(native):
        failures.append({"id": c["id"], "warnings": {"rust": rust, "native": native}})
    for s in c["signatures"]:
        if s.get("args_match") is False or s.get("result_match") is False:
            failures.append({"id": c["id"], "signature": s})
    for w in c["native_warnings"]:
        if not w["typed_details_match"]:
            failures.append({"id": c["id"], "warning_detail": w})
    oracle = json.loads(
        Path(
            next(x["oracle_json"] for x in selected if x["name"] == c["id"])
        ).read_text()
    )
    actual_functions = sorted(
        (
            w["line"],
            re.search(
                r'module: "([^"]+)", name: "([^"]+)", arity: (\d+)', w["function"]
            ).groups(),
        )
        for w in c["rust_warnings"]
    )
    expected_functions = sorted(
        (w["line"], (w["module"], w["function"], str(w["arity"])))
        for w in oracle["warnings"]
    )
    if actual_functions != expected_functions:
        failures.append(
            {
                "id": c["id"],
                "function_identity": {
                    "rust": actual_functions,
                    "native": expected_functions,
                },
            }
        )
    input_case = next(x for x in selected if x["name"] == c["id"])
    c.update({key: input_case[key] for key in ("core_json", "sha256")})
    c["case_id"] = input_case["id"]
    c["warning_identity_match"] = (
        rust == sorted(native) and actual_functions == expected_functions
    )
    c["exported_signature_match"] = all(
        s.get("args_match", True) and s.get("result_match", True)
        for s in c["signatures"]
    )
result = {
    "implementation_sha256": {
        str(path.relative_to(root)): hashlib.sha256(path.read_bytes()).hexdigest()
        for path in [
            root / "src/flow.rs",
            root / "src/provider.rs",
            root / "src/dependencies.rs",
            root / "src/core.rs",
            root / "src/program.rs",
            root / "src/program_metadata.rs",
            root / "src/schedule.rs",
            root / "src/lattice.rs",
            *sorted((root / "src/bifs").glob("*.rs")),
            root / "src/bin/core_analyze.rs",
            root / "corpus/bifs.json",
            root / "scripts/type_codec.exs",
            root / "scripts/verify_flow_native.exs",
            root / "examples/structural/engine_structural.erl",
            Path(__file__).resolve(),
        ]
    },
    "cases": comparisons,
    "failures": failures,
    "analysis_context": "bare target PLT; library dependency composition has a separate whole-corpus gate",
    "claim": "complete warning category/line/column equality and exact persisted success-typing argument/return equality; raw native and Rust causes retained; typed call input/domain, guard operation/argument types and pattern subject types compared; warning message formatting not compared",
}
(here / "verification.json").write_text(json.dumps(result, indent=2))
print(json.dumps({"cases": len(comparisons), "failures": len(failures)}))
if failures:
    print(json.dumps(failures, indent=2))
    sys.exit(1)
