# Rust Dialyzer proof of concept

This experiment checks a small compiled Elixir app with OTP Dialyzer and an independent Rust analyzer. It establishes diagnostic parity on the fixture before comparing two Rust solvers.

The current pipeline lets OTP compile each module's `debug_info` to Core Erlang, as Dialyzer itself does, and caches the result per BEAM content hash ([core_cache.exs](prototypes/core_cache/core_cache.exs)). The Rust analyzer loads that cache ([core_cache_analyze](src/bin/core_cache_analyze.rs)). On the 1,082 modules of a Phoenix project's PLT it matches OTP's warnings and all 42,246 inferred signatures exactly; it is still about 19 times slower. See [the Phase 0 notes](bench/phase0/README.md).

An earlier native frontend reimplemented the compiler step in Rust using code extracted from Firefly. It was removed because it could not handle current OTP and dependency code and was no faster than OTP's compiler. The tag `firefly-frontend-snapshot` preserves it.

The app in `examples/tiny` has 22 runtime functions. Eighteen produce no warnings. Four contain deliberate problems: an invalid return contract, a bad built-in call, a bad call to an unspecced function in another module, and a caller that inherits the failure. Both analyzers emit the same six normalized warnings.

The fixture also covers tuples, lists, binaries, atom unions, guards, self-recursion, mutual recursion, and a macro. Elixir expands the macro before Rust sees the program.

## Run the comparison

Use the toolchain versions in `.tool-versions`: Elixir 1.19.4, OTP 28.3, Rust 1.92.0, and Python 3.12. The Mix app has no dependencies. The Rust dependencies are pinned in `Cargo.lock`.

```sh
cargo test --offline
cargo clippy --offline --all-targets -- -D warnings
python3 scripts/verify.py
```

If the Rust dependencies are absent locally, run `cargo fetch` once and retry. The verification script compiles the app directly with `elixirc`, so it needs no Hex packages or running application.

The script fails on any warning mismatch. It checks cold PLT creation, unchanged PLT hashes and modification times, Rust cache reuse, changed-input invalidation, engine-version invalidation, corrupt-cache recovery, repaired-source parity, and changed-callee propagation. It compares inferred signatures between the sweep and worklist solvers. A separately compiled 200-function chain tests scaling and diagnostic parity.

Results remain in `.cache/report.json`. The compiled example, exported forms, normalized OTP and Rust output, and cached PLTs remain under `.cache/example` and `.cache/plts`.

## Run each analyzer

After verification, reuse the saved artifacts:

```sh
elixir scripts/oracle.exs .cache/example/beams .cache/example/oracle.json .cache/plts
target/release/rust-dialyzer-poc .cache/example/program.json --cache .cache/example/rust-cache.json
```

To analyze edits to the example, compile and export again:

```sh
elixirc -o .cache/example/beams examples/tiny/lib/*.ex
elixir scripts/export.exs .cache/example/beams .cache/example/program.json
elixir scripts/oracle.exs .cache/example/beams .cache/example/oracle.json .cache/plts
target/release/rust-dialyzer-poc .cache/example/program.json --cache .cache/example/rust-cache.json
```

The deliberate bad call also produces an Elixir compiler warning. Dialyzer independently analyzes the BEAM files.

## Measure the solvers

The optimized worklist solver is the default. The original full sweep remains selectable for comparison:

```sh
target/release/rust-dialyzer-poc .cache/example/program.json --solver sweep --repeat 1000
target/release/rust-dialyzer-poc .cache/example/program.json --solver worklist --repeat 1000
```

`inference_us` includes graph construction, inference, warning generation, and signature collection across all repetitions. It excludes JSON parsing and process startup. `evaluations` counts inference visits in one repetition and is zero on a result-cache hit. The verification script reports medians from seven samples.

The worklist reevaluates callers when a callee's inferred signature changes. It avoids repeated visits to unaffected functions. The initial measurements showed about 1.5 times faster analysis on the tiny app and about 40 times faster analysis on the deliberately unfavorable dependency chain. See `.cache/report.json` for the current measurements.

These compare two implementations of the same Rust subset. OTP wall times are recorded separately, including VM startup and PLT loading. The Rust result cache skips analysis entirely, while an OTP PLT caches type information. Neither comparison establishes the speed of a full Dialyzer rewrite.

## Implementation limits

Rust reads expanded Erlang abstract forms from BEAM debug information. The exporter filters reflection functions and macro definitions, retaining the runtime code produced by macro expansion. It never reads Dialyzer's warnings or inferred types.

The Rust analyzer infers argument constraints from supported operations and calls, propagates return types, and checks disjoint specs after inference. Its type representation covers broad numeric types, atoms, binaries, lists, tuples, and unions. It does not preserve numeric ranges, list emptiness, tuple correlations, or argument-to-return relationships. Specs do not constrain callers in this prototype. Some accepted programs can therefore contain errors this analyzer misses.

Maps, structs, closures, cases, exceptions, overloaded specs, user-defined types, and unrecognized external calls fail explicitly. Only a small set of Erlang built-ins has signatures. The oracle uses a minimal classic PLT containing `erlang.beam` plus the application modules. Unknown-reference warnings are suppressed for compiler-generated helpers. This is sufficient for the example, not a full OTP dependency analysis.

Cache invalidation compares the complete exported input and an engine-source fingerprint. Any program change invalidates the whole Rust result cache. Dependency-based invalidation across edits and cached function summaries are future work. The worklist currently optimizes a fresh analysis, not an incremental edit.

The next useful step is a larger differential corpus covering valid specs at call sites and polymorphic return relationships, followed by a broader compiler representation and OTP signature database. Keep the oracle checks as each feature grows.

See `docs/design.md` for the design choice and `docs/tasks.md` for the execution record.

## Upstream compatibility corpus

A pinned corpus now inventories 307 OTP semantic cases, 19 Elixir integration cases, and the combined Dialyxir warning examples. The complete assessment currently has two parity cases, 322 unsupported cases or profiles, two mismatches, and one upstream golden mismatch. The 128 operational and type cases remain separately tracked for porting.

```sh
python3 scripts/corpus.py fetch
python3 scripts/corpus.py run
python3 scripts/corpus.py run --reuse-oracles --require-parity
```

The strict command currently fails. Unsupported cases never count as parity. See `docs/coverage.md` for the coverage map, runtime profiles, baseline, and compatibility work order.

## Full compatibility work

Full parity is now the target. [The implementation sets](docs/parity.md) share one Core representation, canonical type algebra, inference engine and dependency store. [The feature registry](corpus/features.json) covers every case with overlapping dependencies rather than assigning each to its first parser error.

The compiler foundation retains every declaration, literal and annotation. All 327 cases roundtrip exactly through OTP's Core compiler profile and decode in Rust: 571 modules and 1,045,056 Core nodes. The integrated closure graph matches all 13,748 functions, 27,131 call-target sets and 594 recursive bindings. Original notices are retained in `third_party/otp`.

The shared type domain and formatter pass 653,626 direct native comparisons. Three original `erl_types_SUITE` tests run through Rust without native algebra fallback, and all six original properties pass 30,000 checks. The remaining form/context APIs are tracked separately. Atomic/scoped constraints, references, recursive SCCs and scalar/structural derived operations pass 60,768 native comparisons, including bounded construction normalization. Native closure ordering passes 6,561 comparisons. Ordered captures, record snapshots, Core generation and production phase migration remain open. Original-table form cache replay passes 11,000 exact value/cache-record outcomes plus 60 context/export-membership outcomes, metadata passes 48,803 outcomes, and finite Core literal type conversion passes 31,800 outcomes. These foundation gates do not establish whole-program parity.

```sh
python3 scripts/verify_frontend.py
python3 scripts/verify_types.py
python3 scripts/verify_type_suite.py
python3 scripts/verify_properties.py
python3 scripts/verify_closure_dependencies.py
python3 scripts/verify_constraints.py
python3 scripts/verify_invalidation.py
python3 scripts/verify_incremental_plan.py
python3 scripts/compatibility.py
```

The complete compatibility gate currently exits 1. The latest whole-corpus Core checkpoint inferred 317/327 groups and matched all available persisted signatures in 167. Category/location warning matches in 109 groups do not establish message, option or complete diagnostic parity. The original prototype remains at 2/327 preliminary diagnostic matches. Cache dependency repair and native analysis decisions have independent checks; complete persistence and operational behavior remain open. Performance work waits for full acceptance.
