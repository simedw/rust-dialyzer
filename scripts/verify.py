#!/usr/bin/env python3
import hashlib
import json
from pathlib import Path
import shutil
import statistics
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
BINARY = ROOT / "target/release/rust-dialyzer-poc"
CACHE = ROOT / ".cache"


def run(args, success=True):
    start = time.perf_counter_ns()
    result = subprocess.run([str(a) for a in args], cwd=ROOT, capture_output=True, text=True)
    elapsed = (time.perf_counter_ns() - start) / 1_000_000
    if success and result.returncode:
        raise RuntimeError(f"{' '.join(map(str, args))}\n{result.stdout}\n{result.stderr}")
    return result, elapsed


def compile_export(source, dest):
    beams = dest / "beams"
    beams.mkdir(parents=True, exist_ok=True)
    run(["elixirc", "-o", beams, *sorted(source.glob("*.ex"))])
    ir = dest / "program.json"
    run(["elixir", ROOT / "scripts/export.exs", beams, ir])
    return beams, ir


def oracle(beams, dest, plts):
    output = dest / "oracle.json"
    _, elapsed = run(["elixir", ROOT / "scripts/oracle.exs", beams, output, plts])
    return json.loads(output.read_text()), elapsed


def rust(ir, *options):
    result, elapsed = run([BINARY, ir, *options])
    return json.loads(result.stdout), elapsed


def parity(reference, actual, label):
    assert reference["warnings"] == actual["warnings"], f"{label}:\nOTP {reference['warnings']}\nRust {actual['warnings']}"


def fingerprint(path):
    path = Path(path)
    return hashlib.sha256(path.read_bytes()).hexdigest(), path.stat().st_mtime_ns


def check_solvers(ir, reference, label):
    sweep, _ = rust(ir, "--solver", "sweep")
    worklist, _ = rust(ir, "--solver", "worklist")
    parity(reference, sweep, label)
    parity(reference, worklist, label)
    assert sweep["signatures"] == worklist["signatures"], f"{label}: solver signatures differ"
    return sweep, worklist


def benchmark(ir, repeats=1000, samples=7):
    results = {}
    for solver in ["sweep", "worklist"]:
        runs = [rust(ir, "--solver", solver, "--repeat", str(repeats))[0] for _ in range(samples)]
        results[solver] = {"median_inference_us": statistics.median(r["inference_us"] / repeats for r in runs), "evaluations": runs[0]["evaluations"], "samples": samples, "repeats_per_sample": repeats}
    results["speedup"] = results["sweep"]["median_inference_us"] / results["worklist"]["median_inference_us"]
    return results


def main():
    run(["cargo", "build", "--release", "--offline"])
    CACHE.mkdir(exist_ok=True)
    report = {}
    with tempfile.TemporaryDirectory(prefix="verify-", dir=CACHE) as temporary:
        scratch = Path(temporary)
        plts = scratch / "plts"
        beams, ir = compile_export(ROOT / "examples/tiny/lib", scratch / "broken")
        cold, cold_ms = oracle(beams, scratch / "broken", plts)
        assert cold["cache"]["base_status"] == cold["cache"]["app_status"] == "built"
        before = {p: fingerprint(cold["cache"][p]) for p in ["base_plt", "app_plt"]}
        warm, warm_ms = oracle(beams, scratch / "broken", plts)
        assert warm["cache"]["base_status"] == warm["cache"]["app_status"] == "reused"
        assert before == {p: fingerprint(warm["cache"][p]) for p in before}, "OTP cache rewritten"
        sweep, worklist = check_solvers(ir, warm, "broken")
        assert len(warm["warnings"]) == 6
        assert len(worklist["signatures"]) == 22
        rust_cache = scratch / "rust-cache.json"
        first, _ = rust(ir, "--cache", rust_cache)
        cache_before = fingerprint(rust_cache)
        second, _ = rust(ir, "--cache", rust_cache)
        assert not first["cache_hit"] and second["cache_hit"]
        assert cache_before == fingerprint(rust_cache), "Rust cache rewritten on hit"
        parity(warm, second, "Rust cache hit")
        stale_cache = json.loads(rust_cache.read_text())
        stale_cache["version"] = "stale-engine"
        rust_cache.write_text(json.dumps(stale_cache))
        refreshed, _ = rust(ir, "--cache", rust_cache)
        assert not refreshed["cache_hit"]
        parity(warm, refreshed, "engine version invalidation")
        rust_cache.write_text("broken cache")
        recovered, _ = rust(ir, "--cache", rust_cache)
        assert not recovered["cache_hit"]
        parity(warm, recovered, "corrupt cache recovery")
        report["fixture"] = {"functions": len(worklist["signatures"]), "warnings": warm["warnings"], "otp_cold_wall_ms": cold_ms, "otp_warm_wall_ms": warm_ms, "otp_cold_timings_us": cold["timings_us"], "otp_warm_timings_us": warm["timings_us"], "rust_sweep_evaluations": sweep["evaluations"], "rust_worklist_evaluations": worklist["evaluations"]}

        repaired_sources = scratch / "repaired-src"
        shutil.copytree(ROOT / "examples/tiny/lib", repaired_sources)
        fixes = {"issues.ex": [("byte_size(42)", 'byte_size("ok")'), ("increment(:wrong)", "increment(1)")], "math.ex": [("wrong_return(integer()) :: binary()", "wrong_return(integer()) :: integer()") ]}
        for name, replacements in fixes.items():
            path = repaired_sources / name
            text = path.read_text()
            for before_text, after_text in replacements:
                assert before_text in text
                text = text.replace(before_text, after_text)
            path.write_text(text)
        repaired_beams, repaired_ir = compile_export(repaired_sources, scratch / "repaired")
        repaired, _ = oracle(repaired_beams, scratch / "repaired", plts)
        assert repaired["warnings"] == []
        assert repaired["cache"]["base_status"] == "reused" and repaired["cache"]["app_status"] == "built"
        check_solvers(repaired_ir, repaired, "repaired")
        invalidated, _ = rust(repaired_ir, "--cache", rust_cache)
        assert not invalidated["cache_hit"] and invalidated["warnings"] == []
        again, _ = rust(repaired_ir, "--cache", rust_cache)
        assert again["cache_hit"] and again["warnings"] == []

        changed = repaired_sources / "math.ex"
        changed.write_text(changed.read_text().replace("def increment(value), do: value + 1", "def increment(value), do: byte_size(value)"))
        changed_beams, changed_ir = compile_export(repaired_sources, scratch / "repaired")
        changed_reference, _ = oracle(changed_beams, scratch / "changed", plts)
        check_solvers(changed_ir, changed_reference, "changed unspecced callee")
        changed_result, _ = rust(changed_ir, "--cache", rust_cache)
        assert not changed_result["cache_hit"] and len(changed_result["warnings"]) == 3

        unsupported = json.loads(ir.read_text())
        for module in unsupported["modules"]:
            for form in module["forms"]:
                if form[0] == "function" and form[2] == "greet":
                    form[4][0][4] = [["made_up_expression", 6]]
        unsupported_ir = scratch / "unsupported.json"
        unsupported_ir.write_text(json.dumps(unsupported))
        result, _ = run([BINARY, unsupported_ir, "--cache", rust_cache], success=False)
        assert result.returncode != 0 and "unsupported expression" in result.stderr
        corrupt_ir = scratch / "corrupt.json"
        corrupt_ir.write_text('{"version":1,"modules":[{"name":"bad","forms":[["function"]]}]}')
        result, _ = run([BINARY, corrupt_ir], success=False)
        assert result.returncode != 0 and "panicked" not in result.stderr

        report["tiny_solver_benchmark"] = benchmark(ir)
        source = scratch / "chain-src"
        source.mkdir()
        lines = ["defmodule Tiny.Chain do", "  def bad, do: f0000(:wrong)"]
        for i in range(200):
            body = f"f{i+1:04d}(value)" if i < 199 else "value + 1"
            lines.append(f"  def f{i:04d}(value), do: {body}")
        lines.append("end")
        (source / "chain.ex").write_text("\n".join(lines) + "\n")
        chain_beams, chain_ir = compile_export(source, scratch / "chain")
        chain_reference, _ = oracle(chain_beams, scratch / "chain", plts)
        assert len(chain_reference["warnings"]) == 2
        check_solvers(chain_ir, chain_reference, "200-function chain")
        report["chain_solver_benchmark"] = benchmark(chain_ir, repeats=25)
        startup = [rust(ir)[1] for _ in range(7)]
        cache_file = scratch / "bench-cache.json"
        rust(ir, "--cache", cache_file)
        cached_startup = [rust(ir, "--cache", cache_file)[1] for _ in range(7)]
        report["rust_wall_ms"] = {"uncached_median": statistics.median(startup), "cached_median": statistics.median(cached_startup), "samples": 7}
        example = CACHE / "example"
        example.mkdir(exist_ok=True)
        persistent_beams = example / "beams"
        if persistent_beams.exists():
            shutil.rmtree(persistent_beams)
        shutil.copytree(beams, persistent_beams)
        shutil.copy2(ir, example / "program.json")
        persistent_plts = CACHE / "plts"
        persistent_plts.mkdir(exist_ok=True)
        for name in ["base_plt"]:
            path = Path(warm["cache"][name])
            shutil.copy2(path, persistent_plts / path.name)
        persistent_oracle, _ = oracle(persistent_beams, example, persistent_plts)
        persistent_rust, _ = rust(example / "program.json", "--cache", example / "rust-cache.json")
        parity(persistent_oracle, persistent_rust, "persistent example")
        (example / "rust.json").write_text(json.dumps(persistent_rust, indent=2) + "\n")
        report["artifacts"] = {"program": str(example / "program.json"), "otp": str(example / "oracle.json"), "rust": str(example / "rust.json"), "plts": str(persistent_plts)}
        report["checks"] = ["exact warning parity", "no safe-function false positives", "macro expansion", "self and mutual recursion", "PLT content and mtime reuse", "changed BEAM app PLT invalidation", "Rust cache hit and content invalidation", "engine version invalidation", "corrupt cache recovery", "repaired zero-warning parity", "changed unspecced callee propagation", "unsupported and malformed IR rejection", "sweep/worklist signature parity", "generated dependency-chain parity"]
    (CACHE / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))
    print(f"PASS: report saved to {CACHE / 'report.json'}")


if __name__ == "__main__":
    main()
