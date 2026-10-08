# Library lookup design

Fixture 320 on accepted candidate20 spends about 0.306 seconds creating the
call world and 0.004 seconds running inference. Candidate21 splits setup into
0.039 seconds converting library signatures and 0.267 seconds converting library
contracts. These are two measured traced rounds, not speedup evidence.

`CorePhases::new` calls `CallWorld::from_library`, which scans every imported
summary and contract. The world then receives current target signatures and
activated contracts through `CorePhases::synchronize`. Body calls, constraint
generation, and function warnings consume those facts. The library belongs to
the enclosing compiled analysis and outlives the pipeline.

The original `dialyzer_dataflow:state__fun_info` looks up a signature and contract
for the reached MFA. It does not normalize every imported contract at setup.

## Alternatives

1. Filter imported facts using the static callgraph. This would preserve the
   existing owned maps, but the graph is not a complete list of dynamic calls
   discovered by analysis. Reject this design rather than adding call-specific
   exceptions.
2. Borrow the complete library and look up facts when consumers request them.
   Keep owned current-phase overrides separate. This covers arbitrary reached
   MFAs and removes the scan without removing available facts. Select this design.

## Caller and type sketch

The intended call sequence is `CallWorld::from_library(&library)`, followed by
current-phase publication and named fact lookups. Construction becomes infallible;
lookup owns conversion and its errors.

```rust,ignore
struct CallWorld<'library> {
    library: Option<&'library Store>,
    // Existing owned target signatures, active contracts, anonymous signatures,
    // and closure call sites remain phase-owned.
}

impl<'library> CallWorld<'library> {
    fn from_library(library: &'library Store) -> Self;
    fn named_signature(&self, name: &Mfa) -> Result<Option<Signature>, String>;
    fn active_contract(&self, name: &Mfa) -> Result<Option<ActiveContract>, String>;
}
```

Owned target facts take precedence over the borrowed library. Missing facts return
`None`; failed reached conversions return errors. Contract conversion can reuse
the existing per-contract `OnceLock`. Do not add a shared mutable cache or a
function-name whitelist. Further caching of reached signatures needs measurements.

## Migration and proof

Migrate generation, body-call candidates, and function warnings to the same
lookup methods. Thread the library lifetime through `CorePhases`, `BodyCalls`,
and `BodyFlow`. Update inspection probes to enumerate actual library names and
call the lookup API; inspecting override maps would silently omit library facts.

Acceptance needs all imported signature values and raw records compared with
accepted20, all Elixir fixture diagnostics/signatures unchanged, original body
call and function-warning checks, and fresh public PLT lifecycle checks. A public
probe must separately prove that an unused malformed library contract is ignored
by Native and by the new path. Preserve earlier failed probes and distinguish
export/import failures from call-world conversion failures.

Implement in a fresh candidate22. Candidate21 is instrumentation only; accepted20
remains the baseline until the change and its complete consumer migration pass.

## Acceptance

Candidate22 passes the planned migration checks. Five untraced pairs reduce
fixture320 median2.902914s to2.663806s,8.24%. The final traced pipeline is
0.004846s versus0.314097s in accepted20. All3314 imported records and5407
signatures,21750 original calls,12000 function warnings, five original classic
lifecycle cases,288 target-mode diagnostic cases and27 SCC cases match. All20
boundary statuses match Native; exact error reasons are not claimed. Baseline20
already matched those20 statuses, so the historical gap assertion was replaced
with strict agreement. An unused malformed contract now matches Native. The
tiny/all19 fixture warning gate and tiny Native PLT tables pass. A larger build
preserves the previous Rust tables, including the documented Native contract gap.
Root157 tests, all-target compilation and strict library Clippy pass;31 patch
files replay exactly. Full public CLI migration and incremental acceptance remain
open.
