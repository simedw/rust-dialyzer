# Constant-expression lowering oracle

`native_constants.erl` was compiled with debug info by the pinned OTP 28.3 compiler. The independent forms, raw Core and Dialyzer-cleaned Core snapshots were captured using `scripts/beam_core_port/capture.exs`. Tests compare the whole module at both Core boundaries, preserving annotations, variable names, function order and IEEE float bits.

The 24 functions cover unary negatives, 90/130-bit integer values, integer and float arithmetic, signed integer div/rem, bitwise operators, positive/negative shifts, negative zero, Boolean and comparison expressions in map keys, numeric map keys and string/character-list append patterns with variable tails.

Production `constant.rs` follows `erl_eval:partial_eval/1`: evaluate only supported constant AST forms; rewrite the original root only when its result is an integer, float or atom; keep the original annotation. Nonconstant expressions and arithmetic badarg leave the AST unchanged. The `++` pattern rewrites follow `v3_core:pattern/2` and preserve each left-hand cons/string annotation. Integer results/shifts have a two-million-bit resource limit and recursive expressions a depth limit of 512.

To regenerate:

```sh
mkdir -p /private/tmp/beam-core-port-constant
cp tests/fixtures/beam_core_port/constants/native_constants.erl /private/tmp/beam-core-port-constant/
erlc +debug_info -o /private/tmp/beam-core-port-constant /private/tmp/beam-core-port-constant/native_constants.erl
elixir scripts/beam_core_port/capture.exs tests/fixtures/beam_core_port/constants /private/tmp/beam-core-port-constant/native_constants.beam
```

The captured source path is part of the oracle annotations. Use the displayed path when regenerating the fixture. Test execution requires no VM or compiler installation.
