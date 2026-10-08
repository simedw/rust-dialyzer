# Firefly typed Core to analyzer wire prototype

`source.rs` exports `adapt(module: &firefly_syntax_core::Module, codemap: &CodeMap, seed: serde_json::Value) -> Result<Value, String>`. It returns a version-6-compatible module record, not a full program.

The seed must contain untouched `terms`, `source_forms`, and independent raw-form `attributes` entries `{name,value,annotations}` referring into those terms. Optional `contract_variable_maps` and `map_iterator_orders` are preserved. Seed `nodes`/`core` are never read. Parent must wrap the output with the actual compiler profile and atom-layout/closure provenance. All output expressions are translated from the actual Firefly Core, including synthesized boolean cases for Firefly `If`; no opaque placeholders or oracle expression reuse.

`driver.rs` accepts `BEAM SEED.json OUTPUT.json`. It invokes the native BEAM reader and complete upstream `AbstractErlangToAst -> SemanticAnalysis -> CanonicalizeSyntax -> AstToCore` pipeline, then adapts. It creates bounded whitespace CodeMap documents using filenames and dimensions from compiled annotations; no original source or Erlang/Elixir process is used. Line/column tuples and file terms are emitted from actual spans. `compiler_generated` comes from Firefly annotation or unknown-span generated entities.

Original source variable names from `seed.source_variables` retain atom identities for diagnostic printing; generated non-function variables are consistently alpha-renamed to integers across the module (first encounter assigns identity; repeated named symbols reuse it; each anonymous `_` binder gets a fresh identity). Function variables retain atom/arity tuples. This representation normalization avoids inventing runtime atom indices for temporary variable symbols. Function definitions follow Firefly's BTreeMap order; exports are explicitly sorted rather than iterating a HashSet.

Nonempty literal maps fail explicitly because true native iterator provenance for newly materialized terms has not been established. Empty literal maps have their exact empty iterator order. Map expressions, all six binary-entry specification variants, integer/float/binary/bitstring/list/tuple literals, closures, exceptions and receive nodes have explicit conversions. Unsupported input terms or diagnostics remain errors.

This prototype does not prove OTP Core-tree equality or raw annotation preservation. Firefly internally changes annotations, variable names, definition order and compiler transformations. Contracts/type attribute bodies come from original terms independently to avoid losing them. The source-origin sidecar proposed in `../design-direct/design.md` remains the production design; JSON reparse is temporary.

## Build here

The local asdf cargo shim overrides CARGO_HOME. The reproducible scratch build used an offline copy of its registry under adapter/cargo-home and explicit cargo proxy:

```sh
CARGO_HOME="$PWD/cargo-home" RUSTUP_HOME=/Users/simedw/.asdf/installs/rust/1.88 RUSTC_BOOTSTRAP=1 \
 /Users/simedw/.asdf/installs/rust/1.88/bin/cargo build --offline
target/debug/adapt-beam ../harness/beams/frontend_acceptance.beam ../harness/native-seed.json ../harness/firefly-module.json
```

`cargo test --offline` with the same environment passes 10 focused tests: conditional translation, shared integer variable identity, distinct anonymous binders, literal-map failure, OTP module_info call construction, and byte encoding padding. Real acceptance BEAM conversion completes successfully (`acceptance.log`, actual `../harness/firefly-module.json`). Parent owns parser/inference warning comparison.

OTP predefined `module_info/0` and `/1` bodies are constructed as `erlang:get_module_info(Module[,Arg])`, verified against installed `stdlib-7.1/src/erl_internal.erl:570-580`; Firefly runtime-specific injected bodies are not used. Both generated functions carry explicit zero/compiler_generated annotations. Parent acceptance comparison reported all three warning records and locations match.

Broader-corpus normalization: Firefly native BIF primops `error`, `exit`, `throw`, `nif_error`, and `raise/3` become `erlang` calls; existing Core `raise/2` stays a primop. Source variables are preserved from independent raw-form `source_variables` provenance. Map-update origins come from exact `source_origins` file/base-line/base-column matches, carrying raw map-operator line/column rather than arithmetic offsets. Ambiguous provenance errors. These additional rules have explicit regression tests for BIF shape and source variable diagnostics.

Generated rethrow correction: the extracted compiler now emits an explicit `generated_rethrow` marker and consistent `[class,value,info]` order for its generated catch/after rethrows. The adapter maps only marked sites to canonical OTP Core `raise/2 [info,value]`. Explicit unmarked source `erlang:raise/3` remains a BIF call. A focused test verifies the distinction and argument order.

Map-pair provenance correction: upstream Firefly drops raw MapField.span, so the independent seed carries field `{kind,file,key_line,key_column,line,column}` locations. Adapter matches exact key origin and emits raw field annotation. Map operation literals are compiler-generated and never inherit an enclosing map origin. This preserves OTP warning-location behavior, which traverses descendants and chooses their earliest non-generated origin (`binder.rs:warning_location`). Regression verifies field/key/map origins remain distinct.
