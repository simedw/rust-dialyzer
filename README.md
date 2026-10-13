# rust-dialyzer

A reimplementation of Erlang's Dialyzer in Rust. It reads compiled BEAM files, lifts them to Core Erlang, and produces the same warnings as OTP Dialyzer.

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
