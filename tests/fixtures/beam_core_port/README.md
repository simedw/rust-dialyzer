# Pinned compiler fixtures

These BEAM files and ETF stage outputs were captured using Elixir 1.19.4-otp-28 and Erlang/OTP 28.3. They are independent compiler oracles, not native compiler output. `scripts/beam_core_port/capture.exs` regenerates the stage outputs from each BEAM. Compiler options and source hashes are recorded in `docs/beam-core-port/source-profile.json` and the capture script.

The integration tests compare debug information, complete Erlang forms, raw Core and cleaned Core. Forms-only fixtures exercise constructs whose Core lowering remains unsupported. Names, annotations, clauses and declarations are retained; map ordering is canonicalized when comparing fixture bytes. Corpus verification instead compares decoded terms with Elixir’s strict equality.

`inspect-opts-dbgi.etf` is a small real debug-info sample used by the existing ETF decoder regression test. Tests run entirely in Rust and need no installed Elixir or Erlang VM.
