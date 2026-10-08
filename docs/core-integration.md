# Core analysis integration

The acceptance target is the complete upstream semantic corpus and all 150 operational cases. Passing a helper gate establishes only its named scope. The canonical compiled-analysis entry, raw declaration and dependency owners, global declaration journal, and classic persistence writer are integrated. An earlier frozen compiled-analysis snapshot matches all 327 default-profile public warning groups. Typed diagnostic modes, lazy diagnostic sessions, declaration intake chronology, and the proper-header classic checked planner are integrated. Full acceptance on the final source, lazy imported dependency errors, public cache repair, incremental persistence, and migration of the CLI from the Forms engine remain open.

The compiler owns Core, annotations, original type forms and primitive runtime atom identities. Rust owns labels, dependency facts, prepared declarations, constraint graphs, inferred signatures, diagnostics and persisted analysis state. Oracle results are compared after analysis and never supplied as target facts.

## Shared phase inputs

One `DataflowNodes` snapshot supplies Core, labels and literal types to constraint generation, patterns, guards and body traversal. `Program` and `ClosureDependencies` supply function identities and call targets. `SourceCallGraph` supplies ordered incoming edges, escaping functions and SCC membership. Sequential ETS and flat/HAMT traversal derive their order from same-VM atom identities in the compiler snapshot. They receive no native SCCs, adjacency order or inferred types. The immutable source graph has one owner; the mutable phase scheduler owns only active, reaching and condensation graph state. A private-helper fixture may explicitly omit the graph; production cannot silently substitute an empty graph.

`PreparedDefinitions` retains target records, candidate contracts, callbacks, original module tables and reached Forms. Its Forms arena is not an inventory of every library declaration: unused imported bodies remain deferred. Dependency summaries exclude target modules. Target raw signatures and active contracts belong to the analysis phase.

## Success and refinement feature set

Constraint generation writes the canonical constraint arena. Construction, normalization, preparation and solving use one graph representation. Each SCC receives its prior signatures and only the contracts active at that phase. Generated results are restricted to functions belonging to the SCC before validation and persistence. Validated contracts become active between phases.

Module refinement initializes `FunctionState` from source facts, evaluates functions through the FIFO worklist, and publishes changed types using the shared native fixpoint rules. SCC analysis and module refinement alternate until both signatures and contract activation converge. Signature widening and persistence limits remain source-defined.

## Body and diagnostic feature set

The body continuation interpreter owns traversal order and the mutable function state. Pattern binding, safe underapproximation and guard evaluation share its type/substitution map. Case branches start from one scoped join, retain residual argument types between clauses, and join only successful body maps. Pattern failures, guard failures and opaque-union warnings preserve native forced-warning provenance.

Calls consume the existing pure conjunction and alternative accumulator. Raw success signatures, active contract domains, BIF domains and current local returns remain separate. Local argument forwarding uses the original argument types and escaping status. Opacity warnings are added before the candidate-warning snapshot; consensus removes only subsequently added candidate warnings. An external apply target activates function-operator fallback after all candidate side effects.

Guard, body, call and clause warnings project to the same raw warning representation and presentation context. Contract strings use the shared original-form printer. Warning collection runs after convergence. Invalid-contract warnings consume the converged signatures. Each module then concatenates missing-function contracts, body/unused/no-return warnings and callback findings. Callback checks consume converged signatures; warning-pass signatures enter the documentation PLT separately. Contract-range origin rewriting precedes suppression, native sorting and rendering.

`CorePhases` projects SCC bindings only when the solver actually returned them; it never fills omitted nested bindings with `any`. Module refinement reads canonical `FunctionState::function_types`, including Source cleanup of uncalled functions. Letrec maps belong to their module. Original and folded pattern arenas stay separate views over the same compiler labels, and literal folding retains canonical proper/improper list structure.

## Production migration and acceptance

`core_pipeline::analyze` borrows the shared phase inputs and prepared callback state. It creates cold signatures and candidate contracts, runs the canonical Source scheduler and Core analyzer, then collects contract, module-body and callback warnings. It returns converged state, separate documentation signatures and raw warnings. Graph cleanup precedes this call; warning postprocessing and public output follow it.

Two integration shapes were considered: constructing phase/runtime state in every CLI and corpus caller, or one borrowed pipeline entry over existing owners. The pipeline entry keeps convergence and diagnostic publication together without duplicating Core, metadata or library state. `CoreAnalyzer::module_previous` reuses the original function collector for both refinement and diagnostics.

Runtime capture identities belong to one reserved analysis scope. Base tables and derived active tables use distinct identity namespaces. These identities support equality, never ordering of distinct native references. At inference entry, finalized record/export tables remain live; only temporary preparation tables are clean. Native `move_data` extracts those live tables after analysis.

Warm PLT ownership needs a mode-specific audit before CLI migration. Classic changed-file handling calls `delete_module` for changed and removed modules. The analysis worker itself deletes current callgraph keys with `delete_list`, which touches signatures and contracts. Callback insertion replaces only modules with newly produced callback rows. The cold pipeline gate therefore does not prove removal of a last callback or retention of a removed export when PLT checks are bypassed. Actual public lifecycle probes now reproduce the difference: no-check retains the last old callback and a removed export summary, while checked analysis purges both. A new nonempty callback group replaces the old group. The imported view must preserve this chronology: whole-module purge, cleanup over prior membership, current named-key deletion, then inference.

Move the CLI, cache and corpus runner to the composed Core analyzer together, then remove the old inference path and its provider interfaces. Persist complete Rust-owned signatures, contracts, warnings and dependencies atomically. Run reload, unchanged-input reuse, changed-input invalidation and distinct input/output path sequences through that interface.

Acceptance compares every persisted signature and complete diagnostic for all 327 semantic groups, all option profiles and the original operational checks. Source goldens and native failures remain visible. Component gates are rerun on the integrated build; historical evidence cannot stand in for a fresh end-to-end result. Performance work starts after this gate passes.

## Integrated boundary evidence

Fresh Root replays after integration match450 UTF SCC states,3560 generation graphs with3656 captures,456 graph-cleanup cases,474 arithmetic overflow plus478 shift cases,and11620 public-warning adapter batches. These stage checks preserve original raw events and errors; they do not establish complete CLI or cache parity. All155 library tests and strict all-target Clippy pass. Compiler no-spec preparation now passes288 diagnostic and27 SCC/source-state comparisons. Four raw declaration boundaries remain pending the coherent metadata migration. Module-dependency publication matches1149 worlds from327 groups and56 original suite fixtures, plus18 constraints and7 behaviour cases.


## Compiled production entry

`compiled_analysis::analyze` owns phase composition over shared components. It applies explicit whole-module prior invalidation, graph cleanup using original membership, remote declaration preparation, current named-key deletion, canonical callback insertion, composed inference, dependency publication and public warning publication. Its output owns the prior store and prepared definitions alongside completed analysis. The CLI migration still waits for complete physical persistence and raw callback reload support.

Fresh Root acceptance covers24 actual original public API runs over retained callbacks and24 runs over unknown calls, unknown types and missing exported calls. Each checks complete ordered public warning terms under target contract modes, warning profiles and line/column output. These comparisons prove unchecked prior analysis only. They do not claim checked cache updates or full corpus acceptance. `scripts/analysis_probes/replay_public_analysis.py` verifies frozen oracle provenance, input hashes, candidate source and binary before and after execution.

The opaque scanner group now matches134 inferred signatures,134 documentation signatures and47 warnings. The shared solver joins untouched cached maps in ascending branch order followed by newly evaluated maps in reverse order, as the original v2 implementation does. Native nominal `any` checks also control unmatched-return suppression. All2006 original constraint cases and retained-prior/cold integration fixtures still pass after these corrections.

Capture interning reuses existing canonical node IDs when constructing captured state arenas. This prevents repeated shared environment subgraphs from allocating duplicate nodes. Fresh450 SCC,3560 generation and3656 capture comparisons remain exact. The immutable full corpus replay has reached97 groups and is processing the large `otp/opaque/recrec` group; this is not a completed acceptance result.
