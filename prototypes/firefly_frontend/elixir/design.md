# Native Elixir metadata lowering

Design A: clone the bounded Erlang term arena, append Erlang abstract forms while
visiting expanded Elixir definition AST. All specs and attributes retain their
original terms. One state object models variable versions and fresh counters;
pattern and guard context are explicit enums. Encode only the returned root.

Design B: decode metadata into an independent typed Elixir IR, then lower into a
typed Erlang IR and serialize. This creates two large parallel term schemas before
we have enough evidence for stable domain types. Its benefit is exhaustive typed
visitors; its cost is more translation and allocations.

Choose A for this acceptance prototype. Input is the bounded root arena and the
`{elixir_v1, Map, Specs}` term ID. Output is an arena whose root is the proper list
of Erlang forms. No oracle forms, VM callbacks, or source files are input.
Unsupported semantic constructs return an error with their AST tag. Both the
input and generated output are resource bounded. Production can replace the
append-only arena after native Elixir semantics are validated.

Translation rules follow Elixir's Apache-2.0 compiler sources `elixir_erl`,
`elixir_erl_pass`, `elixir_erl_var`, and `elixir_erl_clauses`; preserve their license
and notices. Specs are already Erlang abstract forms inside compiler metadata.
