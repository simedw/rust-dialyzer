# Literal folding during closure pattern renaming

The independent `native_literal_shadow` oracle captures OTP 28.3 forms, raw Core and cleaned Core. Its complete-module test distinguishes three cases:

- Ordinary source tuple/cons values and patterns remain structural under `dialyzer`.
- An anonymous function with no outer known variables retains structural constant patterns.
- An anonymous function with outer known variables runs `rename_shadowing_clauses`. Its generic pattern data is rebuilt by `cerl:ann_make_data`, which folds literal tuple/cons children even if no variable actually shadows an outer name.

The fixture includes nested tuples/lists, literal map pattern values, improper lists and a variable that actually shadows an outer name. Full annotations and generated names are retained. The native helpers preserve the parent annotation and fold only direct literal children. They are wired to shadow-pattern rebuilding, not ordinary source constructors. This follows `v3_core.erl`'s `ren_pat` and `cerl.erl`'s `ann_c_tuple`/`ann_c_cons`.

The test compares exact Erlang terms. ETF allows flat list segments, nested list segments and byte strings to encode the same list, so physical ETF byte equality is insufficient here. Strict comparison still distinguishes integer/float types, signed zero, annotations, clause order and generated variable names. Generated raw Core also passed an independent VM `===` comparison during development.

To regenerate at the captured annotation path:

```sh
mkdir -p /private/tmp/beam-core-port-literal
cp tests/fixtures/beam_core_port/literal_shadow/native_literal_shadow.erl /private/tmp/beam-core-port-literal/
erlc +debug_info -o /private/tmp/beam-core-port-literal /private/tmp/beam-core-port-literal/native_literal_shadow.erl
elixir scripts/beam_core_port/capture.exs tests/fixtures/beam_core_port/literal_shadow /private/tmp/beam-core-port-literal/native_literal_shadow.beam
```

Test execution requires no VM or compiler installation.
