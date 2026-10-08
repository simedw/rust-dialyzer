# Isolated Firefly frontend prototype

Pinned upstream c672f3ad5bebd3d6e9e2fe6056a154ed92fd1d07, Apache 2.0 LICENSE.md and attribution NOTICE retained. This is a 15-crate frontend extraction; no LLVM, Elixir or Erlang process is required for runtime lowering.

## Build and run

```sh
RUSTC_BOOTSTRAP=1 cargo build --offline -p firefly-audit-probe
target/debug/firefly-audit-probe OUTPUT_DIRECTORY FILE.beam [FILE.beam ...]
```

Runtime sequence: AbstractCode::from_beam_file -> AbstractErlangToAst -> SemanticAnalysis -> CanonicalizeSyntax -> AstToCore. Core APIs are public in firefly_syntax_core::{Module,Expr,...}; Pass is in firefly_pass. Probe writes .core text and .core.debug complete Rust debug representations. Parent's analyzer adapter should call these public passes directly, rather than parse debug text.

All 13 tested ordinary Erlang BEAMs lower successfully with no diagnostics: structural engine fixture + 12 whole-module fixtures. Exact paths/outcomes are in ../lowering.log; output in ../output. There are no subprocess calls in probe/src/main.rs. Upstream binary tests pass 47/47, covering touched bit-vector/mask/slice selection behavior. Four additional focused native Elixir compiler tests now pass; see ../evidence/elixir-compiler-tests.log. The complete BEAM test target still has upstream FixInteger test compilation failures.

Source independence: probe pre-registers synthetic whitespace source documents sized from decoded metadata locations under original file names. AbstractErlangToAst now reuses registered CodeMap source IDs before attempting filesystem reads. Zero line/column locations map to first line/column to avoid unsigned underflow. This preserves relative line/column for positive annotations but is a bounded proof scaffold, not the production source-location design. It does not prove diagnostic annotation parity.

Modernization is fully recorded in modernization.patch and modernization-files.json (18 source files; 677 patch lines). reproduce.py reconstructs closure and applies patch from pristine pinned upstream; an exact source/manifests/probe replay was verified under replay/. Changes remove retired feature gates, change obsolete consuming slice APIs to split_off_first, replace const Option unwrap_or with equivalent match, modernize WASI cfgs, remove unused reexport, acknowledge ignored ControlFlow already paired with explicit visitor error flag, and allow registered sources. No compiler semantics/parity claim follows from successful lowering.

Explicit bootstrap remains necessary for trait_alias, box_patterns, compiler proc-macro diagnostics, specialization and other unstable dependencies. Achieving a normal stable-supported library requires additional deliberate refactoring; this prototype does not hide that requirement. The parent prototype now verifies native Elixir metadata translation, analyzer conversion and exact warning/type/annotation parity on its selected corpus. See ../README.md for scope and remaining limits.

Build logs ../build-step1.log through ../build-step6.log document failures and repairs; ../build-probe.log is successful probe build; ../binary-tests.log contains tests. Existing source compatibility edits from previous audit are included in full patch against pristine upstream.

## Local/remote identity correction

Canonicalization now leaves local calls/captures module-less (PartiallyResolved) and only qualifies imports/explicit remote references. AstToCore emits local IApply fnvar when FunctionVar has no module; explicit self-module calls remain ICall and remote captures remain make_fun. Focused native Erlang fixture fixtures/local_remote/firefly_call_identity.erl and compiled .beam exercise local vs remote access to an unexported hidden/1 helper, local vs remote captures, and nested closure local use. Typed Core assertions pass 2/2 using `RUSTC_BOOTSTRAP=1 cargo test --offline -p firefly-audit-probe --test local_remote`; evidence local-call-tests.log. All original 13 Erlang fixtures still lower successfully after correction (local-call-broad-lowering.log). No analyzer-warning parity claim here.

## Generated rethrow identity and ordering

Firefly's internal raise primop expects three [class,reason,info] operands, confirmed by its kernel_to_ssa Raise handling. Upstream try_exception used this order but after_block incorrectly used [class,info,reason]. A shared generated_rethrow constructor now ensures the correct internal order for both catch and after fallback paths and marks these with generated_rethrow + compiler_generated annotations. Explicit user erlang:raise/3 retains its original three BIF operands without that marker.

OTP27 validation via fixtures/rethrow/otp_core_oracle.escript confirms generated Core primop raise takes [info,reason], while explicit erlang:raise remains remote Call with [class,reason,stacktrace]. The analyzer adapter must convert only generated_rethrow-marked Firefly operations into OTP raise2 [args[2],args[1]]. It must retain explicit unmarked raise3 as the BIF.

Focused typed tests in probe/tests/rethrow.rs assert the final Core operand identity against Try.evars for catch and after, marker preservation, and explicit BIF distinction. These 2 tests plus previous 2 local/remote tests all pass (rethrow-tests.log). All original13 fixtures still lower after the fix (rethrow-broad-lowering.log). This proves IR preservation at the corrected boundary, not end-to-end runtime exception behavior or analyzer-warning parity. The OTP process is used only as a test oracle; lowering executable itself remains Rust-only.
