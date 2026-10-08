# Native frontend wire design

## Usage and acceptance first

`native_frontend fixture.beam --emit-core core.json` must read an ordinary OTP-produced Erlang BEAM, decode its native debug information in Rust, lower its abstract forms using the extracted Firefly frontend, and emit the existing v6 Core arena schema. `CoreProgram::parse` then validates that output and `compiled_analysis::analyze` runs without frontend compiler subprocesses. Native compilation and OTP Core export remain test oracles, outside the measured execution.

First fixture: one exported `id/1` with a user type and spec, one guard-constrained arithmetic function, and an exported bad caller passing an atom. Compare the warning tag, MFA, file and line, and retain a correctly typed caller that emits no warning. Assert raw forms/spec/type/file attributes survive and `Program::build_with_contracts(..., Use)` sees them. This is a compiler-plus-engine integration proof, not merely JSON parser success. Run with a frozen empty/bounded prior dependency fixture and oracle profiles; explicitly label those fixtures rather than claiming native profile generation has been solved.

## Recommendation: existing v6 JSON as first boundary

Use a coherent adapter from Firefly Core to the analyzer's existing flat JSON schema, then call `CoreProgram::parse`. JSON is already the verified boundary, so this avoids simultaneously inventing a new in-memory validator and changing analyzer invariants. It also permits emitting and inspecting the actual artifact, diffing it against OTP-derived output, and running existing analyzer binaries. JSON overhead is acceptable for the first semantic proof; after parity, replace only its transport with a checked arena builder using the same adapter rules.

Do not serialize OTP `c_*` tuples first: our analyzer has no direct OTP Core term loader, so that adds an encoding and a second flattening adapter without helping the initial proof. Do not use Firefly's Core pretty printer: textual Core is another parser and Firefly's printer cannot restore discarded source metadata.

## Required types and modules

```rust
struct NativeBeamModule { name: String, raw_forms: ErlangTerms, forms_root: TermId }
struct CoreWireModule { name: String, source_forms: usize, terms: Vec<TermWire>,
    nodes: Vec<NodeWire>, core: usize,
    contract_variable_maps: Vec<[usize; 2]> }
struct FrontendContext { profile: CompilerProfile, atom_layout: NativeAtomLayout,
    source_files: SourceFileIndex }

fn decode_erl_debug_info(bytes: &[u8]) -> Result<NativeBeamModule, FrontendError>;
fn lower_native_forms(module: &NativeBeamModule) -> Result<FireflyModule, FrontendError>;
fn adapt_core(module: &NativeBeamModule, lowered: &FireflyModule,
    context: &FrontendContext) -> Result<CoreWireModule, FrontendError>;
fn emit_program(modules: Vec<CoreWireModule>, context: FrontendContext)
    -> Result<String, FrontendError>;
```

`native_frontend/beam.rs` checks container/backend/version, then uses the existing bounded ETF decoder. `forms.rs` feeds only executable forms to Firefly while retaining every original form in a raw sidecar. `adapter.rs` owns all Firefly-expression-to-CoreExpr conversion, annotations, attributes and arena interning. `wire.rs` owns serde output, v6 keys and postorder references. `oracle.rs` is test-only comparison, not runtime fallback.

## Exact analyzer contract

At `src/core.rs:359`, `CoreProgram::parse` requires root `version` 5 or 6, `profile` (`otp`, `erts`, `elixir`, `dialyzer`, `core_options`, `term_map_order` equal to `native_to_list_with_iterator`) and `modules`. Each module requires `name`, `source_forms` (proper-list term reference), `terms`, `nodes`, `core`; v6 additionally requires `contract_variable_maps`. Node children and term children must reference earlier arena entries. Nodes carry an `annotations` term reference. Literal/var/opaque nodes carry `value`; map nodes additionally carry `map_arg` and `map_pattern`. Maps in the term arena need a complete `iterator_order` permutation. Float values preserve IEEE bits; binary/bitstring values preserve bytes and exact bit lengths. There is no `preprocessed` key in this root or perf26 Core schema: the preserved abstract form list is `source_forms`.

`compiled_analysis::analyze` at `src/compiled_analysis.rs:57` additionally refuses a missing `atom_layout`, even though the parser allows absence. `contract_closure_profile` is optional in the wire schema but may be needed by contract solving; use an explicitly identified test fixture initially, not a fabricated profile. Root `scripts/core_export.exs:210` is the reference encoder. `scripts/beam_export.exs` is the obsolete v1 forms-only path and deliberately drops type/opaque metadata; it is not this target.

`Program::index_metadata` at `src/program_metadata.rs:4` reads attributes from the Core module node, not just the retained source_forms list. Hence preserve/reconstruct literal attributes (`type`, `opaque`, `nominal`, `spec`, `callback`, `record`, `behaviour`, `export_type`, `optional_callbacks`, `dialyzer`, and `file`) in original order and OTP-compatible payload shape. Keeping raw_forms alone is insufficient.

## Correctness blockers and explicit policy

1. Firefly `compiler/syntax_core/src/ir/module.rs:12` has exports/functions/compile settings but no generic attributes collection. `AstToCore::run` (`syntax_erl/src/passes/translate/ast_to_core.rs:97`) transfers selected fields and discards source type/spec attributes at this boundary. Keep all raw forms as a sidecar and reconstruct attributes from it; never rely on Firefly's Module for these.
2. Firefly `Expr` includes `If` and `Receive`; analyzer `CoreExpr` has its own limited variants. Adapter must prove the relevant pipeline has fully lowered these, or return UnsupportedExpr. Firefly AstToCore already chains variable-use, exported-variable and receive rewrites. Do not silently translate unsupported constructs into arbitrary opaque nodes.
3. Firefly export iteration is a HashSet; sort module and exported function order deliberately, while preserving source attribute order. OTP-generated variable names and function order can differ: an alpha-normalized structural diff may be useful, but raw warning comparison must remain exact. Normalizing annotations away would hide warning suppression bugs.
4. Source spans cannot be replaced with default line zero: `source_origin` (`src/program_metadata.rs:346`) treats absent/nonpositive lines as generated. Emit actual file/line/column and preserve CompilerGenerated flags as `compiler_generated` (or generated=true) annotation terms. Native forms use Erlang source annotations; Firefly spans require an accurate file mapping.
5. BEAM atom table indices are not live VM atom identities. `NativeAtomLayout::parse` (`src/native_runtime_hash.rs:141`) expects native tagged atom identities recovered from recorded hashes. Native hash ordering affects analysis/persistence. A frozen oracle layout works only for a bounded prototype; production needs a coherent replacement of runtime identity/order dependencies or a native runtime profile implementation. Do not create dummy hashes and claim parity.
6. Map term order is compiler provenance, not insertion order or lexical order. Similarly, `contract_variable_maps` must reproduce bounded_fun variable discovery and map iterator ordering (`scripts/core_export.exs:178-208`). The first fixture can avoid bounded fun/maps, but preservation of raw attrs must already work and coverage limits must be stated.
7. OTP exporter uses no_copt/dialyzer/no_inline/strict_record_tests and `dialyzer_clean_core.clean`. Firefly targets native compilation and is not an OTP-clean-Core clone. Semantic agreement must be demonstrated on feature groups; do not require byte-identical Core for every alpha-renaming but do require warning/location parity and executable-order/binding equivalence.

## Design alternatives

Direct in-memory adapter saves serialization and makes Firefly Expr -> CoreExpr type mapping straightforward. However, CoreProgram public fields allow constructing invalid DAGs and missing provenance that the current parser rejects. It is a good second step once a shared `ValidatedCoreBuilder` exists. For the prototype, JSON -> existing parse keeps the trusted boundary and gives inspectable evidence with the least new infrastructure.
