use super::*;
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
    fn list(&mut self, items: Vec<TermId>) -> TermId {
        self.term(ErlangTerm::List {
            items,
            tail: TermId(0),
        })
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
    fn attr(&mut self, name: &str, raw: TermId) {
        let key = self.lit(name);
        let value = self.node(CoreExpr::Literal(raw));
        self.attrs.push((key, value));
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
                terms: self.terms.into(),
                nodes: self.nodes,
            }],
            node_count,
            term_count,
            kinds: BTreeMap::new(),
        }
    }
}
fn reference(node: NodeId) -> NodeRef {
    NodeRef {
        module: ModuleId(0),
        node: node.0,
    }
}
#[test]
fn let_argument_uses_outer_parameter_and_body_uses_shadow() {
    let mut c = RawCore::new();
    let parameter = c.var("X");
    let argument = c.var("X");
    let binder = c.var("X");
    let body = c.var("X");
    let let_node = c.node(CoreExpr::Let {
        vars: vec![binder],
        argument,
        body,
    });
    c.named("shadow", vec![parameter], let_node);
    let program = Program::build(&c.finish()).unwrap();
    assert_eq!(
        program.variable_uses[&reference(argument)],
        program.binder_nodes[&reference(parameter)]
    );
    assert_eq!(
        program.variable_uses[&reference(body)],
        program.binder_nodes[&reference(binder)]
    );
    assert_ne!(
        program.variable_uses[&reference(argument)],
        program.variable_uses[&reference(body)]
    );
}
#[test]
fn mutually_recursive_letrec_closures_resolve_and_capture_outer_parameter() {
    let mut c = RawCore::new();
    let x = c.var("X");
    let xuse = c.var("X");
    let fkey = c.fvar("f", 0);
    let gkey = c.fvar("g", 0);
    let guse = c.fvar("g", 0);
    let fcall = c.node(CoreExpr::Apply {
        operator: guse,
        args: vec![],
    });
    let fbody = c.node(CoreExpr::Tuple(vec![xuse, fcall]));
    let f = c.fun(vec![], fbody);
    let fuse = c.fvar("f", 0);
    let gbody = c.node(CoreExpr::Apply {
        operator: fuse,
        args: vec![],
    });
    let g = c.fun(vec![], gbody);
    let entry = c.fvar("f", 0);
    let invoke = c.node(CoreExpr::Apply {
        operator: entry,
        args: vec![],
    });
    let group = c.node(CoreExpr::Letrec {
        definitions: vec![(fkey, f), (gkey, g)],
        body: invoke,
    });
    let outer = c.named("loop", vec![x], group);
    let core = c.finish();
    let p = Program::build(&core).unwrap();
    let plan = build_plan(&core, &p).unwrap();
    let fid = p.function_nodes[0][&f.0];
    let gid = p.function_nodes[0][&g.0];
    let outerid = p.function_nodes[0][&outer.0];
    assert_eq!(p.functions[fid.0].parent, Some(outerid));
    assert!(
        p.functions[fid.0]
            .captures
            .contains(&p.binder_nodes[&reference(x)])
    );
    assert_eq!(
        plan.closures.call_targets[&reference(fcall)].local,
        std::collections::BTreeSet::from([gid])
    );
    assert_eq!(
        plan.closures.call_targets[&reference(gbody)].local,
        std::collections::BTreeSet::from([fid])
    );
}

#[test]
fn try_results_and_exception_variables_have_separate_scopes() {
    let mut c = RawCore::new();
    let argument = c.lit("ok");
    let result = c.var("Value");
    let success = c.var("Value");
    let ex = c.var("Error");
    let handler = c.var("Error");
    let node = c.node(CoreExpr::Try {
        argument,
        vars: vec![result],
        body: success,
        exception_vars: vec![ex],
        handler,
    });
    c.named("exception", vec![], node);
    let core = c.finish();
    let p = Program::build(&core).unwrap();
    assert_eq!(
        p.variable_uses[&reference(success)],
        p.binder_nodes[&reference(result)]
    );
    assert_eq!(
        p.variable_uses[&reference(handler)],
        p.binder_nodes[&reference(ex)]
    );
    assert_ne!(
        p.bindings[p.binder_nodes[&reference(result)].0].scope,
        p.bindings[p.binder_nodes[&reference(ex)].0].scope
    );
    let mut wrong = core.clone();
    let CoreExpr::Var(value) = wrong.modules[0].nodes[success.0].expr else {
        panic!()
    };
    wrong.modules[0].nodes[handler.0].expr = CoreExpr::Var(value);
    assert!(
        Program::build(&wrong)
            .unwrap_err()
            .contains("unbound variable")
    );
}
#[test]
fn pattern_binary_size_and_map_keys_use_pre_binding_environment() {
    let mut c = RawCore::new();
    let outer = c.var("X");
    let size = c.var("X");
    let bound = c.var("X");
    let flags = c.lit("integer");
    let bit = c.node(CoreExpr::Bitstr {
        value: bound,
        size,
        unit: flags,
        typ: flags,
        flags,
    });
    let binary = c.node(CoreExpr::Binary(vec![bit]));
    let guard = c.var("X");
    let body = c.var("X");
    let clause = c.node(CoreExpr::Clause {
        patterns: vec![binary],
        guard,
        body,
    });
    let arg = c.lit("binary");
    let case = c.node(CoreExpr::Case {
        argument: arg,
        clauses: vec![clause],
    });
    c.named("bits", vec![outer], case);
    let p = Program::build(&c.finish()).unwrap();
    assert_eq!(
        p.variable_uses[&reference(size)],
        p.binder_nodes[&reference(outer)]
    );
    assert_eq!(
        p.variable_uses[&reference(guard)],
        p.binder_nodes[&reference(bound)]
    );
    let mut c = RawCore::new();
    let outer = c.var("Key");
    let key = c.var("Key");
    let bound = c.var("Key");
    let op = c.lit("exact");
    let pair = c.node(CoreExpr::MapPair {
        operation: op,
        key,
        value: bound,
    });
    let base = c.lit("empty");
    let map = c.node(CoreExpr::Map {
        base,
        pairs: vec![pair],
        pattern: true,
    });
    let guard = c.lit("true");
    let body = c.var("Key");
    let clause = c.node(CoreExpr::Clause {
        patterns: vec![map],
        guard,
        body,
    });
    let argument = c.lit("map");
    let case = c.node(CoreExpr::Case {
        argument,
        clauses: vec![clause],
    });
    c.named("map_key", vec![outer], case);
    let p = Program::build(&c.finish()).unwrap();
    assert_eq!(
        p.variable_uses[&reference(key)],
        p.binder_nodes[&reference(outer)]
    );
    assert_eq!(
        p.variable_uses[&reference(body)],
        p.binder_nodes[&reference(bound)]
    );
}
#[test]
fn closure_let_binding_and_variable_alias_preserve_dependency_target_set() {
    let mut c = RawCore::new();
    let x = c.var("X");
    let usex = c.var("X");
    let closure = c.fun(vec![], usex);
    let f = c.var("F");
    let farg = c.var("F");
    let alias = c.var("Alias");
    let usealias = c.var("Alias");
    let apply = c.node(CoreExpr::Apply {
        operator: usealias,
        args: vec![],
    });
    let inner = c.node(CoreExpr::Let {
        vars: vec![alias],
        argument: farg,
        body: apply,
    });
    let outer = c.node(CoreExpr::Let {
        vars: vec![f],
        argument: closure,
        body: inner,
    });
    c.named("capture", vec![x], outer);
    let core = c.finish();
    let p = Program::build(&core).unwrap();
    let plan = build_plan(&core, &p).unwrap();
    let target = p.function_nodes[0][&closure.0];
    assert_eq!(
        plan.closures.call_targets[&reference(apply)].local,
        std::collections::BTreeSet::from([target])
    );
    assert!(
        p.functions[target.0]
            .captures
            .contains(&p.binder_nodes[&reference(x)])
    );
}
#[test]
fn type_ast_edges_do_not_mistake_type_declaration_names_for_tags() {
    let mut c = RawCore::new();
    let tag = c.atom("atom");
    let anno = c.int(7);
    let module = c.atom("external");
    let mod_ast = c.tuple(vec![tag, anno, module]);
    let name = c.atom("thing");
    let name_ast = c.tuple(vec![tag, anno, name]);
    let args = TermId(0);
    let payload = c.list(vec![mod_ast, name_ast, args]);
    let remote = c.atom("remote_type");
    let ast = c.tuple(vec![remote, anno, payload]);
    let typename = c.atom("remote_type");
    let decl = c.tuple(vec![typename, ast, args]);
    let list = c.list(vec![decl]);
    c.attr("type", list);
    let core = c.finish();
    let p = Program::build(&core).unwrap();
    assert_eq!(p.metadata[0].types[0].name, "remote_type");
    assert_eq!(p.type_dependencies.len(), 1);
    assert_eq!(
        p.type_dependencies.first().unwrap().target,
        TypeName {
            module: "external".into(),
            name: "thing".into(),
            arity: 0
        }
    );
}
#[test]
fn exact_source_origins_keep_columns_files_and_generation() {
    let mut c = RawCore::new();
    let line = c.int(12);
    let column = c.int(9);
    let location = c.tuple(vec![line, column]);
    let file = c.atom("file");
    let character = c.int(97);
    let filename = c.list(vec![character]);
    let pair = c.tuple(vec![file, filename]);
    let annotations = c.list(vec![location, pair]);
    let body = c.lit("ok");
    c.nodes[body.0].annotations = annotations;
    c.named("source", vec![], body);
    let p = Program::build(&c.finish()).unwrap();
    let origin = &p.origins[0][body.0];
    assert_eq!(origin.file.as_deref(), Some("a"));
    assert_eq!(origin.line, Some(12));
    assert_eq!(origin.column, Some(9));
    assert!(!origin.generated);
    assert!(p.origins[0].last().unwrap().generated);
}
#[test]
fn unbound_uses_and_ambiguous_dag_binding_reuse_are_errors() {
    let mut c = RawCore::new();
    let body = c.var("Missing");
    c.named("unbound", vec![], body);
    assert!(
        Program::build(&c.finish())
            .unwrap_err()
            .contains("unbound variable")
    );
    let mut c = RawCore::new();
    let x = c.var("X");
    let usex = c.var("X");
    let binder = c.var("X");
    let let_node = c.node(CoreExpr::Let {
        vars: vec![binder],
        argument: usex,
        body: usex,
    });
    c.named("dag", vec![x], let_node);
    assert!(
        Program::build(&c.finish())
            .unwrap_err()
            .contains("DAG variable reused")
    );
}
#[test]
fn deep_lexical_scopes_use_iterative_walk_and_lookup() {
    let mut c = RawCore::new();
    let parameter = c.var("Root");
    let mut body = c.var("Root");
    let literal = c.lit("ok");
    for i in 0..10_000 {
        let var = c.var(&format!("unused{i}"));
        body = c.node(CoreExpr::Let {
            vars: vec![var],
            argument: literal,
            body,
        });
    }
    c.named("deep", vec![parameter], body);
    let p = Program::build(&c.finish()).unwrap();
    assert_eq!(p.bindings.len(), 10_002);
    assert_eq!(p.variable_uses.len(), 1);
    drop(p);
}

#[test]
fn dependency_plan_resolves_self_calls_and_keeps_external_and_dynamic_sites() {
    let mut c = RawCore::new();
    let x = c.var("X");
    let f = c.var("F");
    let current = c.lit("probe");
    let name = c.lit("f");
    let xuse = c.var("X");
    let fuse = c.var("F");
    let recurse = c.node(CoreExpr::Call {
        module: current,
        name,
        args: vec![xuse, fuse],
    });
    let erlang = c.lit("erlang");
    let length = c.lit("length");
    let xuse = c.var("X");
    let external = c.node(CoreExpr::Call {
        module: erlang,
        name: length,
        args: vec![xuse],
    });
    let fuse = c.var("F");
    let dynamic = c.node(CoreExpr::Apply {
        operator: fuse,
        args: vec![],
    });
    let body = c.node(CoreExpr::Tuple(vec![recurse, external, dynamic]));
    c.named("f", vec![x, f], body);
    let core = c.finish();
    let program = Program::build(&core).unwrap();
    let plan = build_plan(&core, &program).unwrap();
    assert_eq!(plan.functions.vertices.len(), 1);
    assert!(plan.functions.components[0].recursive);
    assert_eq!(plan.inspect().internal_call_sites, 1);
    assert_eq!(plan.external_calls.len(), 1);
    assert_eq!(plan.dynamic_calls.len(), 1);
    assert_eq!(plan.external_calls[0].target.name, "length");
}

#[test]
fn type_only_dependency_cycle_uses_the_same_graph_without_functions() {
    let mut c = RawCore::new();
    let anno = c.int(1);
    let tag = c.atom("user_type");
    let a = c.atom("a");
    let b = c.atom("b");
    let zero = TermId(0);
    let b_ref = c.tuple(vec![tag, anno, b, zero]);
    let a_ref = c.tuple(vec![tag, anno, a, zero]);
    let a_decl = c.tuple(vec![a, b_ref, zero]);
    let b_decl = c.tuple(vec![b, a_ref, zero]);
    let a_payload = c.list(vec![a_decl]);
    let b_payload = c.list(vec![b_decl]);
    c.attr("type", a_payload);
    c.attr("opaque", b_payload);
    let core = c.finish();
    let program = Program::build(&core).unwrap();
    let plan = build_plan(&core, &program).unwrap();
    assert!(plan.functions.vertices.is_empty());
    assert_eq!(plan.type_declarations.len(), 2);
    assert_eq!(plan.declarations.components.len(), 1);
    assert!(plan.declarations.components[0].recursive);
    assert_eq!(plan.declarations.components[0].members.len(), 2);
    assert!(plan.external_types.is_empty());
}

fn build_plan(
    core: &CoreProgram,
    program: &Program,
) -> std::result::Result<crate::schedule::Plan, String> {
    let policy = crate::closure_dependencies::DependencyPolicy::parse(include_str!(
        "../corpus/dependency_policy.json"
    ))?;
    let closures =
        crate::closure_dependencies::ClosureDependencies::analyze(core, program, &policy)?;
    crate::schedule::Plan::build(program, closures)
}
