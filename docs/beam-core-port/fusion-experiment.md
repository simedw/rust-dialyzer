# Core emission fusion experiment

## Phases

- [x] Trace compiler, cleanup, atom context and adapter ownership.
- [x] Compare two independent designs and select the first experiments.
- [x] Verify direct typed-node construction against the existing adapter.
- [x] Verify a selective cleanup view and fused emission against the existing complete pipeline.
- [x] Measure allocations separately from timing, then benchmark complete workloads.
- [x] Retain or reject the experimental paths from measured results.

## Design decision

Both independent designs recommend retaining the proven raw lowerer for this first experiment. Its splitting, shadow renaming, exports/liveness and annotation passes inspect and rewrite provisional Core trees. Replacing only its final node constructors would not eliminate those intermediate trees. A compact editable compiler IR is a larger follow-on experiment.

Test two changes separately. `core_emission: direct` avoids the adapter's RawNode conversion, repeated child-group construction and per-node validation formatting, while retaining complete program validation. `core_emission: fused` also replaces materialized cleanup with a selective view over raw Core. Ordinary nodes and lists are not rebuilt; only sparse goto-letrec replacements allocate generic wrappers. Both emit the same final compact terms and typed Core nodes as the reference path.

The view owns cleanup semantics, including untouched patterns and binders, filtered case clauses, and transformed-root ignored annotations. A shared description drives both exact reachable atom discovery and final emission. Atom collection must exclude dropped raw branches and include synthetic cleanup atoms and Core record tags. Map ordering requires the complete cross-module atom namespace before adaptation. Node IDs remain occurrence-based, and annotations/literals retain the reference interning order.

The reference remains selectable through `core_emission: reference`, the default. No type-checker algorithm changes are part of this experiment. Input manifests select the path explicitly; there is no compiler fallback.

Allocation diagnostics use a separate counted MiMalloc binary, enabled only during frontend loading and validation. Counts include allocation/zeroed allocation calls, reallocation calls and cumulative requested bytes across all participating threads. Requested bytes are cumulative traffic, not resident or live bytes. Atomic counters perturb timing; whole-system timings use the normal uninstrumented analyzer.

## Grounding and alternatives

`compile_beam` recovers debug metadata, lifts forms, lowers raw Core and materializes cleanup. Stages share prefixes, so cleanup does not copy the whole raw arena, but it appends replacement records/lists and retains raw intermediates. `pipeline::assemble` keeps all compilations through context creation and adaptation. `adapter::Builder` compacts values and creates postorder typed node occurrences through a generic RawNode validator.

Design A prioritizes removing adapter grouping/validation allocations before introducing the cleanup view. Design B prioritizes the removable cleanup representation, with direct construction as an independent control. The synthesis keeps both modes to attribute changes separately. Both reject an immediate lowerer rewrite because provisional term identity and mutable tree passes are pervasive. Complete module arrays, atom scopes, warning JSON and signatures are correctness gates; wall, CPU, allocation traffic and peak RSS determine whether fusion is useful.

## Measured decision

All fixture, whole-corpus and full-output checks pass. Direct emission removes about 10 million allocation calls per workload and roughly six million reallocations. The fused view saves fewer calls and increases cumulative requested bytes. Whole-run direct timings show modest nominal gains with substantial variation; fused cleanup adds no consistent benefit. Keep both paths explicit on the experimental branch, with reference as default. Do not promote this view implementation. A borrowed child cursor and a compact provisional compiler IR remain distinct next candidates. See the complete evidence in `reports/core-emission-fusion-2026-10-12/results.md`.
