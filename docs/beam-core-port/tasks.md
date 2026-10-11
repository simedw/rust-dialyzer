# Native BEAM to cleaned Core port

- [x] Ground the existing bridge, installed compiler semantics and corpus contracts.
- [x] Compare staged and direct lowering designs; choose staged compatibility.
- [x] Implement checked BEAM/debug metadata ingestion without invoking a VM.
- [x] Implement metadata lifting/recovery and match complete forms across all 1,032 corpus BEAM files exactly.
- [x] Implement a forms-to-Core and cleanup prototype; match complete supported modules.
- [x] Preserve independent compiler fixtures and exact boundary comparisons.
- [ ] Finish whole-corpus Core lowering, including records, structural matches, binaries, try/receive and comprehensions.
- [ ] Integrate native cleaned Core with analyzer metadata and prove signatures/warnings parity.
- [ ] Promote the experiment into the default pipeline after those gates pass.

Full compiler coverage is the destination. The current worktree is an experiment with complete corpus lifting and partial Core lowering, not a production replacement.
