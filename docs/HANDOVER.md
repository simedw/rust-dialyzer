# Handover: rust-dialyzer performance work

Last updated 2026-10-09. `main` = `perf-analysis` at `e3ed138` (plus the commit adding this file).

## Goal

A Rust reimplementation of OTP 28 Dialyzer for Elixir projects that is much faster than `mix dialyzer` (dialyxir) and reports **exactly** the same warnings. The order of warnings doesn't matter. The current focus is the **full from-scratch run** (no PLT), because that is where Rust can gain most over OTP.

**Exact parity is the hard constraint.** Every change must reproduce OTP's warnings and its inferred signatures for every function, including private ones, bit for bit.

## Where we are

Phoenix scenario: 600 modules from scratch. That is the 93 Phoenix modules plus the 507 OTP/Elixir/deps modules they reach, with no PLT. Measured back to back on 2026-10-09 at load 13–16, 14 cores, 36 GB RAM:

| | Wall | CPU (user+sys) | Instructions |
|---|---:|---:|---:|
| OTP dialyzer (`run_dialyzer.exs build`, 14 schedulers) | 17.0–17.4 s | ~83 s | 0.89 T |
| Rust `core_cache_analyze` @ e3ed138 | **12.1–12.5 s** | **~69 s** | **0.68 T** |

Caveat: OTP's time includes compiling beams to Core. Rust reads a prebuilt Core cache instead. A cold cache build for 1,082 modules takes about 6.7 s; a warm cache check takes about 0.2 s.

Parity at e3ed138, all exact:
- **Phoenix:** 199/199 warnings, 1544/1544 signatures.
- **Full 1,082 modules:** 400/400 warnings, 42,246/42,246 signatures.
- **Subset:** 7,433 warnings and 7,117 signatures, both parallel and sequential.
- **Unit tests:** 248 pass.

Peak memory is 14.9 GB on Phoenix and 19.6 GB on the full run.

Timeline on Phoenix: start of session about 44 s wall (6.6 T instructions on the full run). After micro-optimisation and parallelism, about 20 s and 1.13 T. After the type and refinement redesign, 12 s and 0.68 T.

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

1. **One unexplained panic.** It happened once, in 1 of about 34 verify-mode runs, on the step-4 interning binary (`b2af693`): a std BTree `navigate.rs:534` unwrap on `None` in a worker thread. It hasn't reproduced since (12 clean verify runs at `087169e`, all with identical output). A latent race in the new lock-free type table (`src/lattice.rs` interner, the `src/atom.rs` pattern) can't be ruled out. **Run ThreadSanitizer or Miri on the interner before relying on this in production.**
2. **Closure-capture shortcut.** `capture_state()` (`src/constraints/generation_state.rs`) returns a constant stand-in unless `RUST_DIALYZER_EXACT_CAPTURE=1`. All oracles match with it, but `VERIFY_FASTPATH` doesn't cover it.
3. **Probe in the benchmark data.** The Phoenix copy at `.cache/phase0/phoenix/deps/plug/lib/plug/conn.ex` still contains a `__phase0_probe__` edit. All oracles and caches were built from that edited copy, so they are consistent with each other. Removing the edit means rebuilding the Core cache and the oracles.
4. **Compiler warnings.** Two unused imports (`DataflowKey`, `BTreeMap`) produce warnings after the merge.

## Next tasks, roughly in priority order

1. **Race check of the interner** (issue 1 above). ThreadSanitizer needs a nightly toolchain with `-Zsanitizer=thread`; alternatively, use Miri on the interner unit tests or a loom-style test.
2. **Profile the new baseline** before choosing the next targets; the profile has shifted. Last known large items, at about 0.79 T:
   - `memmove` about 12%;
   - malloc/free about 6–7%;
   - `TermOrderArena::append_core_term` 3.7% (closure-capture terms for ordering);
   - `DataflowNodes::native_subtrees` 2.6%;
   - `ConstraintTree::solve` self 2.5%.
3. **Refinement leftovers** (from the `perf-refinement` agent):
   - stop cloning the whole `BinderState` per clause or guard visit;
   - cut `collect`/`Vec` churn in `evaluate` and `finish_collection`;
   - skip `mark_fresh` writes of `any` for pattern variables on clauses that fail to match;
   - `DataflowMap` is still about 8% of refinement (trie get, inserts, dropping discarded clause maps).
4. **Solver bookkeeping:** dense per-SCC variable ids plus bitsets for flags and restart, and sorted vecs for masks and updated/affected sets. Estimated ~5%.
5. **Collapse the constraint pipeline stages.** The native arena, the prepared form and the solver tree each copy the constraints. Collapsing them is estimated at 3–12%.
6. **Inside the type nodes:** take reference counting out of the per-family storage, and add a per-thread cache in front of the interner (each a small %).
7. **Wall time.** The critical path is a few giant SCCs and modules analysed on one core each, e.g. `elixir_parser:yeccpars2_612/7` (643 functions, about 1 s × 3 iterations), `cow_http_hd:langtag_region/3`, `erl_parse`, `erl_eval:expr/2`. Making those single units cheaper is the main wall-time lever left.
8. **Core cache cost.** A fair end-to-end comparison should include building the Core cache, which takes about 6.7 s cold for 1,082 modules on one BEAM VM. Consider making it faster or incremental.
9. **Later product work:**
   - a warm/incremental mode with cached library layers, dialyxir-style, for the everyday case;
   - packaging as a `mix` task.

## Research reports (inputs for tasks 2–6)

The three design reviews from 2026-10-09 (two Opus subagents and a Codex `gpt-6-astra` run) agreed on interning plus memoised type operations, which is now done. Their remaining proposals are listed in tasks 3–7. Codex's write-up is at `/tmp/codex-research/answer.md`, and research scratch is under `/tmp/research-*`. These live in `/tmp` and may be gone after a reboot.

## Housekeeping

- **Branches fully contained in `main`, safe to delete:**
  - `perf-analysis` (identical to main);
  - `perf-order-keys` and `perf-refinement`;
  - `worktree-agent-a097077f7514985fe` and `worktree-agent-a9e3aa94882594e68`;
  - `remove-firefly-frontend`.
- **Worktrees to remove:** `.claude/worktrees/agent-*` (`git worktree remove`).
- **Deletable:** `.cache/phase0/otp-shuffle/out` (~860 MB).
- **Background:** `bench/phase0/README.md` has the Phase 0 results and the OTP timing tables. `docs/performance-findings-2026-10-08.md` has the earlier performance analysis.
