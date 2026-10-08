# Proof of concept tasks

## Full compatibility work

- [x] Read decision-affecting principles: foundational thinking, model the domain, redesign from first principles, type-system discipline, boundary discipline, build the lever, outcome-oriented execution, sequence verifiable units, minimize reader load, never block on the human, encode lessons in structure, subtract before adding, prove it works, unslop.
- [x] Ground: pinned OTP implementation and complete upstream corpus; 2/327 semantic cases currently pass, 46 warning categories observed.
- [x] Architecture: compare compiler boundaries, select one semantic representation, define domain and inference ownership before implementation.
- [x] Build a multi-label feature registry covering all cases and dependencies; do not group by the first unsupported parser construct.
- [x] Lossless BEAM/Core boundary: retain all metadata and annotations; verify exact OTP compiler-profile equality and roundtrip on all 327 cases.
- [x] Semantic program index: resolve scopes, function/type dependencies and diagnostic origins; complete source/Core path checks on all 327 inputs.
- [ ] Type algebra: scalar sets/ranges, identifiers, bitstrings, lists/termination, tagged tuple families, products, functions, maps, nominal/opaque identity; differential tests against OTP and algebraic properties.
- [ ] Intraprocedural inference: full normalized expression/pattern vocabulary, branch and guard constraints, subtraction, exception flow, widening.
- [ ] Interprocedural inference: call graph/SCCs, higher-order escaping functions, argument/return constraints, complete BIF transfer functions and dependency summaries.
- [ ] Contracts and nominal types: recursive/parameterized aliases, remote resolution, bounded and overloaded specs, opacity, records, callbacks and behaviours.
- [ ] Diagnostics: all native warning profiles, suppression attributes, stable identities and locations, source goldens and compiler-generated code treatment.
- [ ] Lifecycle: PLT/dependency compatibility, incremental invalidation, CLI/options, persisted schema/versioning and operational test equivalents.
  - [x] Classic PLT interchange: import shared term arenas, verify source fingerprints, exclude every target-module summary/contract, reject malformed input.
  - [ ] Persist and invalidate Rust-owned summaries, contracts, warnings and dependency state.
- [ ] Full acceptance: every semantic case passes independently, original operational/type/consistency checks have executable equivalents, six lattice properties pass with published seeds; resolve the upstream golden line drift without hiding it.
- [ ] Optimize only after acceptance; compare identical warnings, signatures, dependency state and changed-input behaviour, then measure cold and warm runs.

Current integration checkpoint:

- [x] Keep success signatures, raw contract constraints, and BIF constraints independent through call transfer and failure evidence.
- [x] Verify the whole 327-case engine with imported library contracts: 317 inferred, 167 exact persisted-signature matches, 109 warning category/location matches. This is a partial checkpoint; message and option equivalence remain unverified.
- [x] Integrate the coherent type API layer: constructors, predicates, accessors, runtime literals, native record ordering, and list/map wrappers share one Type representation.
- [x] Rerun all 653,626 type outcomes, three original upstream type tests, and 30,000 original properties after main-project integration.
- [x] Verify module/function warning attributes against all 327 real inputs and 893 synthetic combinations in an isolated candidate.
- [x] Verify return classification against all 13,748 native functions; connect it to diagnostic policy after preserving native forced-warning provenance.
- [ ] Integrate and independently rerun compiler-form resolution, local contract compilation, imported contract transfer, and local validation as separate gates of the contracts feature set.
- [x] Integrate coherent closure-target/escape/call-graph migration and independently rerun all 327 inputs: 13,748 functions, 27,131 call-target sets and 594 recursive bindings.

Root integrates sequentially. Read-only source readers and the feature scanner use separate scratch paths. No external publication is requested. Completing the inventory or one feature set does not complete full parity.

- [x] Read poteto-mode principles and decision-affecting leaf skills: foundational thinking, model the domain, build the lever, prove it works, sequence verifiable units, laziness protocol, unslop.
- [x] Ground with how: empty repository; compiler debug backend provides expanded Erlang forms; OTP Dialyzer supplies the independent oracle and PLT.
- [x] Architect ground and sketch: compare direct Core interpreter with expanded Erlang forms. Choose forms for this bounded probe because they retain clauses, guards, specs, and locations with fewer lowering rules. Keep compiler-owned expansion.
- [x] Agree: proceed within authorized local scope; no external publication.
- [x] Blocking first steps: confirm installed compiler/analyzer toolchains and choose serializable compiler boundary.
- [x] Independent workstreams: fixture/oracle delegate owns examples and OTP scripts; root owns Rust engine and comparison driver.
- [x] Shared mutable state: delegate uses separate temporary directory and copies only owned files; root owns Cargo and docs.
- [x] Smallest safe decomposition: one Elixir owner and one Rust owner; integration remains serial.
- [x] Implement fixture and oracle; verify cold PLT build and unchanged PLT reuse.
- [x] Implement Rust domain and global inference; compare normalized warnings and absence of false positives.
- [x] Exercise changed program and invalidation, including a repaired fixture and unsupported syntax rejection.
- [x] Measure parity first, then benchmark and optimize one measured cost.
- [x] Review actual artifacts, comments, formatting, tests, and documentation.
- [x] Scrap phase: revisit shape only if repeated implementation friction contradicts it.
- [x] Opening PR skipped: user requested local proof of concept, no external-state authority.
- [x] Ordered commits skipped: leave reviewable local changes without changing the user's git history.

## Performance checkpoint

- [x] Capture baseline with the full-sweep solver: evaluation counts and repeated release timings.
- [x] Ground hypothesis in real graph: unchanged functions dominate scans on a long dependency chain.
- [x] Compare worklist architecture with sweep; retain both behind a solver option.
- [x] Verify exact warning and inferred-signature equality before accepting measurements.
- [x] Capture post-change repeated timings and function visits in `.cache/report.json`.
- [x] PR measurement and opening skipped: local experiment only; no publication requested.
- [x] Technical-writing and unslop applied to README and design record.
- [x] No-comments reviewed inline: no comment-sicko agent is available; no application comments added.

## Upstream compatibility corpus

- [x] Reuse decision-affecting principles: build the lever, prove it works, model the domain, foundational thinking, sequence verifiable units, laziness protocol, separate shared state, unslop.
- [x] Ground OTP tests and Elixir integration tests against pinned source revisions.
- [x] Architect corpus registry and status model before changing the runner.
- [x] Blocking first steps: acquire source and inspect suite harness/options/expected results.
- [x] Independent workstreams: OTP inventory and Elixir inventory read independently; root owns corpus runner.
- [x] Shared mutable state: read-only delegates use disjoint scratch checkouts; root is sole writer of corpus files.
- [x] Smallest safe decomposition: delegate discovery, serialize harness integration after suite contracts are known.
- [x] Inventory all upstream suites and classify case eligibility without hiding exclusions.
- [x] Run independent OTP oracle and Rust across suitable cases; report parity, mismatch, unsupported, and harness failures separately.
- [x] Build a coverage map and ordered compatibility backlog; add Elixir-specific dimensions and property-testing gaps.
- [x] Verify reproducible discovery/reporting, review comments and docs, preserve concrete artifacts.
- [x] External publication skipped: local work only.

- [x] Fix Root Causes applied to adapter failures: compile binary output under declared module names, encode Unicode warning chardata correctly, and emit JSON null for absent metadata.
- [x] Keep actual engine gaps unchanged and visible: two callback mismatches, unsupported types/control flow/options, and one native golden location drift.
- [x] Verify strict gate accepts a supported selection and rejects the complete incomplete corpus. Preserve snapshot and local raw records.
- [x] Lattice properties and operational tests inventoried as not ported; no full-language equivalence claimed.

## Incremental state feature set

- [x] Ground native IPLT checks, dependency repair and warning selection in the pinned source.
- [x] Compare whole-Engine serialization with module-indexed snapshots; select stable module identities and separate warning selection.
- [x] Prove pure dependency expansion and repair against actual native private functions: 5,002 graph cases, including 10,000-module chains.
- [x] Prove native rebuild/reuse/partial-analysis decisions for every native check result: 5,002 cases.
- [x] Prove parsed snapshot version, implementation/module fingerprints and warning settings select native check results: 7,307 cases.
- [x] Prove stored warning-map and dependency merging plus requested-module selection: 7,460 cases. All-module selection membership is checked; native final sorting remains with diagnostics.
- [ ] Connect compiler-produced fingerprints and on-disk snapshot decoding to those checks.
- [ ] Persist complete module results atomically; prove reload, retry and distinct input/output paths.
- [ ] Run the original cache lifecycle and operational sequences through the Rust interface.

## Success-typing constraint feature set

- [x] Integrate shared atomic and scoped executor with the canonical type domain.
- [x] Independently rerun 60,768 native atomic, scoped, reference, SCC, scalar/structural-derived and dynamic-worklist outcomes, including error payloads and branch-cache invalidation.
- [x] Integrate function references and native recursive SCC executor.
- [x] Add source-defined scalar/boolean/nominal derived operations through one typed arena and shared BIF provider.
- [x] Prove cons, binary segment and map derived operations against 10,150 native outcomes.
- [ ] Complete phase record snapshots and ordered captured-value normalization.
- [ ] Generate constraints for every Core expression and migrate the production success phase.
- [ ] Alternate SCC success analysis and module refinement; activate validated target contracts between phases.
- [ ] Compare every persisted signature and complete diagnostic across all 327 corpus inputs.

## Diagnostic production feature set

- [x] Prove canonical call conjunction and candidate consensus: 20,588 native outcomes.
- [x] Prove reachability, no-return, comparison, suppression and warning-list emission: 52,665 native outcomes.
- [ ] Migrate Core warning collection after production success/refinement phases are native.
- [ ] Reproduce raw native warning payloads and complete rendering options.

## Callback conformance feature set

- [x] Port shared required/optional, export, inferred type and declared spec checks.
- [x] Compare native private check_all_callbacks over 9,704 cases, including None/Unit, opacity, structured types and spec mismatch ordering.
- [ ] Connect callback definitions, behaviour attributes and precise warning origins to the production warning collector.

## Native dataflow environment and labels

- [x] Prove shared native-equivalent label allocation on all 327 groups: 1,045,056 node occurrences and 748,040 labeled occurrences.
- [x] Integrate typed CoreLabel identities; keep node-index ordering out of native joins.
- [x] Prove persistent type/substitution storage and nested changed-key joins across 5,002 native operation traces.
- [ ] Prove invalid join-scope assertions and deep map lifetimes.
- [x] Integrate the shared node/literal adapter, fresh-pattern rules and typed forward/reverse pattern binder.
- [x] Independently verify 133,138 corpus binder outcomes plus16,128 synthetic/40 context/2 allocation-order cases, and complete Core printing for571 modules plus504 synthetic/long-string cases.
- [ ] Reverify Core producer proof after adding document allocation budgets.
- [ ] Complete malformed private Core assertions, arbitrary layout wrapping and output expansion boundaries.

## Warning rendering

- [x] Integrate all 56 source-defined message clauses and 17 helper clauses in a closed Rust expression evaluator.
- [x] Independently reproduce 15,592 captured warning/options and 1,540 source-derived strings, plus 623 native ordering cases.
- [x] Prove the shared flat raw-term comparator: 31,814 native outcomes, including 50,000-deep literals and 10,000-level shared DAGs.
- [x] Integrate full raw warning ordering and tested deep input boundary.
- [x] Implement canonical Erlang syntax/parser/layout for all captured default rendering, with 984 helper, 1,093 document and 363 additional syntax/error outcomes.
- [ ] Produce complete raw warning terms from Rust Core analysis; rendering captured native terms does not verify warning production.

## Form and context cache foundation

- [x] Integrate atom/integer variable identities through shared Forms, declarations, Resolver and contracts.
- [x] Preserve original module tables in Arc-backed cache handles across edited source dictionaries.
- [x] Independently reproduce 48,803 metadata outcomes, 11,000 exact native value/cache-record outcomes, and 60 exported-membership/context outcomes.
- [ ] Complete exact resolver exception payloads, remote policy contexts, Site filenames and large recursive form cases.

## Shared literal and persistence boundaries

- [x] Share native exact-key map normalization across term order, literal types and ETF transport.
- [x] Prove 31,800 finite literal conversions; reject deep/exponential type ownership before allocation.
- [x] Prove 15,911 native ETF decode/encode outcomes with compressed and historical encodings.
- [x] Prove actual BEAM module fingerprints for all 327 groups and 571 modules.
- [x] Preserve native integer bitstring constructor records separately from canonical inferred types; prove 10,369 constructor outcomes.
- [x] Independently freeze 824 classic/IPLT file transport outcomes, including two actual native table reloads and unchanged input snapshots.
- [x] Exercise atomic replacement with concurrent readers and existing symlink/permission preservation.
- [ ] Connect fingerprint checks, summary ownership, warning/dependency repair and file commit to the complete analysis pipeline.

- [x] Prove bounded native constraint construction normalization over 2,007 exact graph/error outcomes and closure ordering over 6,561 pairs plus 64 factories.
- [x] Independently verify 1,001 native prepared catalogues and 2,000 complete constructor→prepare→solve exports/errors, retaining masks and reference-last order.

- [x] Prove shared function-arity/residual operations over 3,018 outcomes and shared contract callbacks over 1,000 outcomes; all twelve constraint gates now pass 60,768 outcomes.
- [x] Preserve both native map orders in Core v5, validate iterator permutations, and reproduce 32 compiled large-map attribute fingerprints alongside 571 corpus modules.

- [x] Prove native SCC equality, module subtype refinement and depth-five persisted signature comparisons across 5,000 native outcomes through one shared signature API.

- [x] Integrate lazy Forms metadata and structured errors/events; reproduce 26 exact context/record outcomes and 7,253 io_lib iolists with captured VM map order.
- [ ] Complete malformed cache-hit/budget short-circuits in the shared Forms parser and lazy record-field loading.

- [x] Prove contract-range origin rewriting, cross-file handling, same-origin contract-warning filtering and final partition order over 5,000 native warning batches.

- [x] Integrate flat captured values and validate 26,462 native comparisons, retaining explicit unsupported ordering for distinct opaque handles; ordinary31,814 and source closure6,561 regression gates pass.

- [x] Reproduce the three ewgi golden-location differences causally in temporary copies; preserve the pinned original golden failure and semantic oracle.
- [x] Reverify document allocation limits against the complete Core producer corpus and all504 synthetic/long-string outputs.
- [x] Integrate deferred raw Forms and the shared native operand boundary; independently prove572 cache/budget/record contexts and16,210 raw outcomes, retaining2,168 unsupported cases and2 native timeouts.

- [x] Integrate one canonical function state, captured environments, FIFO worklist and pinned dictionary order; verify9,000 native scenarios,72,891 transitions and all13,748 functions in571 corpus modules.
- [x] Extend the shared raw operand continuation across scalar, function, product and list families;106,332 exact native outcomes,1,662 explicit unsupported cases and2 native timeouts.
- [x] Preserve per-module dependency insertion order and reproduce ordered callgraph edges, escaping and incoming callers across all327 cases; native SCC traversal order remains open.
- [ ] Complete original-source Core constraint generation with one shared arena/solver: binary segments, apply targets, clauses/guards/residuals, maps and exceptions.

- [x] Integrate structural Core constraint generation into the shared arena/preparer/solver:1,504 exact original-BEAM graphs and exported success bindings,3,082 exact source closure captures; keep unsupported control constructs explicit.
- [x] Derive shared scheduler SCC partitions from the source graph;11,440 partitions match the original native condensation across all327 cases.


## Coherent Core integration

- [x] Preserve compiler-owned contract variable-map provenance: 216 traversal checks across four fresh VMs, including 32/33 entries.
- [x] Verify the version-6 compiler boundary independently on all 327 groups and 571 modules without overwriting running proof inputs.
- [x] Extend shared constraint generation through normal-mode try/catch and documented primops: 3,560 exact graphs and 3,656 native closure captures.
- [x] Verify shared underapproximation against 2,560 synthetic clauses and 59,189 corpus clauses.
- [x] Verify module function initialization with Rust-derived graph facts for all 327 cases.
- [x] Verify shared literal folding: 75,391 corpus roots and 1,272 synthetic producer outcomes.
- [x] Verify prepared declarations in Root: 327 groups, 1,849 records, 1,486 contract/callback captures and final tables.
- [x] Verify 26 reached preparation boundaries and 90 exact default-format outcomes.
- [x] Verify imported defining-module scopes, deferred record/header parsing and reached malformed metadata across 60 native boundaries; arbitrary VM-only metadata remains explicit.
- [x] Verify clauses, guards, maps and dynamic apply constraints using shared pattern identities and phase-owned state: 4,520 complete clause graphs and 3,480 phase fixtures.
- [x] Verify shared guard control across 413,592 outcomes, including guard-internal cases and 103,845 exact raw warning logs.
- [ ] Complete record-sensitive raw constraint captures and whole-corpus guard preparation.
- [x] Verify shared body traversal across 247,532 native outcomes: structural expressions, maps, binaries, cases, records, nested calls and two-pass module-loop fixtures; compare complete maps, function state and raw warnings.
- [x] Independently verify original analyze_scc/7 on 1,512 cases, including 252 empty-SCC errors and 1,260 exact raw states.
- [x] Verify shared closing function-warning collection on 12,000 cases and 9,720 raw events, including 6,000 preceding body-warning traversals.
- [x] Verify all 3,314 imported raw contract/callback records against the actual baseline PLT; construct one shared call world for generation and body traversal.
- [x] Separate contract validation from commit with an opaque validated token, preserving validation→signature persistence→contract activation.
- [x] Refresh integrated body gates with anonymous home metadata and infix guard warning coverage: 80,142 additional infix cases.
- [x] Integrate exact Source callgraph finalize/reset/module postorder lifecycle.
- [ ] Complete module body/guard analysis and guard argument production in the shared warning state.
- [ ] Integrate SCC/module convergence, contract activation and production Core diagnostics; migrate all CLI/cache callers then delete the old flow.
- [ ] Run complete semantic, warning-option, persistence, invalidation and operational acceptance before performance work.

- [x] Replay all 327 guard corpus groups with independent Root prepared metadata: 59,189 roots and 355,134 exact outcomes.
- [x] Verify outer SCC/module loop ordering over 216 declared-output cases; actual phase inference remains a separate gate.
- [x] Integrate native v1/v2 dependency-finalization selection into one canonical generator and v2 solver.
- [x] Integrate contract/callback collectors and independently replay 3,198 complete original warning lists and 1,886 contract acceptance sets.
- [x] Integrate Source internal hashing, sequential ETS history and flat/HAMT key traversal as primitive runtime-order modules; full scheduler remains open.
- [x] Project complete prepared declarations and final inferred signatures into the shared contract/callback diagnostic pass with native ordering.
- [x] Verify actual module phase inference and warning traversal on571 modules in all327 groups with absent target priors:all types and1974 ordered raw events match.
- [ ] Extend whole-corpus composed acceptance to active target contracts and preceding-phase profiles.

- [x] Independently verify 60 complete compiler modules across five prior signature profiles: native inferred types, warning-pass types and 136 ordered raw warning events.
- [x] Expose warning-pass signatures and raw warnings from one shared `module_diagnostics` traversal; delete the redundant warnings-only API.
- [x] Export same-VM atom identities with compiler Core and verify parsing without native inferred facts.
- [x] Replay integrated runtime primitive order: 1,491 hashes, 1,120 ETS histories and 156 flat/HAMT maps.
- [x] Reverify folded lists and folded call formatting after shared ownership corrections: 75,391 corpus roots, 1,272 synthetic outcomes and 16,128 shared Binder outcomes.

- [x] Integrate one canonical adjacency owner, Source scheduler, canonical SCC type and direct PhaseSchedule implementation; fresh Root evidence covers 3,970 lifecycle operations, 90 graph histories and 347 preserved helper cases.
- [x] Integrate diagnostic metadata adapter, explicit callback preparation state and original classic-PLT callback-list provenance; fresh Root 144-case gate matches 15,067 raw diagnostic events.
- [x] Fix shared recursive numeric widening used by list key comparisons; fresh Root replay matches 4,263 original operation outcomes and the complete lists_key_bug module.
- [x] Replace arbitrary finite shift budget with pinned VM system-limit/catch semantics; fresh Root replay matches 478 arithmetic cases and five complete limit module profiles.
- [x] Integrate graph-cleanup missing/unknown call diagnostics over original PLT membership and shared compiler callsite origins;fresh Root456 cases exact.
- [x] Trace inference and warning publication ownership; compare per-caller assembly with a borrowed pipeline entry and choose the latter.
- [x] Add one composed cold Core pipeline and reuse the canonical module prior collector for diagnostics.
- [x] Verify composed cold inference against actual original scheduling and phases on six fixtures:24 converged signatures,24 documentation signatures and5 ordered warning events.
- [ ] Extend composed inference acceptance to the complete corpus; integrate raw warning postprocessing and public output.
- [ ] Continue acceptance after component checkpoints; no permission or external-state blocker exists.
- [x] Exercise the real composed cold path on a compiler-exported local-call module; Native comparison remains pending.
- [x] Integrate deferred UTF segment-unit validation;fresh Root450 actual SCC cases,3560 generation cases and3656 captures exact.

- [x] Integrate canonical warning publication;fresh Root11620 batches and2688 exact errors pass.
- [x] Integrate complete finite arithmetic overflow and uncaught range-shift SystemLimit boundary;fresh Root474 overflow and478 shift cases pass.
- [x] Integrate compiler-only cleanup state and consumed prior-PLT lifecycle; fresh Root proves three retained-prior runs, three table projections and six cold runs.

- [x] Reverify compiler target-contract mode in Root:288 diagnostic rows and27 SCC/source-state rows exact;20 malformed boundaries retain4 explicit raw metadata gaps.
- [x] Complete raw declaration identity and whole-list optional callback parsing as one metadata-owner migration; fresh Root replay of4,274 original cases passes. Duplicate captured errors and nonatom dependency publication remain separate acceptance boundaries.
- [x] Reverify dependency publication against327 groups,56 original suite fixtures and reached boundary cases in Root.
- [x] Prove current composed opaque scanner convergence:134 inferred signatures,134 documentation signatures and47 ordered warnings exact after shared disjunction join correction.

- [x] Fresh Root dependency publication:327 upstream+56 suite fixtures across1149 worlds,18 constraints,7 exact behaviours and2 explicit compiler boundaries.
- [x] Fix composed guard diagnostic phase ordering;verify otp/indent/simple original whole-module result.
- [ ] Diagnose large SCC capture memory failure;all327 composed acceptance remains incomplete (87/88 completed groups exact before SIGKILL).

- [x] Verify generated record-guard annotation ordering against actual otp/indent/simple composed Native result:196 inferred+196 Doc signatures and171 raw warning events exact.
- [x] Integrate shared capture node interning; fresh Root450 SCC,3560 generation,3656 capture comparisons and opaque scanner whole-group pass.

- [x] Independently reverify shared disjunction map join order against all2006 original solver cases after Root integration.
- [x] Add canonical compiled analysis entry including cleanup, prior lifecycle, preparation, inference, dependencies and public warnings; first24 prior-callback and24 unknown/missing-call public API comparisons exact.
- [ ] Finish raw callback projection and library reload schema with classic physical persistence.
- [ ] Diagnose composed otp/opaque/recrec progress in isolated instrumentation while immutable full-corpus replay continues.
- [ ] Migrate CLI, cache and corpus to canonical compiled analysis and delete the prototype inference APIs.

- [x] Independently replay the integrated raw metadata owner on4,274 cases with frozen original inputs and unchanged Root source/binary hashes:813 literal hashes,1491 primitive hashes,156 maps,1120 ETS histories,352 declaration/optional keys,20 malformed boundaries,17 callback groups,17 public raw-declaration lists and288 mode diagnostics.
- [x] Freeze isolated compressed BEAM fingerprint proof:108 OriginalNative variants,845 freshly verified plain digests and24 explicit caller limits pass. Integration with physical persistence remains pending.

- [x] Fresh integrated public entry after raw metadata migration:24 callback runs/64 events,24 unknown or missing-call runs/24 events and2 selected corpus groups/7 events exact.
- [x] Verify the original line-location and ignore-contract suite settings through the production entry:2 original public runs/3 events exact.
- [ ] Complete actual suite warning/contract/location profile acceptance across327 groups; fresh original Native producer runs against an immutable compiler bridge.

- [x] Integrate captured duplicate-declaration owner and independently replay379 checks in Root:70 actual Original boundaries,4 typed profile lifecycle checks,288 complete diagnostics and17 callback persistence groups. All157 library tests and strict all-target Clippy pass.
- [x] Reverify canonical public entry after captured preparation:24 callback runs/64 events,24 unknown-call runs/24 events and2 corpus groups/7 events exact.
- [ ] Integrate shared global declaration journal/completion order, raw behaviour/dependency metadata and final classic persistence owner; then run the complete default and actual-suite-profile corpus on one final source snapshot.

- [x] Integrate the global contract/callback journal and remove independent callback completion scheduling; fresh Root423 checks pass, including44 actual Original worker-order cases,288 diagnostic runs,17 callback groups and74 captured/profile cases. All157 tests and strict all-target Clippy pass; fresh24 public callback runs still exact.
- [x] Verify bounded parallel corpus replay against2 actual suite-profile public runs/3 events; output is reassembled in original order and compared exactly.
- [ ] Add typed production analysis mode: Original plt_build skips diagnostic/Doc collection, while succ_typings runs callbacks before legal warning filtering. Verify actual mode paths with the CLI/persistence wave.

- [x] Complete frozen default-profile actual public API acceptance for all327 registered semantic groups/2617 events; verify exact IDs against the full inventory. This proof binds the earlier public candidate, not the later Root raw/global integrations.

- [x] Integrate canonical raw dependency/behaviour metadata. Fresh Root423 captured/global/diagnostic/callback regressions and6 composed pipeline cases/24 inferred+24 Doc signatures/5 warnings pass after integration; full raw/publication replay remains pending.
- [ ] Verify the integrated final classic persistence owner with fresh physical PLTs and original readers; then integrate typed diagnostic mode/session and migrate CLI callers.

- [x] Independently verify integrated raw metadata and dependency publication after classic integration:4908 raw cases,48803 metadata regressions,3198 diagnostics/1886 accepted-contract comparisons,1149 publication worlds,18 constraints and7 Source behaviours exact;2 legacy compiler schema boundaries retained. Original26 Source/BEAM artifacts and all source/input inventories unchanged.
- [x] Fresh public callback lifecycle after classic integration:24 runs/64 ordered warning events exact with the genuine original lifecycle prior.

- [x] Integrate typed PLT-build versus success-typing modes and lazy diagnostic sessions; remove eager DiagnosticInputs.collect and migrate all callers. Fresh Root464 checks pass:32 actual Original public modes,288 Use/Ignore diagnostics,144 diagnostic regressions,67 installed Original Source/BEAM paths bound; all157 tests and strict Clippy pass.
- [x] Verify physical classic persistence in Root:133 complete PLTs accepted/equal through actual Original readers;1153 hash pairs,1136 dependency recipes,126 header merges,27 behaviour maps exact with all source/tools/inputs/binaries unchanged. This run precedes the typed-mode/intake successor; final same-source operational gate remains open.
- [ ] Verify final integrated full327 signature/Doc/raw/public corpus, actual suite option profiles and150 operational cases after CLI/cache owners converge.

- [x] Fix declaration intake chronology in the canonical metadata owner. Fresh Root98 comparisons pass:24 freshly produced Original captured errors and74 genuine original capture/profile inputs. Mislabelled successor proof outputs are excluded as inputs.
- [ ] Run all327 engine groups on immutable current candidate3 while coherent CLI, checked-repair and incremental owners proceed; this engine checkpoint does not substitute for final150 operational cases.

- [x] Integrate the proper-header classic checked planner. Fresh Root1676 Original comparisons pass:1632 private,30 actual public cplt checks with exact fingerprint-call chronology,14 ordering cases.1674 results are raw exact;2 compare typed exit reasons without dynamic VM stacks.12 explicit missing-layout boundaries remain; malformed imported dependency values are a separate coherent successor.157 library tests and strict Clippy pass.

- [x] Fresh current type algebra acceptance:3 Original suite assertion cases and6 properties×5000 trials pass with complete bound source/input/binary inventory unchanged.
- [x] Execute22 actual Original generated consistency case functions with only new_tests/2 bridged to Rust. Each passes;22 omitted-case negative controls reproduce the Original failure. Registry-only case data is provided directly, with no PLT setup or analyzer claim.

- [x] Save the verified150-case operational predicate registry in corpus/operational_cases.json, with15 coverage sets and10 pinned Original source hashes. No execution status is inferred from the registry.
- [x] Add reusable isolated type verification runner; fresh3 Original assertions and30000 property-law trials pass in /private/tmp/rust-dialyzer-operational-types-root-1. Property CT wrappers are explicitly excluded from this proof.
- [x] Bind the generated consistency runner to the150-case registry and verify22 exact cases plus22 failure controls in /private/tmp/rust-dialyzer-operational-consistency-root-3.

- [x] Integrate coherent raw warning-cache successor, preserving deferred raw old values and reached Source merge/select/purge errors. Fresh Root1094 actual Original cases exact; strict all-target Clippy passes. This remains a component proof, not a public incremental workflow pass.

- [x] Verify frozen engine candidate3 default public327/2617 ordered warnings; independently recheck273 source+922 oracle hashes. This predates common PLT/raw dependency successor and does not establish final CLI/incremental parity.
- [ ] Rerun candidate3 composed and actual suite-profile gates interrupted during disk exhaustion; preserve failed runs without claiming acceptance.

- [x] Integrate common CompletedPlt and raw dependency values; Root fresh proof loads133 CPLTs through Original readers, verifies1153 hashes/1136 dependency recipes/126 headers/27 behaviour maps, and582 raw-value comparisons plus72 entire physical dict records. Proper checked-header1676 comparisons retained (two typed exit reasons without VM stack parity). Full public operational parity remains open.
