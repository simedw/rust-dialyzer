# Dialyzer compatibility coverage

The upstream suites give this experiment a concrete compatibility target. They cover much more than the initial fixture. A finite regression suite cannot prove equivalence for every Erlang or Elixir program.

## Sources and inventory

The source revisions are pinned in `corpus/sources.json`. Downloaded code and its original licenses remain under `.cache/upstream`; no upstream test or golden result is edited.

| Source | Pinned version | Registered cases | Input programs |
| --- | --- | ---: | ---: |
| [OTP semantic suites](https://github.com/erlang/otp/tree/ab489c0b260c637a9487f8753909ea94dacbbec2/lib/dialyzer/test) | OTP 28.3 | 307 in 22 suites | 526 recursive `.erl` files |
| [Elixir Dialyzer integration](https://github.com/elixir-lang/elixir/blob/794441a2554f100ef1e58b77b4429cd6390f4f31/lib/elixir/test/elixir/kernel/dialyzer_test.exs) | Elixir 1.19.4 | 19 | 18 files, 21 modules |
| [Dialyxir warning examples](https://github.com/jeremyjh/dialyxir/tree/13bcb7da40d609f280840e311cdeadfdadc2ca39/test/examples) | Dialyxir 1.4.8 | 1 combined analysis | 17 files, 23 modules |

OTP cases include 252 individual source files and 55 directories analyzed as groups. The harness analyzes immediate `.erl` siblings in a group, not every nested source recursively. Its separate 526-file inventory includes historical application trees.

Elixir's 19 cases are clean-code regression assertions. One enables `specdiffs`; callbacks and macrocallbacks analyze their selected modules together. Dialyxir's examples exercise warnings but have no upstream semantic goldens. Their combined result is a live OTP reference, explicitly labeled that way.

`corpus/inventory.json` records every semantic case, source hashes, native option-file contents, original golden locations, and the handwritten suite registries. Native Erlang `all/0` and `groups/0` provide the operational case names.

## Current acceptance

Full parity remains open. Frozen engine candidate 3 matches all 327 default-profile public warning groups and 2,617 ordered warning events; Root independently reverified its 273 source and 922 oracle artifact hashes. This checkpoint predates the common PLT and raw dependency successor integrations. Its composed and suite-profile replays terminated during disk exhaustion and have not passed. Those gates must be rerun, followed by final integrated acceptance. These engine gates do not establish public CLI or cache lifecycle parity.

Canonical raw declarations, global contract/callback publication, diagnostic modes and phase ordering, classic persistence, and the proper-header classic checked planner are integrated. Public CLI migration, complete incremental persistence, and late interpretation of malformed imported metadata remain active implementation work. Performance work follows complete acceptance on the final implementation.

`corpus/operational_cases.json` records the distinct 150 operational cases in 15 coverage groups and binds their Original sources. Its predicate index records no execution passes; the full Original case and helper bodies define acceptance. Fresh Root verification runs the three Original deterministic type cases and 30,000 Original property trials. `scripts/verify_operational_consistency.py --out /fresh/path` executes all 22 generated registry case functions and 22 omitted-case failure controls through the Rust registry boundary. It supplies registry case data directly and makes no claim about PLT-producing suite setup. The remaining public operational cases need their actual operation paths.

Detailed scoped evidence and remaining work are recorded in `docs/tasks.md` and `docs/core-integration.md`. Historical checkpoints below explain the initial gaps.

## Historical Forms prototype baseline

The complete 327-case semantic assessment ran on Elixir 1.19.4 and Dialyzer 5.4 / OTP 28. The result is:

| Status | Cases |
| --- | ---: |
| Diagnostic parity | 2 |
| Unsupported syntax, types, or dependencies | 305 |
| Unsupported warning/options profile | 17 |
| Diagnostic mismatch | 2 |
| Upstream golden mismatch | 1 |

The two parity cases are Elixir's valid callback and private-macro fixtures. No OTP semantic case has earned parity yet. Matching a clean callback fixture does not establish callback validation: the two genuine mismatches are `behaviour/sample_behaviour_old` and `behaviour/undefined_callbacks`, where Rust misses callback diagnostics.

All 307 OTP cases compile to BEAM after the adapter accounts for module names that differ from filenames. Native OTP agrees with 306 upstream goldens. `opaque/ewgi` remains a reference failure: its expected warning locations lag the source by one line. A causal probe removes the single added copyright comment line in temporary source copies: all three warning lines then match the untouched golden. `scripts/verify_golden_drift.py` retains hashes and the original failure; it does not alter the semantic oracle or award Rust parity. All 19 official Elixir integration tests independently pass with the installed matching compiler.

The run observed 46 warning message categories. The initial Forms prototype emitted only `call`, `no_return`, and `invalid_contract`. Those categories required much broader semantic coverage.

`corpus/baseline.json` preserves the assessment summary and each case status. `.cache/corpus/report.json` adds full local artifact paths, observed categories, first blocking reasons, and warning differences. Every case has its raw OTP warning terms and original formatted messages beside its exported compiler representation.

## What the upstream tests exercise

| Dimension | Evidence | Rust work remaining |
| --- | --- | --- |
| Type lattice | `erl_types_SUITE`, `property_test/erl_types_prop.erl` | Numeric bounds, atom sets, bitstrings, function types, maps, records, recursive types, opaque types, canonical unions and depth limiting |
| Inference and call graphs | `small`, `callgraph`, `user`, `r9c` | Higher-order calls, closures, recursive relationships, richer return constraints and OTP call signatures |
| Control flow | `small`, Elixir integration fixtures | Cases, disjunctive guards, comprehensions, receives, exceptions, compiler-generated `cond`, `with`, and `try` |
| Data construction and matching | `map`, `small`, `opaque` | Map updates, record layouts, binary segments, patterns, unreachable clauses and improper lists |
| Specs and contracts | `specdiffs`, `overspecs`, `underspecs`, `overlapping_contract`, `extra_return`, `missing_return`, `options3` | Aliases, remote and opaque types, type variables, bounded and overloaded specs, caller contract constraints and spec warning modes |
| Behaviours and callbacks | `behaviour`, Elixir callback fixtures | Preserve attributes and validate callback implementations and callback specs |
| Warning policy and rendering | `nowarn_function`, `unmatched_returns`, `line`, `indent`, `indent2`, `options1`, `options2` | Suppression attributes, optional warning switches, source annotations, columns and message payloads |
| Tool lifecycle | `cplt_SUITE`, `iplt_SUITE`, `incremental_SUITE`, `dialyzer_SUITE`, `dialyzer_cl_SUITE` | Equivalent Rust tests for cache creation, mutation, invalidation, format rejection, CLI and API behavior |
| Supporting tools | `abstract_SUITE`, `dialyzer_utils_SUITE`, `typer_SUITE` | Generated abstract-code checking, utility behavior and inferred-type export |
| Elixir compilation | Official Elixir suite | Rewrites, macrocallbacks, structs, protocols, assertions, opaque Elixir library types and debug information |

The first blocking reason for a case is not a complete list of its features. Adding support for that reason may reveal further blockers in the same program.

There are 128 handwritten OTP Common Test cases and 22 generated suite-consistency checks recorded separately as `not_ported`. They are not included in the semantic parity denominator. The 128 include three deterministic lattice tests and six PropEr properties. The properties request 5,000 trials each for absorption, associativity, commutativity, idempotence, identity and depth limiting. Missing PropEr or its abstract-code generator must remain a visible skip.

Elixir's separate compiler type-checker tests and typespec translation tests are useful frontend references. They are not Dialyzer parity cases and are not included in these counts. Dialyxir wrapper and message-formatting tests likewise do not prove analyzer semantics.

## Runner and gates

From the repository root:

```sh
python3 scripts/corpus.py fetch
python3 scripts/corpus.py inventory
python3 scripts/corpus.py run
```

The OTP runner compiles the original upstream helpers and checks source analysis against the original goldens using the native harness. It builds a full `erts/kernel/stdlib` PLT, preserves per-suite warning settings, include paths, macros and grouped modules, then separately checks the BEAM analysis. Source and BEAM raw warning differences remain frontend failures.

The Elixir runner uses the official suite's PLT profile of `erts` plus 25 selected modules. It compiles all fixtures once and analyzes the exact modules selected by each case. Dialyxir retains its configured warning flags and all observed warning categories.

Rust consumes the same shared BEAM exporter as the original proof of concept. Diagnostic comparison includes module, function, arity, source line, message category and warning tag. This is diagnostic-identity parity; complete message payloads, inferred typings and source-column parity are separate future targets.

To iterate on Rust using the saved oracles:

```sh
python3 scripts/corpus.py run --reuse-oracles
python3 scripts/corpus.py run --reuse-oracles --require-parity
```

Cached oracle reuse requires the same source revisions and runtime versions. The first command completes an assessment even when compatibility is incomplete. Its report sets `parity_complete` to false. `--require-parity` exits nonzero for any selected unsupported, mismatched, failed or missing case. It currently fails, as it should.

Select cases without removing the rest from the coverage ledger:

```sh
python3 scripts/corpus.py run --suite small
python3 scripts/corpus.py run --kind elixir
python3 scripts/corpus.py run --reuse-oracles --case elixir/14_no_warnings_on_defmacrop --require-parity
```

The default per-case OTP deadline is 60 seconds. `--timeout` changes it; timeouts never become passing cases. Skipped selections remain `not_run` in the report.

## Compatibility work order

1. Preserve all semantic compiler attributes and build a richer type model. Use the upstream lattice laws as tests for unions, intersections, subtype relations and canonicalization before expanding inference.
2. Support type aliases, remote types, bounded specs and valid contracts at call sites. Unsupported type forms are the most common first blocker.
3. Add branch and pattern constraints, then maps, records, binaries and comprehensions. Case expressions are the largest control-flow blocker.
4. Add function types, closures and higher-order calls. Extend the fixpoint algorithm only against these differential cases.
5. Add behaviour/callback diagnostics, warning switches, suppression and full OTP signatures. Keep all warning categories visible.
6. Port the 128 lifecycle, API, utility and type cases into observable Rust contracts. Add incremental edit sequences and cache equivalence checks.
7. Add differential generated programs and type-operation fuzzing with reproducible seeds and minimized counterexamples. Upstream regressions plus properties and generated tests give stronger evidence than a fixed corpus alone.

Performance comparisons should follow diagnostic and type-model parity for the selected workload. The earlier 42-times worklist improvement measures this prototype's own solver on a synthetic chain, not broad Dialyzer compatibility.

## Coherent implementation sets

The full compatibility architecture and executable gates are in `docs/parity.md`. `corpus/features.json` now scans complete source metadata and Core for all 327 cases, including compiler-generated definitions. It records dependencies and overlaps for contracts 163, control flow 127, named types 107, records 85, higher-order functions 86, maps 48, opacity 35, behaviours 25 and recursion 57. These counts overlap and describe input features, not completed support.

The compiler interchange gate passes all 327 actual inputs plus every Core node kind and literal edge cases. The type-operation gate passes 214,807 scalar/structural/callable/map comparisons. The Core scalar gate matches four whole modules, including typed warning details and exported success typings. The earlier abstract-form baseline remains 2/327; selected Core matches do not establish complete corpus parity.
