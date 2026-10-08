# Native dataflow feature set

Success typing builds and solves constraints. Dataflow uses the success PLT, validated contracts, current local returns, reachability and a mutable function table to refine module results and collect diagnostics. Those phase-owned facts cannot be represented by one approximate traversal. A failed argument conjunction makes a success constraint fail, while dataflow can retain a signature return for its diagnostic.

The dataflow migration begins with its environment and pattern binder. Native environments contain both type entries and variable substitutions. Branch joins retain the outer environment, track which existing keys changed, union their branch types, and restore nested join scopes. Substitutions affect both lookup and later writes. Fresh pattern variables start at Any; map keys and binary size expressions remain in the outer scope. Keeping only a map of variable types loses these rules.

Use a shared, typed Core label table that maps existing Core node references and lexical binding identities to the source-equivalent integer labels. Label ordering matters: joins write changed keys in native label order, and writes through aliases can affect subsequent keys. Core arena node indexes do not preserve that order. Persist module results by qualified identities, rather than these analysis-local labels. A shared node-facts boundary classifies literals, variables, multiple values and structural patterns from the existing Core/term arenas. The environment owns persistent type/substitution maps and nested join scope identities; a branch belongs to the scope that created it. Native reference assertion failures remain visible at the join boundary.

Pattern binding must migrate as one unit: aliases, literals including refolded map patterns, variables and letrec types, tuples and typed records, lists, maps, multiple values and binary segments. Forward and reverse matching have separate source-defined behavior. Tuple alternatives join successful environments; failed alternatives do not leak partial bindings. Opacity diagnostics require the original pattern, expected type and actual type even when binding eventually fails.

The verification lever will invoke original native map and pattern functions in isolated copies of dialyzer_dataflow, using actual labelled Core terms. It compares resulting types, substitutions, branch changes and exact failures. Core generation and native phase orchestration are required before replacing the production engine; direct environment/pattern checks alone cannot close whole-program parity.


## Integration boundaries

PreparedDefinitions owns resolved records, candidate contracts, callbacks, final module type tables, and unknown-type events. Candidate contracts become active only after validation against Rust-inferred signatures. Library type declarations can resolve target forms; library target success signatures cannot initialize target inference.

SourceCallGraph derives module edges, escaping functions, and ordered incoming callers from compiler Core. FunctionState consumes these facts and the phase-owned raw signature snapshot. Its captured environments and FIFO worklist remain the same state during refinement and warning collection. SCC partition equivalence is proved; observable convergence and diagnostics must establish whether internal SCC scheduling differences matter.

Generator constructs one NativeConstraintArena and uses its shared preparation and solver. Exception rollback restores persistent traversal state. Clause generation and dataflow share Patterns and underapproximation; synthetic pattern identities remain signed and distinct from compiler labels. Allocation event order must be derived from source traversal, never copied from a native inferred graph.

FoldedCore is a formatting view. It preserves original NodeRef evidence and appends flat folded nodes without replacing the analysis label table. Binder warnings and guard warnings use one WarningState with native term equality and insertion order. Formatting and raw operand failures occur in native evaluation order, even when warning emission is disabled.

Each of these boundaries has a direct native comparison lever. They become one production Core analyzer in a single caller migration once complete. The old abstract-forms flow and its CLI callers are then deleted. Partial component proofs cannot close the complete compatibility gate.
