# Native Elixir frontend correctness gate

This harness separates oracle preparation, native frontend execution, and analysis.
Oracle preparation invokes Elixir/OTP explicitly; native execution must use only
compiled BEAM bytes and captured dependency/runtime profiles. Oracle Core bodies,
warning lists, and source-form ETFs are comparison targets, never frontend inputs.

The corpus uses existing debug-enabled BEAM files: the four Tiny modules,
nineteen Elixir upstream Dialyzer test groups, and twenty-three Dialyxir example
modules. There are twenty-one grouped analyses, forty-nine module occurrences
(forty-eight unique modules), and thirty-three actual OTP warnings. Elixir's
upstream regression tests contribute zero-warning checks; Tiny and Dialyxir
provide positive warning checks. `capture-summary.json` identifies every beam
and records its SHA-256.

Run from the repository root:

```sh
elixir prototypes/firefly_frontend/elixir_harness/capture_inputs.exs "$PWD" "$PWD/prototypes/firefly_frontend/elixir_harness/generated/inputs"
elixir prototypes/firefly_frontend/elixir_harness/generate.exs "$PWD" "$PWD/prototypes/firefly_frontend/elixir_harness/generated/oracle"
python3 prototypes/firefly_frontend/elixir_harness/run_baseline.py "$PWD"
```

The scripts require existing `examples/tiny/_build/dev/lib/tiny/ebin` and
`.cache/corpus/elixir` fixtures and Elixir dependency PLT from the repository's
corpus preparation. They do not recompile targets or mutate the PLT. The source
BEAM hashes make this requirement explicit. The captured Core is roundtripped
and checked against OTP's own `dialyzer_utils.get_core_from_beam` output by
`scripts/core_export.exs`.

`generated/inputs/<Module>/` contains the raw `debug_info_v1` ETF, its metadata
ETF, oracle `erlang_v1` forms ETF, and text inspection views. `capture_md5.exs`
can produce a deterministic ETF fixture for the generated `__info__/1` exports
MD5. Tiny.Math's fixture digest is `91e9656d6ebb9533434dd44627f41050`.

`generated/oracle/` contains grouped oracle Core and warning rows, dependency
facts, and installed OTP closure profiles. These profiles define the frozen
analysis context; they do not implement Elixir translation. Large generated
artifacts are ignored by Git. `run_baseline.py --case tiny` can select a group.
Baseline results verify the existing analyzer on OTP Core and do not demonstrate
that the native frontend works.

Run the native gate after building the single merged executable:

```sh
python3 prototypes/firefly_frontend/elixir_harness/run_native.py "$PWD" --native-bin target/release/native-elixir-dialyzer
```

The default executable is `target/release/native-elixir-dialyzer`. The gate invokes
it once for all selected groups and traps VM commands on PATH. Requests contain
compiled BEAM paths, analysis options and whitelisted runtime context. Oracle
Core bodies and expected warnings are excluded. Actual source forms, attributes,
warnings and inferred types are compared externally. `--case tiny` selects one
group. The executable's optional Core output is enabled only for this check.

## End-to-end timing after correctness

`benchmark.py` requires the selected cases to have passed warning/type checks and
verifies that the executable hash is unchanged. Each native sample launches one
fresh process for the entire module set. No intermediate frontend files are
written; Core output is disabled. Every native run must match both expected
warnings and inferred signatures outside the timed native subprocess.

```sh
python3 prototypes/firefly_frontend/elixir_harness/benchmark.py "$PWD" --native-bin target/release/native-elixir-dialyzer --case tiny --case dialyxir/examples --rounds 5
```

Both implementations use the same unchanged dependency PLT facts. The script
validates the source PLT path and SHA-256 recorded in the captured native prior
snapshot, alternates execution order, performs warmups and reports medians
including startup. It does not use an incremental application cache.

OTP analysis invokes `dialyzer.run` through a small Elixir runner, without Core
export or recompiling targets. Its internal analysis time is reported alongside
full subprocess time. OTP wall time includes runner startup and warning
serialization. Native wall time includes BEAM ingestion, lowering, analysis and
result projection. Native dependency facts are captured JSON; OTP uses a binary
PLT. Runtime profiles remain captured preparation context.

## Rust phase instrumentation

Add `--phase-timings` to enable opt-in Rust phase records and require every
instrumented scope to report release mode. Timings are retained per sample and
summarized by scope across all modules. The underlying switch is
`RUST_DIALYZER_TIMINGS=1`; Rust writes `RUST_DIALYZER_TIMING {JSON}` lines to stderr
and leaves normal stdout unchanged. Disabled timers take no clock readings or
phase-vector allocations.

```sh
python3 prototypes/firefly_frontend/elixir_harness/benchmark.py "$PWD" --native-bin target/release/native-elixir-dialyzer --case dialyxir/examples --rounds 5 --phase-timings --output /private/tmp/elixir-phases.json
```

Scopes cover ingestion/translation, Firefly passes, in-memory Core validation,
dependency import, inference and diagnostics. Child scopes are included in their
parent, so do not add their totals together. `teardown_or_unfinished` records
remaining cleanup or work before an error. Scope totals exclude their own timer
JSON emission; subprocess wall time includes emission and process startup.
Independent phase medians need not sum exactly to median wall time.

## Comparison with the split baseline

`compare_merged.py` alternates preserved split release binaries and the merged
executable, with ordinary OTP measured alongside each. It uses a recorded split
benchmark under `evidence/` only for this historical comparison. The ordinary
native gate and benchmark have no split runtime path.

```sh
python3 prototypes/firefly_frontend/elixir_harness/compare_merged.py "$PWD" --native-bin target/release/native-elixir-dialyzer --seed-bin /path/to/preserved/native-beam-seed --adapter-bin /path/to/preserved/adapt-beam --probe /path/to/preserved/compiled_analysis_probe --rounds 5 --output /private/tmp/elixir-merge-comparison.json
```

The preserved split binary hashes must match `evidence/elixir-instrumented-gate.json`.
Rebuilt diagnostic tools are not automatically treated as that historical baseline.
