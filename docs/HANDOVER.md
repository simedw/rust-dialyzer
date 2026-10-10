# Handover: rust-dialyzer performance work

Last updated 2026-10-10. The latest analyzer change reuses certified whole-root results within large immutable SCC solves. Contract and module diagnostics overlap from the previous accepted change. Its [fresh recheck](../reports/halving-rust-2026-10-09/diagnostic-overlap/REPORT.md) gives another 3.5%/1.7% median wall reduction on Phoenix/Elixir, with all output and full parity gates exact. The latest exporter change is `aabdd4a`. The latest accepted build improvement is profile-guided compilation through `bench/phase0/build_pgo.py`. Ordinary Cargo builds do not enable PGO. `perf-analysis` is still at `c164bf8`.

Operational status: benchmark and agent jobs were stopped after disk exhaustion. [Disk recovery](../reports/storage-recovery-2026-10-09/README.md) reclaimed about 35.9 GiB and left about 40 GiB free. Completed large outputs were archived with verified hashes, and old untracked Cargo targets were removed. Benchmark, parity, and PGO drivers now require a 10 GiB disk reserve. Historical raw output paths may now have a `.gz` suffix; original head-to-head reference outputs remain unchanged. The diagnostics overlap change has now passed the complete normal/VERIFY/subset parallel/sequential gate and is committed. Three fresh pairs measured Phoenix 9.90 to 9.65 seconds and Elixir 10.43 to 10.03 seconds with exact warnings and signatures. The earlier gate was interrupted at subset sequential for disk recovery; the fresh complete recheck exits 0. `target/pgo/core_cache_analyze` now contains the validated overlap PGO binary; the old binary is preserved at `target/pgo/pre-overlap-core_cache_analyze`.

The current [same-window cumulative comparison](../reports/halving-rust-2026-10-09/cumulative-pgo-summary.json) includes fresh Core compilation and every dependency module. Three alternating pairs against the frozen `aa83b55` starting implementation give Phoenix 13.08 to 9.82 seconds and Elixir 13.60 to 10.20 seconds, a 24.9%/25.0% reduction. Peak footprint falls from 8.693 to 8.120 GB and 9.119 to 8.514 GB. All twelve analyses preserve every warning and all 31,050/39,126 signatures. Older baseline medians were 15.53/16.49 seconds under different host load; do not compare those directly with current candidate times or substitute the older `ad54597` measurements to claim halving.

The compact source export, direct ETF node records, parallel signature serialization, composite value memo, and native Core transport are committed. Native transport preserves complete typed module equality on 600+846 real modules and five deep fixtures, in normal and VERIFY modes. Its full warning/signature regression gate is green. [Native transport measurements and proof](../reports/halving-rust-2026-10-09/native-production-findings.md) show a modest additional gain and a small peak-memory increase. [PGO measurements and reproduction](../reports/halving-rust-2026-10-09/pgo-findings.md) add another 10.2%/9.4% fresh-runtime reduction. The measured binary is preserved at `target/pgo/core_cache_analyze`. Dense graph IDs, allocator purging, native-reader group sizing and deferred clause recovery were rejected after exact-output measurements. Decisions and rejected patches are in [the experiment report](../reports/halving-rust-2026-10-09/README.md).

The [latest fresh OTP comparison](../reports/halving-rust-2026-10-09/pgo-otp-summary.json) runs both tools three times in the same window. Phoenix is 16.26 seconds for OTP versus 9.72 seconds for fresh Core plus Rust, a 1.67x speedup. Elixir is 16.56 versus 10.25 seconds, a 1.62x speedup. Peak footprint is 1.498 versus 8.095 GB on Phoenix and 1.493 versus 8.520 GB on Elixir. Rust with an already built Core cache takes 7.74/8.17 seconds. All warnings match fresh OTP and every inferred signature matches the original reference. The current user target is 5x fresh OTP speed on both Phoenix and Elixir. That target remains open. Six further trials were rejected: disabling outlining, increasing the PGO value pool, borrowed Core-expression matching, 32/64 MiB type tables, and native-CPU compilation. No rejected flags or source changes enter production.

The [current architecture audit](../reports/halving-rust-2026-10-09/next-step-audit.md) found no additional cheap change with a grounded material saving. Arena values and dependency handles already move; preparation staging duplicates topology rather than deep types. The next research step is focused current-PGO profiling of giant-SCC solver restarts, group work and cache behavior. Previous TIMINGS2 worker totals are distorted and must not be used as current removable wall time. No benchmark or agent jobs remain active.

The latest follow-up rejects three isolated variants after exact-output screens: indexed per-SCC cache storage, shared-prefix binding tries, and append/reverse case accumulation. Source remains at the accepted diagnostics-overlap implementation. [Current production sampling](../reports/halving-rust-2026-10-09/production-sampling/REPORT.md) uses the exact accepted PGO binary, with no compiler/profile mismatch, and preserves both full outputs. Sampling points to body evaluation, clause handling, substitution and allocation as substantial work; counts are not removable wall-time estimates. Next, investigate reuse of repeated root solves and partial type substitution. The conservative read-dependency semantics of root-result reuse and partial type substitution need proof before a retained algorithm change. All jobs from this follow-up have completed; isolated sources and measured binaries remain under `/private/tmp/rust-5x-*`. The 5x target is still open.

The next isolated trial, [constant-subtree substitution reuse](../reports/halving-rust-2026-10-09/later-rejects/constant-substitution/REPORT.md), also failed its hardware screen. Both library modes pass 259 tests and all four full-inventory outputs are exact, but instructions are effectively unchanged and wall results are mixed. No source change was retained. The audit established that existing variable flags and collect_var_names omit map-entry values even though recursive substitution visits them; any new substitution or root-input memo must account for those reads. The trial used a separate propagated flag to handle this case safely. No jobs remain active. Investigate binding-sensitive substitution reuse only after deriving complete lookup and context semantics; constant-only reuse has no measured gain.

Two [binding-sensitive substitution memos](../reports/halving-rust-2026-10-09/later-rejects/binding-substitution/REPORT.md) also failed their runtime checks. Actual lookup traces handle map values and skipped list tails; both variants pass 260 normal/VERIFY tests and all twenty full-inventory outputs are exact. The owned-trace three-pair medians lower instructions/CPU by about 0.7–0.8% but worsen walls on both projects. Borrowed IDs and reused vectors lower instructions by 0.8–0.9% with slower walls and unchanged CPU in their screen. Neither enters production. Do not continue capacity tuning without new evidence. Next, inspect constraint-generation work and repeated constant-node allocation in NativeConstraintArena::any; source IDs may be shared while preparation assigns occurrence IDs, but this proposal is unimplemented and unmeasured. General atomic-node interning requires a separate ordering and dependency audit. All jobs from these trials are complete.

The [constant-node sharing trial](../reports/halving-rust-2026-10-09/later-rejects/shared-any/REPORT.md) improved ordinary-release medians by about 0.9% in a three-pair comparison, but its source-matched fresh PGO check regressed Phoenix in all three pairs. Fresh medians were Phoenix 9.48 to 9.74 seconds and Elixir 10.17 to 10.07 seconds. Reject; production source/binary remain unchanged. Four library runs each pass 258 tests, and all thirty complete outputs match warnings and signatures. A separate [typed-atomic interner](../reports/halving-rust-2026-10-09/later-rejects/typed-atomic-sharing/REPORT.md) also failed its screen with increased instructions and CPU. All jobs are complete. Do not tune these storage variants further without new evidence. Next, design a broader reduction in repeated giant-SCC work, deriving complete binding-read/write, failure-cache and provider-concurrency semantics before choosing root reuse or speculation. The architect skill provides the multi-design workflow; it has been read, but design work has not started. The 5x goal remains active and unmet.

The [solver reuse design and census](../reports/halving-rust-2026-10-09/solver-reuse/REPORT.md) now provide a grounded repeated-work result. Two design packages were compared and reviewed by gpt-6-astra; whole-root certificates were selected over a subgroup transition that loses branch identity. Each inventory has 4,761 matching roots covering 1,429,343 atomic evaluations, about 29.75% of atomic work inside nine selected giant-SCC visits. Complete normal-census witnesses match bindings, updates, loops and inferred types; counts are not wall-saving estimates. A compact isolated replay prototype passes 264 normal and 264 VERIFY library tests, both full VERIFY inventories, and all measured outputs. Its preliminary exact-source PGO fresh medians are Phoenix 9.46 to 9.32 seconds and Elixir 10.06 to 9.84 seconds, with 4/6 pairs faster and lower CPU/instructions. No source is accepted yet: VERIFY-only disjunction enumeration can broaden guards and miss normal-hit coverage, provider/equality/dynamic-substitution fixtures and a fixed-input assertion remain, retention needs an explicit budget, and the complete parity gate has not run. Main source/production PGO remain unchanged. All jobs from this research cycle are terminal. Measured source and PGO are in /private/tmp/rust-5x-root-replay; a distinct unbuilt correction copy is /private/tmp/rust-5x-root-replay-final/source. Finish those proof/retention corrections, freshly train and benchmark the final source, then complete parity before adoption. Both the 5x goal and exact-output requirement remain active.

The [corrected root-replay prototype](../reports/halving-rust-2026-10-09/solver-reuse/correction/REPORT.md) is now built and measured. It fixes VERIFY disjunction tracing, documents provider purity, asserts fixed inputs, bounds retained guards and adds nine semantic fixtures. Both library modes pass 273 tests with one ignored, all twelve ready-screen outputs and both full VERIFY inventories preserve complete warnings/signature arrays, and a patch reconstructs all 285 source hashes. Ordinary ready-cache medians are Phoenix 8.14 to 7.90 seconds and Elixir 8.83 to 8.43, with six of six pairs faster and nearly flat CPU. Production is unchanged; these times exclude Core export and do not update the OTP ratio. The corrected measured source/binary are under /private/tmp/rust-5x-root-replay-final. Next, mechanically mirror normal retention after a VERIFY hit, test aggregate admission/replacement limits, finalize the default-build source shape, then freshly train PGO, compare fresh workloads and run the complete parity gate before adoption. The previous paragraph describes the correction as unbuilt at the earlier research checkpoint; it is superseded by this report. All current jobs have finished; 5x remains unmet.

The [default-build whole-root replay](../reports/halving-rust-2026-10-09/solver-reuse/production/REPORT.md) has now passed the remaining adoption gates. It mirrors normal retention after VERIFY hits, directly tests aggregate byte/entry/replacement policy, removes experiment cfgs and unused instrumentation, and allocates fixed snapshots only in VERIFY. Both library modes pass 275 tests with one ignored. Source-matched PGO fresh windows improve Phoenix 9.60→9.40 and 9.51→9.19 seconds, and Elixir 10.51→9.94 and 10.07→9.95; eleven of twelve pairs are faster and CPU/instructions fall in both windows. All 24 measured complete arrays, both training outputs, full-inventory VERIFY and the complete normal/VERIFY/1082-module/subset parallel/sequential gate are exact. The 8 MiB limit bounds retained guard Vec buffers only, not total memory or VERIFY witnesses. Production adoption uses the exact measured four-file source and PGO binary SHA256 5f00a43d99632b949e4e5b0732a52d6d54ddac77347613f1c1644a0f42589c02; the previous validated overlap binary is preserved at target/pgo/pre-root-replay-core_cache_analyze. This supersedes the earlier production-unchanged checkpoints. A new matched fresh OTP comparison and profiling of this accepted binary are next; do not reuse the previous OTP timings to claim an updated ratio. The 5x goal remains active and unmet.

The [post-replay matched fresh OTP comparison](../reports/halving-rust-2026-10-09/solver-reuse/production/otp/REPORT.md) now measures Phoenix OTP 16.37 seconds versus fresh Core plus Rust 9.32, and Elixir 16.56 versus 9.99: 1.76x/1.66x faster. All six warning multisets and complete unique signature arrays are exact. Accepted source is 85188ed and installed PGO SHA256 is unchanged from the replay acceptance proof. [New production sampling](../reports/halving-rust-2026-10-09/post-root-sampling/REPORT.md) still shows body/solver/clause and allocation work. An isolated function-definition metadata reuse trial is building under /private/tmp/rust-5x-function-definitions; production source is unchanged. Its initial shared-target build reused the baseline executable and was discarded before timing. A separate candidate target and differing-binary assertion now prevent that artifact collision. No performance or acceptance claim exists for this trial yet. The 5x target remains unmet.

The [fixed function-definition metadata trial](../reports/halving-rust-2026-10-09/later-rejects/function-definitions/REPORT.md) is rejected after a source-matched fresh PGO screen. Median fresh walls are Phoenix 9.27→9.28 seconds and Elixir 9.84→9.86, despite about 0.9% fewer instructions. Both corrected library modes, training arrays, full-inventory VERIFY and all twelve measured complete arrays are exact. The complete parity gate was not run for this rejected source; production remains 85188ed and PGO SHA256 5f00a43d99632b949e4e5b0732a52d6d54ddac77347613f1c1644a0f42589c02. The [body reuse synthesis](../reports/halving-rust-2026-10-09/body-reuse-design/SYNTHESIS.md) chooses certified silent whole evaluations as the first no-skip census, with separate stamp/effectful exclusions and explicit native-versus-VERIFY observation handling. Two independent packages and a gpt-6-astra judgment are saved. No census or body skip exists yet. All benchmark jobs for this checkpoint are complete; 5x remains unmet.


The [partial body census](../reports/halving-rust-2026-10-09/body-reuse-census/REPORT.md) now has corrected C2 and refined C3 exact no-skip outputs for both complete inventories in normal and VERIFY modes. C2 passes 280 library tests per mode; C3 passes 283, one ignored. C3 separates empty warning transfers, overlapping per-body reason sets and stamp-only final owner projections. Unscoped repeated inputs with unchanged strict owner projection cover 2,669,886/3,327,914 refinement frames, about 9.76%/10.20%, but no local/provider dependency, return/effect match, complete scope witness or replay certificate exists. Broader repeated-input groups cover about half of refinement frames; keep them in scope, without treating operation counts as removable seconds. Complete patches, compressed outputs, independent 11-output/286-file reconstruction proof and gpt-6-astra reviews are committed. Main analyzer source and installed PGO remain unchanged. No performance benchmark or production adoption applies to census instrumentation. Next, prove exact local reads and concrete within-invocation provider semantics; complete witnesses and fresh PGO/benchmark/full parity gates are required before activation. The earlier statement that no census exists is superseded by this partial foundation, not a complete-effects census.

C1 Phoenix VERIFY also reproduced the unresolved std BTree panic, now with a stack through Type::append_children, collect_var_names and NativeConstraintArena::atomic. The source trace narrows the affected iterator to tuple arity groups, BTreeMap<usize, Vec<TupleVariant>>; custom atom/type/variable ordering does not order that map. The failed run and unchanged-binary passing rerun are preserved separately. Passing C2/C3 runs do not explain or fix the failure. Further debugging needs bounded operand/traversal/group provenance, not a replacement map that hides the symptom. All census jobs at this checkpoint are complete; the 5x goal remains active and unmet.

The [local-read C4 census](../reports/halving-rust-2026-10-09/body-read-census/REPORT.md) now passes 289 library tests per mode and both complete inventories in normal/VERIFY. Requiring the immediately preceding silent body, equal ordered local reads and equal return Type reduces observed matches to 271/637 bodies and 10,517/30,475 refinement frames, below 0.1%. No skip is activated. Admission refusals are separately counted and zero in normal refinement; trace overflow, unknown reads and prior silent-policy exclusions still limit the observed subset. Concrete provider facts appear fixed within one invocation, but recorder/cache/capture effects and complete error/scope witnesses remain unproved. The next step is mismatched-read/control attribution before choosing wider lifetime or effectful replay; keep the broader roughly 50% repeated-input group in scope. Compressed output, full final patch and independent 286-file/four-output proof are saved. Production source and installed PGO remain unchanged, the BTree panic is unresolved and 5x remains unmet.

The [C5 local mismatch attribution](../reports/halving-rust-2026-10-09/body-read-differences/REPORT.md) passes 291 tests per mode and all four exact full outputs. Every mismatched body in the admitted preceding-silent normal subset has a position-aligned FunctionType-value difference; most also have changed LocalReturn values. Removing ForwardEntry's result comparison alone would not restore the lost matches under this transcript policy. These overlapping observations do not prove final-result changes or causal dependencies. Next measure direct work deletion in forward_arguments: avoid a joined Vec and product-type construction when all joined arguments remain equal, preserving join/error order. No production change or speed claim applies yet; broader body reuse remains in scope and 5x remains unmet.

The [lazy forwarding trial](../reports/halving-rust-2026-10-09/later-rejects/lazy-forwarding/REPORT.md) is rejected. Its ordinary prepared-cache screen is faster in all six pairs, but source-matched fresh PGO gives Phoenix 9.40→9.47 seconds, slower in all three pairs, and Elixir 10.03→9.85. CPU/instructions fall, but both-workload fresh wall acceptance fails. Final tests pass 277 per mode; all 26 measured/training complete outputs are exact. Full-inventory VERIFY and the complete parity gate were not run for this rejected source. Production remains 85188ed and installed PGO unchanged. Next test direct argument-vector equality with the original eager join loop, removing only redundant product conversions; no result or acceptance exists for that alternative yet. 5x remains unmet and the BTree panic unresolved.

## Goal

A Rust reimplementation of OTP 28 Dialyzer for Elixir projects that is much faster than `mix dialyzer` (dialyxir) and reports **exactly** the same warnings. The order of warnings doesn't matter. The current focus is the **full from-scratch run** (no PLT), because that is where Rust can gain most over OTP.

**Exact parity is the hard constraint.** Every change must reproduce OTP's warnings and its inferred signatures for every function, including private ones, bit for bit.

## Earlier prebuilt-Core measurements

The following measurements are historical and use Phoenix project-only reporting. Use the fresh-cache, all-module comparison above for the current target.

Phoenix scenario: 600 modules from scratch. That is the 93 Phoenix modules plus the 507 OTP/Elixir/deps modules they reach, with no PLT. Measured on 2026-10-09, 14 cores, 36 GB RAM:

| | Wall | CPU (user+sys) | Instructions | Peak memory |
|---|---:|---:|---:|---:|
| OTP dialyzer (`run_dialyzer.exs build`, 14 schedulers), load 13–16 | 17.0–17.4 s | ~83 s | 0.89 T | |
| Rust @ e3ed138, interleaved, load 14–17 | 12.7–13.9 s | ~75 s | 0.68 T | 15.0 GB |
| Rust @ 4965204, interleaved, load 14–17 | 8.1–8.4 s | ~59 s | 0.51 T | 10.9 GB |
| Rust @ 296e2ad (shared arenas) | ~7.8–8.0 s | | **0.49 T** | **7.1 GB** |

Caveat: OTP's time includes compiling beams to Core. Rust reads a prebuilt Core cache instead. A cold cache build for 1,082 modules takes about 6.7 s; a warm cache check takes about 0.2 s.

Parity at 4965204, all exact (full verify needed a re-run, see known issue 1):
- **Phoenix:** 199/199 warnings, 1544/1544 signatures.
- **Full 1,082 modules:** 400/400 warnings, 42,246/42,246 signatures.
- **Subset:** 7,433 warnings and 7,117 signatures, both parallel and sequential.
- **Unit tests:** 248 pass.

Peak memory is 7.1 GB on Phoenix and 11.0 GB on the full run (1.05 T instructions, was 1.33 T). Verify mode on the full run peaks at about 26 GB (was 28–36 GB).

**Where the memory goes** (2026-10-09, after the shared arenas): RSS reaches its peak at the end of setup and stays flat through inference. Module term and node arenas are `SharedVec`s (`src/shared_vec.rs`): a shared prefix behind an `Arc` plus the entries a copy appended, so setup phases that need "the module arena plus a few terms" no longer deep-copy it. Remaining large holders in the last heap snapshot: term-order arenas (~1.2 GB), the loaded Core itself (~1 GB), `Program::bind`/`build_with_contracts` (~0.55 GB), interned types and memos (~0.35 GB). To re-measure: build with `RUSTFLAGS="--cfg dialyzer_tsan"` (system malloc), run with `MallocStackLogging=lite`, stop the process after setup, and run `malloc_history <pid> -callTree -invert`.

Timeline on Phoenix: start of session about 44 s wall (6.6 T instructions on the full run). After micro-optimisation and parallelism, about 20 s and 1.13 T. After the type and refinement redesign, 12 s and 0.68 T. After the four parallel branches below, 8.3 s and 0.51 T.

## Architecture in one paragraph

OTP compiles each beam's debug_info to Core Erlang. `prototypes/core_cache/core_cache.exs` caches that per beam hash as compact ETF. `src/bin/core_cache_analyze.rs` loads the modules in parallel (`src/core/etf.rs`) and runs `compiled_probe::analyze`. That goes through `compiled_analysis.rs` (setup: labels, call graph, definitions and contracts in `compiled_definitions.rs`) and then `core_pipeline.rs` / `core_analysis.rs` / `core_phases.rs`. That last part is OTP's check-mode loop: SCC constraint generation and solving (the `dialyzer_typesig` equivalent, `src/constraints/*`), then module refinement (the `dialyzer_dataflow` equivalent: `body_*`, `dataflow_*`, `binder*`), iterated to a fixpoint. The type lattice (the `erl_types` equivalent) is `src/lattice.rs` plus its `#[path]` submodules.

## What was done (chronological, all on main)

1. **Input:** the Firefly frontend was replaced by OTP-produced Core, cached as ETF. The Firefly code was deleted on `remove-firefly-frontend`, with a snapshot commit kept before it.
2. **Basics:** mimalloc, FxHash, skipping teardown at exit, interned atoms (`src/atom.rs`, lock-free table), interned MFA names, an `im::OrdMap` resolver cache.
3. **Parallelism:**
   - Parallel ETF load.
   - Parallel setup phases (`ordered_parallel::setup_map`, `join`).
   - Parallel contracts and records in `prepare_definitions`, using borrowing resolver workers.
   - **Dependency-driven SCC scheduling** (`ordered_parallel::dependency_map`): an SCC starts as soon as the SCCs it calls have committed. An overlay with retries keeps exactly what each SCC can see the same as the old wave scheduling. This was the biggest wall-time win, 38 s → 24.5 s.
   - Module refinement runs largest-first.
4. **Memoisation:** SCC results and module refinement are memoised across fixpoint iterations, keyed by the values actually read (`RecordingWorld`, `RecordingCalls`).
5. **Algorithmic fixes:** per-module whole-program scans (`ordered_local_callers`, the resolver's `massage` set), the disjunction join key intersection, natural-run sorting, lazy closure-capture environments, and lazy map-capture tables.
6. **Solver:** the binding map is a persistent radix trie (`src/constraints/bindings.rs`), atom-list conjunctions use a write log, and unification works on borrowed values.
7. **Type representation redesign:**
   - Each distinct type node is stored once, in a lock-free table, and never freed. `Type` is a copyable pointer, equality is pointer equality, and each node caches its hash and flags.
   - Per-thread memos for meet, join, subtract and limit, with hit rates of 70–89%. Only successful results are cached, and only when the result can't depend on nominal or opaque context (`ActivePairGuard`).
   - Commits `15db10a`, `3d5bba2`, `8416746`, `b2af693`, `087169e`. Together they cut instructions by 30%.
8. **Term order:** OTP's constraint order is emulated with byte-string keys compared by memcmp (`NativeOrderKey` in `src/native_order.rs`), and dependency sets are sorted `Arc<[VariableId]>`. About −2%.
9. **Refinement:**
   - Binder state is built once per module, and filenames are parsed once.
   - `DataflowMap` is a label trie (`src/dataflow_trie.rs`) with a chunked `modified` list.
   - Guard scopes are shared, and body expressions are borrowed instead of copied.
   - Letrec refresh is incremental.
   - About −10%.

10. **Four parallel branches (2026-10-09)**, merged one at a time with the full suite after each. Phoenix instructions measured alone against e3ed138; together −26%.
    - **Setup** (`1eee93c`, −4.6%, setup wall ~5 s → ~2.7 s): parallel Core validation, per-module closure dependencies and call-graph edges, parallel module tables and contract forms, shared resolver cache keys, pipeline setup overlapped with the setup branches, literal arenas built once in publication.
    - **Types** (`501173a`, −3.7%, peak −25%): literal comparison arenas hold only terms reachable from their roots, smaller term-order nodes, per-thread order-key memo. Plus interner stress tests and a `dialyzer_tsan` cfg (see known issue 1).
    - **Solver** (`3dc0f87`, −9.5%): dense masks, bitset flags, shared dependency slices, integer dependency unions, boxed generation frames, sorted key sets, lazy binding map, per-thread constraint order-key memo.
    - **Refine** (`4965204`, −5.5%): early exits in opaque case warnings, visited-set operand checks, clause underapproximation memo, in-place binder, per-thread `opacity_conflict` memo, boxed body frames.

## How to build and benchmark

Toolchain (Rust 1.92 under asdf):
```sh
T=/Users/simedw/.asdf/installs/rust/1.92.0/toolchains/1.92.0-aarch64-apple-darwin/bin
export CARGO_HOME=/Users/simedw/.asdf/installs/rust/1.88 CARGO_TARGET_DIR="$PWD/target/perf" RUSTC=$T/rustc
$T/cargo build --release --offline --bin core_cache_analyze
$T/cargo test --release --offline --lib
```
For profiling, use a separate target dir with `CARGO_PROFILE_RELEASE_DEBUG=1`, then run `sample <pid>` on macOS. `cargo test --lib` needs the untracked fixture `reports/real-projects/fixes/external-fun/inspect-opts-dbgi.etf`. Fresh worktrees must copy it in from the main checkout.

**Full parity suite** (run after every change):
```sh
bench/phase0/parity.sh [BINARY] [OUT_DIR]
```
It runs Phoenix (normal and `RUST_DIALYZER_VERIFY_FASTPATH=1`), the full 1,082 (normal and verify), and the subset (parallel and `RUST_DIALYZER_SEQUENTIAL=1`). Expected results:
- Phoenix: `otp 199, rust 199, shared 199, otp-only 0, rust-only 0`, sigs `1544 / 1544`.
- Phoenix output hash: `eadaeb93e4b89908ec1bb33c8f53ba01502d5e16` (`bench/phase0/output_hash.py`).
- Full: 400/400 warnings, 42246/42246 signatures.
- Subset: 7433/7433 warnings, 7117/7117 signatures.

**Head-to-head with OTP** (same 600 modules, from scratch):
```sh
S=.cache/phase0/phoenix-scenario
lockf /tmp/rust-dialyzer-run.lock /usr/bin/time -l elixir bench/phase0/run_dialyzer.exs build --plt /tmp/otp.plt --beams $S/beams.txt > /dev/null
lockf /tmp/rust-dialyzer-run.lock /usr/bin/time -l target/perf/release/core_cache_analyze .cache/phase0/rust-cone/out/prior-library.json $S/manifest.json $S/options.json > /tmp/rust.json
```
Delete `/tmp/otp.plt` before each OTP run.

**Benchmarking rules:**
- **Use instructions retired** from `/usr/bin/time -l` as the primary metric. It stays within about 0.2% between runs. Wall time and CPU are noisy, because the machine runs other loads with load average 10–80. Interleave before/after runs, at least 3 of each.
- **Always run the analyzer under `lockf /tmp/rust-dialyzer-run.lock`.** One run peaks at 15–20 GB on a 36 GB machine. Three agents running at once caused an OOM.
- **Watch the disk.** Every separate `CARGO_TARGET_DIR` costs 0.4–1 GB, and subagent worktrees each build their own. On 2026-10-09 `target/` had reached 46 GB and filled the disk.

Useful env vars:

| Variable | Effect |
|---|---|
| `RUST_DIALYZER_TIMINGS=1` | phase timings |
| `RUST_DIALYZER_TIMINGS=2` | per-SCC timings; slow because of lock contention, don't use for totals |
| `RUST_DIALYZER_VERIFY_FASTPATH=1` | recomputes every fast path, memo hit and shadowed data structure the old way and asserts equality |
| `RUST_DIALYZER_SEQUENTIAL=1` | no parallelism |
| `RUST_DIALYZER_SCC_WAVES=1` | the old wave scheduler |
| `RUST_DIALYZER_EXACT_CAPTURE=1` | the full OTP `#state` snapshot in closure captures (see below) |

## Conventions that keep parity safe

- **Every fast path has a shadow check** under `RUST_DIALYZER_VERIFY_FASTPATH=1` (`crate::lattice::verify_fast_paths()`).
- **Interned pointers are never used for ordering or output.** Allocation order is nondeterministic across threads, so pointers serve only for equality, hashing and memo keys.
- **OTP's constraint order must stay exact.** An experiment in `.cache/phase0/otp-shuffle/` showed that other orders make OTP diverge (an `erl_types` sup loop in `compile:strip_columns`) or change signatures (e.g. `Enumerable.List.reduce/3`).
- **Fixpoint re-runs of large SCCs are genuine.** Skipping a re-run where the only changed input is the SCC's own previous output is *not* exact; `Elixir.Kernel.ParallelRequire:files/2` is a counterexample.
- **One commit per change.** Each commit message records the measurement, and nothing is committed without exact parity.

## Known issues and caveats

1. **One unexplained panic, seen twice.** A std BTree `navigate.rs:534` unwrap on `None` in a worker thread. First on `b2af693` (1 of ~34 verify runs), then on 4965204 in the full verify run of the parity suite (peak 27.7 GB, no backtrace). Two re-runs with `RUST_BACKTRACE=1` passed. Both sightings were in verify mode on large runs, where memory is near the machine's 36 GB. That panic means a BTreeMap's length disagrees with its nodes: memory corruption, a data race, or possibly an inconsistent `Ord` during a bulk build or `split_off`. The interner and atom table were cleared on 2026-10-09: review, 12-thread stress tests, and ThreadSanitizer (stable 1.92 with `RUSTC_BOOTSTRAP=1 RUSTFLAGS="-Zsanitizer=thread -Cunsafe-allow-abi-mismatch=sanitizer"`, `--cfg dialyzer_tsan` to drop mimalloc), with 0 reports on the subset analysis. Not audited: `im`, mimalloc, the per-thread memo `get`, and `Ord` impls on BTree keys. Always run with `RUST_BACKTRACE=1` so the next sighting has a stack.
2. **Closure-capture shortcut.** `capture_state()` (`src/constraints/generation_state.rs`) returns a constant stand-in unless `RUST_DIALYZER_EXACT_CAPTURE=1`. All oracles match with it, but `VERIFY_FASTPATH` doesn't cover it.
3. **Probe in the benchmark data.** The Phoenix copy at `.cache/phase0/phoenix/deps/plug/lib/plug/conn.ex` still contains a `__phase0_probe__` edit. All oracles and caches were built from that edited copy, so they are consistent with each other. Removing the edit means rebuilding the Core cache and the oracles.
4. **Compiler warnings.** Two unused imports (`DataflowKey`, `BTreeMap`) produce warnings after the merge.
5. **Intermediate commits.** On the four 2026-10-09 branches, only each branch tip (and some first commits) ran the full parity suite; every commit compiles, and was checked on Phoenix by its agent.

## Next tasks, roughly in priority order

1. **Chase the BTree panic** (known issue 1). Run verify mode with `RUST_BACKTRACE=1` until it reproduces; audit `Ord` impls on BTree keys and the per-thread memo `get`.
2. **Profile the new baseline.** Leftover ideas from the 2026-10-09 agents:
   - **Refinement:** `FunctionName` strings in `BinderState::current_fun` dominate binder clones (~2.8% of refinement); `LabelTrie::insert` `make_mut` copying (~3%); `LetrecTypes::make_mut` copies a whole BTreeMap (~1%); guard leaves clone map and state through `GuardBound`.
   - **Solver:** `State.cache` by dense scope id; `CaseContext` cloned per clause frame only for error recovery; `NativeConstraintArena::group` VecDeque and collects. Remaining solver cost is mostly lattice work (`substitute`, `unify_bindings`, binding-trie `make_mut`).
   - **Types:** a native order key cached on the type node itself instead of two per-thread memos; removing refcounting from per-family storage (large refactor, small gain).
   - **Setup:** the critical path is the call-graph branch (~1.6 s); intern `SourceDigraph` vertices to integer ids. `Program::build_with_contracts` (~0.55 s) is sequential and needs id renumbering to parallelise.
3. **Collapse the constraint pipeline stages.** The native arena, the prepared form and the solver tree each copy the constraints. Estimated at 3–12%.
4. **Wall time.** The critical path is a few giant SCCs and modules analysed on one core each, e.g. `elixir_parser:yeccpars2_612/7` (643 functions, about 1 s × 3 iterations), `cow_http_hd:langtag_region/3`, `erl_parse`, `erl_eval:expr/2`.
5. **Core cache cost.** A fair end-to-end comparison should include building the Core cache, about 6.7 s cold for 1,082 modules on one BEAM VM.
6. **Later product work:** a warm/incremental mode with cached library layers, dialyxir-style; packaging as a `mix` task.

**Running parallel agents:** give each its own files; have them check candidates on Phoenix only and run the full `parity.sh` only before committing (batching allowed), holding the lock only for the analyzer run. Background subagents die if the parent session restarts.

## Research reports (inputs for tasks 2–6)

The three design reviews from 2026-10-09 (two Opus subagents and a Codex `gpt-6-astra` run) agreed on interning plus memoised type operations, which is now done. Their remaining proposals are listed in tasks 3–7. Codex's write-up is at `/tmp/codex-research/answer.md`, and research scratch is under `/tmp/research-*`. These live in `/tmp` and may be gone after a reboot.

## Housekeeping

- **Branches fully contained in `main`, safe to delete:**
  - `perf-analysis` (at `c164bf8`);
  - `worktree-agent-a42e86a81240b6e16`, `worktree-agent-af274a05b242a86ee`, `worktree-agent-aa5264636c58cb6a6`, `worktree-agent-a5380d2e8425eb4a1` (the four 2026-10-09 branches; their worktrees are under `.claude/worktrees/`);
  - `perf-order-keys` and `perf-refinement`;
  - `worktree-agent-a097077f7514985fe` and `worktree-agent-a9e3aa94882594e68`;
  - `remove-firefly-frontend`.
- **Worktrees to remove:** `.claude/worktrees/agent-*` (`git worktree remove`).
- **Deletable:** `.cache/phase0/otp-shuffle/out` (~860 MB).
- **Background:** `bench/phase0/README.md` has the Phase 0 results and the OTP timing tables. `docs/performance-findings-2026-10-08.md` has the earlier performance analysis.
