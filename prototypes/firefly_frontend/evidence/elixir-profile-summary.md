# Optimized Elixir analyzer profile

A macOS `sample` capture sampled the optimized `compiled_analysis_probe` over twelve copies of the native Dialyxir row, with the same captured dependency context as the gate. No compiler or benchmark was running concurrently. The own-process sampling command required sandbox escalation and was approved.

Of roughly 790 samples, `compiled_analysis::analyze` appeared in 618, `core_pipeline` in 340, and one `module_body` subtree in 157. That subtree spent 125 samples constructing `DataflowNodes`; another construction subtree contributed 110. Literal table construction, term ordering and integer parsing/cloning dominated these repeated setup paths. Dependency import contributed 59 samples. These are inclusive sampled stacks, not additive phase timings.

Source inspection confirmed `module_body` rebuilt folded whole-program DataflowNodes and GuardLiterals for each module refinement and diagnostic pass. Both depend only on immutable FoldedCore data and labels. The measured changes move them to CorePhases construction while keeping Patterns, inference state and warnings fresh.

The first change is independently gated and measured in `elixir-folded-cache-gate.json` and `elixir-performance-folded-cache.json`. Subsequent results are recorded separately. These are within-invocation setup caches, not incremental application analysis caches.
