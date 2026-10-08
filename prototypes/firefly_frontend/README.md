# Native Firefly frontend prototype

This prototype reads compiled Elixir BEAM debug metadata, translates its expanded definitions into Erlang abstract forms in Rust, lowers those forms with an extracted Firefly frontend, and feeds the existing Rust success-typing analyzer. Native execution launches no Erlang or Elixir processes. The native gate and benchmark now use the single `native-elixir-dialyzer` executable. Original production commands still use the existing compiler bridge.

The extraction includes 15 crates at Firefly commit `c672f3ad5bebd3d6e9e2fe6056a154ed92fd1d07`. Apache 2.0 LICENSE and NOTICE are retained under `compiler/`. `compiler/modernization.patch` records the complete changes against that commit. `compiler/reproduce.py` recreates the extraction from a pristine pinned checkout.

## Data flow

1. `input/lib.rs` checks the BEAM container and bounds ETF decoding. For the `elixir_erl` backend, `elixir/` translates saved expanded definitions and specs into Erlang abstract forms. The helper preserves type/spec attributes, original annotations and an AST origin sidecar. Ordinary Erlang debug metadata also remains supported.
2. `native/main.rs` bridges the in-memory forms through Firefly's abstract reader; `adapter/frontend.rs` runs abstract-form conversion, semantic analysis, canonicalization and Core lowering. It registers synthetic source documents using metadata coordinates, so original source files are unnecessary.
3. `adapter/source.rs` translates typed Firefly Core into the existing version 6 arena schema. The analyzer validates the in-memory arena with `CoreProgram::from_value`, then performs its existing success-typing analysis.
4. `elixir_harness/run_native.py` supplies captured runtime profiles and a prior-library snapshot, then compares exact warning terms, inferred function signatures, source forms and attributes with the OTP baseline. A PATH trap rejects attempted VM launches during native execution.

The whole module set is ingested, lowered and analyzed in one native process. Intermediate seed/Core files and per-module subprocesses are removed. An in-memory ETF bridge to Firefly and the adapter arena Value-to-Core validation remain. Dependency snapshots still use captured JSON.

## Verification scope

The Elixir gate passes all 21 grouped analyses: four Tiny modules, nineteen upstream Elixir Dialyzer test groups, and twenty-three Dialyxir example modules. Across 48 unique modules, native results match all 33 warnings exactly, every inferred signature, source form and source attribute. Both positive warnings and zero-warning cases are checked.

See `evidence/elixir-correctness-gate.json` for the comparison and artifact hashes, and `elixir/README.md` for supported language groups. The older `evidence/verification.json` records the preceding Erlang-only phase. These fixtures do not establish complete Elixir or Dialyzer parity.

Coherent translation repairs preserve local versus explicit remote calls, anonymous binding identity, source variable names, module_info bodies, native BIF calls and generated exception rethrows. Map, field, key and generated-operation annotations retain distinct origins. Focused tests cover these rules. The inferred-signature comparison additionally checks that matching warnings did not conceal changed function types.

## Reproduce

Requires Rust with the legacy unstable features enabled, Python 3, and the repository's existing compiled Elixir corpus and dependency PLT. Oracle preparation requires the installed OTP/Elixir environment used by this repository. Runtime lowering is native.

Build the one executable with the legacy Rust toolchain described in [native/README.md](native/README.md), then follow [elixir_harness/README.md](elixir_harness/README.md) to prepare the oracle and run:

```sh
python3 prototypes/firefly_frontend/elixir_harness/run_native.py "$PWD" --native-bin target/release/native-elixir-dialyzer
python3 prototypes/firefly_frontend/elixir_harness/benchmark.py "$PWD" --native-bin target/release/native-elixir-dialyzer --case tiny --case dialyxir/examples --rounds 5
```

For the preceding Erlang-only harness, `prepare.py` captures and validates its separate oracle:

```sh
cd prototypes/firefly_frontend
python3 prepare.py --offline
python3 run/run_corpus.py ../..
```

`prepare.py` builds the existing analyzer, metadata helper and adapter, captures fresh OTP context and validates the original Core baseline. By default it uses the repository's unchanged 13 corpus BEAMs. `--bundled-fixtures` compiles the included 13 fixture sources for a fresh comparison. Generated artifacts and Cargo outputs are ignored.

`--cargo /actual/toolchain/bin/cargo --rustc /actual/toolchain/bin/rustc` bypasses toolchain shims that reset environment variables. This extraction was built with Rust 1.88 and explicit `RUSTC_BOOTSTRAP=1`; a stable-supported port remains unfinished. External Cargo dependencies must already be cached for `--offline`.

For the separate acceptance fixture:

```sh
input/target/debug/native-beam-seed harness/beams/frontend_acceptance.beam harness/native-seed.json harness/oracle-source-forms.etf
adapter/target/debug/adapt-beam harness/beams/frontend_acceptance.beam harness/native-seed.json harness/firefly-module.json
```

Wrap the module in `{ "version": 6, "modules": [module] }`, then use `harness/compare_native.py` against `compiled_analysis_probe`. The comparison utility adds captured profiles only.

## Remaining gates

The Elixir translator explicitly rejects unimplemented groups including receive, super, dynamic structs and struct updates, binary-generator/unique/reducing comprehensions, general Collectable destinations, guarded catch and non-byte-aligned literal bitstrings. Bounded contracts require variable-map provenance, literal metadata maps require iterator-order provenance, and runtime atom/compiler profiles still come from oracle capture. Runtime exception execution equivalence has not been tested by these analyzer comparisons. Source-location storage uses a synthetic CodeMap and needs a production representation.

`elixir_harness/benchmark.py` compares optimized native execution with ordinary `dialyzer.run` using the same dependency facts and verifies warnings on every timed run. Results under `evidence/elixir-performance-*.json` include process startup, separate phase timings, executable hashes and measurement limits. Application incremental analysis is not enabled. Repeated immutable analyzer setup and duplicate adapter terms were removed before the single-process merge; broader language coverage, native profile generation, stable Rust support and production CLI integration remain separate work.

The historical [split Rust timing report](RUST_TIMINGS.md) separates frontend Rust work from subprocess wall time, dependency import, inference and diagnostics. `--phase-timings` collects opt-in measurements and verifies release build modes.

[The merged pipeline results](MERGED_RESULTS.md) compare the single executable with the preserved split release binaries and OTP.
