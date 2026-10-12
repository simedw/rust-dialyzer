# Borrowed child traversal

- [x] Preserve old fused analyzer and allocation binaries from 1cb9e97 as controls.
- [x] Choose fixed group descriptors and borrowed list/pair cursors, preserving modes/filtering/order.
- [x] Verify fixtures and all 1,446 complete module/atom comparisons.
- [x] Benchmark old fused versus borrowed fused: five alternating pairs plus warmup, both workloads.
- [x] Count allocations separately after timing; compare identical frontend windows.
- [x] Save evidence, report measured outcome and commit experimental branch.

Scope: remove NodeShape's Vec<Vec<ViewRef>>, list/pair flattening, Prepare ordinary field clones, and fused adapter's flattened source_children. Retain final NodeId buffers, final typed child vectors, raw lowerer and all validation. No other allocator, inference, atom-scope or default frontend change.
