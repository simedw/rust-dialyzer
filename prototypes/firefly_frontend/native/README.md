# Single-process native Elixir analyzer

`native-elixir-dialyzer` links BEAM ingestion, native Elixir metadata translation,
Firefly lowering and the Rust analyzer into one executable. It runs the whole
module set in one process. It launches no frontend helpers or Erlang/Elixir
runtime and creates no intermediate frontend files.

The libraries preserve one implementation of each stage:

- `native_beam_seed::prepare(path)` returns original source metadata plus translated Erlang forms.
- `firefly_core_adapter::frontend::lower(forms, seed)` lowers and adapts Core in memory.
- `CoreProgram::from_value` validates the in-memory arena without textual JSON serialization.
- `compiled_probe::analyze` shares analysis options and warning/signature projection with the independent oracle probe.

Two representation conversions remain in memory: bounded ETF encoding/decoding
between the translator and Firefly, and the adapter's `serde_json::Value` arena
into validated typed `CoreProgram`. Dependency facts still load from captured
JSON. This merge does not add an application incremental cache or expand the
supported Elixir language groups.

## Build

The extracted Firefly compiler requires Rust 1.88 and bootstrap. The root analyzer
and translator compile with that same toolchain, so only one runtime binary is
needed. Replace toolchain paths with your installation. Offline dependencies
must already be cached.

```sh
CARGO_TARGET_DIR="$PWD/target" RUSTC_BOOTSTRAP=1 RUSTC=/path/to/rust-1.88/bin/rustc RUSTDOC=/path/to/rust-1.88/bin/rustdoc /path/to/rust-1.88/bin/cargo build --release --offline --manifest-path prototypes/firefly_frontend/native/Cargo.toml
```

The executable is `target/release/native-elixir-dialyzer`.

## Run

```sh
target/release/native-elixir-dialyzer PRIOR_LIBRARY.json CLOSURE_PROFILE.json < requests.jsonl
```

Each JSONL request contains compiled `beam_paths`, an `id`, analysis options
`analysis_mode`, `legal_warnings`, `purged_modules`, `use_contracts`, `solvers`
and `location`, plus `context` with exactly `version`, `profile`, `atom_layout`
and `contract_closure_profile`. Runtime/dependency context must correspond to
the compiled application's OTP environment. Elixir source and oracle Core
bodies are not inputs. Expected warnings are never supplied to this executable.

Output is actual warning terms, inferred signatures and module dependencies.
`emit_core: true` adds the native Core arena for independent metadata verification;
benchmarks leave it disabled. `RUST_DIALYZER_TIMINGS=1` writes structured phase
records to stderr. Invalid input reports an error and exits unsuccessfully.

The [Elixir harness](../elixir_harness/README.md) constructs whitelisted requests,
compares actual results with OTP externally, and rejects VM launch attempts. The
old individual stage binaries remain diagnostic tools and the historical split
benchmark baseline; the native gate and benchmark use only this executable.
