# Prototype boundary

Use Firefly's typed native Erlang frontend, translate its Core expressions into the existing version 6 arena format, and let `CoreProgram::parse` validate the complete result before analysis. This combines the direct design's typed visitor with the wire design's existing validation boundary. It avoids a second analyzer and keeps the comparison focused on frontend behavior.

The metadata extractor independently decodes BEAM debug forms using the repository's bounded ETF decoder. It preserves original forms and constructs OTP-compatible type/spec attributes. Exact raw AST origins support diagnostic names and locations. Firefly independently decodes executable bodies and lowers them through semantic analysis, canonicalization and Core conversion. The prototype therefore decodes twice; adoption should unify ingestion.

Only installed runtime profiles and the prior-library snapshot come from oracle preparation. Native output supplies every executable node, attribute and source form. Oracle Core bodies are comparison evidence, never frontend input. Generated module_info bodies follow the documented OTP construction rule.

The JSON transport intentionally reuses a proven parser; it duplicates terms and is unsuitable for a performance claim. A later typed arena adapter can remove serialization after acceptance gates cover the complete frontend feature groups. Ordinary Erlang debug metadata is the first gate. Elixir metadata, opaque/bounded contracts, literal map ordering and native runtime profile construction remain separate gates.
