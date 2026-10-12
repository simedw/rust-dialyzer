# Native BEAM frontend: retain the compiler's stage boundaries

## Usage first

```rust
// Production caller: no Erlang/Elixir process, textual source parser, or fallback VM.
let profile = CompatibilityProfile::otp28_3_elixir1_19_4();
let frontend = BeamFrontend::new(profile, FrontendLimits::default());
let module: CoreModule = frontend.compile(&beam_bytes)?;
let program = CoreProgram::from_modules(&frontend.analysis_context()?, vec![module])?;

// Development oracle: executed only by the fixture capture/verification driver.
// Oracle generates each boundary from the pinned installed OTP/Elixir backend.
compare_lifted_forms(&native.forms, &oracle.erlang_v1)?;
compare_core(&native.unclean, &oracle.core_v1, Comparison::ExactAnnotations)?;
compare_core(&native.cleaned, &oracle.cleaned, Comparison::ExactAnnotations)?;
```

The public interface owns profile selection, decode limits, intermediate validation, lowering, cleanup, and provenance. It returns today's CoreModule directly. ETF cache transport is optional persistence, not a required compiler stage.

## Actual current flow

`prototypes/core_cache/core_cache.exs:debug_and_forms` asks beam_lib for Dbgi/Abst, checks the debug_info_v1 backend, and for recognized backends obtains `erlang_v1` forms. `compile` then independently asks the same backend for `core_v1`, calls dialyzer_clean_core.clean, builds NativeCoreTransport's record, and serializes ETF.

Installed/recovered `elixir_erl.erl:43–76` confirms that both erlang_v1 and core_v1 call dynamic_form(Map,nil). The current bridge therefore lifts Elixir metadata twice. core_v1 additionally uses elixir_erl_compiler:erl_to_core and compile:noenv_forms with from_core/to_core before returning. The recovered compiler helper must be inspected before claiming its exact pass list; it was not among the initial five extracted sources.

`erl_abstract_code.erl` on OTP28.3 returns stored forms directly for erlang_v1, while core_v1 runs compile:noenv_forms with captured compiler options plus requested options. Compiler options include `no_copt`, `to_core`, `no_inline`, `strict_record_tests`, and `dialyzer`. OTP compile.erl expands records and calls v3_core; no_copt suppresses the optional Core optimization block, but does not eliminate required abstract-form preparation or v3_core's internal phases.

These are not parsing a text language. The input is already expanded Elixir metadata or Erlang abstract forms in ETF. Decoder throughput alone cannot remove lifting, generated functions, clause normalization, pattern compilation, variable management, cleanup, and metadata transport work.

## Whole design A: staged compatibility compiler (recommended)

Use explicit data boundaries while keeping implementation behind one frontend:

1. **Decoder:** bounded BEAM IFF chunk slicing and bounded ETF decoding. Reuse src/external_term.rs, which already builds ErlangTerms iteratively and supports compressed ETF, exact float bits, bitstrings, and external fun literals. It rejects unsupported runtime identities rather than fabricating values. Select Dbgi backend/version and verify module identity. Missing, encrypted, unknown-backend, malformed, and unsupported-version states are distinct errors.
2. **Lifting:** Elixir elixir_v1 metadata -> typed Erlang abstract forms. For erl_abstract_code, validate/lift stored forms into the same representation. Expanded metadata means no tokenizer, source parser, macro expansion, package resolution, or full Elixir source compiler is required.
3. **Lowering:** Erlang forms -> OTP-compatible unclean Core, including required forms preparation and v3_core-like variable/guard/pattern lowering. Keep an internal Core builder; emit today's CoreNode/CoreExpr arenas only when that ownership makes construction practical. Do not perform optimizations excluded by the reference profile.
4. **Cleanup:** exact dialyzer_clean_core behavior over unclean Core. This is a separate semantic stage: it removes ignored/raising clauses and rewrites letrec_goto; it is not harmless serialization cleanup.
5. **Transport/context:** construct CoreModule and compiler provenance directly. Optional ETF output supports an existing cache consumer. Read closure FunT and pinned profile facts natively; preserve source atom chronology, bounded-contract variable maps, and map ordering provenance. No source-body AST needs to be serialized alongside the final Core.

```rust
struct BeamFrontend { profile: CompatibilityProfile, limits: FrontendLimits }
struct CompatibilityProfile {
    backend: BackendVersion,
    core_options: CoreOptions,
    native_order: NativeOrderProfile,
    closure_profile: ClosureProfile,
}
enum DebugModule {
    Erlang { forms: ErlangForms, options: CompileOptions },
    Elixir { metadata: ExpandedElixirModule, specs: ErlangForms },
}
struct ExpandedElixirModule {
    identity: ModuleName,
    file: SourceFile,
    definitions: Vec<ExpandedDefinition>,
    declarations: DeclarationForms,
    unreachable: FunctionSet,
    reflection: ReflectionMetadata,
    options: CompileOptions,
}
struct ExpandedDefinition {
    name: FunctionName,
    kind: DefinitionKind, // def/defp/defmacro/defmacrop
    annotation: ElixirAnnotation,
    clauses: Vec<ExpandedClause>,
}
struct ErlangForms { terms: ErlangTerms, functions: Vec<AbstractFunction>, attributes: DeclarationForms }
struct UncleanCore { builder: CoreArena, provenance: CompilerProvenance }
struct CleanCore { builder: CoreArena, provenance: CompilerProvenance }
enum FrontendStage { Beam, Etf, DebugSchema, Lift, PrepareForms, LowerCore, CleanCore, Provenance }
struct FrontendError {
    stage: FrontendStage,
    module: Option<ModuleName>,
    function: Option<FunctionName>,
    location: Option<SourceLocation>,
    construct: ConstructTag,
    reason: FrontendErrorKind,
}
impl BeamFrontend {
    fn compile(&self, bytes: &[u8]) -> Result<CoreModule, FrontendError>;
}
// Private boundaries, usable by boundary-oracle tests, not public orchestration.
fn lift(debug: DebugModule, profile: &CompatibilityProfile) -> Result<ErlangForms, FrontendError>;
fn lower(forms: ErlangForms, profile: &CompatibilityProfile) -> Result<UncleanCore, FrontendError>;
fn clean(core: UncleanCore) -> Result<CleanCore, FrontendError>;
fn finish(core: CleanCore, profile: &CompatibilityProfile) -> Result<CoreModule, FrontendError>;
```

Use typed abstract syntax for constructs under active implementation, with raw declaration terms retained for exact specs/types/records/attributes. This avoids repeatedly dispatching arbitrary ETF tuples in semantic code without inventing a lossy type metadata model. Parse external schemas once; type invariants thereafter are trusted. Stage newtypes prevent accidentally feeding unclean Core to analysis.

Suggested module ownership: beam_frontend/mod.rs owns compile and diagnostics; beam_frontend/debug.rs owns chunk/debug schema; beam_frontend/elixir.rs owns lifting and variable scope; beam_frontend/abstract_core.rs owns OTP-compatible lowering; beam_frontend/clean.rs owns cleaner invariants. core.rs owns the final existing domain format; external_term.rs stays a generic decoder. Helpers for clauses, binaries, and comprehensions remain implementation submodules, not public passes callers must coordinate.

### Why exact lifting is substantial

Recovered dynamic_form builds prefix file/module/compile forms, partitions public/private/macros, skips unreachable definitions, preserves definition-specific file switches, generates __info__/1 and its spec, and adds attributes/specs in a defined order. Macros become MACRO-* functions with one additional caller argument. Clause translation can inject __CALLER__ setup. Negative zero is explicitly represented as an operator around positive zero, so a simple literal copier differs.

elixir_erl_var maps metadata `version` to generated Erlang names, with per-name counters and context-dependent naming. Rebinding, hygiene, and unnamed temporaries must preserve version identity. elixir_erl_pass handles pin guards, match-time extra guards, map access differences in guard/body, captures, binary segments, case/try/receive/for/with, and generated failure branches. The five recovered sources total 2,211 lines excluding compiler/comprehension/helper dependencies; v3_core alone is 4,634 lines. This is feasible compiler work, not a tiny ETF adapter.

### Metadata that cannot be dropped

Keep line/column/file and generated flags; function annotations; compiler-generated/dialyzer_ignore/letrec_goto markers; specs, types, opaque types, callbacks/macrocallbacks/optional callbacks, records, exports, deprecation and struct metadata. Preserve literal float bits, proper/improper lists, bitstrings and external function identity.

CoreModule currently requires map_iterator_orders and bounded contract-variable maps. NativeCoreTransport captures both independently of serialized map order. ETF map pairs do not prove OTP maps:iterator order. The analysis context also needs scoped native atom hashes (scripts/compiler_atom_primitives.exs), closure profiles from dialyzer_typesig/contracts FunT, and a compiler option/version profile. Production must reproduce those natively or use an explicitly pinned compatibility artifact; it must never launch a VM to fill missing provenance. A made-up stable hash/order is incompatible with native-order behavior. Existing native_runtime_hash/native_map_order can consume verified facts, but do not yet prove deriving every fresh atom's OTP internal hash without an oracle.

## Whole design B: expanded Elixir directly to Core

Decode the same metadata, then build CoreExpr immediately, translating Elixir scopes and clauses directly to Core. Retain declarations separately and generate compiler reflection functions directly. This can remove the intermediate Erlang syntax allocation and repeated traversal, and can use one iterative arena builder throughout.

It loses the Erlang-forms oracle boundary and folds Elixir expression translation, OTP pattern compilation, generated failure shapes, naming, and cleanup conventions into one coupled pass. A functionally correct direct Core tree can still produce different labels, native dependency enumeration, closures, approximations, and warnings. It would also need a second independent path for erl_abstract_code unless abstract forms are translated to a shared Core-normalization IR. That shared IR is effectively the staged design again.

For exact OTP-compatible output, the direct approach must reproduce the staged decisions anyway. Prefer staged construction first. Once boundary fixtures establish exactness, fuse allocation/traversal for proven families behind the same public API; measure rather than assume that an extra internal typed arena erases the native speed advantage.

## First executable slice

Build native BEAM Dbgi decode and schema inspection for both backends, then native Elixir->Erlang forms lifting for one whole tiny module that has a literal-return function, identity argument, local/remote call, public/private definitions, and generated __info__. Compare entire erlang_v1 forms, including generated reflection functions, declarations, order, and annotations. This is a real lifting executable; do not call it a Core compiler yet.

In parallel architectural terms (not concurrent source editing), the first end-to-end Core slice should accept already-lifted Erlang forms for that same tiny module, lower simple functions/variables/literals/calls plus module_info generated functions, run the native cleaner, and compare complete cleaned Core. Then connect the two paths into one no-VM BEAM->Core executable. If the first selected Elixir fixture needs captures or complex __info__ patterns beyond implemented lowering, diagnose it explicitly; do not drop reflection functions to make the example pass.

## Oracle and adoption gates

- Capture oracle Dbgi, erlang_v1 forms, pre-clean Core, post-clean Core, provenance, and final analysis output using installed pinned OTP28.3/Elixir1.19.4, with source/profile hashes. The oracle may run Elixir; production frontend must not.
- Validate intermediate exact structure and annotations. Remap arena allocation IDs only; do not alpha-normalize compiler variable names or reorder clauses/definitions to conceal differences that affect labels and inference.
- Add construct fixtures for rebinding/hygiene, pins/guards, binary/map patterns, try/rescue/catch/after, receive, comprehensions/with, captures/macros, structs, records, callbacks/specs/opaque types, and source file switches. Test malformed chunks, limits, unknown backends, unsupported versions, and explicit construct errors.
- Run both complete Phoenix/Elixir inventories, compare every public/private signature and warning multiset, and test sequential/parallel analysis modes. Preserve native order sidecars and compiler profiles in the comparison. Also compare errors rather than swallowing native compile failure.
- Measure fresh frontend wall time, CPU, allocations/RSS, and each native stage only after successful output comparisons. Current warm cached-Core runs cannot measure frontend replacement savings.

Missing construct diagnostics must identify backend/version, module/function, location, and path/tag. There is no silent Any/opaque substitution, dropped definition, or external-VM fallback. Partial coverage is an explicit executable compiler slice, not production support for the full corpus.

## Decision

Choose staged compatibility. It duplicates no source language parser and removes both current duplicate lifting and the external runtime/transport round trip. Direct lowering remains a future measured fusion of proven stages. Firefly historical documentation is not an available implementation: docs/HANDOVER explicitly records its deletion; no current compiler source availability is inferred from prior reports.
