# Performance work

- [x] Read poteto performance playbook, Prove It Works, Fix Root Causes, Build the Lever and How.
- [x] Preserve the existing CLI binary and isolate instrumented sources in /private/tmp/rust-dialyzer-perf-1.
- [x] Capture baseline stage timings for the tiny app.
- [x] Trace the dominant stage to its owning data structures and operations.
- [x] Design and implement compact prior transport and bounded contract captures; inspect the diff.
- [x] Compare three measured rounds, exact tiny warnings/types/documentation and all 3,314 imported contract/callback terms. Reverify the final formatted source on the next fixture.
- [ ] Continue through the Elixir regression cases individually, recording measurements and correctness.
- [ ] Check broad correctness after structural changes.
- [ ] Review parallelism only where measured independent work justifies it.
- [ ] Report measurement artifacts and remaining scope.

PR creation is skipped because no external publication was requested. Work proceeds inline because delegation was not authorized.

First regression case elixir/01_no_warnings_on_specdiffs passes on final formatted candidate: preserved CLI median 14.232 s, candidate 7.935 s, three measured rounds after warmup, exact warnings/signatures/documentation, unchanged source/input inventory. Tiny median 17.122 s to 9.521 s. See docs/performance-results.json and /private/tmp/rust-dialyzer-perf-1. Root strict library Clippy passes. The isolated CLI has two pre-existing Clippy findings outside the scoped change; it is not claimed clean.

Reproduction: scripts/prepare_profile_cli.py verifies its complete baseline manifest and replays scripts/profile_cli_candidate.patch into a fresh output directory. Build its release CLI, then use scripts/profile_cli_case.py on each input. The recreated source patch was applied successfully to /private/tmp/rust-dialyzer-perf-replay-1.

Second-step optimization is accepted on case 308: median 8.330 s to 6.729 s against preserved compact step1, three measured rounds, exact warnings/signatures/documentation and unchanged input/source inventory. Case 309 passes at 8.414 s to 6.749 s. Root scripts/core_export.exs retains the checked export_terms wrapper; production compact exporters use capture_terms. Actual Original CPLT header, all five tables, bigint/float/binary/bitstring, improper list and large map exports are identical through checked and capture paths. See /private/tmp/rust-dialyzer-perf-2/capture-equivalence.json. The updated complete CLI patch replays cleanly in /private/tmp/rust-dialyzer-perf-replay-2.

Fourth step passes case311 (elixir/05_no_warnings_on_macrocallback): atom collection folds directly into a set, preserving native metadata. Three measured CLI rounds reduce median 4.000s to 3.763s, with complete warning/signature/Doc equality and unchanged source/input hashes. The patch reproduces every changed source file in /private/tmp/rust-dialyzer-perf-replay-4. Case312 now has prior-export subphase instrumentation in isolated perf-5; it is not an accepted optimization yet.

Fifth step passes case312 (elixir/06_no_warnings_on_callback): reuse decoded header filenames instead of calling Native plt_info, preserving all221 files and complete dependency exports. Three-round median3.716s to3.584s. Root compact exporter equivalence also passes. Case313 conditional helper-loading experiment preserves outputs but regresses3.478s to3.648s and is rejected; accepted driver remains perf-5.

Accepted perf-5 valid PLT-build gate passes against Native: exact warnings and all five output tables read by Original dialyzer_cplt (rows35/0/19/0/0), immutable BEAMs/PLT/binary. Complete11-file patch replay matches accepted sources byte-for-byte. No final broad327-group/150-operation parity is claimed by this gate.

Identity-entry redesign phases: [x] ground current compiler/identity flow and compare separate entry versus persistent worker inline; [x] select explicit two-entry CompilerBridge and shared CompilerIdentity digest owner; [ ] implement isolated perf-7; [ ] verify old/new digest equality, mutation rejection and positive bridge; [ ] measure case313 and accept or scrap based on actual outputs and timings. Delegation remains skipped because it is not authorized.

Identity-entry redesign accepted in perf-7: old/new digest exact, mutation rejection and positive compiler bridge pass; case313 three-round untraced median3.253s to3.099s with all warning/signature/Doc output exact. Final original five classic lifecycle cases and Native-read PLT tables pass. All14 patch files replay exactly. The profiler now supports balanced traces and untraced timing to avoid instrumentation asymmetry.

Case314 instrumented importer passes complete warnings/signatures/Doc and immutable source/input check. It identifies raw contract reconstruction as0.260s of0.428s import cost. Accepted source remains perf-7. Instrumentation-only successor is perf-8 and is reproduced by profile_prior_instrumentation.patch after the accepted cumulative patch. Next work is contract representation/ownership, retaining bounded private arenas.

Case314 is verified on perf-11 with one-line forms-arena ownership transfer. Three untraced rounds preserve all output and show only a small noisy1.1% median improvement. Do not claim a major speedup.3314 records/5407 signatures remain byte-identical; Root157 library tests and strict library Clippy pass; fresh Native PLT build warnings and all five tables match. The14-file cumulative patch replays exactly. Sparse-copier variants9/10 preserve all270545 actual Native roots but are excluded for under1% whole-run gains. Next fixture targets dominant compiler preparation overhead.

Case318 final acceptance tasks:

- [x] Re-read performance playbook and verification principles; preserve prior traces and candidate sources.
- [x] Verify final parser values/errors and Root library checks after core-only lint cleanup.
- [x] Rerun untraced full CLI timings in isolation and Native PLT table comparison.
- [x] Replay the cumulative patch, record evidence, and advance to case319.

Case319 tasks:

- [x] Trace both versions and split atom traversal from primitive hash export. Candidate19 is instrumentation only.
- [x] Compare direct-map collector layouts on actual dependency header/tables,22 modules and synthetic inputs with immutable provenance.
- [x] Measure five isolated untraced pairs and accept or reject candidate20.
- [x] Replay all25 candidate files exactly. Root collector integration skipped: its older exporter has no raw PLT collector; retain modern CLI migration as separate coherent work.

Case320 tasks:

- [x] Trace accepted20 baseline and inspect library-world setup and inference.
- [x] Instrument setup, library signatures/contracts, inference and diagnostics in isolated candidate21.
- [x] Measure and verify outputs before choosing the next structural change.

Lazy world architecture tasks:

- [x] Ground all consumers and compare static filtering with complete borrowed-library lookup; select lookup.
- [x] Prove Native ignores an unused malformed existing contract that Rust20 rejects during phase setup.
- [x] Implement the borrowed world and migrate generation, calls, function warnings and inspection probes in candidate22.
- [x] Run124 library tests and all-target compilation.
- [x] Verify native public regression, all records/signatures, fixtures and original consumer/lifecycle gates.
- [x] Measure isolated CLI timings; accept candidate22 with final-source proofs.

Case321 tasks:

- [x] Profile accepted22 and read the local serde Value deserializer to ground the transport cost.
- [x] Test moving five owned arenas around metadata deserialization in candidate23; preserve field presence and null behavior.
- [x] Compare valid imported worlds and malformed owned-boundary results with accepted22.
- [x] Accept the import-stage optimization; full-command timing is inconclusive and no overall speedup is claimed.

Case322 tasks:

- [x] Inspect repeated module compilation and self-roundtrip work; read original get_core_from_beam.
- [x] Instrument read, compile, clean, reference compile and roundtrip costs in isolated24.
- [x] Choose explicit production capture and checked verification interfaces from measured costs. A mode flag is rejected because it makes validation depend on caller state.
- [x] Verify27 exact module captures and actual CLI outputs; reject25 after7 pairs regress4.95%,6/7 slower. Accepted23 unchanged.

Case323 tasks:

- [x] Trace accepted23 on the next fixture: three repeated PLT header decodes dominate useful target inference.
- [ ] Choose a structural optimization from measured evidence and verify before timing.

Header snapshot tasks:

- [x] Compare caching with reuse inside one request; choose one read/decode snapshot without persistent caches.
- [x] Implement candidate26 from accepted23; preserve standalone exporter entry and Native PLT loading.
- [x] Verify complete dependency artifacts and all actual CLI outputs.
- [x] Accept26 after7 full pairs, Native tiny PLT gate,5 classic lifecycle cases and31-file replay.

Case324 tasks:

- [x] Trace accepted26 on opaqueness fixture; exact outputs and input/source hashes. Traced command difference inconclusive; preparation/import dominate inference.
- [ ] Use measured dominant costs to choose the next structural change.

Arena cache experiment tasks:

- [x] Ground persistent map updates in the277ms contract capture stage; compare map with private ETS.
- [x] Preserve key shapes and IDs; implement scoped ETS lifetime with cleanup on failure in candidate27.
- [x] Measure6 actual dependency arenas over7 pairs; exact captures/failure cleanup, no material timing gain. Reject27 without full build.
- [x] No acceptance: local stage failed to improve.

Native dependency transport investigation:

- [x] Trace Native CPLT dict-to-table conversion and existing Rust SnapshotFile ownership.
- [ ] Measure direct physical decode separately; do not mistake decoding for complete library import.
- [ ] Design coherent native dependency intake preserving callback ordering, type map iteration, target purging and source fingerprints.
