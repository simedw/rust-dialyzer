# rust-dialyzer

A reimplementation of Erlang's Dialyzer in Rust. It reads compiled BEAM files, lifts them to Core Erlang, and produces the same warnings as OTP Dialyzer (OTP 28.3).

```sh
cargo build --release --bin core_cache_analyze
python3 scripts/beam_core_port/native_manifest.py beams.txt manifest.json --dialyzer-ebin /path/to/otp/lib/dialyzer-5.4/ebin
target/release/core_cache_analyze prior-library.json manifest.json options.json
```

`bench/phase0/parity.sh` checks warning parity against OTP Dialyzer.

MIT licensed. Code adapted from Erlang/OTP and Elixir is under Apache 2.0; see `third_party/otp/NOTICE` and `docs/beam-core-port/NOTICE`.
