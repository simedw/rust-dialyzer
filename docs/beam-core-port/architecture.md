# Native BEAM frontend

The replacement owns BEAM decoding, Elixir debug-metadata lifting, OTP-compatible forms-to-Core lowering and Dialyzer cleanup. Compiler processes belong only in oracle generation. Unsupported constructs return stage/module/function diagnostics. No fallback, omitted definition or fabricated type counts as support.

Two independent sketches compared a staged compatibility port with expanded Elixir directly to Core. Both recommend the staged port. It shares lowering with Erlang and permits exact comparisons at the forms and Core boundaries. Direct lowering still needs source forms for specs and diagnostics and couples too many naming/annotation rules for the first implementation. The synthesis keeps the shared flat term arena, explicit stage outputs, and construct census from both sketches.

Experimental interface: `compile_beam(bytes)` returns native cleaned Core and recovered source forms. Internal stage interfaces operate on the existing flat `ErlangTerms` arena:

- `debug::read_beam(bytes, limits) -> DebugModule`: checked BEAM chunks and backend schema.
- `elixir::lift(debug_terms) -> ErlangTerms`: expanded Elixir definitions to complete Erlang forms.
- `lower::lower(forms, module) -> ErlangTerms`: OTP-shaped unclean Core.
- `clean::clean(core) -> ErlangTerms`: Dialyzer cleanup.

Raw declaration and annotation terms remain exact. Existing `external_term` decoding/encoding is reused. Stage output files are diagnostic artifacts; the frontend does not need to serialize between stages. Reflection functions, private definitions, specs, type attributes and generated failure branches are in scope. Macros are already expanded in BEAM metadata, but their generated definitions/caller convention remain necessary.

The current backend calls Elixir dynamic_form twice, once for erlang_v1 and again for core_v1. The native stages reuse a single lifting result. Elixir's ordinary erl_to_core goes directly to v3_core; parse-transform options take a separate compiler path and require explicit rejection until ported. Erlang compiler input additionally requires record expansion.

Compiler semantics are pinned to installed Elixir1.19.4 and OTP28.3. Elixir source was recovered through its own abstract_code debug information, not guessed from historical docs. OTP v3_core and dialyzer_clean_core source is installed locally. Ported logic retains Apache-2.0 attribution.

Separate integration obligations: native atom internal hashes/order, literal-map iterator provenance, bounded-contract variable maps and Dialyzer closure profiles. Existing oracle contexts can isolate frontend equivalence in research; they do not prove that context creation is VM-free. Full promotion requires both full Phoenix/Elixir support and analyzer parity.

Gate order: chunk/schema validation; exact erlang_v1 forms; exact raw and cleaned Core including annotations and compiler names; complete warnings/signatures; whole frontend timing with support counts. A supported tiny module is a first compiler slice, not completion of the whole port.

Term arenas share an immutable prefix through `SharedVec`. Stages append new terms without copying input strings, lists or tuples. Snapshots seal appended terms into immutable shared blocks; they do not copy the existing prefix. The ordinary SharedVec.freeze API still provides contiguous storage to existing callers. Map ordering visits only the roots needed for each comparison. ETF decoding validates atom-key maps directly and prepares ordering only for general map keys. The native frontend opts into atom, nil and byte-integer interning; resource limits still count all decoded values. Its command-line tools use the analyzer’s existing MiMalloc allocator.
