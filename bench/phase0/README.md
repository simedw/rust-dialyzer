# Phase 0: baseline for a full Dialyzer run

Goal: a full `mix dialyzer` run, including building or updating the PLT, much faster than dialyxir. Correctness means the same multiset of raw dialyxir warnings; order does not matter.

Measured 2026-10-08 on Phoenix 1.8.15 (prod deps), Elixir 1.20.4, OTP 28.3, dialyxir 1.4.7, 14-core Mac. These are single runs on a machine with other load, not medians. Treat them as guides.

## Setup

`.cache/phase0/phoenix` is a copy of `.cache/real-projects/phoenix-v1.8.15` with `{:dialyxir, "1.4.7", only: [:dev, :prod], runtime: false}` added. `.cache/phase0/env` is an isolated `MIX_HOME`/`HEX_HOME`, so a cold run starts with no PLTs.

```sh
W=$PWD/.cache/phase0
export PATH=$PWD/.cache/real-projects/elixir-v1.20.4/bin:$PATH MIX_HOME=$W/env/mix HEX_HOME=$W/env/hex MIX_ENV=prod
cd $W/phoenix && mix dialyzer --format raw   # warnings go to stderr
```

## Scripts

- `run_dialyzer.exs`: builds a PLT from an explicit beam list, or checks beams against a PLT. It calls `:dialyzer.run` exactly as dialyxir does (`warnings: [:unknown]`) and prints raw warnings in dialyxir's format. `--seed` shuffles the file order.
- `warnset.py`: compares two raw outputs as multisets of warning lines.
- `cone.exs`: computes the set of PLT modules the project can reach, using the PLT's own `mod_deps` plus xref calls from the project. It writes beam lists for the whole PLT and the cone.

## Results

### dialyxir end to end

| Scenario | Wall | User + sys |
|---|---:|---:|
| Cold (no PLTs) | 40.1 s | 138.6 + 24.1 s |
| ↳ Erlang core PLT, 201 modules | 7.9 s | |
| ↳ Elixir PLT, 271 modules | 5.9 s | |
| ↳ deps PLT, 591 modules | 22.0 s | |
| ↳ project analysis, 93 modules | 1.6 s | |
| Warm, nothing changed | 2.5 s | 3.2 + 1.7 s |
| Forced PLT check, nothing changed | 5.1 s | 5.9 + 2.1 s |
| Forced PLT check, `Plug.Conn` edited | 13.2 s | 24.0 + 6.2 s |

Warnings: 199 (150 `warn_unknown`, 21 `warn_undefined_callbacks`, 18 `warn_return_no_exit`, 5 `warn_failing_call`, 3 `warn_behaviour`, 1 `warn_contract_types`, 1 `warn_not_called`). `run_dialyzer.exs` reproduces the same multiset.

dialyxir checks PLT staleness only by hashing `mix.lock`. Editing a dependency without a lock change leaves the PLT stale. `--force-check` emulates a version bump.

### PLT shape (all give the identical 199 warnings)

| PLT built from | Modules | Build wall | Build user |
|---|---:|---:|---:|
| Everything, one joint build | 1,082 | 30.2 s | 126.2 s |
| Everything except the project | 990 | 27.6 s | 123.7 s |
| Cone + project | 600 | 20.7 s | 88.3 s |
| Cone only | 507 | 18.8 s | 82.0 s |
| Cone only, 1 scheduler | 507 | 87.4 s | 61.6 s (+6.4 sys) |

- dialyxir's three layers give the same result as one joint analysis.
- dialyxir puts the project into its deps PLT, so a cold run analyzes the project twice. That is unnecessary.
- The cone drops 53% of modules but only about 35% of CPU. The expensive modules (Elixir stdlib, `stdlib`, `kernel`) are always reachable.
- OTP's single-core cost is about 130 ms of CPU per module. 14 schedulers give about a 5× wall-time speedup.

### Determinism

All of these gave the identical 199 warnings:
- the project checked on 1 scheduler and on 14;
- the project checked with 3 shuffled file orders;
- the cone PLT rebuilt on 1 scheduler and with 2 shuffled orders.

Dialyzer computes its own callees-first SCC order regardless of file order. So this shows results don't depend on thread interleaving within that order. It does not show that any order works.

### Rust on the cone: it cannot build a PLT yet

I gave the integrated release (`native-elixir-dialyzer`, SHA-256 `4c2cfa52…`) the 507 cone modules as targets, with a one-module stub PLT. The harness setup is in `.cache/phase0/rust-cone`. The run aborts. Rust has only been checked on project code analyzed against facts exported from an OTP-built PLT. It has not been run on OTP, Elixir and dependency code as targets.

The failures found so far (`.cache/phase0/rust-cone/out/rejected.txt`):

- **22 modules use OTP 28 strict or zip generators** (`generate_strict`, `b_generate_strict`, `zip`), which the abstract-forms decoder rejects. Examples are 15 `compiler` modules, `beam_lib`, `erl_lint`, `rand`, `qlc_pt` and `crypto`.
- **6 panics in the frontend.** `erlang` (record-index pattern in `syntax_erl/src/visit.rs:236`), `net_kernel`, `tls_sender` and `cowboy_http` (`unreachable`), `base64` (type construct `Unmatch`), `dets_v9` (bitsize assertion).
- **4 semantic errors on `-compile` options:** `gen_server`, `global`, `global_group`, `inet_gethost_native`.
- **2 unbound-variable errors:** `prim_tty`, `qlc`.
- **At least one `Native(FunctionClause)` analysis error** without a module name, left after removing the above (473 modules remaining). Most modules also report it when run alone (`probe_modules.py`), probably because callees like `erlang` are missing, so per-module probing can't locate it.

### Rust vs OTP CPU on the same work

Elixir 1.20.4's 447 modules as targets, against the same dependency PLT (`reports/real-projects/elixir-merged-release-benchmark` inputs). Both report the same 156 warnings. Single runs.

| | Wall | User + sys | Peak memory |
|---|---:|---:|---:|
| Rust (integrated release) | 64.5 s | 65.6 + 5.2 s | 6.35 GB |
| OTP, 14 schedulers | 8.1 s | 18.5 + 4.0 s | |
| OTP, 1 scheduler | 40.6 s | 28.8 + 3.4 s | |

- Rust runs essentially single-threaded (about 1.1 cores).
- **Rust uses 2.2–3.1× more CPU than OTP** for the same analysis.
- Rust is 1.6× slower than OTP even when OTP has only one scheduler.

Parallelism alone can't close the gap. A perfect 8× parallel speedup would bring Rust to about 8 s, roughly OTP's current wall time. The per-unit cost has to drop too.

### Core from OTP instead of the Rust frontend

Every frontend failure above comes from reimplementing OTP's compiler step in Rust. Dialyzer itself compiles each module's `debug_info` to Core with OTP's compiler. `prototypes/core_cache/core_cache.exs` does the same, once per BEAM content hash, into a per-module cache (`src/bin/core_cache_analyze.rs` loads it).

- Cold Core cache for all 1,082 PLT modules: 6.7 s (one VM, 14 schedulers). Warm check: 0.16 s plus about 0.5 s of VM startup.
- The cache is still JSON (737 MB). Binary ETF is next.

Analysis bugs found and fixed on the way to a clean run:

1. **Guard depth cap of 128** (`guard_control.rs`, `underapprox.rs`) rejected cowlib's macro-generated guards and OTP's `json`. Now `MAX_GUARD_DEPTH = 16_384`. The analysis thread and `ordered_prefetch` workers get large stacks to match.
2. **Record override keys** (`resolver.rs::native_record_values`) were looked up in a term order built only from the record definition. `#r{f :: T}` overrides in specs (OTP's `cerl`) failed with "term ID outside arena".
3. **Built-in type redefinitions in `erlang`** (`resolver.rs::resolve_contract`). OTP rewrites locally redefined built-in type names except in module `erlang` (`dialyzer_utils:core_to_attr_tuples/1`). Rust skipped that exception for contracts, so `no_return()` became `none()` instead of `unit()`, and NIF stubs looked like they never return. That bug alone accounted for 1,544 extra warnings across 62 modules.

Analysis errors now name the phase, module and contract where they occur.

**Result: all 1,082 modules analyzed from scratch (no PLT) match OTP exactly. 222 of 222 warning terms are identical; none are missing or extra.** Same stub PLT and warning options for both. `warn_unknown` is disabled, and signatures are not compared yet.

| Same 1,082 modules | Wall | CPU | Peak memory |
|---|---:|---:|---:|
| OTP (`oracle.exs`) | 22.4 s | 119.9 s | — |
| Rust (`core_cache_analyze`) | 416.7 s | 431.5 s | 11.0 GB |

Rust is 19× slower in wall time and uses 3.6× more CPU. It is essentially single-threaded. Loading 737 MB of JSON accounts for about 15 s.

### Full correctness and the binary cache

`full_oracle.exs` (derived from the inferred-signature oracle, with `--unknown`) captures OTP's warnings and every named signature, private functions included. Against it, the Rust analysis of all 1,082 modules matches exactly:

- **Warnings with `warn_unknown` enabled:** 400 of 400 identical (`compare_warnings.py`).
- **Inferred signatures:** 42,246 of 42,246 identical, none missing or extra (`reports/real-projects/fixes/inferred-signatures/compare.py`).

The cache is now compact ETF (`src/core/etf.rs`; `CoreExport.build_module(..., :etf)`). Index lists are packed u32 binaries. Modules decode in parallel straight into `CoreModule` and are validated by `CoreProgram::from_modules`. The JSON reader shares node construction through `RawNode`/`build_node`.

| Cache | Cold build | Size | Rust load |
|---|---:|---:|---:|
| JSON | 6.7 s | 737 MB | ~15 s |
| ETF | 5.3 s | 502 MB | 2.5 s |

Results from the ETF cache are identical: 400/400 warnings and 42,246/42,246 signatures. Analysis itself is unchanged at about 415 s, against OTP's 22 s.

## Open items

- Locate the remaining `FunctionClause` failure. Then make all 1,082 PLT modules go through the Rust frontend and analyzer.
- A real dependency version bump (lock change) rather than a single-module edit.
- A second project, to check that the cone result generalizes.
