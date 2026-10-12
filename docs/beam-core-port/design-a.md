# Native BEAM debug information to cleaned Core: staged compatible frontend

## Caller usage first

```rust
let profile = FrontendProfile::otp_28_3_elixir_1_19_4();
let decoded = BeamDebug::read(&beam_bytes, &profile, limits)?;
let forms = frontend.lift(&decoded)?;             // reusable for source metadata
let lowered = frontend.lower(&forms)?;
let cleaned = frontend.clean(lowered)?;
let module = frontend.finish(forms, cleaned)?;    // existing CoreModule
```

Production frontend performs no subprocess invocation and does not load a VM. A separate development oracle can invoke the installed Elixir/OTP backend and export stage references. Unsupported constructs fail with module/function/source/stage information; they do not silently become `Any`, disappear, or trigger VM fallback.

For the analyzer, the final public interface can remain one deep operation:

```rust
let module: CoreModule = NativeFrontend::new(profile).compile_beam(bytes, limits)?;
let program = CoreProgram::from_modules(context, modules)?;
```

The staged methods are useful internal and oracle boundaries, not a requirement that every production caller coordinate five passes.

## What the current code actually does

`prototypes/core_cache/core_cache.exs` reads bytes, hashes the whole BEAM, and caches a profile-keyed ETF record. On a miss `debug_and_forms` obtains Dbgi and selects `erl_abstract_code` or `elixir_erl`, invokes its `debug_info(:erlang_v1, ..., [])`, then `compile` separately invokes the same backend with `:core_v1` and fixed analysis options. It cleans the returned tree and builds transport metadata. This is neither just binary decoding nor an Elixir source parser.

Recovered installed Elixir1.19.4 compiler source lives in `/private/tmp/beam-core-port/oracles`. `elixir_erl.erl` lines 43–75 establish both Elixir paths: `erlang_v1` runs `dynamic_form(Map,nil)` and returns `Prefix ++ Specs ++ Forms`; `core_v1` runs `dynamic_form` again, merges Map.compile_opts with supplied options, calls `elixir_erl_compiler:erl_to_core`, then `compile:noenv_forms` with from_core/to_core. The bridge therefore repeats Elixir metadata lifting to obtain source forms and Core. Expanded definitions are already stored in debug metadata; macro expansion, parsing and executing the user's compiler do not need reproducing.

`dynamic_form` requires module, relative_file, attributes, definitions, unreachable, deprecated and compile_opts. It builds file/module/compile prefix attributes, removes unreachable definitions, groups and orders definitions, translates macros to their generated runtime names with an extra caller argument, creates generated module functions, and attaches specs/attributes. `translate_clause` contains a surprising annotation rule: a clause with the same source line as the module line receives generated line zero. Preserve it. `elixir_erl_var` resolves variable versions rather than simply matching names; it allocates `_@N` and `_name@N` names and retains match/guard context. `elixir_erl_pass` handles already expanded match/tuple/map/struct/binary/block/capture/fn/case/try/receive/for/with/calls; its try/clauses/for helpers are real semantic work, not parsing speed.

For Erlang debug information, installed OTP28.3 `erl_abstract_code.erl` returns stored forms for erlang_v1. Core-v1 combines original CompilerOpts with analysis options and invokes compile:noenv_forms.

OTP28.3 `compile.erl` abstract-form route performs compile-directive handling and `erl_expand_records`, then `v3_core`, even with no_copt. `v3_core:module` adds predefined module functions, preserves attribute/file/export handling, and lowers each function through `body -> ubody -> cbody -> lbody`. Its counters, generated fallback clauses, evaluation-order sequencing, binders and dialect-specific annotations matter to the existing analyzer. no_copt disables later Core optimizations; it does not make forms-to-Core a tree renaming pass. The exact Elixir `erl_to_core` helper still needs reading from its recovered source before deciding which shared expansion/validation steps apply; do not assume it is identical to the Erlang backend route.

`dialyzer_clean_core` is a separate transformation. It recursively rebuilds relevant nodes, removes clauses annotated dialyzer_ignore and clauses whose direct bodies are match_fail/raise primops, and rewrites compiler letrec_goto into analysis-oriented cases with dialyzer_unknown. It preserves ordinary recursive comprehensions. It does not simply discard annotations.

`NativeCoreTransport.build` is another distinct pass: source-atom discovery order, term-root traversal, map iterator provenance, bounded contract variable maps, node/root counts. Deep terms >=192 use the flat CoreExport arena fallback. `CoreExport` additionally interns terms and exports float bits and map order. A native in-memory frontend can avoid ETF serialization altogether, but must populate the same CoreModule facts used downstream.

## Existing Rust building blocks

- `external_term::decode` already supports bounded ETF decoding, including compressed ETF, into ErlangTerms. It is iterative and preserves scalar float bits and arbitrary integers. Reuse it rather than add a second parser.
- `fingerprint::classic_beam_fingerprint` already demonstrates BEAM FOR1/chunk walking and gzip handling. Refactor a reusable checked chunk view rather than call the fingerprint parser as the semantic decoder; verify total container length, padding, duplicate required chunks and module identity explicitly.
- `core.rs` already has typed CoreExpr for all required Core nodes, NodeId/TermId arenas, annotations, module source forms, contract maps and map iterator order. Construct those directly, then run existing CoreProgram validation.
- `core/etf.rs` is a cache transport reader, not a BEAM debug backend. A native compiler should not serialize to this format merely to parse it back.
- Historical Firefly frontend files were not located. This design assumes no Firefly implementation or reusable compiler.

## Two whole designs

### A. Expanded Elixir metadata to typed Erlang abstract forms to compatible Core

Decoder dispatches backend/version, then a native Elixir lift reproduces dynamic_form/variable/clauses helpers and emits typed Erlang abstract forms. Erlang modules enter the same forms layer directly. A version-pinned native lowering follows the relevant OTP expansion and v3_core passes. Cleanup follows the installed Dialyzer cleaner. Finalization attaches source metadata/provenance without transport serialization.

This pays for a forms arena, but the forms already serve types/specs/records/source warnings downstream. One lifting result replaces today's two dynamic_form invocations. Each stage has an executable oracle and a precise first mismatch. Both Elixir and Erlang coverage share one Core lowerer. The lowerer can use compact arena IDs and explicit traversal tasks instead of allocating nested BEAM tuples; this is where Rust can change representation while preserving output.

### B. Expanded Elixir directly to Core

Decoder builds a typed expanded Elixir IR and lowers directly to Core; source abstract forms are synthesized separately for analyzer metadata. A direct path can fuse variable renaming, expansion and Core sequencing and potentially avoid the transient forms arena. It has a smaller high-level compiler pass count.

It does not eliminate compatibility obligations. It must still reproduce the effects of generated Erlang functions, macro conventions, guard expansion, record semantics, failed match handling, binders/counters, Core fallback clauses and the cleaner. Types/specs still need forms compatible with the analyzer. The two logical translations can diverge in generated names, annotation placement and source atom visitation. A behaviorally equivalent Core tree can produce different exact warning/signature results because this analyzer mirrors native ordering and labels. This design has no intermediate forms oracle to isolate lift versus lower mismatches.

Recommendation: A first. Native representation and one-pass metadata lifting are ambitious enough without simultaneously changing semantic lowering. Once exact forms and Core parity are established, selectively fuse producers/consumers while retaining stage-oracle output modes. Direct design B is a later measured optimization, not the foundation for claiming exact compatibility.

## Types and ownership

```rust
struct BeamView<'a> { chunks: Vec<Chunk<'a>>, module: AtomName }
enum DebugPayload {
    Erlang { forms: ErlangForms, compiler_options: CompilerOptions },
    Elixir(ExpandedModule),
}
struct ExpandedModule {
    name: AtomName,
    definitions: Vec<ExpandedDefinition>,
    attributes: MetadataTerms,
    specs: MetadataTerms,
    options: CompilerOptions,
    source: SourceMetadata,
    original_terms: ErlangTerms,
}
struct ExpandedDefinition {
    name: FunctionName,
    kind: DefinitionKind, // Def, Defp, Macro, PrivateMacro
    metadata: SourceMetadata,
    clauses: Vec<ExpandedClause>,
}
struct ErlangForms { arena: FormsArena, forms: Vec<FormId>, source_terms: ErlangTerms }
struct UncleanCore { arena: CoreArena, module: NodeId, source: ModuleMetadata }
struct CleanedCore { arena: CoreArena, module: NodeId, source: ModuleMetadata }
struct FrontendProfile { versions: VersionPin, options: AnalysisOptions, native: NativeCompatibilityProfile }
struct Unsupported {
    stage: Stage,
    module: AtomName,
    function: Option<FunctionName>,
    source: SourceLocation,
    term_path: Vec<PathPart>,
    construct: ConstructTag,
}
```

Use distinct UncleanCore/CleanedCore to prevent skipping cleanup. Preserve raw metadata terms alongside typed interpretations; unsupported fields are not casually normalized. Compiler options are parsed into a domain model: reject unsupported parse/core transforms or encrypted custom debug backends rather than execute them. Resource limits belong at bytes/arena expansion boundaries. Each module owns its mutable compiler state; immutable profile data is shared across independent module workers. Preserve input module order at final collection.

Module ownership: `beam_debug` handles chunks/ETF/backend validation; `elixir_lift` handles expanded definitions and hygiene; `erl_forms` owns typed form IR and source metadata; `forms_to_core` owns OTP-compatible expansion/lowering and per-function counters; `core_clean` owns cleaner semantics; `frontend` exposes compile_beam and final CoreModule conversion. No public wire tuple variants or transport flags need leak into the analyzer.

## Metadata and compatibility blockers

Retain file/location/generated annotations, original term representation, float bits, literal map keys/iteration order, definition ordering, macro arity/names, captures and compiler-created function identities. Specifications and bounded-variable-map provenance are required even when executable expressions do not use them.

The current program context additionally includes VM atom internal hashes and Dialyzer closure code profiles. These are not present in ordinary Dbgi and cannot be invented by naming a Rust compiler profile. `CompilerAtomPrimitives.export` explicitly obtains ERTS internal atom hashes, while NativeAtomLayout depends on that provenance. A pure native bridge needs an explicit solution for the analyzer's native runtime-order context: reuse version-pinned non-executable compatibility data during research, or port/profile the native ordering machinery and prove how new module atoms receive identities. Oracle-generated sidecars may establish a development reference, but requiring a newly launched VM to generate production context violates the requested boundary. Mark this separately from lifting/lowering; bytecode FunT alone does not reconstruct all compiler-process atom provenance. Any first executable slice using an imported oracle context must report that limitation.

## First executable slice

Build the checked BEAM/Dbgi reader and typed backend decoder, then native Elixir lifting for zero-arity public/private definitions returning literals and versioned variable identity functions. Include generated module prefix/exports/spec attributes; oracle compare full erlang_v1 forms for a tiny module. Add the smallest shared forms-to-Core module/function/clause/literal/variable lowering plus compatible per-function counters and module_info/__info__ generated bodies required by that fixture. Clean and return a validated CoreModule, using an explicitly imported research compatibility context where necessary. Reject every other expression visibly.

A decoder-only benchmark is useful evidence but is not a frontend replacement. A function-only lowerer must be labelled partial coverage; it cannot emit a silently incomplete module. Keep an executable supported-construct census to expand this slice across calls, matches/guards/case, fun captures, maps/binaries, try/receive and comprehensions.

## Oracle and parity gates

1. Decode gate: exact Dbgi wrapper/backend/version/module/metadata term comparison, including compressed/uncompressed chunks and malformed/resource-limited cases.
2. Lift gate: compare native forms with backend erlang_v1 for each supported fixture, preserving annotation payloads/order, variable names and generated definitions. Specs source forms are checked independently of execution expressions.
3. Lower gate: compare raw Core from native lowering with backend core_v1 under the same compiler options. First differing node path includes annotation, binder and literal terms. Do not blanket alpha-rename or remove generated annotations to make a mismatch disappear.
4. Clean gate: compare the cleaned tree, including generated raising clauses, letrec_goto replacement and ordinary recursive comprehensions.
5. Metadata gate: independently compare contract variable maps, discovered source atoms, map iterator order, compiler profile and closure context. This prevents correct-looking executable Core with wrong warning ordering inputs.
6. End-to-end gate: run both Phoenix and Elixir complete inventories with identical warning multisets and all public/private signatures. Stage equality supports diagnosis; analyzer equality remains the practical goal.
7. Performance gate: measure cold BEAM decode/lift/lower/clean/finalize separately, count bytes/forms/nodes, and compare whole bridge and analyzer wall/CPU/peak memory. Include compilation support coverage and failures in the result, so a subset compiler cannot claim full-workload acceleration.

All VM invocations live in oracle generation commands. Native benchmark binaries run with no VM invocation capability. Exact production promotion waits until the entire requested inventory is supported and context generation is native or supplied through a documented VM-free contract.
