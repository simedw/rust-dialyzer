# Firefly frontend acceptance harness

All Erlang/Elixir invocations are oracle preparation only. The final native frontend must ingest `beams/frontend_acceptance.beam`, decode native debug forms, and generate Core bodies itself. `oracle-core.json` must not supply those bodies or attributes.

Regenerate on the installed OTP 28 / Elixir 1.19.4 installation:

```
elixir generate.exs /path/to/rust-dialyzer /path/to/this/harness
```

This compiles `frontend_acceptance.erl` once with native debug info, reads the repository's existing `.cache/corpus/otp/base.plt`, exports its prior library snapshot, captures original compiler closure profiles, exports original OTP Core for comparison, and invokes the actual public `dialyzer.run` success-typing analysis.

The fixture contains exported type `count/0`, typed identity, guarded integer addition, a correctly typed `good/0`, an atom passed to integer addition, and an invalid return contract. Native public Dialyzer produces exactly three warnings: no local return (line 11), failing add call (line 11), and invalid wrong_return contract (line 12). No warnings should name good/0 or id/1.

The existing analyzer was run with the original oracle Core, the newly saved prior snapshot and closure profile: `oracle-rust-result.jsonl` reports `public_warnings_match=true`, three warnings, and zero cleanup warnings. This verifies harness input compatibility before native Core integration.

Artifacts:

- `oracle-row.jsonl`: original expected warnings plus target context.
- `oracle-source-forms.etf`: raw original abstract forms for exact preservation comparison only.
- `prior-library.json`: prior PLT snapshot, with no target analysis facts.
- `closure-profile.json` / `contract-profile.json`: original installed compiler closure layouts.
- `oracle-core.json`: expected OTP Core and captured atom layout.
- `oracle-warnings.txt`: human-readable public Dialyzer warnings.
- `provenance.json`: artifact hashes and explicitly limited proof scope.

Run native-generated Core through the same analyzer:

```
python3 compare_native.py native-core.json --probe /path/to/rust-dialyzer/target/debug/compiled_analysis_probe
```

The comparison utility uses only the supplied native artifact's module bodies/forms/attributes. Missing root profile/atom/closure fields are filled from the explicitly captured oracle test context. If native lowering introduces extra atoms, extend the captured atom profile using a native OTP export of those atom names and record that extra oracle-only step; do not fabricate tagged identities.

This is one bounded Erlang frontend proof. It does not establish Elixir debug metadata support, full frontend feature parity, production native profile generation, or a speed advantage.

## Existing Erlang corpus

`generate_corpus.exs ROOT HARNESS` captures 13 existing BEAM files without recompiling them: `engine_structural` and the 12 `.cache/whole_modules/fixtures/*.beam` files. Each target gets its own actual public Dialyzer run using the same unchanged base prior, original OTP Core snapshot, raw abstract forms ETF, captured atom layout and original compiler profiles. `corpus/oracle-row.jsonl` includes each beam path and SHA256 plus the expected warning term and count. There are 23 expected warnings total.

The corpus covers anonymous functions, binaries, guards/cases, discarded closures, equality, improper lists, local call narrowing, exact map updates, no-return bodies, recursion and try. These are existing small fixtures, not an exhaustive OTP frontend test suite. Native generated bodies must replace each row's `core.modules`; source forms and attributes must come from native BEAM decoding, while root captured profiles remain explicit bounded-test inputs.

`corpus/oracle-rust-result.jsonl` is the baseline analyzer run over original OTP Core. `corpus/provenance.json` records its result and hashes once all rows have completed. Baseline success is distinct from Firefly lowering success.
