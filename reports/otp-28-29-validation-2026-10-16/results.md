# OTP 28 and OTP 29 validation

Publication check, 2026-10-17: The branch excludes pre-existing tuple-storage and benchmark edits. Its exact source passed 803 enabled workspace tests, 79,612 native raw-type comparisons, and 2,408 native record comparisons. Eight optional tests remain ignored. See [branch-checks.json](branch-checks.json) and [the workspace log](branch-workspace-tests.log.gz). The broader release comparisons below describe the original working tree, which included those independent optimizations.

Validated on 2026-10-16 with authenticated OTP 28.3 / Dialyzer 5.4 and OTP 29.1.2 / Dialyzer 6.0.4.

Both workers run analysis in Rust. The launcher selects the matching worker from its own directory. OTP 29 has a separate compiled type domain, compiler rules and native record analysis.

Fixed OTP 28 impossible-alias lowering and the boxed ExternalFun test patterns. OTP 29 now includes native record transport, type operations and analysis, plus compiler changes for records and comprehensions. Type-alias arguments resolve records in the caller's module, which fixes library loading in the command-line path.

| Check | OTP 28.3 | OTP 29.1.2 |
|---|---:|---:|
| Enabled Rust release tests | 417 passed | 396 passed |
| Type comparisons, each mode | 653,626 | 659,256 |
| BIF comparisons, each mode | 48,165 | 54,073 |
| Constraint comparisons, each mode | 60,768 | 60,768 |
| Captured closure pairs, each mode | 6,561 | 6,561 |
| Exact warning cases | 327 | 330, each mode |
| Additional native record cases, each mode | n/a | 1,204 |
| Original deterministic erl_types suite cases | 3 | 3 |
| Original property law trials | 30,000 | 30,000 |

Each mode means normal execution and RUST_DIALYZER_VERIFY_FASTPATH=1. Type comparisons include exact native errors and bounded upstream nontermination checks.

The OTP 29 end-to-end record/comprehension tests match all 21 exported signatures and 10 ordered public warnings with an empty prior. The runtime oracle checks 18 terms, including declaration-order variants, through ETF transport, three hashes and all 324 pairwise comparisons. The compiler tests compare complete raw and cleaned Core and all three adapters.

OTP 28 project parity also passed:

- 1,082 modules: 42,246 exact signatures and 400 exact warnings, normal and VERIFY.
- 217-module subset: 7,117 exact signatures and 7,433 exact warnings, normal and sequential.
- Phoenix: 1,544 exact signatures and 199 exact warnings, normal and VERIFY.

Both command-line runs analyzed their project with OTP and Elixir libraries and emitted the expected invalid-spec warning with exit code 2.

The first two large-project comparison commands failed because the old comparator could not read gzip oracle files. The analyzer outputs were intact. After fixing the reader, comparison of those saved outputs passed exactly. The JSON report retains the original errors and the recovery evidence.

## Scope and skipped tests

The compatibility claim is limited to the authenticated pinned distributions, not every build of either major.

This report covers repository tests and native Dialyzer differential checks. It does not claim to run the complete upstream Erlang/ERTS test distribution.

Type probes cover their declared API operations. They do not cover every erl_types metadata helper. Constraint probes cover atomic/scoped constraints and closure ordering; the public end-to-end tests cover the production analysis path separately.

Large-project checks use OTP 28 cached compiler inputs. The 327-case OTP 29 warning replay uses the same compiled BEAM fixtures with fresh native OTP 29 library PLTs and warning oracles. OTP 29 compilation is tested independently by raw/clean Core, native-record/comprehension fixtures and three added upstream cases.

Native errors and bounded upstream nontermination are checked outcomes in the differential type tests, not skipped cases.

Project timings were collected under concurrent compiler load and are not a performance comparison.

The default suites ignored 6 tests on OTP 28 and 2 on OTP 29. The available OTP 28 location diagnostic was then run explicitly and passed. The remaining optional fixture tests were not run:

- OTP 28: beam_frontend::erl_prepare::tests::record_preparation_oracles ... ignored, requires RECORD_PREP_ORACLES fixture directory
- OTP 28: entire_corpus_adapter_matches_compiler_transport ... ignored, requires independently captured corpus compiler oracles and manifest
- OTP 28: corpus::entire_corpus_direct_adapter_matches_compiler_transport ... ignored, requires independently captured corpus compiler oracles and manifest
- OTP 28: full_corpus_emission_and_atom_scope_parity ... ignored, requires the saved whole-workload BEAM inventory and runtime layouts
- OTP 28: complete_native_runtime_scope_analysis ... ignored, requires complete independently captured compiler Core and analyzer inputs
- OTP 29: beam_frontend::erl_prepare::tests::record_preparation_oracles ... ignored, requires RECORD_PREP_ORACLES fixture directory
- OTP 29: temporary_contracts::location_snapshot_tests::actual_location_cost_diagnostic ... ignored, actual retained Core population diagnostic; run explicitly

## Reproduce

Build and native comparison commands are in [README.md](../../README.md#tests). The warning runner requires the cached original case inventory. Machine-readable results, source hashes, command lines and evidence hashes are in [results.json](results.json). The complete release test logs are stored beside it as gzip files.
