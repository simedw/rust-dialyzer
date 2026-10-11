# Original OTP runtime context fixtures

The original `dialyzer_typesig.beam` and `dialyzer_contracts.beam` come from OTP 28.3 / Dialyzer 5.4 in the installed distribution. They are Apache 2.0 licensed OTP compiler artifacts, not generated native compiler output. Their SHA256 hashes are pinned by the native context builder.

The JSON expectations were captured independently using OTP `beam_lib` atom/chunk readers and `crypto.hash`. They retain the complete FunT table, including original row order, labels, indexes, captures and uniq. Tests do not need Erlang or Elixir installed.

To regenerate JSON against the selected installed compiler:

```sh
elixir scripts/beam_core_port/capture_runtime_context.exs tests/fixtures/beam_core_port/runtime_context
```

The Rust generic reader supports both ordinary unsigned-count byte-length atom tables and OTP 28 negative-count compact-tagged-length UTF8 atom tables. The original OTP fixtures exercise the latter encoding.
