# Pre-merge native Elixir correctness and performance

The current single-executable results are in [MERGED_RESULTS.md](MERGED_RESULTS.md). The figures below describe the earlier split pipeline.

The native BEAM → Elixir metadata → Erlang abstract forms → Firefly Core → Rust analyzer path passes all 21 selected grouped analyses across 48 unique modules (49 occurrences). It matches 33 exact warning terms, inferred function signatures, source forms and attributes, without launching Erlang or Elixir during native execution. Original Elixir source files and oracle Core bodies are not frontend inputs.

## Optimized measurements

Five timed rounds after one warmup, alternating execution order. Fresh processes, warm filesystem, unchanged dependency facts and no application incremental cache. Every timed run checks exact warnings.

| Case | First native | Final native | Ordinary Dialyzer | Final wall speedup |
|---|---:|---:|---:|---:|
| tiny (4 modules) | 283.5 ms | 202.7 ms | 514.2 ms | 2.54× |
| elixir/17_no_warning_on_exunit_assertions (1 modules) | 252.0 ms | 173.9 ms | 515.3 ms | 2.96× |
| dialyxir/examples (23 modules) | 1146.6 ms | 407.9 ms | 556.9 ms | 1.37× |

The 23-module native run decreased from 1,146.6 to 407.9 ms (64.4%). Its context-plus-analysis phase decreased from 912.2 to 238.3 ms. These full subprocess figures include startup: OTP internal analysis for that case is 118.8 ms, while the native phase includes JSON parsing and dependency-context loading. The wall-time comparison does not establish that Rust’s inference algorithm alone is faster.

## What changed

1. Folded dataflow tables build once per analysis instead of once per module refinement/diagnostic pass.
2. Guard literal/order tables likewise build once. Both caches derive only from immutable Core and labels; mutable patterns, inference and warnings remain fresh.
3. Newly emitted adapter terms share IDs. Original seed IDs and source references remain unchanged. For Dialyxir, terms dropped from 49,753 to 11,166 and compact Core JSON from 1,977,457 to 581,707 bytes.

Each change has its own full gate and timing checkpoint under `evidence/`. The profile rationale is in [elixir-profile-summary.md](evidence/elixir-profile-summary.md). Seven translator tests, eleven adapter tests and four focused compiler tests pass. Replaying the pinned compiler extraction plus its recorded patch reproduces all 196 closure files exactly.

## Limits and next work

This is selected-corpus parity, not complete Elixir or Dialyzer parity. [The translator README](elixir/README.md) lists rejected language groups. Native runs still need captured dependency facts and runtime profiles. Production commands still use the compiler bridge; the native prototype has explicit binaries and a harness. Incremental application analysis is not enabled in these measurements. The extracted compiler still requires the legacy Rust/bootstrap setup.

The next performance work should separate dependency loading from inference, then remove per-module subprocesses and JSON transport. Broader Elixir support needs further grouped correctness gates before making general compatibility claims.

## Repeat the measurement

For the current one-executable path, follow [native/README.md](native/README.md) and [elixir_harness/README.md](elixir_harness/README.md). The preserved historical split benchmark is `evidence/split-baseline-benchmark.py`; its executable hashes must match the recorded split gate.
