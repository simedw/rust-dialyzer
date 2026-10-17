# rust-dialyzer

A reimplementation of Erlang's Dialyzer in Rust. It reads compiled BEAM files, lifts them to Core Erlang, and produces the same warnings as OTP Dialyzer.

With a profile-guided build, cold-start native frontend and analysis benchmarks are about 3–3.5× faster than OTP Dialyzer. It still uses several times more memory.

| Project | Modules | OTP Dialyzer | rust-dialyzer | Speedup | Peak RSS (OTP → Rust) |
|---|--:|--:|--:|--:|--:|
| Elixir | 846 | 17.6 s | 5.7 s | **3.06×** | 1.4 → 5.1 GiB |
| Phoenix | 600 | 17.0 s | 5.3 s | **3.23×** | 1.3 → 4.9 GiB |
| Large Elixir app (internal) | >15k | 3 m 51 s | 1 m 5 s | **3.56×** | 5.1 → 12.6 GiB |

Wall-clock time, median of 3 runs on an Apple M4 Max with 14 workers/schedulers, OTP 28 and Elixir 1.19. Both tools report the same warnings. The runs start without a PLT; filesystem caches are not flushed, and host load is recorded.

## Check a Mix project

Use OTP 28.3 or OTP 29.1.2 and Elixir 1.19 on `PATH`, with the project compiled for that runtime. Runtime authentication checks the pinned Dialyzer BEAM files: Dialyzer 5.4 for OTP 28 and 6.0.4 for OTP 29.

```sh
cargo build --release --bin rust-dialyzer
cargo build --release -p rust-dialyzer-otp29 --bin rust-dialyzer-otp29
(cd ~/src/my_app && mix compile)
target/release/rust-dialyzer ~/src/my_app
```

This analyzes the project together with all its dependencies, Elixir and OTP, without a PLT, and prints Dialyzer's warnings for the project's own modules. Add `--include-app mix --include-app ex_unit` if the project uses Mix tasks or test helpers; otherwise calls into them are reported as unknown. `--env` picks the build (default `dev`) and `--app` overrides which apps to report on. The exit code is 0 without warnings and 2 with warnings.

The command selects the worker for the installed OTP runtime. Keep both executables in the same directory. Each worker contains its own Rust type system and compiler behavior, including native records on OTP 29. Plain `cargo build` and `cargo test` select the OTP 28 package.

Some dependencies still use constructs the BEAM frontend cannot lift; the run then stops with the module and function it failed on.

## Tests

Run the Rust tests for each version:

```sh
cargo test --release --offline
cargo test --release --offline -p rust-dialyzer-otp29
```

The OTP 29 package includes complete raw and cleaned Core fixtures, native-record pattern and guard captures, and literal transport, ordering, and hash checks. The fixtures include scripts that regenerate their native oracles.

For live comparisons, build the probes with `cargo build --release --offline -p rust-dialyzer-otp29 --bins`. Set `OTP29_INSTALL` to the runtime installation and `OTP29_SOURCE` to its source tree. The runners use the matching Rust probe:

```sh
python3 scripts/verify_types.py --otp 29 --otp-install "$OTP29_INSTALL" --binary target/release/type_probe_otp29 --out .cache/types29 --compress-jsonl
python3 scripts/verify_constraints.py --otp 29 --otp-install "$OTP29_INSTALL" --otp-source "$OTP29_SOURCE" --binary target/release/constraints_probe_otp29 --out .cache/constraints29 --compress-artifacts
python3 scripts/verify_bifs.py --otp 29 --otp-install "$OTP29_INSTALL" --binary target/release/bif_probe_otp29 --out .cache/bifs29
python3 scripts/verify_otp29_records.py --runtime "$OTP29_INSTALL" --binary target/release/type_probe_otp29 --out .cache/records29
python3 scripts/verify_otp29_analysis.py --runtime "$OTP29_INSTALL" --probe target/release/core_cache_analyze_otp29 --out .cache/analysis29
python3 scripts/verify_otp_warning_corpus.py --otp-install "$OTP29_INSTALL" --binary target/release/core_cache_analyze_otp29 --out .cache/warnings29
```

The warning-corpus runner needs the captured case inventory under `.cache/engine`. It builds fresh native library PLTs for the selected runtime, then compares exact public warnings.

Set `RUST_DIALYZER_VERIFY_FASTPATH=1` to repeat a runner with the reference checks enabled. `bench/phase0/parity.sh` checks warnings and inferred types against OTP Dialyzer on Phoenix, Elixir, and a subset corpus. It needs the cached inputs under `.cache/phase0`.

## License

MIT. Code adapted from Erlang/OTP and Elixir is under Apache 2.0; see `third_party/otp/NOTICE` and `docs/beam-core-port/NOTICE`.
