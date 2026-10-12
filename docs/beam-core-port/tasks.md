# Native BEAM to cleaned Core port

- [x] Ground the existing bridge, installed compiler semantics and corpus contracts.
- [x] Compare staged and direct lowering designs; choose staged compatibility.
- [x] Implement checked BEAM/debug metadata ingestion without invoking a VM.
- [x] Implement metadata lifting/recovery and match complete forms across all 1,032 corpus BEAM files exactly.
- [x] Finish whole-corpus Core lowering, including records, structural matches, binaries, try/receive, comprehensions, exports and compiler scope rules.
- [x] Preserve independent compiler fixtures and exact forms, raw Core and cleaned Core comparisons.
- [x] Integrate native cleaned Core with analyzer metadata and validate all 1,446 workload module occurrences against independent compiler transport.
- [x] Prove complete native warning/signature parity in imported and fresh native contexts.
- [x] Benchmark both complete workloads with fresh compilation, full inference, output, CPU and peak memory.

BEAM manifests now default to native runtime context and fused emission, without a compiler process or fallback. The CLI accepts `--frontend native|compiler`; compiler-produced Core-file manifests remain supported. Arbitrary compiler-version and nonempty prior-library migration remain separate work.
