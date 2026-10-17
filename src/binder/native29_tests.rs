use super::*;
use crate::{
    core::{
        CompilerProfile, CoreExpr, CoreModule, CoreNode, CoreProgram, CoreVersion, ErlangTerm,
        NodeId, TermId, TermMapOrder,
    },
    folded_core::FoldedCore,
    guard_control::{GuardControl, GuardEnvironment, GuardError},
    guard_literals::GuardLiterals,
    guard_ops::GuardEval,
    guard_producer::GuardProducer,
    guard_warning::GuardWarning,
    labels::{CoreLabel, Labels},
    lattice::{NativeRecordName, PresentationContext, RecordField},
    program::{ModuleId, NodeRef},
};
use serde_json::{Value, json};

#[derive(Clone, Debug, PartialEq)]
struct IgnoredWarning;
impl From<OpaqueMatch> for IgnoredWarning {
    fn from(_: OpaqueMatch) -> Self {
        Self
    }
}
impl From<GuardWarning> for IgnoredWarning {
    fn from(_: GuardWarning) -> Self {
        Self
    }
}

#[derive(Default)]
struct Tree {
    terms: Vec<ErlangTerm>,
    nodes: Vec<CoreNode>,
}
impl Tree {
    fn new() -> Self {
        Self {
            terms: vec![ErlangTerm::Nil],
            nodes: vec![],
        }
    }
    fn term(&mut self, term: ErlangTerm) -> TermId {
        let id = TermId(self.terms.len());
        self.terms.push(term);
        id
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
    fn atom(&mut self, name: &str) -> NodeId {
        let term = self.term(ErlangTerm::Atom(name.into()));
        self.node(CoreExpr::Literal(term))
    }
    fn variable(&mut self) -> NodeId {
        let term = self.term(ErlangTerm::Atom("X".into()));
        self.node(CoreExpr::Var(term))
    }
    fn record(&mut self, anonymous: bool, value: NodeId) -> NodeId {
        let argument = self.atom("ok");
        let id = if anonymous {
            self.node(CoreExpr::Literal(TermId(0)))
        } else {
            self.atom("entry")
        };
        let key = self.atom("value");
        let pair = self.node(CoreExpr::RecordPair { key, value });
        self.node(CoreExpr::Record {
            argument,
            id,
            pairs: vec![pair],
        })
    }
    fn call(&mut self, source: NodeId) -> NodeId {
        let module = self.atom("erlang");
        let name = self.atom("is_record");
        let tag = self.atom("external");
        let record = self.atom("entry");
        self.node(CoreExpr::Call {
            module,
            name,
            args: vec![source, tag, record],
        })
    }
    fn field(&mut self, source: NodeId) -> NodeId {
        let name = self.atom("record_field");
        let record = self.atom("entry");
        let field = self.atom("value");
        self.node(CoreExpr::Primop {
            name,
            args: vec![source, record, field],
        })
    }
    fn finish(self, root: NodeId) -> CoreProgram {
        let node_count = self.nodes.len();
        let term_count = self.terms.len();
        CoreProgram {
            version: CoreVersion::V6,
            profile: CompilerProfile {
                otp: "29".into(),
                erts: String::new(),
                elixir: String::new(),
                dialyzer: String::new(),
                core_options: vec![],
                term_map_order: TermMapOrder::NativeToListWithIterator,
            },
            atom_layout: None,
            contract_closure_profile: None,
            modules: vec![CoreModule {
                name: "fixture".into(),
                source_forms: TermId(0),
                core: root,
                terms: self.terms.into(),
                nodes: self.nodes.into(),
                map_iterator_orders: BTreeMap::new(),
                contract_variable_maps: BTreeMap::new(),
            }],
            node_count,
            term_count,
            kinds: BTreeMap::new(),
        }
    }
}
fn node(id: NodeId) -> NodeRef {
    NodeRef {
        module: ModuleId(0),
        node: id.0,
    }
}
fn record(field: Type) -> Type {
    Type::record(
        NativeRecordName::Named {
            module: "external".into(),
            name: "entry".into(),
        },
        BTreeMap::from([("value".into(), RecordField::Present(field))]),
    )
}
fn state(core: &CoreProgram) -> BinderState<IgnoredWarning> {
    let mut state = BinderState::for_compiler_module(core, ModuleId(0), false).unwrap();
    state.native_records = BTreeMap::from([(
        ("external".into(), "entry".into()),
        BinderRecord {
            prototype: record(Type::builtin(BuiltinType::Integer).unwrap()),
            fields: vec![],
        },
    )])
    .into();
    state.native_record_imports = BTreeMap::from([("entry".into(), "external".into())]).into();
    state
}
fn text(value: Type) -> String {
    PresentationContext::default().type_to_string(&value).unwrap()
}
fn expected(id: &str) -> Value {
    let oracle: Value = serde_json::from_str(include_str!(
        "../../backend29/tests/fixtures/otp29/dataflow_oracle.json"
    ))
    .unwrap();
    oracle["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == id)
        .unwrap()
        .clone()
}
#[test]
fn native_record_pattern_matches_captured_otp29_binding_and_failures() {
    for id in ["anonymous", "field_noop", "field_failure"] {
        let mut tree = Tree::new();
        let x = tree.variable();
        let value = if id == "field_failure" {
            tree.atom("bad")
        } else {
            x
        };
        let pattern = tree.record(id == "anonymous", value);
        let core = tree.finish(pattern);
        let labels = Labels::build(&core, CoreLabel(0)).unwrap();
        let nodes = crate::dataflow_nodes::DataflowNodes::build(&core, &labels).unwrap();
        let mut map = DataflowMap::default();
        nodes.mark_fresh(&[node(pattern)], &mut map).unwrap();
        if id == "field_noop" {
            nodes
                .enter_type(node(x), Type::integer(1.into()), &mut map)
                .unwrap();
        }
        let state = state(&core);
        let mut patterns = Patterns::new(&nodes);
        let mut binder = Binder {
            patterns: &mut patterns,
        };
        let input = if id == "anonymous" {
            Type::atom("impossible")
        } else {
            Type::any()
        };
        let actual = match binder.bind(
            &[PatternRef::Core(node(pattern))],
            &[input],
            &map,
            &state,
            Direction::Forward,
        ) {
            Ok(bound) => {
                json!({"id":id,"status":"ok","type":text(bound.types[0]),"binding":text(nodes.lookup(node(x), &bound.map).unwrap())})
            }
            Err(BindError::Failure(failure)) => {
                json!({"id":id,"status":"failure","kind":format!("{:?}",failure.kind).to_lowercase(),"type":text(failure.actual)})
            }
            Err(error) => panic!("{id}: {error:?}"),
        };
        assert_eq!(actual, expected(id), "{id}");
    }
}
#[test]
fn native_record_guards_match_captured_otp29_types_and_source_refinement() {
    for (id, input, eval) in [
        ("is_record_positive", Type::any(), GuardEval::Positive),
        (
            "is_record_mismatch",
            Type::atom("other"),
            GuardEval::Unknown,
        ),
        (
            "is_record_negative",
            record(Type::builtin(BuiltinType::Boolean).unwrap()),
            GuardEval::Negative,
        ),
        ("field_positive", Type::any(), GuardEval::Positive),
        (
            "field_false",
            record(Type::atom("false")),
            GuardEval::Positive,
        ),
        ("other_primop", Type::none(), GuardEval::Positive),
    ] {
        let mut tree = Tree::new();
        let x = tree.variable();
        let root = if id.starts_with("is_record") {
            tree.call(x)
        } else if id == "other_primop" {
            let name = tree.atom("is_native_record");
            tree.node(CoreExpr::Primop {
                name,
                args: vec![x],
            })
        } else {
            tree.field(x)
        };
        let core = tree.finish(root);
        let labels = Labels::build(&core, CoreLabel(0)).unwrap();
        let nodes = crate::dataflow_nodes::DataflowNodes::build(&core, &labels).unwrap();
        let folded = FoldedCore::build(&core);
        let folded_labels = Labels::build(&folded.program, CoreLabel(0)).unwrap();
        let folded_nodes =
            crate::dataflow_nodes::DataflowNodes::build(&folded.program, &folded_labels).unwrap();
        let patterns = Patterns::new(&folded_nodes);
        let producer = GuardProducer {
            original: &nodes,
            folded: &folded,
            patterns: &patterns,
        };
        let literals = GuardLiterals::new(&folded).unwrap();
        let registry = crate::bifs::registry::Registry::standard().unwrap();
        let guard = GuardControl {
            nodes: &nodes,
            producer: &producer,
            literals: &literals,
            registry: &registry,
        };
        let mut map = DataflowMap::default();
        nodes.enter_type(node(x), input, &mut map).unwrap();
        let actual = match guard.evaluate(
            node(root),
            &map,
            &GuardEnvironment::new(),
            eval,
            &state(&core),
        ) {
            Ok(bound) => {
                json!({"id":id,"status":"ok","type":text(bound.value),"binding":text(nodes.lookup(node(x), &bound.map).unwrap())})
            }
            Err(GuardError::Failure {
                fatal: false,
                reason: None,
            }) => json!({"id":id,"status":"failure"}),
            Err(error) => panic!("{id}: {error:?}"),
        };
        assert_eq!(actual, expected(id), "{id}");
    }
}
