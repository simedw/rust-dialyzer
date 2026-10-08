# Historical split Rust phase timings for native Elixir

This report records the pre-merge split pipeline. The current gate and benchmark use [one executable](native/README.md); see [merged results](MERGED_RESULTS.md).

All three Rust executables were built with `cargo build --release`. Every recorded Rust scope reports `release`, derived from disabled debug assertions under these Cargo builds. Earlier performance measurements also used the explicit `target/release` helper/analyzer and release adapter paths.

Opt-in instrumentation writes JSON to stderr with `RUST_DIALYZER_TIMINGS=1`; normal stdout stays unchanged. The full instrumented 21-group Elixir correctness gate passes, with all warnings, inferred types and source metadata unchanged. Every timed run verifies warnings.

## 23-module Dialyxir group

Five fresh-process rounds after one warmup, warm filesystem, unchanged dependency facts, no application incremental cache. Medians below come from the instrumented run.

| Whole pipeline | Median |
|---|---:|
| BEAM helper subprocesses | 97.0 ms |
| Firefly adapter subprocesses | 115.3 ms |
| Analyzer subprocess | 263.9 ms |
| Python orchestration/transport residual, approximate | 23.7 ms |
| Total native wall | 499.8 ms |
| Ordinary Dialyzer wall in same run | 620.8 ms |

## Rust work inside those subprocesses

| Stage | Median |
|---|---:|
| All helper Rust work | 20.8 ms |
| All adapter Rust work | 38.4 ms |
| Dependency import, inclusive | 99.9 ms |
| ↳ JSON/raw dependency artifact parsing | 92.7 ms |
| ↳ Native dependency-store construction | 6.6 ms |
| Inference and refinement | 63.2 ms |
| Diagnostics | 21.9 ms |
| Core pipeline setup | 5.9 ms |
| Definition/type/contract preparation | 18.8 ms |
| Core JSON reserialization and parsing | 9.3 ms |

Nested rows overlap with parent scopes and must not be added to them. Phase medians are computed independently; residual subtraction is approximate.

The frontend subprocesses take 212.3 ms while their instrumented Rust work takes 59.2 ms. The approximate 153.1 ms gap includes process startup, binary/runtime initialization and work outside the function scopes. The prototype starts a helper and adapter for each module, or 46 frontend processes for this group. The Python residual additionally includes parsing module JSON and composing the analyzer input.

Dependency parsing is a second major cost: it decodes and converts a 12,190,537-byte captured JSON dependency snapshot each invocation. Reading the snapshot itself takes only 1.6 ms. Inference takes 63.2 ms. Tiny likewise spends about 104 ms importing that same dependency context, despite having only four modules.

The next optimization candidates are to run translation/lowering in one process for the whole module set, remove JSON transport and avoid repeatedly decoding the dependency snapshot. These measurements give more reason to work there than to start by changing the inference algorithm.

## Instrumentation and measurement limits

Disabled timers take no clock readings and allocate no phase vectors. Two timer tests pass and recorded phase durations partition their scope totals. Timers exclude their own JSON formatting/emission from internal scope totals; subprocess wall includes it. Nested stderr output is included in a parent phase.

The disabled-timer follow-up was nearly identical for Tiny, but the larger group and OTP also changed substantially between batches. This sequential on/off comparison cannot reliably isolate timer overhead under the observed host variation. It is not evidence of a new speed regression or an instrumented speed improvement. Treat phase milliseconds as approximate, and use the raw per-round records to inspect variation.

## Reproduce

After rebuilding optimized binaries, rerun the full native gate with those exact paths. Then:

```sh
python3 prototypes/firefly_frontend/evidence/split-baseline-benchmark.py "$PWD" --seed-bin target/release/native-beam-seed --adapter-bin /path/to/release/adapt-beam --probe target/release/compiled_analysis_probe --case tiny --case dialyxir/examples --rounds 5 --phase-timings --output /private/tmp/elixir-phases.json
```

Remove `--phase-timings` for a run with timers disabled. The benchmark rejects missing timing records or debug-mode records when the flag is enabled. See [the harness guide](elixir_harness/README.md) for setup. Raw measurements: [enabled](evidence/elixir-phase-timings.json), [disabled](evidence/elixir-timers-disabled.json), [full correctness gate](evidence/elixir-instrumented-gate.json).
