# Native Elixir debug metadata frontend

`translate(&ErlangTerms, metadata: TermId) -> Result<ErlangTerms, String>` consumes
`{elixir_v1, Map, Specs}` from a compiled BEAM `Dbgi` chunk. The returned arena root
is a list of Erlang abstract forms accepted by the extracted Firefly compiler.
The parent input helper reads the BEAM and encodes those forms to ETF. Elixir
source files and oracle Core bodies are not inputs to native translation.

The translator preserves variable versions and diagnostic names, raw specs,
source annotations, exports, attributes and generated `__info__/1`, including
its independently computed deterministic exports MD5. Generated and source
locations have separate representations. Metadata traversal is limited to 256
levels and two million arena terms before lowering.

Feature groups are separate source modules: arena and annotations; module forms
and metadata; literals; expression/pattern lowering; control flow; exceptions;
comprehensions. Common scope state models variable version identity, per-name
fresh counters, macro caller access and exception stacktrace access.

Verified 48 captured Elixir modules produce exactly the same Erlang abstract
forms as the compiler backend (all annotations and generated forms included).
This is a frontend correctness gate; warning and inferred-type parity are
verified by the parent native analyzer harness. The compiler oracle is used only
by the verification binary, not by `translate`.

```sh
cargo build --offline --manifest-path prototypes/firefly_frontend/elixir/Cargo.toml
cargo test --offline --manifest-path prototypes/firefly_frontend/elixir/Cargo.toml
python3 prototypes/firefly_frontend/elixir/verify_forms.py \
  prototypes/firefly_frontend/elixir_harness/generated/inputs
```

For one independently captured module:

```sh
prototypes/firefly_frontend/elixir/target/debug/translate-elixir metadata.etf native-forms.etf
prototypes/firefly_frontend/elixir/target/debug/compare-forms native-forms.etf oracle-forms.etf
```

Supported groups include ordinary functions and macros, patterns and guards,
local/remote/operator and anonymous calls, literals and binary segments,
map construction/update and static structs, captures and closures, generated
boolean cases, `case`, `cond`, `with`, exception rescue normalization, catch,
after and else, and enum-generator comprehensions with filters and list/binary
destinations. Shared compiler rewrites include String.Chars binary fast paths
and basic constant MapSet.new.

The prototype explicitly rejects semantic groups not ported yet: receive,
super, dynamic struct names and struct updates, binary-generator comprehensions,
unique/reducing comprehensions and general Collectable destinations, guarded
catch forms and non-byte-aligned literal bitstrings. Other compiler pure-call
optimizations are not evaluated; ordinary calls retain their runtime semantics.
This acceptance corpus does not establish support for every Elixir program.

See LICENSE and NOTICE for source algorithm provenance.
