# Rust Dialyzer findings and performance checkpoint

Recorded on 8 October 2026. This is a snapshot of verified results, measured costs, and open work. Later runs may supersede it.

## Current result

The native Rust analyzer runs on the compiled Elixir and Phoenix projects without launching Elixir or Erlang. The current integrated release matches all 156 Elixir warning terms and all 11,405 named inferred signatures. Phoenix matches all 32 warning terms and all 1,544 named signatures.

**The current integrated release is still slower than OTP Dialyzer on both projects.** The performance goal remains open.

| Project | Rust wall time | OTP wall time | Rust slowdown |
| --- | ---: | ---: | ---: |
| Elixir 1.20.4 | 51.582 s | 8.422 s | 6.12 times |
| Phoenix 1.8.15 | 5.233 s | 2.303 s | 2.27 times |

These are clean single fresh-process pairs, without a warmup. They guide development but do not establish a stable median. Application incremental reuse is disabled. Both analyzers use the same compiled targets and cached dependency information.

The integrated executable is [native-elixir-dialyzer](/Users/simedw/Documents/ChatGPT/rust-dialyzer/target/native-perf-combined-structural-rebuild/release/native-elixir-dialyzer). Its SHA-256 is `4c2cfa52f7294cf164bcb0cf2e41aa531f1a409b9016ab88e5c9d889e56f2dfc`. Its verification includes 230 library tests, all target checks, 21 native fixture groups, and full project warning and named-signature comparisons.

The completion criterion requires five fresh-process pairs after a warmup, followed by an independent confirmation batch. Rust must beat OTP on both projects while preserving exact results. Preparation cost must be disclosed separately. No isolated component improvement satisfies that criterion.

Evidence: [integrated verification](/Users/simedw/Documents/ChatGPT/rust-dialyzer/reports/performance/combined-structural-rebuild/verification.json), [performance task list](/Users/simedw/Documents/ChatGPT/rust-dialyzer/reports/performance/full-project-hillclimb/tasks.md), and [decision history](/Users/simedw/Documents/ChatGPT/rust-dialyzer/reports/performance/full-project-hillclimb/decision.tsv).

## Inputs and compiler stages

The native executable reads BEAM files. A BEAM file contains executable bytecode and other chunks. The analyzer uses compiler debug metadata to recover the expanded program; it does not infer source-level types by interpreting BEAM instructions.

Erlang and Elixir expose different debug metadata. Erlang can provide Erlang abstract forms. Elixir provides its own expanded definitions and metadata, which need a version-aware translation into Erlang abstract forms. Compilation to BEAM therefore does not remove the need for an Elixir adapter. Macros have already expanded before analysis.

The native path has these stages:

1. Read bounded BEAM chunks and decode External Term Format metadata.
2. Translate Elixir expanded definitions into Erlang abstract forms, retaining declarations, specs, locations, and relevant generated-code annotations.
3. Run the extracted Firefly frontend's semantic analysis, canonicalization, and Core Erlang lowering.
4. Adapt typed Firefly Core into the analyzer's internal Core arena and validate it.
5. Construct labels, dataflow information, call dependencies, declarations, and dependency summaries.
6. Infer success types, validate contracts, and produce diagnostics.
7. Serialize the requested results.

Core Erlang is a compiler intermediate representation. It expresses functions, calls, patterns, control flow, and values. It is not LLVM machine-level IR, and BEAM bytecode is not itself an AST. The debug metadata gives us a program representation suitable for this analysis.

The current native path passes typed program structures between linked stages. Earlier versions wrote intermediate seed and Core files, launched several executables, and converted ordinary Core through JSON. Those boundaries have been removed from the native path. Standalone diagnostic tools and older compiler-bridge commands still exist for comparison and verification.

The production native run needs compiled target BEAMs and cached dependency facts. It does not receive precomputed target warning results or target inferred types. Preparation scripts and OTP oracle runs can launch a VM outside the timed native analysis. That is distinct from the analyzer itself launching a VM.

Source: [native frontend](/Users/simedw/Documents/ChatGPT/rust-dialyzer/prototypes/firefly_frontend/native/frontend.rs), [Elixir adapter](/Users/simedw/Documents/ChatGPT/rust-dialyzer/prototypes/firefly_frontend/elixir), and [compiled analysis](/Users/simedw/Documents/ChatGPT/rust-dialyzer/src/compiled_analysis.rs). Some older README timings and descriptions refer to earlier stages of the implementation.

## Library selection

We audited six projects independently and checked actual fixtures, malformed inputs, build behavior, and source constraints. Container readers, ETF decoders, and compiler frontends solve different parts of the problem, so one ranking across all six would be misleading.

| Candidate | What the audit established | Decision |
| --- | --- | --- |
| Firefly | A coherent Rust compiler frontend was extractable. The unmodified project used obsolete Rust features and did not directly support the needed Elixir debug backend. | Extract and modernize its frontend, then supply our Elixir metadata adapter. |
| Eir | The sampled adapter discarded specs, type and opaque attributes, locations, and Core annotations. Its output was Eir IR. The focused build also failed. | Keep as a possible algorithm reference, not the primary frontend. |
| beam_file 0.3.1 | Raw extraction and byte roundtrip passed 37 BEAMs. Its typed reader rejected all 37, including modern AtU8 chunks. Short headers caused panics. | Keep our bounded chunk reader. |
| eetf 0.12.0 | Sampled modern metadata decoded, but further probes found Latin-1 errors, old-float precision loss, map hash inconsistencies, stack overflow, and compressed-stream validation gaps. | Do not adopt unchanged. |
| fasteetf 0.2.0 | Sampled metadata matched, but legacy atom support, unsafe initialization, and compression validation raised concerns. | Do not adopt without repairs and stronger proof. |
| ELP | Useful Erlang source and IDE models, but the full service used Erlang and another executable. The audit did not find a native BEAM-to-Core path. | Exclude from this frontend role. |

The extraction contains 15 Firefly crates at commit `c672f3ad5bebd3d6e9e2fe6056a154ed92fd1d07`. The Apache 2.0 LICENSE and NOTICE remain with the extracted source. A modernization patch and reproduction script record modifications. The audit established a reuse route for the sampled compiler code, not a complete license inventory of every future dependency.

The initial audit did not prove Core generation or warning parity. The later executable gates did. This distinction matters: successful byte decoding alone is insufficient evidence for a compiler replacement.

Evidence: [library audit](/Users/simedw/Documents/ChatGPT/rust-dialyzer/reports/library-audit/README.md), [extracted compiler](/Users/simedw/Documents/ChatGPT/rust-dialyzer/prototypes/firefly_frontend/compiler), and [native frontend results](/Users/simedw/Documents/ChatGPT/rust-dialyzer/prototypes/firefly_frontend/MERGED_RESULTS.md).

## Type inference and specs

The analyzer implements success typing. It computes types that describe values a function can accept and return without a definite failure. A spec is a declared contract against which inferred behavior is checked. The implementation does not treat a spec as proof that the body behaves correctly.

The type domain includes atoms, integers and ranges, floats, bitstrings, lists, tuples, maps, functions, identifiers, products, variables, opaque types, and nominal types. Types form a lattice. `join` combines possibilities, `meet` refines overlapping possibilities, and subtraction removes possibilities where the domain supports it.

Core traversal generates constraints. The solver tracks variable bindings, refines constraints, and repeats dependent work until bindings stabilize. Strongly connected components group mutually dependent functions. The ordinary default selects solver v2; it does not run both v1 and v2.

Unification is one operation within this system. It relates type variables and structures, but the entire analyzer is not a Hindley-Milner unifier. Guard refinement, built-in transfers, opaque restrictions, contract checks, widening, and recursive fixed points all affect the result.

Depth limiting keeps recursive analysis finite. Seemingly redundant work can preserve observable behavior: callback order, exception order, raw records, union flags, and warning locations. Optimizations must preserve those behaviors rather than merely return a plausible type.

Source: [type algebra](/Users/simedw/Documents/ChatGPT/rust-dialyzer/src/lattice.rs), [SCC analysis](/Users/simedw/Documents/ChatGPT/rust-dialyzer/src/constraints/analyze_scc.rs), [constraint solver](/Users/simedw/Documents/ChatGPT/rust-dialyzer/src/constraints/tree.rs), and [body analysis](/Users/simedw/Documents/ChatGPT/rust-dialyzer/src/body_flow.rs).

## Test coverage and its limits

We found an upstream Elixir Dialyzer integration suite, OTP Dialyzer semantic suites, and Dialyxir warning examples. The initial pinned inventory contains 307 OTP semantic cases, 19 Elixir integration groups, and one combined Dialyxir group. OTP also has operational and type tests that need separate treatment.

The native Elixir acceptance gate covers 21 grouped warning and type comparisons across 48 unique modules and 33 warnings. The newer full project gates cover Elixir's own 447 target modules and Phoenix's 93 target modules. Dependency facts are separate from those target cohorts.

The canonical type and solver checks include large direct differential corpora. The current sparse representation candidates pass 668,829 byte-exact lattice, raw-type, unification, atomic-constraint, tree, and SCC commands. These are stronger checks than compilation or a few matching warning counts.

There is also an earlier frozen engine checkpoint with parity on all 327 default-profile semantic groups and 2,617 ordered warning events. That checkpoint predates later integrations. It must not be described as final integrated CLI, incremental lifecycle, or every-options parity. Operational behavior, complete incremental persistence, and malformed imported metadata have separate requirements.

The full project checks compare exact warning terms and every named inferred signature. Retained-Core candidate comparisons additionally compare the entire output, including anonymous signatures and metadata, against the same frozen input. Anonymous counts differ between retained-Core and freshly lowered native cohorts because of existing frontend naming differences. Those cohorts cannot be mixed to claim parity.

A finite suite cannot prove equivalence on every possible program. Unsupported cases never count as passes, and older partial results remain historical evidence.

Evidence: [coverage inventory](/Users/simedw/Documents/ChatGPT/rust-dialyzer/docs/coverage.md), [native fixture runner](/Users/simedw/Documents/ChatGPT/rust-dialyzer/prototypes/firefly_frontend/elixir_harness/run_native.py), and [named-signature comparator](/Users/simedw/Documents/ChatGPT/rust-dialyzer/reports/real-projects/fixes/inferred-signatures/compare.py).

## Caching and benchmark scope

Dependency caching and application incremental caching are different. A cached dependency library supplies previously established facts about OTP, Elixir, or Phoenix dependencies. Application incremental reuse would avoid recomputing unchanged target modules. The current performance comparison disables application incremental reuse on both sides.

For Elixir, the comparison analyzes 447 targets, including 34 Erlang modules, with 588 OTP dependency modules. Phoenix analyzes 93 targets with a dependency cohort of 1,338 modules: 588 OTP modules, 447 Elixir modules, and 303 modules from 18 other packages.

The original tiny experiment exercises PLT creation and reuse, input and engine invalidation, corruption recovery, and changed-callee propagation. Those tests do not establish complete parity for OTP's public incremental workflow on large projects. Nonincremental analysis works now; complete incremental lifecycle parity remains separate work.

The pinned full project preparation records took about 4.529 seconds for Elixir and 5.135 seconds for Phoenix. These are preparation measurements, not recurring analysis time, and not the cost of rebuilding every dependency PLT from scratch. Timed analysis reuses those prepared inputs. The preparation files record the exact commands and outputs.

All speed results discussed here use optimized Rust release builds. Debug builds are useful for development but do not support performance claims. The native extracted frontend currently builds with Rust 1.88 and `RUSTC_BOOTSTRAP=1` for its compatibility requirements. The project records the selected toolchain and executable hashes for each experiment.

Evidence: [comparison runner](/Users/simedw/Documents/ChatGPT/rust-dialyzer/reports/real-projects/harness/run.py), [Elixir preparation](/Users/simedw/Documents/ChatGPT/rust-dialyzer/reports/real-projects/elixir/preparation.json), and [Phoenix preparation](/Users/simedw/Documents/ChatGPT/rust-dialyzer/reports/real-projects/phoenix/preparation.json).

## Where the time goes

An instrumented integrated release before the latest structural changes showed the following Elixir costs. These measurements identify budgets; the instrumentation was enabled and the values are not a clean benchmark.

| Scope | Measured time | Relationship |
| --- | ---: | --- |
| Native frontend | 4.240 s | Separate frontend scope |
| Validation | 0.894 s | Separate recorded scope |
| Compiled analysis | 48.337 s | Includes the Core pipeline and other work |
| Core pipeline | 42.417 s | Nested within compiled analysis |
| Pipeline setup | 1.469 s | Part of the Core pipeline |
| Inference | 36.184 s | Part of the Core pipeline |
| Diagnostics | 4.229 s | Part of the Core pipeline |
| Pipeline teardown | 0.535 s | Part of the Core pipeline |

Within inference, aggregate recorded SCC generation was 10.889 seconds, constraint preparation 2.842 seconds, and solving 7.705 seconds. Module refinement totaled 9.434 seconds, with 8.542 seconds in body work. These scopes come from phase records and must not all be added to the outer totals.

A separate 20-second CPU sample found 26.65% of exclusive samples in allocator functions, 6.23% in `memmove`, 2.96% in ordered-map searches, and 2.23% in variable-integer comparisons. The sample covered part of the run. Inclusive stack percentages overlap and cannot be summed.

Sparse operation timing identified substantial body-clause, meet, limit, transform, sorting, grouping, opacity, substitution, and join work. One census counted about 42.76 million recursive `is_limited` calls. The estimated own cost was about 1.7 seconds in that diagnostic run. This supports an exact depth-summary experiment, not a claim that it will remove the full slowdown.

The latest structural frontend census put Elixir synthetic source construction at about 283 ms, abstract conversion at 66 ms, Core lowering at 780 ms, and adaptation at 952 ms. Those figures show that removing the remaining synthetic source formatting alone cannot deliver the required several-fold gain.

The main remaining cost is inference and its type operations. Eliminating VM startup and executable boundaries helped, but it did not make inference cheaper automatically.

Evidence: [sampling artifacts](/Users/simedw/Documents/ChatGPT/rust-dialyzer/reports/performance/combined-product-noops/sampling-off), [body and lattice investigation](/Users/simedw/Documents/ChatGPT/rust-dialyzer/reports/performance), and [phase instrumentation](/Users/simedw/Documents/ChatGPT/rust-dialyzer/src/phase_timings.rs).

## Changes already integrated

The most useful accepted changes removed unnecessary representations or copies while keeping the algorithm's ordering.

- The native frontend and analyzer run in one executable. An earlier small Dialyxir benchmark fell from 467.20 ms with split executables to 262.48 ms merged, versus 590.98 ms for OTP. That small-fixture result does not predict full project speed.
- Typed Core replaces the ordinary JSON conversion and reparse between compiler stages.
- Call-graph construction borrows the existing validated dataflow walk rather than rebuilding it.
- Owned solver maps and suspended conjunction frames avoid unnecessary persistent-map copies while retaining rollback and fixed-point snapshots. A census fell from 7,671,669 copied map nodes to 1,205,067.
- Independent final module diagnostics run in parallel against a frozen inferred world, with ordered results and errors.
- Unification uses an exact binding-length version where bindings grow monotonically, rather than cloning the binding map to track active snapshots.
- Flat conjunctions record pending indices directly rather than allocate discarded affected-variable maps.
- Private binder operations consume state their callers already own.
- Borrowed family-presence and variable queries avoid temporary owned structures and child vectors.
- Product, nominal, and nominal-set payloads share immutable ownership. Together with related layout changes, the outer Type size fell from 72 bytes to 32 bytes.
- Binding operations avoid absent removals and equivalent writes.
- Slot rebuilds retain scalar fields and required sentinels without first cloning compound payloads that they immediately discard.

The clean Elixir guidance moved from about 66.398 seconds at the typed-frontend and owned-map stage to 61.847 seconds, then 53.175 seconds, then 51.582 seconds. The corresponding OTP measurements remain roughly 8 seconds. Each result has its own input and executable hashes; these are a history of individual calibrations, not a statistical comparison across stages.

Principles that changed the implementation were choosing the type data shape first, deleting avoidable work, isolating candidate sources and targets, and verifying the actual executable against full results. Local reversible experiments did not need repeated user approval.

## Sparse type representation and common-type reuse

The current integrated representation stores eight optional type-family payloads in one shared Slots object. Most constructed family types contain exactly one family. A census found 38,334,486 single-family constructions out of 40,230,592 in Elixir, or 95.29%. Phoenix had 5,256,965 out of 5,380,679, or 97.70%.

An initial candidate put all eight payloads behind individual Arc owners while retaining an outer Slots owner. It made heterogeneous joins cheaper, but introduced an extra allocation for ordinary single-family constructors. Constructor component speed fell to roughly 0.65 to 0.81 of the baseline in several cases. That design has not been selected.

The second candidate uses a direct single-family variant and allocates a Slots owner only for unions or raw records that require their original shape. All construction passes through one normalization boundary. Raw union flags, empty raw unions, family order, equality, and Debug output remain part of the proof.

| Operation | Baseline allocation requests | Sparse requests | Baseline requested bytes | Sparse requested bytes |
| --- | ---: | ---: | ---: | ---: |
| Nil construction | 1 | 1 | 328 | 40 |
| Any atom construction | 1 | 1 | 328 | 48 |
| Any integer construction | 1 | 1 | 328 | 80 |
| Any list construction | 4 | 4 | 704 | 128 |
| Any tuple construction | 1 | 1 | 328 | 48 |
| Any map construction | 3 | 3 | 376 | 104 |
| List joined with integer | 3 | 1 | 376 | 88 |
| Tuple joined with integer | 4 | 1 | 816 | 88 |

These are allocator requests in focused operations, not resident memory or total application allocations. Compound same-family component results are mixed; some remain several percent slower. Whole-project timing decides whether the design is worth integrating.

A related candidate reuses 32 finite common types in per-thread OnceCell tables. It covers common scalar types, true and false atoms, selected integers and bitstrings, common list shapes, and built-in intervals. Per-thread ownership avoids shared Arc refcount contention. Dynamic inputs retain their original conversion and fallback behavior.

A global shared table regressed an eight-thread component experiment. The per-thread table avoids that contention and eliminates warmed allocations for the selected constructors. Representative component results were about 5 ns for scalar constructors, compared with roughly 29 to 34 ns, and about 11 ns for an any-list constructor, compared with roughly 103 ns. Some dynamic fallbacks regressed by about 3 to 4 percent. These are component observations, not full project speed claims.

The combined sparse and per-thread candidate passes 236 library tests, all 668,829 differential commands, and complete retained Elixir and Phoenix output equality. It has not yet passed a fresh native full-project gate or a clean full-project timing comparison. Production is unchanged.

Evidence: [sparse representation review](/Users/simedw/Documents/ChatGPT/rust-dialyzer/reports/performance/family-payload-review/review.md), [sparse proof](/Users/simedw/Documents/ChatGPT/rust-dialyzer/reports/performance/family-payload-ownership/single-proof/verification.json), [allocation measurements](/Users/simedw/Documents/ChatGPT/rust-dialyzer/reports/performance/family-payload-ownership/single-component-allocations.json), and [combined proof manifest](/Users/simedw/Documents/ChatGPT/rust-dialyzer/reports/performance/builtin-constructors/single-proof/manifest.json).

## Parallel inference needs ordered certification

Independent final diagnostics are already parallel. Function bodies inside recursive SCC solving are different: earlier publications can affect later bodies.

The isolated inner-SCC prototype evaluates bodies against a persistent snapshot. It records reads in the closed function and recursive-label domain. The coordinator consumes results in original order and checks whether an earlier publication changed a value that a later body read. A conflict causes that body to rerun against the reached current map before publication.

The prototype preserves reached result and error order, uses fresh solve caches, and leaves arbitrary providers sequential by default. Only the immutable Registry opts into parallel execution. Speculative panic hooks still occur at speculative time, so arbitrary hook side effects are not serially equivalent. This limitation is recorded rather than hidden.

The latest rebase passes 238 library tests, 15,203 raw solver cases, 401 generated SCC groups, 216 original event rows, and complete retained Elixir and Phoenix equality. A four-worker diagnostic run evaluated 13,589 Elixir bodies and reran 1,665 after conflicts. Initial body compute was about 7.533 seconds, with 0.887 seconds in reruns and 4.630 seconds of eligible-group wall time.

Phoenix had only 18 eligible bodies. Inner-SCC concurrency is therefore unlikely to be a large Phoenix improvement by itself. Earlier isolated SCC speedups did not produce a clear whole-project gain. Quiet same-binary Elixir trials with one, four, and eight workers are in progress at this checkpoint; other analyzer and build work is held. The first one-worker trial matched the entire output but took 101.261 seconds, far above earlier guidance. Its timing wrapper then failed because the sandbox blocks a sysctl query. That sample is preserved, the runner is switching to direct child resource accounting, and no speed conclusion has been drawn. Holding our jobs does not guarantee stable machine performance.

Naive parallel Firefly lowering is also unproven. Its global symbol interner and symbol ordering can make allocation order observable. Source feasibility alone does not establish deterministic compiler parity.

Evidence: [parallel design](/Users/simedw/Documents/ChatGPT/rust-dialyzer/reports/performance/parallel-inference/inner-scc-structural/design.md) and [rebase proof](/Users/simedw/Documents/ChatGPT/rust-dialyzer/reports/performance/parallel-inference/inner-scc-structural/rebase.json).

## Other experiments and rejected shortcuts

A warning query built a complete sorted set of native return types only to ask whether two distinct returns existed. Its replacement skips structurally equal types, projects lazily, and stops at the first distinct native record. Structural inequality is not enough, because different supported raw types can project to the same native record.

That isolated change passes 232 library tests, 559,682 focused raw shape batches, and complete retained project comparisons. The focused query component improved by about 2.43 times. The patch is ready, but it has not been integrated or assigned a full-project gain. A combined source with sparse types, common-type reuse, and this query is prepared but not built.

SafeUnderapprox evaluation repeats work: Elixir had 1,232,066 evaluations for 103,393 unique keys. However, the measured pure-work budget was below one second. Caching would also need exact callback and error behavior. The experiment is deferred rather than adding a large cache for a small demonstrated budget.

Dependency decoding had about 180,000 Elixir lookups but only about 86 ms of measured decode work. Alias source copying and atom-table copying also had small budgets. Broad caches were deferred.

Persistent BindingMap cloning remains frequent, but only a small fraction of clones are followed by writes in the investigated dataflow cohort. Binder failure paths also require real rollback snapshots. Replacing every map with a different data structure has not been justified by the current evidence.

A jemalloc injection preserved full results but showed no useful clean whole-project improvement: Elixir measured about 53.663 seconds versus 53.175 seconds for the default, and Phoenix 5.232 seconds versus 5.212 seconds. It remains outside production. Thin LTO was also measured earlier; it did not close the gap.

Generic success caches, ignored errors, warning suppression, precomputed target facts, and application incremental reuse would not establish the requested nonincremental result. No such shortcut counts toward the goal.

Evidence: [warning query proof](/Users/simedw/Documents/ChatGPT/rust-dialyzer/reports/performance/warning-distinct/verification.json), [query boundary](/Users/simedw/Documents/ChatGPT/rust-dialyzer/reports/performance/warning-distinct/boundary.md), and [under-approximation census](/Users/simedw/Documents/ChatGPT/rust-dialyzer/reports/performance/underapprox-reuse/census-summary.json).

## Measurement and verification lessons

Concurrent builds, replays, sampling, and analyses invalidate clean speed comparisons. Large observed times from overlapping candidate gates remain correctness evidence only. Component improvements need a quiet whole-project measurement before selection.

Profiling must be disabled for timed comparisons. A timing-enabled sample spent substantial time writing profiling output; that output is not normal analyzer work. CPU percentages from a partial sample also cannot describe every stage of the invocation.

Every frozen source needs a distinct Cargo target directory. Reusing one target across same-named packages at different paths can reuse an unintended executable. Hashing the binary and source inputs is part of the proof, not optional bookkeeping.

Candidate sources remain immutable while jobs run. Each build and analyzer process must finish, and every owned handle must be reaped, before the next clean timing window. A failed patch or interrupted build cannot certify a candidate.

A previous query/rebuild migration had a test-context patch conflict. The initial partial source and interrupted builds were discarded. The corrected source-v2 received fresh library, native, and full project verification. This is why a successful command from an earlier source snapshot is insufficient.

Full comparison reports can embed megabytes of output. Small parsed summaries make status review safer; complete artifacts remain available for audit. Copying source-only snapshots also avoids recursively following report symlinks.

## Work remaining

At this checkpoint, the integrated release remains the 230-test structural version. The sparse representation, common-type table, lazy warning query, and inner-SCC concurrency are isolated candidates.

The immediate work is to finish the quiet concurrency trials, complete the combined candidate's native verification, and run clean full-project timing. Further work must follow measured inference costs. An exact depth summary for immutable compound types is under source-only design review; no implementation or performance claim exists yet. The [depth design](/Users/simedw/Documents/ChatGPT/rust-dialyzer/reports/performance/immutable-depth-metadata/design.md) records signed-depth, raw-record, equality, and invalidation requirements.

The performance goal remains active. The Markdown checkpoint records what is known; it does not mark speed parity, universal Dialyzer compatibility, or incremental workflow parity as complete.
