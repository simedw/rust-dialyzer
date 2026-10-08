# Native BIF catalogue and transfer oracle

The catalogue comes from preprocessing and inspecting the pinned `erl_bif_types.erl` abstract forms with `epp`. It unions exact MFA heads of `type/4` and `arg_types/3`. It records each source clause and guard, helper dependency graphs, cross-transfer aliases, transitive `erl_types` operations and callback-dependent transfer groups. It does not derive capabilities from warning fixtures or module exports.

Run from the repository root:

```sh
elixir /private/tmp/rust-dialyzer-bif-catalogue/scripts/bif_catalogue.exs \
  .cache/upstream/otp/lib/dialyzer/src/erl_bif_types.erl \
  /private/tmp/rust-dialyzer-bif-catalogue/catalogue.json
```

The generator privately compiles the pinned source under a new module name. Every catalogue entry's admissible argument domains, return on unconstrained arguments and `is_known` result must equal the installed native module. A source hash, native module path and runtime versions identify the comparison. The complete preprocessed forms are saved as `source_forms.etf`. These are native datatype fixtures, not warnings.

OTP 28.3 has 148 exact MFAs: 93 in `erlang`, 46 in `lists`, and 9 in `maps`. Each has one exact argument-domain clause and one exact return-transfer clause. Both functions end with a generic guarded fallback for atom module/function names and arities 0 through 255. All 298 clauses are accounted for, including the two fallbacks. There are no unresolved dynamic MFA-head families in this pin.

The source defines no math transfer. All 26 native `math` exports were separately checked and `is_known` returned false. The generic transfer still applies: ordinary inputs return `any`, and impossible input types return `none`. Two catalogue entries, `erlang:build_stacktrace/0` and `erlang:info/1`, do not appear among runtime module exports. This is why export enumeration cannot define the catalogue.

`admissible_domains` describes the static domains returned by `arg_types/3`. `return_on_any` is the result of `type/3`, not a replacement for the argument-dependent `type/4` transfer. `clips_to_admissible_domains` identifies the source's `strict/5` helper. A Rust implementation must reproduce each transfer's native algorithm once the canonical type API is ready.

Twenty transfers depend on callback argument or return types. The dependency graph follows aliases such as `foldr` to `foldl`, `mapfoldr` to `mapfoldl`, and `usort/2` to `sort/2`. Each entry names its required type API operations and one-based callback argument positions. Calls through a variable inside helpers are recorded as dynamic calls; that records callback application, rather than an unresolved MFA-head family.

The direct protocol accepts one JSON request per input line and returns one JSON response per line:

```sh
elixir /private/tmp/rust-dialyzer-bif-catalogue/scripts/bif_oracle.exs \
  < /private/tmp/rust-dialyzer-bif-catalogue/requests.jsonl \
  > /private/tmp/rust-dialyzer-bif-catalogue/responses.jsonl
```

A request has `operation` equal to `domains` or `transfer`, and `mfa` with `module`, `function`, `arity`. Transfer requests also carry an `arguments` array with exactly that many native type expressions. For example:

```json
{"operation":"transfer","mfa":{"module":"erlang","function":"+","arity":2},"arguments":[{"constructor":"t_integer","arguments":[{"integer":"4"}]},{"constructor":"t_integer","arguments":[{"integer":"7"}]}]}
```

Type expressions call exported `erl_types:t_*` constructors or operations through `constructor` and recursively decoded `arguments`. Parameter values use `atom`, decimal `integer`, `list`, or `tuple`. `type_etf` accepts a base64 native type term through safe ETF decoding. Every constructor result and transfer argument must pass `erl_types:is_erl_type/1`.

Each native type response includes exact tagged native term JSON, a base64 ETF term, and the native display string. The tagged representation preserves atom/string distinctions, integer precision, tuples, proper/improper lists, maps and binary payloads. It describes OTP's native `#c` representation, rather than pretending to be an independent domain schema. It can seed later Rust canonicalization comparisons. Display strings are for inspection; comparisons should use canonical native terms or native `t_is_equal`.

`transfer` returns the original arguments, the static domains, their intersections as `admissible_arguments`, and the actual native transfer return. It does not use the intersections as a substitute for invoking the transfer. Unknown static domains remain explicit, with no fabricated signature.

Nine saved probes exercise exact integer arithmetic, rejected binary arguments, callback result propagation, callback failure with nonempty versus possibly empty lists, known and absent map keys, and the generic math fallback. The expected results were obtained by invoking the native implementation, not by copying warning diagnostics. Map algebra probes are stored separately in `map_probes.jsonl`.
