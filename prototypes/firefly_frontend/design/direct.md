# Candidate A: direct Firefly Core to analyzer arena

## Usage and boundary

The first executable should accept an ordinary Erlang BEAM with `erl_abstract_code` metadata and emit the same warning records and locations as the existing analyzer. It must decode and lower without spawning Erlang/Elixir. Offline oracle fixtures and explicitly supplied runtime-order evidence are permitted in the prototype; they must not be mistaken for a standalone production solution.

```rust
let decoded = read_beam(&bytes, &limits)?;
let frontend = NativeFrontend::new(pinned_profile, provenance);
let core: CoreProgram = frontend.lower(decoded)?;
let output = compiled_analysis::analyze(AnalysisInput { core: &core, /* existing inputs */ })?;
```

Choose a single extracted frontend owning Firefly semantic analysis, canonicalization and Core lowering. Convert its typed Core directly into existing `CoreModule`/`CoreNode` arenas. Keep Firefly types private. Do not use Firefly's printed Core as an interchange format, or feed textual abstract forms through its lexer: input terms are already decoded.

## Proposed public API

```rust
pub struct DecodedBeam {
    pub module: ModuleName,
    pub backend: DebugBackend,
    pub forms: ErlangTerms, // immutable original abstract forms, including attributes
}
pub enum DebugBackend { ErlAbstractCodeV1, ElixirErlV1 }
pub struct NativeFrontend { profile: FrontendProfile, provenance: RuntimeProvenance }
impl NativeFrontend {
    pub fn lower(&self, input: DecodedBeam) -> Result<CoreProgram, FrontendError>;
}
pub struct RuntimeProvenance {
    pub atom_layout: NativeAtomLayout,
    pub map_iterator_orders: /* per module term order evidence */,
    pub contract_variable_maps: /* form -> original native variable-map terms */,
    pub contract_closure_profile: ClosureCodeProfile,
}
pub enum FrontendError {
    InvalidBeam { offset: usize, detail: String },
    UnsupportedBackend { version: String, backend: String },
    UnsupportedForm { term: TermId, tag: String },
    Semantic { origin: SourceOrigin, detail: String },
    UnsupportedCore { origin: SourceOrigin, kind: String },
    MissingOrigin { kind: String },
    MissingRuntimeProvenance { component: String },
    InvalidArena { detail: String },
}
```

`ElixirErlV1` is a named future boundary, not silently accepted input. Initially return `UnsupportedBackend` until its reconstruction adapter exists. No subprocess fallback and no `Opaque` expression substitution for unsupported compiler shapes.

## Modules and source evidence

1. `beam_input`: reuse our bounded BEAM/ETF ingestion; return original terms plus explicit backend classification. Firefly `library/beam/src/code/mod.rs:59` accepts `debug_info_v1/erl_abstract_code` only. Its historical `Abst` handling is at line 53.
2. `forms_to_firefly`: decoded-term adapter into the Firefly Erlang AST. Snapshot attributes and origins before Firefly passes. Syntax-pp term representation is `compiler/syntax_pp/src/ast.rs:26`; preserve binary and bitstring coverage from our terms rather than assume syntax-pp's narrower term enum covers all metadata.
3. `firefly_frontend`: follow upstream `compiler/driver/src/compiler/passes/lower_ast.rs:24`: `SemanticAnalysis -> CanonicalizeSyntax -> AstToCore`. Preserve full ordering; isolated `AstToCore` misses record/call expansion and semantic bookkeeping. `AstToCore::run` at `compiler/syntax_erl/src/passes/translate/ast_to_core.rs:98` internally runs translation, variable-use annotation, export rewriting, and receive rewriting.
4. `core_to_arena`: exhaustive typed `Expr` conversion into `CoreExpr`, postorder local IDs, a single shared term interner for original forms and new literals/annotations. Firefly `syntax_core/src/ir/mod.rs:21` and analyzer `src/core.rs:53` are similar but not identical: Firefly `If` has no corresponding analyzer node, and map/bitstring representation differs. Define explicit lowering for `If`; reject it until implemented. Receive primop rewrites must be matched to analyzer/OTP semantics, not accepted by name alone.
5. `metadata_restore`: rebuild `CoreExpr::Module` attributes from untouched original forms in their original encounter/group order, rather than Firefly Core `Module`, whose fields omit type/spec attributes (`syntax_core/src/ir/module.rs:12`). Analyzer metadata indexing reads these literal attributes (`src/program_metadata.rs:5`). Keep spec/type bodies as exact original terms, including bound constraints, callback/opaque/record/export_type/suppression and file transitions.
6. `arena_validation`: extract the existing JSON parser's graph/shape validation into a typed constructor shared by parser and native adapter. The current `CoreProgram::parse` is JSON-only (`src/core.rs:359`), while its fields and `CoreModule` are public (`src/core.rs:283,328`). Avoid native adapter bypass of forward-reference, proper-list and function-definition checks.

## Exact provenance contract

`CoreModule.source_forms` points to the untouched original forms in the shared terms arena. Reconstruct every spec/type attribute value from those terms, preserving list, tuple, map, integer, float-bit, binary and bitstring values. Rebuilding from Firefly's normalized AST cannot prove preservation.

Each source expression carries its original abstract annotation and active file identity into the frontend. Generated nodes inherit an origin deliberately and include `compiler_generated`; source nodes do not acquire that flag merely because Firefly rebuilt them. Convert Firefly annotations selectively: internal `Vars`/`Type` keys are compiler analysis metadata, not OTP annotations. Retain all original annotations in a sidecar keyed by carried source identity because Firefly's keyed annotation map cannot preserve ordered or repeated raw annotation terms (`syntax_base/src/annotations.rs:41`). Missing span/source file information is an error, not default line 0. `program_metadata.rs:346` interprets integer line, line/column tuple, file tuple, `compiler_generated`, generated=true, and treats lines below 1 as generated; those are observable warning semantics.

Function ordering and export ordering are deliberate: Firefly uses `BTreeMap` functions and `HashSet` exports, whereas native arena label allocation follows traversal order. Never iterate the export HashSet directly. Restore original source definition/export order where required and compare callgraph labels/order-sensitive outputs with the oracle. Exact Core tree equality is stronger than needed for semantics but warning parity alone cannot expose all lowering defects; use both structural review and diagnostic checks.

## Runtime atom-order gap

Do not substitute BEAM Atom/AtU8 indices or alphabetically sorted names for ERTS runtime atom identities. The analyzer's native hashing depends on `NativeAtomLayout`; `compiled_analysis.rs:57` rejects a missing layout. Hashes and native map iteration/contract variable-map closure order are independently provenance-sensitive (`src/core.rs:283-312`, `native_runtime_hash.rs:6`). Prototype can consume captured oracle provenance from a file without spawning a process, but report this external input and limit any standalone claim. Production needs an explicit stable ordering redesign proven semantically equivalent, or a native reconstruction of the pinned runtime convention. Never fill missing maps with an empty iterator or fabricated atom IDs.

## Whole-shape alternative

Alternative B emits raw OTP-like Core terms (or existing version-6 flat JSON) and invokes `CoreProgram::parse`. This cheaply reuses validation, supplies a visible comparison artifact, and can compare with the current exporter (`scripts/core_export.exs:211`), but incurs term/JSON serialization and reparse and still requires all annotation, attribute, provenance and Firefly-to-OTP normalization work. It does not solve the missing semantics. Use JSON only for test snapshots or debugging; prefer the direct typed adapter plus shared validation for the actual extraction boundary.

## Verification unit

Start with an Erlang fixture containing correct typed functions, one bad call, one spec mismatch, an exported type and two files/line origins. Record backend, original forms equality, retained metadata, expression kinds, warnings and exact locations. Expand by coherent groups: pattern/guards; closure/letrec; exception flow; binaries; receive/comprehensions. Unsupported shapes fail before inference. Run the existing analyzer on OTP-exported Core and Firefly-adapted Core with identical prior/profile inputs. Benchmark only after both outputs match, accounting separately for decoding, semantic/canonicalization/lowering, adaptation and analyzer phases.
