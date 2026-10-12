# Run the native BEAM frontend

The native frontend compiles all files in the saved Phoenix and Elixir inventories without starting Elixir or Erlang. It reads expanded debug information from each BEAM and feeds the existing Rust analyzer. Use the pinned OTP 28.3 Dialyzer BEAMs for the runtime context. The default frontend remains unchanged; select native mode through a manifest.

## Build and run

Run these commands from the `codex/beam-core-rust` worktree. This experiment was built with Rust 1.92.0.

```sh
cargo build --release --bin beam_core_compile --bin core_cache_analyze
python3 scripts/beam_core_port/native_manifest.py \
  reports/beam-core-port-2026-10-11/evidence/phoenix-beams.txt \
  /tmp/phoenix-native-manifest.json \
  --dialyzer-ebin /Users/simedw/.asdf/installs/erlang/28.3/lib/dialyzer-5.4/ebin
PATH=/usr/bin:/bin target/release/core_cache_analyze \
  /Users/simedw/Documents/ChatGPT/rust-dialyzer/.cache/phase0/rust-cone/out/prior-library.json \
  /tmp/phoenix-native-manifest.json \
  /Users/simedw/Documents/ChatGPT/rust-dialyzer/reports/head-to-head-2026-10-09/phoenix/options.json \
  > /tmp/phoenix-native-analysis.json
```

Replace `phoenix` with `elixir` to run the other inventory. The absolute BEAM paths, cold prior and analyzer options refer to the saved inputs on this machine. Keep the same prior and options when comparing frontends. A different nonempty prior library needs its atom and ordering provenance incorporated into the native context first; this run does not migrate arbitrary persisted PLTs.

`PATH=/usr/bin:/bin` excludes the installed Elixir command during native analysis. The native pipeline itself has no compiler subprocess or fallback. Its manifest contains BEAM paths and the two pinned original Dialyzer binaries; it does not require a compiler-produced Core cache or captured runtime context.

Compile an individual BEAM to a diagnostic ETF file:

```sh
target/release/beam_core_compile clean \
  tests/fixtures/beam_core_port/Elixir.NativeSlice.beam /tmp/native-core.etf
```

The stage commands are `forms`, `core` and `clean`. Analysis passes native arenas directly to the typed adapter; these ETF files are only for inspection and differential verification.

## Check compiler compatibility

Stored fixtures require no installed compiler. Run the library and compiler integration suites:

```sh
cargo test --release --lib \
  --test beam_core_frontend --test beam_core_adapter \
  --test beam_core_comprehensions --test beam_core_control \
  --test beam_core_exports --test beam_core_pipeline \
  --test beam_core_record_preparation --test core_lower_aliases \
  --test core_lower_matches --test core_lower_oracles --test core_lower_scopes
```

The whole-corpus oracle requires Elixir 1.19.4-otp-28 and OTP 28.3. It compares complete decoded terms, including names and annotations:

```sh
python3 scripts/beam_core_port/prepare_verification.py \
  target/release/beam_core_compile /tmp/beam-core-verification \
  reports/beam-core-port-2026-10-11/evidence/phoenix-beams.txt \
  reports/beam-core-port-2026-10-11/evidence/elixir-beams.txt
lockf -k /tmp/rust-dialyzer-run.lock elixir \
  scripts/beam_core_port/verify_inventory.exs \
  /tmp/beam-core-verification/comparison-plan.tsv \
  /tmp/beam-core-verification/oracle-results.json
```

Expect 1,032 successful comparisons at each of the forms, raw Core and cleaned Core stages. The inventories overlap; the script deduplicates file paths.

## Benchmark the complete system

Create both native manifests in `/tmp/beam-core-bench-inputs` using the manifest command above. Stop other builds and benchmark jobs. The output directory must be new:

```sh
lockf -k /tmp/rust-dialyzer-run.lock python3 \
  scripts/beam_core_port/benchmark_system.py \
  "$PWD" "$PWD/target/release/core_cache_analyze" \
  /tmp/beam-core-bench-inputs /tmp/beam-core-whole-system \
  --rounds 5 \
  --prior /Users/simedw/Documents/ChatGPT/rust-dialyzer/.cache/phase0/rust-cone/out/prior-library.json \
  --options-root /Users/simedw/Documents/ChatGPT/rust-dialyzer/reports/head-to-head-2026-10-09
```

This macOS benchmark uses `/usr/bin/time -l` to collect CPU time and peak RSS. Those counters require access to the system clock information outside the restricted Codex shell sandbox. The benchmark performs one warmup and five measured pairs per workload, alternates order, and creates an empty compiler cache for every compiler run. It compares complete warnings and signatures on every run and records input, source and binary SHA256 digests. Generated analysis JSON is compressed; temporary compiler caches are removed after each measurement.

The compiler baseline includes fresh Core export followed by the same analyzer executable. Native mode compiles and adapts in memory. Both measurements include full analysis and output serialization. Compiler peak RSS is the larger of its sequential exporter and analyzer peaks, rather than their sum.
