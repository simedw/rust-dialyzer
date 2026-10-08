# Full parity implementation

The target is OTP 28.3 with the pinned Elixir and Dialyxir corpus. One Rust Core analyzer will own inference, contracts, diagnostics and persistent summaries. The current abstract-forms interpreter remains an executable baseline until the Core analyzer reproduces it, then its callers and implementation are removed together.

The case registry uses overlapping feature sets. Completing a syntax parser or matching a warning count does not complete a set. Each set needs type-operation evidence, whole-program differential cases and the relevant lifecycle checks.

| Set | Shared implementation | Required evidence | Current state |
| --- | --- | --- | --- |
| Compiler domain | Flat Core/term arenas, complete declarations, lexical scopes, function identities, source origins | All compiler nodes and literal kinds; every corpus input retained; scope and origin checks | 327 exact version-6 roundtrips over 571 modules, scope/dependency indexes and declaration grammar checks pass; contract-variable map traversal passes 216 checks in four fresh VMs |
| Type algebra and BIFs | Canonical union families, widening, subtraction, depth limits, module-sensitive nominal identity, argument-dependent transfers | Native operation comparisons and all six full-domain lattice laws | 653,626 native type/solver/formatting outcomes, three original upstream type tests and 30,000 original property checks pass. All 148 specialized BIFs pass 48,165 outcomes, including native exceptions. Remaining type APIs stay explicit |
| Core flow | Patterns, residual clauses, guards, branches, exceptions, closures, dynamic calls and effects | Whole clean and failing programs for each construct; no silent widening of unknown constructs | Four scalar modules and one structural module match selected typed warnings and exported signatures in bare PLTs. The whole-corpus engine checkpoint infers 317/327 groups and matches all persisted signatures in 167; native pattern binding matches 133,138 corpus outcomes and the Core producer matches all 571 modules. Shared function state matches 9,000 scenarios, 72,891 transitions and 13,748 functions. Source-derived callgraph initialization matches all 327 cases. Literal folding matches 75,391 corpus roots and 1,272 synthetic producer outcomes; a shared generic warning binder matches 16,128 outcomes. Full success-typing and body traversal parity remain open |
| Success typing and dependencies | SCC constraints, module refinement, call and type edges, external summaries, recursive limits | Cross-module, recursive and higher-order programs; signatures compared independently | Native closure graph migration and 60,768 atomic/scoped/reference/SCC/scalar/structural-derived/dynamic/normalization outcomes and 6,561 closure-order pairs pass; 26,462 captured term comparisons, 3,560 Core generator cases and 3,656 VM closure captures also pass. Ordered graph edges and incoming callers match all 327 cases; 11,440 SCC partitions match. Complete Core generation and production orchestration remain open |
| Contracts and diagnostics | Alias expansion, bounded/overloaded contracts, callbacks, opacity, warning causes, details and native policy | Exact diagnostics and origins under every warning profile; clean Elixir cases | All 327 compiler environments match type resolution, finalized contracts and available contract validations; imported contract transfer passes 41,162 cases. Call conjunction/candidate consensus passes 20,588 outcomes; native warning emission rules pass 52,665. Callback conformance passes 9,704 native cases. Full diagnostic production remains open |
| Persistent state and interfaces | Classic/incremental stores, hashes, call/type dependency repair, options and exit behavior | 128 operational/type cases and 22 consistency checks through equivalent Rust actions | Pure incremental dependency expansion/repair passes 5,002 native comparisons; 824 native file outcomes and two actual native PLT table reloads pass; compiler fingerprints match all 571 corpus modules plus 32 compiled map-attribute modules. Complete analysis lifecycle and operational behavior remain open |

`corpus/features.json` labels every one of the 327 cases using the complete compiler input and a separately identified lexical source supplement. It records group overlaps and dependencies. For example, records require tagged tuple precision and declaration resolution; behaviours require callbacks, exports, contracts and external summaries; higher-order calls require function types, closure scopes and dynamic call resolution. These are implementation units, not individual-warning patches.

Run the completed foundation checks:

```sh
python3 scripts/verify_frontend.py
python3 scripts/verify_types.py
python3 scripts/verify_bifs.py
python3 scripts/verify_properties.py
python3 scripts/verify_type_suite.py
python3 scripts/verify_closure_dependencies.py
python3 scripts/verify_invalidation.py
python3 scripts/verify_warnings.py
python3 scripts/verify_dependencies.py
python3 scripts/verify_flow.py
python3 scripts/features.py --inventory corpus/inventory.json --report .cache/corpus/report.json --output .cache/core/features.json
cargo test --offline
cargo clippy --offline --all-targets -- -D warnings
```

The compiler bridge uses OTP's debug-info backend, exact Dialyzer compiler options and cleaning transform. It performs no type inference and reads no warning output. Version 6 represents terms and Core nodes in flat tables with validated backward references. It includes executable macros and reflection functions, unlike the tiny baseline exporter. The distinction matters when interpreting the earlier two passing corpus cases.

`python3 scripts/compatibility.py` is the complete acceptance gate. It currently exits 1: semantic parity is 2/327 and the other analysis/lifecycle gates remain open. Compiler fidelity, program indexing, type algebra and selected scalar inference are separately reported; neither can make the complete gate pass. The frontend gate checks implementation and exported-input hashes to reject stale evidence.

Evidence remains under `.cache/core`, `.cache/types`, `.cache/properties`, `.cache/bifs`, `.cache/flow`, `.cache/dependencies`, `.cache/warnings`, `.cache/engine` and `.cache/compatibility`. Decisions are recorded in `docs/decisions.tsv`. Performance work waits for complete acceptance; the earlier worklist timings describe only the tiny prototype.

The declaration arena retains 17,640 forms, 1,053 named declarations, 1,513 contract alternatives and 9,169 record fields across the complete corpus. It distinguishes bounded functions and constraints, local and remote references, and generic domains; parsing alone does not validate or expand contracts. The classic PLT importer preserves 5,407 dependency summaries, 3,241 contracts and 73 callbacks from the 221-module native base. Summaries belonging to the analyzed modules are excluded, so imported target results cannot substitute for Rust inference. Imported contract transfer is verified independently; activation of validated target contracts and incremental state remain open.

The Core engine corpus runner (`scripts/verify_corpus_engine.py --refresh-native`) compares every target against signatures persisted using the original library PLT and retains full native warning terms. Library artifacts are fingerprinted separately and target facts are excluded from Rust inputs. This context is stricter than the bare PLT checkpoints; partial category/location matches cannot make the complete gate pass. Warning-profile transitions pass 8,521 native comparisons across all 26 options and tags; source/function warning policy and native return classification now have separate complete gates; connecting all native causes and formatting remains open.

Call transfer has a canonical shared signature and one independent calculator. Its production migration belongs with the success/refinement phase replacement: a failed argument conjunction forces success constraints to fail, while dataflow still retains the signature return for a diagnostic. The existing approximate traversal cannot correctly serve both phases. No fixture-specific phase switch is used to conceal this difference.
