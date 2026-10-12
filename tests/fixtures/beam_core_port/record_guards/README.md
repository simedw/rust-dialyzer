# Record guards and pattern equality oracle

The 11-function `native_record_guards` fixture was compiled and captured independently with OTP 28.3. Tests compare whole raw Core and cleaned Core as exact Erlang terms, including annotations, variable names and exception scope.

Two compiler rules explain the corpus failures addressed here:

1. `erl_internal:new_type_test` recognizes `is_record/2` and `is_record/3`. `v3_core:gexpr_test` therefore treats them as Boolean tests directly. `is_function/2` retains its separate unsafe/protected treatment.
2. `v3_core:uguard` inserts pattern equality tests into the existing linear guard preamble. It evaluates existing guard lets before assigning the final guard value to the new right-hand temporary. Treating the complete guard let tree as one nested argument changes Core structure and obscures the expected source annotations. Prefix flattening must stop at a protected `c_try` boundary.

Coverage includes plain and qualified record tests, comma conjunctions, guard alternatives, negation, `is_function/2`, pinned case alternatives, repeated pattern variables, two simultaneous pattern equalities and a throwing `length/1` expression that must remain protected.

To regenerate at the captured source path:

```sh
mkdir -p /private/tmp/beam-core-port-record-guards
cp tests/fixtures/beam_core_port/record_guards/native_record_guards.erl /private/tmp/beam-core-port-record-guards/
erlc +debug_info -o /private/tmp/beam-core-port-record-guards /private/tmp/beam-core-port-record-guards/native_record_guards.erl
elixir scripts/beam_core_port/capture.exs tests/fixtures/beam_core_port/record_guards /private/tmp/beam-core-port-record-guards/native_record_guards.beam
```

Test execution requires no VM or compiler installation.
