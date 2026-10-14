use super::*;
use crate::core::*;
use crate::program::*;
use std::collections::BTreeMap;
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
        let core = self.node(CoreExpr::Module(Box::new(crate::core::ModuleExpr {
            name,
            exports,
            attributes: self.attrs.clone(),
            definitions: self.defs.clone(),
        })));
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
                nodes: self.nodes.into(),
            }],
            node_count,
            term_count,
            kinds: BTreeMap::new(),
        }
    }
}

fn analyze(core: &CoreProgram) -> (Program, ClosureDependencies) {
    let p = Program::build(core).unwrap();
    let policy = DependencyPolicy::parse(include_str!("../corpus/dependency_policy.json")).unwrap();
    let d = ClosureDependencies::analyze(core, &p, &policy).unwrap();
    (p, d)
}
fn id(p: &Program, node: NodeId) -> FunctionId {
    p.functions.iter().find(|f| f.node == node).unwrap().id
}
#[test]
fn conditional_apply_preserves_each_matching_target() {
    let mut b = RawCore::new();
    let x = b.var("X");
    let a = b.lit("a");
    let fa = b.fun(vec![], a);
    let z = b.var("Z");
    let c = b.lit("c");
    let fc = b.fun(vec![z], c);
    let d = b.lit("d");
    let fd = b.fun(vec![], d);
    let truth = b.lit("true");
    let p1 = b.lit("true");
    let p2 = b.lit("false");
    let pv = b.var("Other");
    let c1 = b.node(CoreExpr::Clause {
        patterns: vec![p1],
        guard: truth,
        body: fa,
    });
    let c2 = b.node(CoreExpr::Clause {
        patterns: vec![p2],
        guard: truth,
        body: fd,
    });
    let c3 = b.node(CoreExpr::Clause {
        patterns: vec![pv],
        guard: truth,
        body: fc,
    });
    let branch = b.node(CoreExpr::Case {
        argument: x,
        clauses: vec![c1, c2, c3],
    });
    let f = b.var("F");
    let apply = b.node(CoreExpr::Apply {
        operator: f,
        args: vec![],
    });
    let body = b.node(CoreExpr::Let {
        vars: vec![f],
        argument: branch,
        body: apply,
    });
    let root = b.named("run", vec![x], body);
    let core = b.finish();
    let (p, d) = analyze(&core);
    let site = NodeRef {
        module: ModuleId(0),
        node: apply.0,
    };
    assert_eq!(
        d.call_targets[&site].local,
        std::collections::BTreeSet::from([id(&p, fa), id(&p, fd)])
    );
    assert!(!d.call_targets[&site].external);
    assert!(d.dependencies[id(&p, root).0].local.contains(&id(&p, fc)));
    assert!(!d.call_targets[&site].local.contains(&id(&p, fc)));
}
#[test]
fn multi_return_keeps_function_coordinates() {
    let mut b = RawCore::new();
    let a = b.lit("a");
    let fa = b.fun(vec![], a);
    let z = b.lit("z");
    let fz = b.fun(vec![], z);
    let values = b.node(CoreExpr::Values(vec![fa, fz]));
    let x = b.var("X");
    let y = b.var("Y");
    let apply = b.node(CoreExpr::Apply {
        operator: y,
        args: vec![],
    });
    let body = b.node(CoreExpr::Let {
        vars: vec![x, y],
        argument: values,
        body: apply,
    });
    b.named("run", vec![], body);
    let core = b.finish();
    let (p, d) = analyze(&core);
    assert_eq!(
        d.call_targets[&NodeRef {
            module: ModuleId(0),
            node: apply.0
        }]
            .local,
        std::collections::BTreeSet::from([id(&p, fz)])
    );
}
#[test]
fn callback_arguments_escape_and_link_to_parent() {
    let mut b = RawCore::new();
    let value = b.lit("done");
    let closure = b.fun(vec![], value);
    let module = b.lit("library");
    let name = b.lit("callback");
    let call = b.node(CoreExpr::Call {
        module,
        name,
        args: vec![closure],
    });
    let root = b.named("run", vec![], call);
    let core = b.finish();
    let (p, d) = analyze(&core);
    assert!(d.escaping.local.contains(&id(&p, closure)));
    assert!(
        d.dependencies[id(&p, closure).0]
            .local
            .contains(&id(&p, root))
    );
    assert!(
        d.dependencies[id(&p, root).0]
            .local
            .contains(&id(&p, closure))
    );
}
#[test]
fn deep_sequence_is_iterative() {
    let mut b = RawCore::new();
    let lit = b.lit("ok");
    let mut body = lit;
    for _ in 0..25000 {
        body = b.node(CoreExpr::Seq {
            argument: lit,
            body,
        });
    }
    b.named("run", vec![], body);
    let core = b.finish();
    let (_, d) = analyze(&core);
    assert_eq!(d.escaping.local.len(), 1);
}
