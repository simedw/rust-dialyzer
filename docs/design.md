# Analyzer design

The bounded proof of concept analyzes expanded Erlang forms exported from compiled BEAM files. Its sweep and worklist implementations reproduce the example warnings and signatures. That baseline remains executable while the complete Core inference pipeline is built. Full compatibility uses the architecture below; the old forms interpreter will be removed with its callers when that replacement passes acceptance.

## Upstream corpus design

Two corpus shapes were compared: copying selected fixtures into local tests, or retaining pinned upstream sources with a complete registry. The registry keeps every case visible, including grouped inputs, native options and unsupported cases. It avoids silently turning a cherry-picked subset into a compatibility claim.

The organizing record is a case with a source revision, files, native configuration, oracle outcome and Rust outcome. The independent oracle checks the original source goldens before comparing compiled BEAM inputs. Operational CT cases and lattice properties have a separate ledger because they cannot be treated as source-warning fixtures.

`corpus.py` owns discovery, selection and status reporting. `corpus_oracle.exs` owns OTP's native harness, compiler options and PLT profile. `corpus_elixir.exs` owns the official Elixir module grouping and the Dialyxir example profile. All paths export through `beam_export.exs`; the original `export.exs` command remains a thin CLI for that shared compiler boundary.

## Full compatibility architecture

The proof-of-concept forms interpreter is a bounded baseline. Full compatibility uses one lossless Core Erlang representation and the OTP compiler frontend. Rust owns type operations, success typing, contracts, warnings and persistent summaries. The compiler bridge never asks OTP for inferred types or warning decisions.

Two complete designs were compared: implement all abstract-form lowering in Rust, or use OTP's existing Core lowering. The first would duplicate records, comprehensions, guards, exceptions and generated failure paths. The second matches the actual Dialyzer boundary and accepts both Erlang and Elixir debug-info backends. Choose Core with the exact pinned compiler options and cleaning transform. Keep the old engine only as an executable baseline until the Core engine reproduces it; then migrate callers and delete it together. There will be no permanent dual analyzer API.

The version 5 interchange distinguishes atoms, arbitrary integers, float bit patterns, binaries, partial-byte bitstrings, tuples, proper/improper lists and maps. It retains every abstract attribute, every executable definition and all Core annotations. The compiler profile requires `term_map_order: native_to_list_with_iterator`. Map entries retain native `maps:to_list` order for ETF fingerprints. A validated per-map permutation retains `maps:iterator` order for pattern refolding. These orders differ on large maps. Hash-consing uses encoded structural records and child identities, preserving that order through enclosing terms. Each module has flat node and interned term tables; references point to earlier entries, so decoding and dropping a program need no recursive ownership. Core node groups follow `cerl:subtrees/1`, with an explicit map base because that API omits `map_arg`. Roundtrip verification reconstructs the exact Core tree, using non-folding tuple/list constructors so generated node annotations survive. A roundtrip pass establishes frontend fidelity, not inference parity.

The coherent feature sets are interchange and origins; type algebra and BIF transfers; full Core control flow and function identity; SCC success typing and dependency refinement; contracts, nominal types, behaviours and diagnostics; persisted summaries and operational interfaces. Contracts constrain inference inside the shared solver: they are not bolted onto warnings after analysis. Per-case feature labels are overlapping dependencies, not exclusive buckets based on the first parser error.

The type domain must retain `none` versus `unit`, finite atom/integer sets with OTP widening, fixed structural union families, tagged tuple alternatives, list termination/nonempty shape, function products, map normalization and module-sensitive nominal identity. Smart constructors own canonicalization; structural equality and subtype then have stable meaning. Direct operation comparisons against OTP precede use in inference. Unknown language constructs remain explicit failures.

Full acceptance includes all 327 semantic fixtures, exact diagnostic details and origins, inferred signature comparisons, and executable equivalents of the 128 operational/type cases and 22 generated consistency checks. The six seeded lattice properties belong to the 128 cases; the operational acceptance denominator is 150. The recorded upstream golden line drift remains a separate failing reference check until investigated. Packaging tests need an explicitly scoped Rust interface equivalent. No finite fixture suite proves every possible program, so generated differential testing continues after the fixed corpus passes.

## Inference phase ownership

Dependency summaries seed external calls. Cold target functions begin without imported target results. Target contracts remain inactive until SCC success analysis validates them against inferred types. Each SCC pass collects nested functions, solves its constraints, restricts results to that SCC's owned functions, and inserts newly valid contracts. New contract activation counts as a change even if signatures are equal.

SCC convergence compares types limited to depth four, skipping failed or uncalled functions. A changed SCC schedules module refinement in native module postorder. Module refinement uses a different convergence rule: the old limited type must be a subtype of the new limited type. Changed function labels rebuild the affected SCC order for another success pass. Stored signatures use depth five. The shared `success_phase` module reproduces these comparison and persistence rules across 5,000 native outcomes; the production traversal and phase orchestration remain open.

Warnings run after convergence with a distinct warning-mode state. They combine missing-function contracts, Core dataflow warnings and callback conformance. Contract-range warnings undergo the native origin rewrite and same-origin contract-warning filtering before final ordering and rendering. Rendering captured warnings verifies text and ordering; only the complete collector can establish diagnostic parity.
