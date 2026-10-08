# Single-executable Elixir results

The native Elixir pipeline now runs BEAM ingestion, metadata translation, Firefly Core lowering and analysis in one executable: `target/release/native-elixir-dialyzer`. It calls reusable Rust libraries instead of launching per-module helpers. Intermediate seed JSON, forms ETF and Core JSON files are removed from native execution. The adapter arena is validated in memory without textual JSON serialization.

## Correctness held constant

All 21 selected groups across 48 unique modules pass, matching all 33 warnings, inferred signatures, source forms, attributes and diagnostic locations. The gate runs one native process for all requests, excludes oracle Core bodies and expected warnings from requests, and traps VM commands. Benchmark runs likewise supply only BEAM paths, options and frozen runtime metadata. Seven translator tests and eleven adapter tests belong to the preceding implementation; this extraction additionally passes the shared Core JSON/value validation regression test and actual error-boundary checks.

## Release comparison

Five measured rounds after one warmup pair, alternating split and merged order. Every implementation runs in fresh processes; OTP is measured alongside each split/merged sample. Both paths use the same unchanged dependency facts, compiled targets and captured runtime profiles. No application incremental cache.

| Case | Split native | Merged native | Ordinary Dialyzer | Gain over split | Gain over OTP |
|---|---:|---:|---:|---:|---:|
| Tiny, 4 modules | 211.7 ms | 169.6 ms | 566.9 ms | 1.25× | 3.34× |
| Assertions, 1 module | 181.8 ms | 159.6 ms | 545.1 ms | 1.14× | 3.42× |
| Dialyxir, 23 modules | 467.2 ms | 262.5 ms | 591.0 ms | 1.78× | 2.25× |

The largest measured native run is 43.8% shorter after the merge. This is a wall-time comparison, including startup, ingestion, analysis and result projection. It does not establish that inference alone is faster than OTP.

The preserved split analyzer uses Rust 1.92 and its frontend uses Rust 1.88/bootstrap. The merged release executable uses Rust 1.88/bootstrap throughout, which the build and instrumented scopes verify. Split expected-warning comparison occurs inside its probe; merged comparisons occur externally. That small protocol difference is included in the results. Measurements remain sensitive to host load.

## Remaining time in the merged process

A separate five-round instrumented run reports these approximate medians:

| Stage | Time |
|---|---:|
| Ingestion, translation and Core adaptation | 31.6 ms |
| In-memory Core validation | 3.1 ms |
| Dependency snapshot import | 96.8 ms |
| Inference/refinement | 57.6 ms |
| Diagnostics | 22.0 ms |
| Full native wall in instrumented batch | 271.8 ms |

These phases are not a complete additive partition of wall time. Nested scopes overlap, and independently computed medians do not sum exactly. The full raw records include preparation, projection, cleanup and startup. Dependency parsing remains the largest measured individual Rust phase; no dependency caching change was smuggled into the merge.

## Run and reproduce

Build instructions and the JSONL input/output protocol are in [native/README.md](native/README.md). The default installed executable is `target/release/native-elixir-dialyzer`:

```sh
python3 prototypes/firefly_frontend/elixir_harness/run_native.py "$PWD"
python3 prototypes/firefly_frontend/elixir_harness/benchmark.py "$PWD" --native-bin target/release/native-elixir-dialyzer --case tiny --case dialyxir/examples --rounds 5
```

Add `--phase-timings` for instrumentation. The gate enables optional native Core output for metadata comparison; ordinary timed runs leave it off. [compare_merged.py](elixir_harness/compare_merged.py) reproduces the split/merged comparison when the preserved split release binaries are available.

Raw evidence: [correctness gate](evidence/elixir-merged-gate.json), [alternating comparison](evidence/elixir-merged-comparison.json), [release phase timings](evidence/elixir-merged-phase-timings.json).

## Scope

The native gate and benchmark now have one executable path. Individual stage binaries remain diagnostic tools, not children of the merged executable. The original production compiler-bridge CLI has not been switched to the prototype. Runtime/dependency profiles still require explicit preparation, the extracted compiler needs Rust/bootstrap, and the previously documented unsupported Elixir groups remain unsupported. In-memory ETF and Value-to-typed-Core representation conversions remain. This merge removes process and file boundaries; it does not establish full Elixir or Dialyzer parity.
