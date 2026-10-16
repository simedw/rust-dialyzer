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

Needs OTP 28 and Elixir 1.19 on `PATH`, and the project compiled with them.

```sh
cargo build --release --bin rust-dialyzer
(cd ~/src/my_app && mix compile)
target/release/rust-dialyzer ~/src/my_app
```

This analyzes the project together with all its dependencies, Elixir and OTP, without a PLT, and prints Dialyzer's warnings for the project's own modules. Add `--include-app mix --include-app ex_unit` if the project uses Mix tasks or test helpers; otherwise calls into them are reported as unknown. `--env` picks the build (default `dev`) and `--app` overrides which apps to report on. The exit code is 0 without warnings and 2 with warnings.

Some dependencies still use constructs the BEAM frontend cannot lift; the run then stops with the module and function it failed on.

## Tests

`cargo test --release` runs the unit tests. `bench/phase0/parity.sh` checks warnings and inferred types against OTP Dialyzer on Phoenix, Elixir and a subset corpus; it needs the cached inputs under `.cache/phase0`.

## License

MIT. Code adapted from Erlang/OTP and Elixir is under Apache 2.0; see `third_party/otp/NOTICE` and `docs/beam-core-port/NOTICE`.
