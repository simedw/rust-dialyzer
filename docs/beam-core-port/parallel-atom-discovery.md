# Atom-discovery scheduling experiment

Both independent designs choose native-only module-local atom discovery inside the existing compile workers. The global atom namespace remains a barrier; map ordering and typed adaptation wait for every name set. Merge sets by moving owned strings, then free per-module sets before runtime setup/adaptation. Imported contexts do not discover atoms.

This removes serial tree-walk time, not semantic work, allocation calls, or traversal count. Compile phase now includes atom discovery; compare full frontend load, CPU, memory and paired whole wall. Parallel scratch sets can increase peak memory. Do not combine leaf/single-child changes in this measurement.

Deferred alternatives: true preparation/discovery fusion needs retractable provenance for discarded goto branches and preserved subtrees; shared-prefix form/value traversal could remove repeated metadata work but requires explicit arena-identity invariants. Leaf/single-group specialization can reduce cursor overhead, but is independent of the serial context bottleneck.

Control0503224 verified by saved binary digest; new fused mode remains experimental. Gates: fixtures native layout and exact module arrays, reversed module-order namespace, whole-project native and imported outputs, 24 paired native full-output checks, allocation diagnostic counts and scope fingerprints after timing.
