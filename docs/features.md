# Corpus feature inventory

Run from the repository root:

```sh
python3 scripts/features.py \
  --inventory corpus/inventory.json \
  --report .cache/corpus/report.json \
  --output .cache/corpus/features.json
```

`--upstream-root` defaults to `.cache/upstream` beside the inventory's repository. `--engine-root` defaults to that repository. Python's standard library is the only dependency.

The scanner joins inventory IDs to report artifact paths. From the report it consumes only `id` and the parent directory of `oracle_file`. It never opens an oracle file, reads a Rust diagnostic, or groups by a case's assessment status. Case identities must match exactly; duplicate report IDs fail. Missing programs remain explicit cases with `coverage: missing_program`.

Each case prefers `core.json` beside its old `program.json` when present. Version 3 uses module-local term and Core-node tables with postorder references. The scanner validates references and exposes source AST sequences lazily. Its iterative walk decodes only accessed term values and never builds a recursive ErlangTerm tree. Core traversal follows node references iteratively and includes `map_arg`; it reports both occurrence counts and unique reachable IDs, plus unreachable table nodes. Versions 1 and 2 remain supported during migration. Version 2 tagged source terms decode to arrays for AST counting only. This deliberately collapses tuple/list distinctions and is not an analysis representation. The selected input hash and version are recorded. Each module is scanned recursively. If `source_forms` exists it takes precedence over `forms`. Every annotated node is counted, including nested types, clauses, guard expressions, binary segments, comprehension generators, operators and calls. Static calls establish a function graph. Strongly connected components identify actual recursive functions. Functions in the same exported program resolve internally; other static calls are recorded as external dependencies. The known finite Rust builtin table distinguishes unsupported external signatures.

The old exporter keeps only runtime functions and `file`, `module`, `spec` attributes. The original 327 version 1 cached programs lack declarations needed to describe source semantics completely. Complete version 5 Core exports carry complete source forms; `source_metadata_complete` distinguishes those inputs. `missing_source_metadata` records this loss; absence of an exported attribute never proves absence in the source. A complete `source_forms` field removes this declaration-loss marker for that case. Core node counts traverse every child group and every explicit map base argument. `core_traversal_complete` requires a Core tree for every module; counting Core nodes says nothing about Rust semantic support. Per-case and aggregate coverage counts allow partial export migrations without overstating completeness.

Pinned `.erl` and `.ex` source files supply a separate lexical supplement. It recognizes declaration-looking lines beginning with `-attribute` or `@attribute`, records source hashes and line numbers, and labels observations `lexical_attribute:*`. These counts are lexical evidence, not compiler truth. Multiline strings or quoted code may create false positives; preprocessing, macro expansion and included headers may create omissions. `lexical_only_groups` makes these memberships visible. Every pinned input file was present in the verified run.

Groups are overlapping feature sets. A map case can also require higher-order calls, recursive named types and contract checking. `group_dependencies` names the domain capabilities needed for each family. `group_overlap_counts` measures cross-family intersections. This supports scheduling coherent feature work without making the first parser rejection the owner of a case. `basic_terms` covers cases with no larger observed or lexical family.

`missing_ir` combines absent syntax, type constructors, external signatures, exported declarations and precision gaps. A node accepted by the parser is not evidence that its semantics match Dialyzer. The current integer domain erases literal values and ranges, so comparison-bearing cases receive a precision label even though comparison syntax parses. The finite capability snapshot is explicit in the script and tied to hashes of `src/ir.rs` and `src/engine.rs`. Update those constants after changing the Rust implementation.

The JSON contains all registered cases, raw node counts, features, group memberships, explicit gaps, representative node paths, source evidence, recursion and external-call lists. Group counts and missing-IR counts are counts of cases, not counts of nodes. One case can contribute to several counts. The schema describes the stable structural fields; feature identifiers are extensible strings.
