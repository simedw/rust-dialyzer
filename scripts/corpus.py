#!/usr/bin/env python3
import argparse
from collections import Counter, defaultdict
import hashlib
import json
from pathlib import Path
import re
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
LOCK = ROOT / "corpus/sources.json"
INVENTORY = ROOT / "corpus/inventory.json"
SOURCES = ROOT / ".cache/upstream"
OUTPUT = ROOT / ".cache/corpus"
WARNING_KEY = ("module", "function", "arity", "line", "category", "tag")
RUST_WARNING_TAGS = {"call": "warn_failing_call", "no_return": "warn_return_no_exit", "invalid_contract": "warn_contract_types"}


def execute(args, **kwargs):
    return subprocess.run([str(a) for a in args], cwd=ROOT, check=True, **kwargs)


def capture(args):
    return execute(args, capture_output=True, text=True).stdout.strip()


def save(path, data):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(data, indent=2, sort_keys=True) + "\n")


def fetch():
    for name, source in json.loads(LOCK.read_text()).items():
        destination = SOURCES / name
        if not destination.exists():
            execute(["git", "clone", "--depth", "1", "--branch", source["tag"], "--no-checkout", "--filter=blob:none", "--sparse", source["url"], destination])
        execute(["git", "-C", destination, "fetch", "--depth", "1", "origin", source["revision"]])
        execute(["git", "-C", destination, "sparse-checkout", "set", *source["sparse"]])
        execute(["git", "-C", destination, "checkout", "--detach", source["revision"]])
    validate_sources()


def validate_sources():
    for name, source in json.loads(LOCK.read_text()).items():
        directory = SOURCES / name
        actual = capture(["git", "-C", directory, "rev-parse", "HEAD"])
        if actual != source["revision"]:
            raise RuntimeError(f"{name}: expected {source['revision']}, got {actual}; run corpus.py fetch")
        changed = capture(["git", "-C", directory, "diff", "--name-only", "HEAD"])
        if changed:
            raise RuntimeError(f"{name}: upstream tracked files modified: {changed}")


def relative(path):
    return str(path.relative_to(SOURCES))


def source_case(case_id, kind, paths, **fields):
    return {"id": case_id, "kind": kind, "files": [relative(p) for p in paths], "source_sha256": {relative(p): hashlib.sha256(p.read_bytes()).hexdigest() for p in paths}, **fields}


def inventory():
    validate_sources()
    cases = []
    suites = []
    tests = SOURCES / "otp/lib/dialyzer/test"
    for data in sorted(tests.glob("*_SUITE_data")):
        name = data.name.removesuffix("_SUITE_data")
        src = data / "src"
        if not src.is_dir() or name in ("dialyzer_cl", "plt"):
            continue
        registered = []
        for item in sorted(src.iterdir()):
            if not (item.is_dir() or item.suffix == ".erl"):
                continue
            paths = sorted(item.glob("*.erl")) if item.is_dir() else [item]
            name_case = item.stem if item.is_file() else item.name
            golden = data / "results" / name_case
            case_id = f"otp/{name}/{name_case}"
            registered.append(case_id)
            cases.append(source_case(case_id, "otp", paths, suite=name, case=name_case, suite_dir=relative(data), grouped=item.is_dir(), expected_file=relative(golden) if golden.exists() else None, expected_clean=not golden.exists()))
        suites.append({"suite": name, "cases": registered, "options_file": relative(data / "dialyzer_options"), "options_term": (data / "dialyzer_options").read_text().strip()})
    ct_path = ROOT / ".cache/ct-inventory.json"
    execute(["elixir", ROOT / "scripts/ct_inventory.exs", SOURCES / "otp", ct_path])
    handwritten = json.loads(ct_path.read_text())
    for suite in handwritten:
        suite["file"] = relative(Path(suite["file"]))
        suite["status"] = "not_ported"
    elixir_dir = SOURCES / "elixir/lib/elixir/test/elixir"
    suite_file = elixir_dir / "kernel/dialyzer_test.exs"
    source = suite_file.read_text()
    fixtures = elixir_dir / "fixtures/dialyzer"
    fixture_paths = sorted(fixtures.glob("*.ex"))
    module_files = {}
    for path in fixture_paths:
        for name in re.findall(r"^defmodule\s+(\w[\w.]*)", path.read_text(), re.M):
            module_files["Elixir." + name] = path
    previous_end = 0
    pattern = r'test "([^"]+)", context do\s*(.*?)assert_dialyze_no_warnings!\(context\)'
    for number, match in enumerate(re.finditer(pattern, source, re.S), start=1):
        name, body = match.groups()
        modules = ["Elixir." + m for m in re.findall(r"copy_beam!\(context, ([\w.]+)\)", body)]
        prefix = source[previous_end:match.start()]
        warning_tag = re.search(r"@tag warnings: \[([^]]*)\]", prefix)
        warnings = re.findall(r":(\w+)", warning_tag.group(1)) if warning_tag else []
        paths = sorted(set(module_files[m] for m in modules))
        slug = re.sub(r"[^a-z0-9]+", "_", name.lower()).strip("_")
        cases.append(source_case(f"elixir/{number:02d}_{slug}", "elixir", paths, name=name, modules=modules, warning_options=warnings, expected="clean", suite_file=relative(suite_file)))
        previous_end = match.end()
    examples = sorted((SOURCES / "dialyxir/test/examples").glob("*.ex"))
    cases.append(source_case("dialyxir/examples", "dialyxir", examples, expected="live_reference", warning_options=["unknown", "unmatched_returns", "error_handling", "underspecs"]))
    result = {"version": 1, "sources": json.loads(LOCK.read_text()), "semantic_cases": cases, "otp_generated_suites": suites, "otp_handwritten_suites": handwritten, "otp_consistency_checks": [{"suite": s["suite"], "status": "not_ported"} for s in suites], "counts": {"semantic_cases": len(cases), "by_source": dict(Counter(c["kind"] for c in cases)), "otp_semantic_suites": len(suites), "otp_single_file_cases": sum(c["kind"] == "otp" and not c["grouped"] for c in cases), "otp_grouped_cases": sum(c["kind"] == "otp" and c["grouped"] for c in cases), "otp_recursive_erl_files": sum(len(list((data / "src").rglob("*.erl"))) for data in tests.glob("*_SUITE_data") if data.name.removesuffix("_SUITE_data") in {s["suite"] for s in suites}), "otp_handwritten_cases": sum(len(s["cases"]) for s in handwritten), "otp_consistency_checks": len(suites), "elixir_fixture_files": len(fixture_paths), "elixir_fixture_modules": len(module_files), "dialyxir_example_files": len(examples)}}
    save(INVENTORY, result)
    print(json.dumps(result["counts"], indent=2))
    return result


def normalized(warnings):
    def identity(warning):
        return tuple(warning.get(key, RUST_WARNING_TAGS.get(warning.get("category")) if key == "tag" else None) for key in WARNING_KEY)
    return sorted((identity(w) for w in warnings), key=repr)


def runtime_versions():
    expression = ':application.load(:dialyzer); {:ok, vsn} = :application.get_key(:dialyzer, :vsn); IO.puts(:json.encode(%{otp: System.otp_release(), erts: List.to_string(:erlang.system_info(:version)), elixir: System.version(), dialyzer: List.to_string(vsn)}))'
    return json.loads(capture(["elixir", "-e", expression]))


def unsupported_options(case, oracle):
    if case["kind"] != "otp":
        return case.get("warning_options", [])
    terms = oracle.get("options_term") or ""
    warnings = re.search(r"warnings:\s*\[([^]]*)\]", terms)
    profile = re.findall(r":(\w+)", warnings.group(1)) if warnings else []
    unsupported = [w for w in profile if w != "no_unknown"]
    if re.search(r"use_spec:\s*false", terms):
        unsupported.append("use_spec=false")
    return unsupported


def classify(case, oracle):
    result = {"id": case["id"], "kind": case["kind"], "oracle_status": oracle.get("oracle_status"), "compile_status": oracle.get("compile_status"), "oracle_warning_count": len(oracle.get("warnings", [])), "oracle_categories": dict(Counter(w["category"] for w in oracle.get("warnings", []))), "oracle_file": oracle["_path"]}
    if oracle.get("oracle_status") not in ("golden_match", "live_reference"):
        result.update(status="oracle_failure", reason="upstream golden output differs" if oracle.get("oracle_status") == "golden_diff" else oracle.get("oracle_status"))
        return result
    if oracle.get("compile_status") != "ok" or not oracle.get("program_json"):
        result.update(status="compile_failure", reason=oracle.get("compile_detail") or oracle.get("error"))
        return result
    if oracle.get("beam_oracle_status", "ok") != "ok":
        result.update(status="oracle_failure", reason=oracle.get("beam_oracle_error"))
        return result
    if oracle.get("source_beam_equal") is False:
        result.update(status="frontend_mismatch", reason="OTP source and BEAM warning identities differ")
        return result
    command = [ROOT / "target/release/rust-dialyzer-poc", oracle["program_json"]]
    try:
        rust = subprocess.run([str(a) for a in command], cwd=ROOT, capture_output=True, text=True, timeout=20)
    except subprocess.TimeoutExpired:
        result.update(status="rust_timeout", reason="Rust exceeded 20 seconds")
        return result
    if rust.returncode:
        text = rust.stderr.strip()
        reason = text.split(":", 1)[0]
        status = "unsupported" if ("unsupported" in text or "no runtime functions" in text or "only literal binaries supported" in text) else "rust_failure"
        result.update(status=status, reason=reason, rust_error=text[:2000])
        return result
    output = json.loads(rust.stdout)
    output_path = Path(oracle["program_json"]).parent / "rust.json"
    save(output_path, output)
    result["rust_file"] = str(output_path)
    result["observed_warning_parity"] = normalized(oracle["warnings"]) == normalized(output["warnings"])
    options = unsupported_options(case, oracle)
    if options:
        result.update(status="unsupported_options", reason=", ".join(options))
    elif any(w.get("module") is None for w in oracle["warnings"]):
        result.update(status="unresolved_warning_identity", reason="oracle warnings cannot be attributed to runtime functions")
    elif result["observed_warning_parity"]:
        result.update(status="parity")
    else:
        result.update(status="mismatch", expected=[list(x) for x in normalized(oracle["warnings"])], actual=[list(x) for x in normalized(output["warnings"])])
    return result


def assess(args):
    validate_sources()
    inv = inventory()
    selected = [c for c in inv["semantic_cases"] if (not args.kind or c["kind"] in args.kind) and (not args.suite or c.get("suite") in args.suite) and (not args.case or c["id"] in args.case)]
    if args.limit:
        selected = selected[:args.limit]
    if not selected:
        raise RuntimeError("no cases selected")
    execute(["cargo", "build", "--release", "--offline"])
    source_state = OUTPUT / "source-revisions.json"
    source_revisions = {"sources": json.loads(LOCK.read_text()), "runtime": runtime_versions()}
    if args.reuse_oracles:
        if not source_state.exists() or json.loads(source_state.read_text()) != source_revisions:
            raise RuntimeError("cached oracle source revisions or runtime differ; run fresh oracle assessment")
    else:
        save(source_state, source_revisions)
    for kind in (() if args.reuse_oracles else ("otp", "elixir")):
        subset = [c for c in selected if c["kind"] == "otp"] if kind == "otp" else [c for c in selected if c["kind"] != "otp"]
        if not subset:
            continue
        resolved = []
        for original in subset:
            case = dict(original)
            case["files"] = [str(SOURCES / p) for p in case["files"]]
            if "suite_dir" in case:
                case["suite_dir"] = str(SOURCES / case["suite_dir"])
            resolved.append(case)
        manifest = OUTPUT / f"{kind}-selected.json"
        save(manifest, {"cases": resolved})
        if kind == "otp":
            command = ["elixir", ROOT / "scripts/corpus_oracle.exs", SOURCES / "otp", manifest, OUTPUT / "otp", args.timeout]
        else:
            command = ["elixir", ROOT / "scripts/corpus_elixir.exs", SOURCES / "elixir", SOURCES / "dialyxir", manifest, OUTPUT / "elixir"]
        execute(command)
    oracles = {}
    for path in OUTPUT.glob("*/*/oracle.json"):
        record = json.loads(path.read_text())
        record["_path"] = str(path)
        oracles[record["id"]] = record
    selected_ids = {c["id"] for c in selected}
    results = []
    for case in inv["semantic_cases"]:
        if case["id"] not in selected_ids:
            results.append({"id": case["id"], "kind": case["kind"], "status": "not_run"})
        elif case["id"] not in oracles:
            results.append({"id": case["id"], "kind": case["kind"], "status": "harness_failure", "reason": "missing oracle record"})
        else:
            results.append(classify(case, oracles[case["id"]]))
    summary = dict(Counter(r["status"] for r in results))
    reasons = Counter(r.get("reason", r["status"]) for r in results if r["status"] not in ("parity", "not_run"))
    profiles = defaultdict(Counter)
    for result in results:
        profiles[result["kind"]][result["status"]] += 1
    report = {"version": 1, "oracles_reused": args.reuse_oracles, "runtime": source_revisions["runtime"], "assessment_complete": True, "parity_complete": all(r["status"] == "parity" for r in results), "sources": inv["sources"], "registered_semantic_cases": len(results), "selected_cases": len(selected), "summary": summary, "by_source": {k: dict(v) for k, v in profiles.items()}, "top_gaps": reasons.most_common(30), "warning_categories_observed": dict(sum((Counter(r.get("oracle_categories", {})) for r in results), Counter())), "operational_ct_cases_not_ported": inv["counts"]["otp_handwritten_cases"], "consistency_checks_not_ported": inv["counts"]["otp_consistency_checks"], "cases": results}
    save(OUTPUT / "report.json", report)
    print("ASSESSMENT COMPLETE", json.dumps(summary, sort_keys=True))
    print(f"Report: {OUTPUT / 'report.json'}")
    if args.require_parity and any(r["status"] != "parity" for r in results if r["id"] in selected_ids):
        return 1
    return 0


def main():
    parser = argparse.ArgumentParser(description="Discover and assess pinned upstream Dialyzer corpora. Unsupported cases never count as parity.")
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("fetch")
    commands.add_parser("inventory")
    run_parser = commands.add_parser("run")
    run_parser.add_argument("--kind", action="append", choices=["otp", "elixir", "dialyxir"])
    run_parser.add_argument("--suite", action="append")
    run_parser.add_argument("--case", action="append")
    run_parser.add_argument("--limit", type=int)
    run_parser.add_argument("--timeout", type=int, default=60)
    run_parser.add_argument("--require-parity", action="store_true")
    run_parser.add_argument("--reuse-oracles", action="store_true", help="compare Rust against saved oracle records with matching pinned source revisions")
    args = parser.parse_args()
    if args.command == "fetch":
        fetch()
    elif args.command == "inventory":
        inventory()
    else:
        return assess(args)
    return 0


if __name__ == "__main__":
    sys.exit(main())
