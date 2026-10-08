use super::*;
use crate::core::CoreModule;
use crate::core::{CompilerProfile, CoreNode};
struct RawCore {
    terms: Vec<ErlangTerm>,
    nodes: Vec<CoreNode>,
    defs: Vec<(NodeId, NodeId)>,
    attrs: Vec<(NodeId, NodeId)>,
}
impl RawCore {
    fn new() -> Self {
        Self {
            terms: vec![ErlangTerm::Nil],
            nodes: Vec::new(),
            defs: Vec::new(),
            attrs: Vec::new(),
        }
    }
    fn term(&mut self, t: ErlangTerm) -> TermId {
        let id = TermId(self.terms.len());
        self.terms.push(t);
        id
    }
    fn atom(&mut self, s: &str) -> TermId {
        self.term(ErlangTerm::Atom(s.into()))
    }
    fn int(&mut self, n: i64) -> TermId {
        self.term(ErlangTerm::Integer(n.to_string()))
    }

    fn tuple(&mut self, items: Vec<TermId>) -> TermId {
        self.term(ErlangTerm::Tuple(items))
    }
    fn node(&mut self, expr: CoreExpr) -> NodeId {
        let id = NodeId(self.nodes.len());
        self.nodes.push(CoreNode {
            id,
            annotations: TermId(0),
            expr,
        });
        id
    }
    fn var(&mut self, s: &str) -> NodeId {
        let id = self.atom(s);
        self.node(CoreExpr::Var(id))
    }
    fn lit(&mut self, s: &str) -> NodeId {
        let id = self.atom(s);
        self.node(CoreExpr::Literal(id))
    }
    fn fvar(&mut self, s: &str, arity: usize) -> NodeId {
        let name = self.atom(s);
        let n = self.int(arity as i64);
        let tuple = self.tuple(vec![name, n]);
        self.node(CoreExpr::Var(tuple))
    }
    fn fun(&mut self, params: Vec<NodeId>, body: NodeId) -> NodeId {
        self.node(CoreExpr::Fun { params, body })
    }
    fn named(&mut self, name: &str, params: Vec<NodeId>, body: NodeId) -> NodeId {
        let key = self.fvar(name, params.len());
        let fun = self.fun(params, body);
        self.defs.push((key, fun));
        fun
    }

    fn finish(mut self) -> CoreProgram {
        let name = self.lit("probe");
        let exports = self.defs.iter().map(|(key, _)| *key).collect();
        let core = self.node(CoreExpr::Module {
            name,
            exports,
            attributes: self.attrs.clone(),
            definitions: self.defs.clone(),
        });
        let node_count = self.nodes.len();
        let term_count = self.terms.len();
        CoreProgram {
            version: crate::core::CoreVersion::V5,
            atom_layout: None,
            contract_closure_profile: None,
            profile: CompilerProfile {
                term_map_order: crate::core::TermMapOrder::NativeToListWithIterator,
                otp: "28".into(),
                erts: "16".into(),
                elixir: "1.19.4".into(),
                dialyzer: "5.4".into(),
                core_options: vec![],
            },
            modules: vec![CoreModule {
                map_iterator_orders: std::collections::BTreeMap::new(),
                contract_variable_maps: std::collections::BTreeMap::new(),
                name: "probe".into(),
                source_forms: TermId(0),
                core,
                terms: self.terms,
                nodes: self.nodes,
            }],
            node_count,
            term_count,
            kinds: BTreeMap::new(),
        }
    }
}
struct NoBifs;
impl BifProvider for NoBifs {
    fn admissible_domains(
        &self,
        _: &FunctionName,
    ) -> std::result::Result<Option<Vec<Type>>, String> {
        Ok(None)
    }
    fn transfer(
        &self,
        _: &FunctionName,
        _: &[Type],
    ) -> std::result::Result<Option<Transfer>, String> {
        Ok(None)
    }
    fn guard(
        &self,
        _: &FunctionName,
        _: &[Type],
        _: bool,
    ) -> std::result::Result<Option<GuardTransfer>, String> {
        Ok(None)
    }
}
struct NoConstraints;
impl ConstraintProvider for NoConstraints {
    fn contract_domains(&self, _: &FunctionName) -> std::result::Result<Option<Vec<Type>>, String> {
        Ok(None)
    }
    fn external_summary(&self, n: &FunctionName) -> std::result::Result<Signature, String> {
        Err(format!("missing external {n:?}"))
    }
    fn contract_constraint(
        &self,
        _: &FunctionName,
        _: &[Type],
    ) -> std::result::Result<Option<Signature>, String> {
        Ok(None)
    }
}
fn infer(c: &CoreProgram) -> Analysis {
    let p = Program::build(c).unwrap();
    let plan = build_plan(c, &p).unwrap();
    Engine {
        core: c,
        program: &p,
        plan: &plan,
        bifs: &NoBifs,
        constraints: &NoConstraints,
        depth: 3,
        iteration_limit: 100,
    }
    .infer()
    .unwrap()
}
#[test]
fn let_sequence_and_shadowing_return_bound_scalar() {
    let mut c = RawCore::new();
    let x = c.var("X");
    let first = c.lit("before");
    let binder = c.var("X");
    let lit = c.lit("after");
    let use_x = c.var("X");
    let inner = c.node(CoreExpr::Let {
        vars: vec![binder],
        argument: lit,
        body: use_x,
    });
    let body = c.node(CoreExpr::Seq {
        argument: first,
        body: inner,
    });
    c.named("shadow", vec![x], body);
    let out = infer(&c.finish());
    assert_eq!(out.signatures[0].result, Type::atom("after"));
}
#[test]
fn clauses_preserve_parameter_provenance_and_residuals() {
    let mut c = RawCore::new();
    let x = c.var("X");
    let use_x = c.var("X");
    let a = c.lit("a");
    let guard = c.lit("true");
    let ret = c.lit("one");
    let ca = c.node(CoreExpr::Clause {
        patterns: vec![a],
        guard,
        body: ret,
    });
    let b = c.lit("b");
    let guard = c.lit("true");
    let ret = c.lit("two");
    let cb = c.node(CoreExpr::Clause {
        patterns: vec![b],
        guard,
        body: ret,
    });
    let case = c.node(CoreExpr::Case {
        argument: use_x,
        clauses: vec![ca, cb],
    });
    c.named("pick", vec![x], case);
    let out = infer(&c.finish());
    assert_eq!(
        out.signatures[0].arguments[0],
        Type::atoms(["a".into(), "b".into()])
    );
    assert_eq!(
        out.signatures[0].result,
        Type::atoms(["one".into(), "two".into()])
    );
}
#[test]
fn static_calls_reject_impossible_arguments_with_original_types() {
    let mut c = RawCore::new();
    let x = c.var("X");
    let use_x = c.var("X");
    let a = c.lit("a");
    let guard = c.lit("true");
    let ret = c.lit("ok");
    let clause = c.node(CoreExpr::Clause {
        patterns: vec![a],
        guard,
        body: ret,
    });
    let case = c.node(CoreExpr::Case {
        argument: use_x,
        clauses: vec![clause],
    });
    c.named("pick", vec![x], case);
    let target = c.fvar("pick", 1);
    let z = c.lit("z");
    let apply = c.node(CoreExpr::Apply {
        operator: target,
        args: vec![z],
    });
    c.named("test", vec![], apply);
    let out = infer(&c.finish());
    assert!(out.signatures[1].result.is_none());
    assert!(out.warnings.iter().any(|w|matches!(&w.cause,WarningCause::CallArguments{actual,expected,..}if actual[0]==Type::atom("z")&&expected[0]==Type::atom("a"))));
}
#[test]
fn self_cycle_uses_unit_second_pass() {
    let mut c = RawCore::new();
    let target = c.fvar("loop", 0);
    let apply = c.node(CoreExpr::Apply {
        operator: target,
        args: vec![],
    });
    c.named("loop", vec![], apply);
    let out = infer(&c.finish());
    assert!(out.signatures[0].result.is_unit());
}
#[test]
fn deep_sequences_are_iterative() {
    let mut c = RawCore::new();
    let lit = c.lit("ok");
    let mut body = lit;
    for _ in 0..10000 {
        body = c.node(CoreExpr::Seq {
            argument: lit,
            body,
        });
    }
    c.named("deep", vec![], body);
    let out = infer(&c.finish());
    assert_eq!(out.signatures[0].result, Type::atom("ok"));
}
#[test]
fn unsupported_constructs_are_explicit_errors() {
    let mut c = RawCore::new();
    let lit = c.lit("opaque");
    let CoreExpr::Literal(t) = c.nodes[lit.0].expr else {
        unreachable!()
    };
    let opaque = c.node(CoreExpr::Opaque(t));
    c.named("opaque", vec![], opaque);
    let core = c.finish();
    let p = Program::build(&core).unwrap();
    let e = build_plan(&core, &p).unwrap_err();
    assert!(e.contains("unsupported native dependency expression opaque"));
    assert!(e.contains("NodeRef"));
}
#[test]
fn try_handler_is_unconditionally_joined_with_fresh_exception_variables() {
    let mut c = RawCore::new();
    let argument = c.lit("ok");
    let result_var = c.var("R");
    let body = c.var("R");
    let class = c.var("Class");
    let reason = c.var("Reason");
    let stack = c.var("Stack");
    let handler = c.lit("caught");
    let try_node = c.node(CoreExpr::Try {
        argument,
        vars: vec![result_var],
        body,
        exception_vars: vec![class, reason, stack],
        handler,
    });
    c.named("try_it", vec![], try_node);
    let out = infer(&c.finish());
    assert_eq!(
        out.signatures[0].result,
        Type::atoms(["ok".into(), "caught".into()])
    );
}
#[test]
fn try_match_failure_still_evaluates_handler() {
    let mut c = RawCore::new();
    let name = c.lit("match_fail");
    let argument = c.node(CoreExpr::Primop { name, args: vec![] });
    let result_var = c.var("R");
    let body = c.var("R");
    let class = c.var("Class");
    let reason = c.var("Reason");
    let stack = c.var("Stack");
    let handler = c.lit("caught");
    let try_node = c.node(CoreExpr::Try {
        argument,
        vars: vec![result_var],
        body,
        exception_vars: vec![class, reason, stack],
        handler,
    });
    c.named("try_it", vec![], try_node);
    let out = infer(&c.finish());
    assert_eq!(out.signatures[0].result, Type::atom("caught"));
}
#[test]
fn catch_has_native_any_result_and_keeps_original_environment() {
    let mut c = RawCore::new();
    let body = c.lit("ok");
    let caught = c.node(CoreExpr::Catch { body });
    c.named("caught", vec![], caught);
    let out = infer(&c.finish());
    assert!(out.signatures[0].result.is_any());
}
#[test]
fn map_literals_keep_mandatory_keys_and_exact_value_types() {
    let mut c = RawCore::new();
    let key = c.atom("a");
    let value = c.int(42);
    let map = c.term(ErlangTerm::Map(vec![(key, value)]));
    let body = c.node(CoreExpr::Literal(map));
    c.named("map", vec![], body);
    let out = infer(&c.finish());
    assert_eq!(
        out.signatures[0].result.map_get(&Type::atom("a")).unwrap(),
        Type::integer(42.into())
    );
    assert!(
        out.signatures[0]
            .result
            .map_get(&Type::atom("b"))
            .unwrap()
            .is_none()
    );
}
#[test]
fn receive_peek_primitive_preserves_product_arity() {
    let mut c = RawCore::new();
    let name = c.lit("recv_peek_message");
    let argument = c.node(CoreExpr::Primop { name, args: vec![] });
    let yes = c.var("Yes");
    let message = c.var("Message");
    let use_message = c.var("Message");
    let body = c.node(CoreExpr::Let {
        vars: vec![yes, message],
        argument,
        body: use_message,
    });
    c.named("receive_peek", vec![], body);
    let out = infer(&c.finish());
    assert!(out.signatures[0].result.is_any());
}
#[test]
fn recursive_success_domains_do_not_lock_before_return_fixpoint() {
    let mut c = RawCore::new();
    let x = c.var("X");
    let use_x = c.var("X");
    let zero_t = c.int(0);
    let zero = c.node(CoreExpr::Literal(zero_t));
    let guard = c.lit("true");
    let first = c.node(CoreExpr::Clause {
        patterns: vec![zero],
        guard,
        body: zero,
    });
    let y = c.var("Y");
    let guard = c.lit("true");
    let target = c.fvar("rec", 1);
    let call = c.node(CoreExpr::Apply {
        operator: target,
        args: vec![zero],
    });
    let second = c.node(CoreExpr::Clause {
        patterns: vec![y],
        guard,
        body: call,
    });
    let body = c.node(CoreExpr::Case {
        argument: use_x,
        clauses: vec![first, second],
    });
    c.named("rec", vec![x], body);
    let out = infer(&c.finish());
    assert!(out.signatures[0].arguments[0].is_any());
    assert_eq!(out.signatures[0].result, Type::integer(0.into()));
}
#[test]
fn function_reference_refinement_uses_canonical_function_type() {
    let mut c = RawCore::new();
    let ok = c.lit("ok");
    c.named("inner", vec![], ok);
    let target = c.fvar("inner", 0);
    let binding = c.var("F");
    let use_f = c.var("F");
    let call = c.node(CoreExpr::Apply {
        operator: use_f,
        args: vec![],
    });
    let body = c.node(CoreExpr::Let {
        vars: vec![binding],
        argument: target,
        body: call,
    });
    c.named("outer", vec![], body);
    let out = infer(&c.finish());
    assert_eq!(out.signatures[1].result, Type::atom("ok"));
}
#[test]
fn tuple_pattern_filters_correlated_alternatives_before_binding_fields() {
    let mut c = RawCore::new();
    let p = c.var("P");
    let use_p = c.var("P");
    let a = c.lit("a");
    let t1 = c.int(1);
    let one = c.node(CoreExpr::Literal(t1));
    let ta = c.node(CoreExpr::Tuple(vec![a, one]));
    let b = c.lit("b");
    let t2 = c.int(2);
    let two = c.node(CoreExpr::Literal(t2));
    let tb = c.node(CoreExpr::Tuple(vec![b, two]));
    let choose = c.lit("yes");
    let guard = c.lit("true");
    let ca = c.node(CoreExpr::Clause {
        patterns: vec![choose],
        guard,
        body: ta,
    });
    let other = c.var("Other");
    let guard = c.lit("true");
    let cb = c.node(CoreExpr::Clause {
        patterns: vec![other],
        guard,
        body: tb,
    });
    let body = c.node(CoreExpr::Case {
        argument: use_p,
        clauses: vec![ca, cb],
    });
    c.named("make", vec![p], body);
    let x = c.var("X");
    let arg = c.var("X");
    let target = c.fvar("make", 1);
    let tuple_arg = c.node(CoreExpr::Apply {
        operator: target,
        args: vec![arg],
    });
    let tag = c.lit("a");
    let value = c.var("Value");
    let pattern = c.node(CoreExpr::Tuple(vec![tag, value]));
    let ret = c.var("Value");
    let guard = c.lit("true");
    let clause = c.node(CoreExpr::Clause {
        patterns: vec![pattern],
        guard,
        body: ret,
    });
    let body = c.node(CoreExpr::Case {
        argument: tuple_arg,
        clauses: vec![clause],
    });
    c.named("read", vec![x], body);
    let out = infer(&c.finish());
    assert_eq!(out.signatures[1].result, Type::integer(1.into()));
}
#[test]
fn map_pattern_requires_key_and_preserves_bound_value() {
    let mut c = RawCore::new();
    let m = c.var("M");
    let use_m = c.var("M");
    let empty_t = c.term(ErlangTerm::Map(vec![]));
    let empty = c.node(CoreExpr::Literal(empty_t));
    let key = c.lit("a");
    let exact = c.lit("exact");
    let value = c.var("Value");
    let pair = c.node(CoreExpr::MapPair {
        operation: exact,
        key,
        value,
    });
    let pattern = c.node(CoreExpr::Map {
        base: empty,
        pairs: vec![pair],
        pattern: true,
    });
    let guard = c.lit("true");
    let ret = c.var("Value");
    let clause = c.node(CoreExpr::Clause {
        patterns: vec![pattern],
        guard,
        body: ret,
    });
    let body = c.node(CoreExpr::Case {
        argument: use_m,
        clauses: vec![clause],
    });
    c.named("read", vec![m], body);
    let out = infer(&c.finish());
    assert_eq!(
        out.signatures[0].arguments[0]
            .map_is_key(&Type::atom("a"))
            .unwrap(),
        Type::atom("true")
    );
    assert!(out.signatures[0].result.is_any());
}
#[test]
fn exact_map_update_of_missing_key_retains_native_warning_types() {
    let mut c = RawCore::new();
    let empty_t = c.term(ErlangTerm::Map(vec![]));
    let empty = c.node(CoreExpr::Literal(empty_t));
    let key = c.lit("a");
    let exact = c.lit("exact");
    let value = c.lit("new");
    let pair = c.node(CoreExpr::MapPair {
        operation: exact,
        key,
        value,
    });
    let body = c.node(CoreExpr::Map {
        base: empty,
        pairs: vec![pair],
        pattern: false,
    });
    c.named("update", vec![], body);
    let out = infer(&c.finish());
    assert!(out.signatures[0].result.is_none());
    assert!(out.warnings.iter().any(|w|matches!(&w.cause,WarningCause::MapUpdate{map,key} if map.map_get(key).unwrap().is_none())));
}
#[test]
fn binary_segments_concatenate_exact_widths_and_refine_integer_values() {
    let mut c = RawCore::new();
    let v_t = c.int(42);
    let value = c.node(CoreExpr::Literal(v_t));
    let size_t = c.int(8);
    let size = c.node(CoreExpr::Literal(size_t));
    let unit_t = c.int(1);
    let unit = c.node(CoreExpr::Literal(unit_t));
    let typ = c.lit("integer");
    let flags = c.node(CoreExpr::Literal(TermId(0)));
    let segment = c.node(CoreExpr::Bitstr {
        value,
        size,
        unit,
        typ,
        flags,
    });
    let binary = c.node(CoreExpr::Binary(vec![segment, segment]));
    c.named("binary", vec![], binary);
    let out = infer(&c.finish());
    assert_eq!(
        out.signatures[0].result,
        Type::bitstring(0.into(), 16.into()).unwrap()
    );
}
#[test]
fn binary_segment_bad_value_keeps_field_and_actual_type() {
    let mut c = RawCore::new();
    let value = c.lit("bad");
    let size_t = c.int(8);
    let size = c.node(CoreExpr::Literal(size_t));
    let unit_t = c.int(1);
    let unit = c.node(CoreExpr::Literal(unit_t));
    let typ = c.lit("integer");
    let flags = c.node(CoreExpr::Literal(TermId(0)));
    let segment = c.node(CoreExpr::Bitstr {
        value,
        size,
        unit,
        typ,
        flags,
    });
    let binary = c.node(CoreExpr::Binary(vec![segment]));
    c.named("binary", vec![], binary);
    let out = infer(&c.finish());
    assert!(out.signatures[0].result.is_none());
    assert!(out.warnings.iter().any(|w|matches!(&w.cause,WarningCause::BinaryConstruction{field:"value",actual,..}if *actual==Type::atom("bad"))));
}
fn binary_reader(signed: bool, all: bool) -> Analysis {
    let mut c = RawCore::new();
    let b = c.var("Binary");
    let arg = c.var("Binary");
    let value = c.var("Value");
    let size = if all {
        c.lit("all")
    } else {
        let t = c.int(8);
        c.node(CoreExpr::Literal(t))
    };
    let t = c.int(1);
    let unit = c.node(CoreExpr::Literal(t));
    let typ = c.lit(if all { "binary" } else { "integer" });
    let flags_term = if signed {
        let t = c.atom("signed");
        c.term(ErlangTerm::List {
            items: vec![t],
            tail: TermId(0),
        })
    } else {
        TermId(0)
    };
    let flags = c.node(CoreExpr::Literal(flags_term));
    let segment = c.node(CoreExpr::Bitstr {
        value,
        size,
        unit,
        typ,
        flags,
    });
    let pattern = c.node(CoreExpr::Binary(vec![segment]));
    let guard = c.lit("true");
    let body = c.var("Value");
    let clause = c.node(CoreExpr::Clause {
        patterns: vec![pattern],
        guard,
        body,
    });
    let body = c.node(CoreExpr::Case {
        argument: arg,
        clauses: vec![clause],
    });
    c.named("read", vec![b], body);
    infer(&c.finish())
}
#[test]
fn unsigned_binary_pattern_binds_precise_eight_bit_integer() {
    let out = binary_reader(false, false);
    assert_eq!(
        out.signatures[0].result,
        Type::range_strings("0", "255").unwrap()
    );
    assert_eq!(
        out.signatures[0].arguments[0],
        Type::bitstring(0.into(), 8.into()).unwrap()
    );
}
#[test]
fn signed_binary_pattern_binds_precise_signed_integer() {
    let out = binary_reader(true, false);
    assert_eq!(
        out.signatures[0].result,
        Type::range_strings("-128", "127").unwrap()
    );
}
#[test]
fn all_binary_pattern_preserves_remainder_type() {
    let out = binary_reader(false, true);
    assert_eq!(
        out.signatures[0].result,
        Type::bitstring(1.into(), 0.into()).unwrap()
    );
}
#[test]
fn invalid_map_base_short_circuits_pair_expressions() {
    let mut c = RawCore::new();
    let base = c.lit("bad_base");
    let key = c.lit("key");
    let opcode = c.lit("unsupported_opcode");
    let value = c.node(CoreExpr::Primop {
        name: opcode,
        args: vec![],
    });
    let operation = c.lit("assoc");
    let pair = c.node(CoreExpr::MapPair {
        operation,
        key,
        value,
    });
    let body = c.node(CoreExpr::Map {
        base,
        pairs: vec![pair],
        pattern: false,
    });
    c.named("bad_map", vec![], body);
    let out = infer(&c.finish());
    assert!(out.signatures[0].result.is_none());
}
#[test]
fn letrec_definition_captures_outer_lexical_value() {
    let mut c = RawCore::new();
    let x = c.var("X");
    let t = c.int(42);
    let argument = c.node(CoreExpr::Literal(t));
    let use_x = c.var("X");
    let local = c.fun(vec![], use_x);
    let key = c.fvar("local", 0);
    let operator = c.fvar("local", 0);
    let call = c.node(CoreExpr::Apply {
        operator,
        args: vec![],
    });
    let inner = c.node(CoreExpr::Letrec {
        definitions: vec![(key, local)],
        body: call,
    });
    let body = c.node(CoreExpr::Let {
        vars: vec![x],
        argument,
        body: inner,
    });
    c.named("outer", vec![], body);
    let out = infer(&c.finish());
    let outer = out.signatures.last().unwrap();
    assert_eq!(outer.result, Type::integer(42.into()));
}
struct ComposedBif;
impl BifProvider for ComposedBif {
    fn admissible_domains(
        &self,
        _: &FunctionName,
    ) -> std::result::Result<Option<Vec<Type>>, String> {
        Ok(Some(vec![Type::all_integers()]))
    }
    fn transfer(
        &self,
        _: &FunctionName,
        args: &[Type],
    ) -> std::result::Result<Option<Transfer>, String> {
        Ok(Some(Transfer {
            accepted: vec![Type::all_integers()],
            result: args[0].clone(),
            can_raise: false,
        }))
    }
    fn guard(
        &self,
        _: &FunctionName,
        _: &[Type],
        _: bool,
    ) -> std::result::Result<Option<GuardTransfer>, String> {
        Ok(None)
    }
}
struct ComposedConstraint;
impl ConstraintProvider for ComposedConstraint {
    fn contract_domains(&self, _: &FunctionName) -> std::result::Result<Option<Vec<Type>>, String> {
        Ok(Some(vec![Type::range_strings("5", "15").unwrap()]))
    }
    fn external_summary(&self, _: &FunctionName) -> std::result::Result<Signature, String> {
        Ok(Signature {
            arguments: vec![Type::range_strings("0", "10").unwrap()],
            result: Type::any(),
        })
    }
    fn contract_constraint(
        &self,
        _: &FunctionName,
        args: &[Type],
    ) -> std::result::Result<Option<Signature>, String> {
        Ok(Some(Signature {
            arguments: vec![Type::range_strings("5", "15").unwrap()],
            result: args[0].clone(),
        }))
    }
}
#[test]
fn calls_intersect_success_contract_and_bif_before_dependent_return() {
    let mut c = RawCore::new();
    let x = c.var("X");
    let arg = c.var("X");
    let module = c.lit("external");
    let name = c.lit("identity");
    let body = c.node(CoreExpr::Call {
        module,
        name,
        args: vec![arg],
    });
    c.named("call", vec![x], body);
    let core = c.finish();
    let program = Program::build(&core).unwrap();
    let plan = build_plan(&core, &program).unwrap();
    let out = Engine {
        core: &core,
        program: &program,
        plan: &plan,
        bifs: &ComposedBif,
        constraints: &ComposedConstraint,
        depth: 3,
        iteration_limit: 100,
    }
    .infer()
    .unwrap();
    let expected = Type::range_strings("5", "10").unwrap();
    assert_eq!(out.signatures[0].arguments, vec![expected.clone()]);
    assert_eq!(out.signatures[0].result, expected);
}
#[test]
fn generic_dynamic_remote_call_refines_module_and_name_atoms() {
    let mut c = RawCore::new();
    let m = c.var("M");
    let f = c.var("F");
    let module = c.var("M");
    let name = c.var("F");
    let body = c.node(CoreExpr::Call {
        module,
        name,
        args: vec![],
    });
    c.named("dynamic", vec![m, f], body);
    let out = infer(&c.finish());
    assert_eq!(
        out.signatures[0].arguments,
        vec![Type::all_atoms(), Type::all_atoms()]
    );
    assert!(out.signatures[0].result.is_any());
}
#[test]
fn singleton_dynamic_module_resolves_shared_local_summary() {
    let mut c = RawCore::new();
    let t = c.int(42);
    let ret = c.node(CoreExpr::Literal(t));
    c.named("callee", vec![], ret);
    let m = c.var("M");
    let argument = c.lit("probe");
    let module = c.var("M");
    let name = c.lit("callee");
    let call = c.node(CoreExpr::Call {
        module,
        name,
        args: vec![],
    });
    let body = c.node(CoreExpr::Let {
        vars: vec![m],
        argument,
        body: call,
    });
    c.named("caller", vec![], body);
    let out = infer(&c.finish());
    assert_eq!(out.signatures[1].result, Type::integer(42.into()));
}
#[test]
fn invalid_remote_module_preserves_target_failure_details() {
    let mut c = RawCore::new();
    let t = c.int(42);
    let module = c.node(CoreExpr::Literal(t));
    let name = c.lit("call");
    let body = c.node(CoreExpr::Call {
        module,
        name,
        args: vec![],
    });
    c.named("caller", vec![], body);
    let out = infer(&c.finish());
    assert!(out.warnings.iter().any(|w|matches!(&w.cause,WarningCause::RemoteTarget{field:"module",actual,..} if *actual==Type::integer(42.into()))));
}
#[test]
fn exhausted_product_residual_preserves_clause_arity_and_coverage_warning() {
    let mut c = RawCore::new();
    let first = c.lit("a");
    let second = c.lit("b");
    let argument = c.node(CoreExpr::Values(vec![first, second]));
    let x = c.var("X");
    let y = c.var("Y");
    let guard = c.lit("true");
    let body = c.lit("ok");
    let clause = c.node(CoreExpr::Clause {
        patterns: vec![x, y],
        guard,
        body,
    });
    let a = c.lit("a");
    let b = c.lit("b");
    let guard = c.lit("true");
    let body = c.lit("covered");
    let covered = c.node(CoreExpr::Clause {
        patterns: vec![a, b],
        guard,
        body,
    });
    let body = c.node(CoreExpr::Case {
        argument,
        clauses: vec![clause, covered],
    });
    c.named("product", vec![], body);
    let out = infer(&c.finish());
    assert_eq!(out.signatures[0].result, Type::atom("ok"));
    assert!(
        out.warnings
            .iter()
            .any(|w| matches!(&w.cause, WarningCause::PatternCovered { .. }))
    );
}
struct TupleGuard;
impl BifProvider for TupleGuard {
    fn admissible_domains(
        &self,
        _: &FunctionName,
    ) -> std::result::Result<Option<Vec<Type>>, String> {
        Ok(Some(vec![Type::any()]))
    }
    fn transfer(
        &self,
        _: &FunctionName,
        _: &[Type],
    ) -> std::result::Result<Option<Transfer>, String> {
        Ok(Some(Transfer {
            accepted: vec![Type::any()],
            result: Type::atoms(["true".into(), "false".into()]),
            can_raise: false,
        }))
    }
    fn guard(
        &self,
        _: &FunctionName,
        args: &[Type],
        truth: bool,
    ) -> std::result::Result<Option<GuardTransfer>, String> {
        let domain = Type::tuple(vec![Type::atom("tag"), Type::any()]).unwrap();
        Ok(Some(GuardTransfer {
            arguments: vec![if truth {
                args[0].meet(&domain).unwrap()
            } else {
                args[0].subtract(&domain).unwrap()
            }],
            result: Type::atom(if truth { "true" } else { "false" }),
        }))
    }
}
struct BroadExternal;
impl ConstraintProvider for BroadExternal {
    fn contract_domains(&self, _: &FunctionName) -> std::result::Result<Option<Vec<Type>>, String> {
        Ok(None)
    }
    fn external_summary(&self, n: &FunctionName) -> std::result::Result<Signature, String> {
        Ok(Signature {
            arguments: vec![Type::any(); n.arity],
            result: Type::any(),
        })
    }
    fn contract_constraint(
        &self,
        _: &FunctionName,
        _: &[Type],
    ) -> std::result::Result<Option<Signature>, String> {
        Ok(None)
    }
}
#[test]
fn scoped_guard_expression_projection_refines_tuple_child_binding() {
    let mut c = RawCore::new();
    let x = c.var("X");
    let child = c.var("X");
    let nil = c.node(CoreExpr::Literal(TermId(0)));
    let tuple = c.node(CoreExpr::Tuple(vec![child, nil]));
    let module = c.lit("predicate");
    let name = c.lit("tagged_tuple");
    let guard = c.node(CoreExpr::Call {
        module,
        name,
        args: vec![tuple],
    });
    let argument = c.lit("ok");
    let pattern = c.lit("ok");
    let body = c.var("X");
    let clause = c.node(CoreExpr::Clause {
        patterns: vec![pattern],
        guard,
        body,
    });
    let body = c.node(CoreExpr::Case {
        argument,
        clauses: vec![clause],
    });
    c.named("guarded", vec![x], body);
    let core = c.finish();
    let program = Program::build(&core).unwrap();
    let plan = build_plan(&core, &program).unwrap();
    let out = Engine {
        core: &core,
        program: &program,
        plan: &plan,
        bifs: &TupleGuard,
        constraints: &BroadExternal,
        depth: 3,
        iteration_limit: 100,
    }
    .infer()
    .unwrap();
    assert_eq!(out.signatures[0].arguments, vec![Type::atom("tag")]);
    assert_eq!(out.signatures[0].result, Type::atom("tag"));
}
struct ContractOnly;
impl ConstraintProvider for ContractOnly {
    fn contract_domains(&self, _: &FunctionName) -> std::result::Result<Option<Vec<Type>>, String> {
        Ok(Some(vec![Type::atom("a")]))
    }
    fn external_summary(&self, n: &FunctionName) -> std::result::Result<Signature, String> {
        Ok(Signature {
            arguments: vec![Type::any(); n.arity],
            result: Type::any(),
        })
    }
    fn contract_constraint(
        &self,
        _: &FunctionName,
        _: &[Type],
    ) -> std::result::Result<Option<Signature>, String> {
        Ok(Some(Signature {
            arguments: vec![Type::atom("a")],
            result: Type::any(),
        }))
    }
}
#[test]
fn failing_call_evidence_keeps_contract_failure_independent_of_success_and_bif() {
    let mut c = RawCore::new();
    let module = c.lit("external");
    let name = c.lit("specified");
    let arg = c.lit("z");
    let body = c.node(CoreExpr::Call {
        module,
        name,
        args: vec![arg],
    });
    c.named("caller", vec![], body);
    let core = c.finish();
    let program = Program::build(&core).unwrap();
    let plan = build_plan(&core, &program).unwrap();
    let out = Engine {
        core: &core,
        program: &program,
        plan: &plan,
        bifs: &NoBifs,
        constraints: &ContractOnly,
        depth: 3,
        iteration_limit: 100,
    }
    .infer()
    .unwrap();
    assert!(out.warnings.iter().any(|w|matches!(&w.cause,WarningCause::CallArguments{evidence:Some(e),..} if e.failed_contract && !e.failed_success && !e.failed_bif && e.contract.as_ref().unwrap().arguments==vec![Type::atom("a")] && e.success.arguments==vec![Type::any()] && e.bif.is_none())));
}
#[test]
fn absent_contract_is_not_relabelled_as_failing_success_constraint() {
    let mut c = RawCore::new();
    let x = c.var("X");
    let use_x = c.var("X");
    let pattern = c.lit("a");
    let guard = c.lit("true");
    let body = c.lit("ok");
    let clause = c.node(CoreExpr::Clause {
        patterns: vec![pattern],
        guard,
        body,
    });
    let body = c.node(CoreExpr::Case {
        argument: use_x,
        clauses: vec![clause],
    });
    c.named("callee", vec![x], body);
    let operator = c.fvar("callee", 1);
    let arg = c.lit("z");
    let body = c.node(CoreExpr::Apply {
        operator,
        args: vec![arg],
    });
    c.named("caller", vec![], body);
    let out = infer(&c.finish());
    assert!(out.warnings.iter().any(|w|matches!(&w.cause,WarningCause::CallArguments{evidence:Some(e),..} if e.failed_success && !e.failed_contract && !e.failed_bif && e.contract.is_none())));
}

fn build_plan(core: &CoreProgram, program: &Program) -> std::result::Result<Plan, String> {
    let policy = crate::closure_dependencies::DependencyPolicy::parse(include_str!(
        "../corpus/dependency_policy.json"
    ))?;
    let closures =
        crate::closure_dependencies::ClosureDependencies::analyze(core, program, &policy)?;
    Plan::build(program, closures)
}

#[test]
fn multi_target_apply_filters_each_candidate_before_joining_results() {
    let mut c = RawCore::new();
    let select = c.var("Select");
    let true_guard = c.lit("true");
    let a = c.var("A");
    let only_a = c.lit("a");
    let result_a = c.lit("accepted_a");
    let clause_a = c.node(CoreExpr::Clause {
        patterns: vec![only_a],
        guard: true_guard,
        body: result_a,
    });
    let body_a = c.node(CoreExpr::Case {
        argument: a,
        clauses: vec![clause_a],
    });
    let fa = c.fun(vec![a], body_a);
    let z = c.var("Z");
    let only_z = c.lit("z");
    let result_z = c.lit("accepted_z");
    let clause_z = c.node(CoreExpr::Clause {
        patterns: vec![only_z],
        guard: true_guard,
        body: result_z,
    });
    let body_z = c.node(CoreExpr::Case {
        argument: z,
        clauses: vec![clause_z],
    });
    let fz = c.fun(vec![z], body_z);
    let pick_a = c.lit("a");
    let pick_z = c.lit("z");
    let ca = c.node(CoreExpr::Clause {
        patterns: vec![pick_a],
        guard: true_guard,
        body: fa,
    });
    let cz = c.node(CoreExpr::Clause {
        patterns: vec![pick_z],
        guard: true_guard,
        body: fz,
    });
    let choice = c.node(CoreExpr::Case {
        argument: select,
        clauses: vec![ca, cz],
    });
    let f = c.var("F");
    let argument = c.lit("a");
    let apply = c.node(CoreExpr::Apply {
        operator: f,
        args: vec![argument],
    });
    let body = c.node(CoreExpr::Let {
        vars: vec![f],
        argument: choice,
        body: apply,
    });
    let run = c.named("run", vec![select], body);
    let core = c.finish();
    let out = infer(&core);
    let p = Program::build(&core).unwrap();
    let id = p.functions.iter().find(|f| f.node == run).unwrap().id;
    assert_eq!(out.signatures[id.0].result, Type::atom("accepted_a"));
    assert!(!out.warnings.iter().any(|w| w.function == id
        && matches!(
            w.cause,
            WarningCause::ApplyArguments { .. } | WarningCause::ApplyCallArguments { .. }
        )));
}
#[test]
fn returned_closure_escapes_with_unconstrained_contextual_inputs() {
    let mut c = RawCore::new();
    let arg = c.var("Arg");
    let use_arg = c.var("Arg");
    let closure = c.fun(vec![arg], use_arg);
    c.named("returns_fun", vec![], closure);
    let core = c.finish();
    let p = Program::build(&core).unwrap();
    let out = infer(&core);
    let id = p.functions.iter().find(|f| f.node == closure).unwrap().id;
    assert_eq!(out.contextual_inputs[id.0], vec![Type::any()]);
    assert_eq!(out.signatures[id.0].result, Type::any());
}
