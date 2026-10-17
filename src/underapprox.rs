//! Source-native safe clause underapproximation shared by success and dataflow.
use crate::{
    core::{CoreExpr, CoreProgram, ErlangTerm, NodeId, TermId},
    labels::{CoreLabel, Labels},
    lattice::{BuiltinType, LatticeError, Type, TypePredicate},
    literal_types::LiteralTypes,
    program::{ModuleId, NodeRef},
};
use std::collections::BTreeMap;
#[derive(Debug)]
pub enum UnderapproxError {
    Native(LatticeError),
    NativeCaseClauseAtom(String),
    ResourceLimit,
    Boundary(String),
}
impl From<LatticeError> for UnderapproxError {
    fn from(e: LatticeError) -> Self {
        Self::Native(e)
    }
}
#[derive(Debug)]
enum Failure {
    DontKnow,
    Error(UnderapproxError),
}
impl From<LatticeError> for Failure {
    fn from(e: LatticeError) -> Self {
        Self::Error(e.into())
    }
}
impl From<UnderapproxError> for Failure {
    fn from(e: UnderapproxError) -> Self {
        Self::Error(e)
    }
}
type Result<T> = std::result::Result<T, Failure>;
type Map = BTreeMap<CoreLabel, Type>;
#[derive(Clone, Copy)]
enum Pat {
    Node(NodeId),
    Term(TermId),
}
enum Action {
    Visit(Pat, usize),
    Tuple(usize),
    Product(usize),
    Cons,
    Alias(CoreLabel),
    Map(Vec<Type>),
}
pub struct SafeUnderapprox<'a> {
    core: &'a CoreProgram,
    labels: &'a Labels,
    literals: &'a [LiteralTypes],
}
impl<'a> SafeUnderapprox<'a> {
    pub fn new(core: &'a CoreProgram, labels: &'a Labels, literals: &'a [LiteralTypes]) -> Self {
        Self {
            core,
            labels,
            literals,
        }
    }
    fn label(&self, module: ModuleId, node: NodeId) -> Result<CoreLabel> {
        self.labels
            .label(NodeRef {
                module,
                node: node.0,
            })
            .ok_or_else(|| {
                UnderapproxError::Boundary("underapprox variable label absent".into()).into()
            })
    }
    pub fn evaluate(
        &self,
        module: ModuleId,
        patterns: &[NodeId],
        guard: NodeId,
    ) -> std::result::Result<Type, UnderapproxError> {
        self.evaluate_with_refold_events(module, patterns, guard, |_| Ok(()))
    }
    /// Reports original literal refolding attempts in native source evaluation order.
    /// The observer owns allocation accounting; no synthetic IDs affect this Type result.
    pub fn evaluate_with_refold_events(
        &self,
        module: ModuleId,
        patterns: &[NodeId],
        guard: NodeId,
        mut observe: impl FnMut(NodeRef) -> std::result::Result<(), UnderapproxError>,
    ) -> std::result::Result<Type, UnderapproxError> {
        let mut execute = || -> Result<Type> {
            let m =
                self.core.modules.get(module.0).ok_or_else(|| {
                    UnderapproxError::Boundary("underapprox module absent".into())
                })?;
            let mut map = Map::new();
            let mut todo = patterns.to_vec();
            let mut seen = std::collections::BTreeSet::new();
            while let Some(node) = todo.pop() {
                if !seen.insert(node.0) {
                    continue;
                }
                let expr = &m.nodes[node.0].expr;
                if matches!(expr, CoreExpr::Var(_)) {
                    map.insert(self.label(module, node)?, Type::any());
                }
                todo.extend(expr.native_children());
            }
            let ty = self.guard(module, guard, &mut map)?;
            if ty
                .meet(&Type::atom("true"))?
                .predicate(TypePredicate::None)?
            {
                return Err(Failure::DontKnow);
            }
            if matches!(m.nodes[guard.0].expr, CoreExpr::Var(_)) {
                map.insert(self.label(module, guard)?, Type::atom("true"));
            }
            self.pattern(module, patterns, &mut map, &mut observe)
        };
        match execute() {
            Ok(t) => Ok(t),
            Err(Failure::DontKnow) => Ok(Type::none()),
            Err(Failure::Error(e)) => Err(e),
        }
    }
    fn literal(&self, module: ModuleId, node: NodeId) -> Option<TermId> {
        match self.core.modules[module.0].nodes[node.0].expr {
            CoreExpr::Literal(t) => Some(t),
            _ => None,
        }
    }
    fn atom(&self, module: ModuleId, node: NodeId) -> Option<&str> {
        let t = self.literal(module, node)?;
        if let ErlangTerm::Atom(a) = &self.core.modules[module.0].terms[t.0] {
            Some(a)
        } else {
            None
        }
    }
    fn value(&self, module: ModuleId, id: TermId) -> Result<Type> {
        self.literals[module.0].get(id).cloned().ok_or_else(|| {
            UnderapproxError::Boundary("underapprox literal was not materialized".into()).into()
        })
    }
    fn guard(&self, module: ModuleId, root: NodeId, map: &mut Map) -> Result<Type> {
        enum Step {
            Visit(NodeId),
            Test(NodeId, Type),
            FunctionArity(NodeId, String),
            And,
        }
        let m = &self.core.modules[module.0];
        let mut todo = vec![Step::Visit(root)];
        let mut values = vec![];
        while let Some(step) = todo.pop() {
            match step {
                Step::Visit(node) => match &m.nodes[node.0].expr {
                    CoreExpr::Var(_) => values.push(
                        map.get(&self.label(module, node)?)
                            .cloned()
                            .ok_or(Failure::DontKnow)?,
                    ),
                    CoreExpr::Literal(t) => match &m.terms[t.0] {
                        ErlangTerm::Atom(_) | ErlangTerm::Integer(_) => {
                            values.push(self.value(module, *t)?)
                        }
                        _ => return Err(Failure::DontKnow),
                    },
                    CoreExpr::Call {
                        module: mod_node,
                        name,
                        args,
                    } => {
                        let mod_name = self
                            .atom(module, *mod_node)
                            .ok_or(LatticeError::FunctionClause)?;
                        let name = self
                            .atom(module, *name)
                            .ok_or(LatticeError::FunctionClause)?;
                        if mod_name != "erlang" {
                            return Err(Failure::DontKnow);
                        }
                        if name == "is_function" && args.len() == 2 {
                            let Some(t) = self.literal(module, args[1]) else {
                                return Err(Failure::DontKnow);
                            };
                            let ErlangTerm::Integer(arity) = &m.terms[t.0] else {
                                return Err(Failure::DontKnow);
                            };
                            todo.push(Step::FunctionArity(args[0], arity.clone()));
                            todo.push(Step::Visit(args[0]));
                        } else if let Some(kind) = guard_test(name).filter(|_| args.len() == 1) {
                            todo.push(Step::Test(args[0], Type::builtin(kind)?));
                            todo.push(Step::Visit(args[0]));
                        } else if name == "and" && args.len() == 2 {
                            if !args.iter().all(|n| {
                                matches!(m.nodes[n.0].expr, CoreExpr::Var(_) | CoreExpr::Literal(_))
                            }) {
                                return Err(Failure::DontKnow);
                            }
                            todo.push(Step::And);
                            todo.push(Step::Visit(args[1]));
                            todo.push(Step::Visit(args[0]));
                        } else {
                            return Err(Failure::DontKnow);
                        }
                    }
                    _ => return Err(Failure::DontKnow),
                },
                Step::Test(arg, domain) => {
                    let ty = values.pop().unwrap().meet(&domain)?;
                    if ty.predicate(TypePredicate::None)? {
                        return Err(Failure::DontKnow);
                    }
                    if !matches!(m.nodes[arg.0].expr, CoreExpr::Literal(_)) {
                        map.insert(self.label(module, arg)?, ty);
                    }
                    values.push(Type::atom("true"));
                }
                Step::FunctionArity(arg, arity) => {
                    let arg_type = values.pop().unwrap();
                    let arity = arity
                        .parse::<i64>()
                        .map_err(|_| LatticeError::FunctionClause)?;
                    let ty = arg_type.meet(&Type::function_with_arity(arity, Type::any())?)?;
                    if ty.predicate(TypePredicate::None)? {
                        return Err(Failure::DontKnow);
                    }
                    map.insert(self.label(module, arg)?, ty);
                    values.push(Type::atom("true"));
                }
                Step::And => {
                    let b = values.pop().unwrap();
                    let a = values.pop().unwrap();
                    if a != Type::atom("true") || b != Type::atom("true") {
                        return Err(Failure::DontKnow);
                    }
                    values.push(Type::atom("true"));
                }
            }
        }
        Ok(values.pop().unwrap())
    }
    fn over(&self, module: ModuleId, root: NodeId) -> Result<Type> {
        let m = &self.core.modules[module.0];
        let mut todo = vec![(root, false, 0usize)];
        let mut values = vec![];
        let mut visited = 0usize;
        while let Some((node, done, depth)) = todo.pop() {
            visited += 1;
            if depth > crate::guard_control::MAX_GUARD_DEPTH || visited > 1_000_000 {
                eprintln!("RUST_DIALYZER_GUARD {}", serde_json::json!({"event":"underapprox_limit","module_id":module.0,"node":root.0,"depth":depth,"visited":visited}));
                return Err(UnderapproxError::ResourceLimit.into());
            }
            match &m.nodes[node.0].expr {
                CoreExpr::Literal(t) => values.push(Some(self.value(module, *t)?)),
                CoreExpr::Tuple(xs) if !done => {
                    todo.push((node, true, depth));
                    todo.extend(xs.iter().rev().map(|n| (*n, false, depth + 1)));
                }
                CoreExpr::Tuple(xs) => {
                    let children = values.split_off(values.len() - xs.len());
                    values.push(
                        children
                            .into_iter()
                            .collect::<Option<Vec<_>>>()
                            .map(Type::tuple)
                            .transpose()?,
                    );
                }
                CoreExpr::Cons { head, tail } if !done => {
                    todo.push((node, true, depth));
                    todo.push((*tail, false, depth + 1));
                    todo.push((*head, false, depth + 1));
                }
                CoreExpr::Cons { .. } => {
                    let tail = values.pop().unwrap();
                    let head = values.pop().unwrap();
                    values.push(match (head, tail) {
                        (Some(h), Some(t)) => Some(Type::cons(h, t)?),
                        _ => None,
                    });
                }
                _ => values.push(None),
            }
        }
        Ok(values.pop().unwrap().unwrap_or_else(Type::any))
    }
    fn pattern(
        &self,
        module: ModuleId,
        patterns: &[NodeId],
        map: &mut Map,
        observe: &mut impl FnMut(NodeRef) -> std::result::Result<(), UnderapproxError>,
    ) -> Result<Type> {
        let m = &self.core.modules[module.0];
        let mut todo = vec![Action::Product(patterns.len())];
        todo.extend(
            patterns
                .iter()
                .rev()
                .map(|n| Action::Visit(Pat::Node(*n), 0)),
        );
        let mut values = vec![];
        let mut visited = 0usize;
        while let Some(action) = todo.pop() {
            visited += 1;
            if visited > 1_000_000 || matches!(action, Action::Visit(_, depth) if depth > crate::guard_control::MAX_GUARD_DEPTH) {
                eprintln!("RUST_DIALYZER_GUARD {}", serde_json::json!({"event":"underapprox_pattern_limit","module_id":module.0,"visited":visited}));
                return Err(UnderapproxError::ResourceLimit.into());
            }
            match action {
                Action::Visit(Pat::Node(node), depth) => match &m.nodes[node.0].expr {
                    CoreExpr::Var(_) => values.push(
                        map.get(&self.label(module, node)?)
                            .cloned()
                            .ok_or(Failure::DontKnow)?,
                    ),
                    CoreExpr::Literal(t) => {
                        observe(NodeRef {
                            module,
                            node: node.0,
                        })?;
                        todo.push(Action::Visit(Pat::Term(*t), depth + 1));
                    }
                    CoreExpr::Tuple(xs) | CoreExpr::Values(xs) => {
                        todo.push(if matches!(m.nodes[node.0].expr, CoreExpr::Tuple(_)) {
                            Action::Tuple(xs.len())
                        } else {
                            Action::Product(xs.len())
                        });
                        todo.extend(
                            xs.iter()
                                .rev()
                                .map(|n| Action::Visit(Pat::Node(*n), depth + 1)),
                        );
                    }
                    CoreExpr::Cons { head, tail } => {
                        todo.push(Action::Cons);
                        todo.push(Action::Visit(Pat::Node(*tail), depth + 1));
                        todo.push(Action::Visit(Pat::Node(*head), depth + 1));
                    }
                    CoreExpr::Alias { variable, pattern } => {
                        let label = self.label(module, *variable)?;
                        values.push(map.get(&label).cloned().ok_or(Failure::DontKnow)?);
                        todo.push(Action::Alias(label));
                        todo.push(Action::Visit(Pat::Node(*pattern), depth + 1));
                    }
                    CoreExpr::Map { base, pairs, .. } => {
                        let Some(base) = self.literal(module, *base) else {
                            return Err(LatticeError::FunctionClause.into());
                        };
                        if !matches!(&m.terms[base.0],ErlangTerm::Map(ps)if ps.is_empty()) {
                            return Err(LatticeError::BadMatchBoolean(false).into());
                        }
                        let mut keys = vec![];
                        let mut vals = vec![];
                        for pair in pairs {
                            let CoreExpr::MapPair {
                                operation,
                                key,
                                value,
                            } = &m.nodes[pair.0].expr
                            else {
                                return Err(LatticeError::FunctionClause.into());
                            };
                            if self.atom(module, *operation) != Some("exact") {
                                return Err(LatticeError::BadMatchBoolean(false).into());
                            }
                            keys.push(self.over(module, *key)?);
                            vals.push(*value);
                        }
                        todo.push(Action::Map(keys));
                        todo.extend(
                            vals.iter()
                                .rev()
                                .map(|n| Action::Visit(Pat::Node(*n), depth + 1)),
                        );
                    }
                    CoreExpr::Binary(_) => return Err(Failure::DontKnow),
                    #[cfg(otp_domain = "29")]
                    CoreExpr::Record { .. } => return Err(Failure::DontKnow),
                    _ => {
                        return Err(UnderapproxError::NativeCaseClauseAtom(
                            m.nodes[node.0].expr.kind().into(),
                        )
                        .into());
                    }
                },
                Action::Visit(Pat::Term(term), depth) => match &m.terms[term.0] {
                    ErlangTerm::Integer(_) | ErlangTerm::Atom(_) | ErlangTerm::Nil => {
                        values.push(self.value(module, term)?)
                    }
                    ErlangTerm::Tuple(xs) => {
                        todo.push(Action::Tuple(xs.len()));
                        todo.extend(
                            xs.iter()
                                .rev()
                                .map(|t| Action::Visit(Pat::Term(*t), depth + 1)),
                        );
                    }
                    ErlangTerm::Map(_) => {
                        let pairs = m
                            .map_iterator_pairs(term)
                            .map_err(UnderapproxError::Boundary)?
                            .collect::<Vec<_>>();
                        let keys = pairs
                            .iter()
                            .map(|p| self.value(module, p.0))
                            .collect::<Result<Vec<_>>>()?;
                        todo.push(Action::Map(keys));
                        todo.extend(
                            pairs
                                .iter()
                                .rev()
                                .map(|p| Action::Visit(Pat::Term(p.1), depth + 1)),
                        );
                    }
                    ErlangTerm::List { items, tail } if items.is_empty() => {
                        todo.push(Action::Visit(Pat::Term(*tail), depth + 1))
                    }
                    _ => return Err(Failure::DontKnow),
                },
                Action::Tuple(n) => {
                    let es = values.split_off(values.len() - n);
                    values.push(Type::tuple(es)?);
                }
                Action::Product(n) => {
                    let es = values.split_off(values.len() - n);
                    values.push(Type::product(es));
                }
                Action::Cons => {
                    let tail = values.pop().unwrap();
                    let head = values.pop().unwrap();
                    if !tail.predicate(TypePredicate::Any)? {
                        return Err(Failure::DontKnow);
                    }
                    values.push(Type::proper_list(head, true));
                }
                Action::Alias(label) => {
                    let pat = values.pop().unwrap();
                    let var = values.pop().unwrap();
                    let ty = var.meet(&pat)?;
                    if ty.predicate(TypePredicate::None)? {
                        return Err(Failure::DontKnow);
                    }
                    map.insert(label, ty.clone());
                    values.push(ty);
                }
                Action::Map(keys) => {
                    let vals = values.split_off(values.len() - keys.len());
                    if !keys
                        .iter()
                        .map(|k| k.predicate(TypePredicate::Singleton))
                        .collect::<std::result::Result<Vec<_>, _>>()?
                        .into_iter()
                        .all(|b| b)
                    {
                        return Err(Failure::DontKnow);
                    }
                    let mut pairs = keys.into_iter().zip(vals).collect::<Vec<_>>();
                    pairs.sort_by_key(|(k, v)| (k.to_native_value(), v.to_native_value()));
                    let mut pairs = pairs.into_iter().peekable();
                    let mut ty = Type::map_any();
                    while let Some((k, mut v)) = pairs.next() {
                        if pairs.peek().is_some_and(|(next, _)| next == &k) {
                            let (_, next) = pairs.next().unwrap();
                            v = v.meet(&next)?;
                            if v.predicate(TypePredicate::None)? {
                                return Err(Failure::DontKnow);
                            }
                        }
                        ty = ty.map_put(k, v)?;
                    }
                    values.push(ty);
                }
            }
        }
        Ok(values.pop().unwrap())
    }
}
fn guard_test(name: &str) -> Option<BuiltinType> {
    Some(match name {
        "is_atom" => BuiltinType::Atom,
        "is_boolean" => BuiltinType::Boolean,
        "is_binary" => BuiltinType::Binary,
        "is_bitstring" => BuiltinType::Bitstring,
        "is_float" => BuiltinType::Float,
        "is_function" => BuiltinType::Function,
        "is_integer" => BuiltinType::Integer,
        "is_list" => BuiltinType::List,
        "is_map" => BuiltinType::Map,
        "is_number" => BuiltinType::Number,
        "is_pid" => BuiltinType::Pid,
        "is_port" => BuiltinType::Port,
        "is_reference" => BuiltinType::Reference,
        "is_tuple" => BuiltinType::Tuple,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{CompilerProfile, CoreModule, CoreNode, ErlangTerms, TermMapOrder};
    #[test]
    fn deep_patterns_fail_explicitly_before_recursive_type_ownership_and_false_guard_shortcuts() {
        let terms = vec![
            ErlangTerm::Nil,
            ErlangTerm::Atom("x".into()),
            ErlangTerm::Atom("true".into()),
            ErlangTerm::Atom("false".into()),
        ];
        let mut nodes = vec![CoreNode {
            id: NodeId(0),
            annotations: TermId(0),
            expr: CoreExpr::Var(TermId(1)),
        }];
        for n in 0..50_000 {
            nodes.push(CoreNode {
                id: NodeId(nodes.len()),
                annotations: TermId(0),
                expr: CoreExpr::Tuple(vec![NodeId(n)]),
            });
        }
        let pattern = NodeId(nodes.len() - 1);
        let true_guard = NodeId(nodes.len());
        nodes.push(CoreNode {
            id: NodeId(nodes.len()),
            annotations: TermId(0),
            expr: CoreExpr::Literal(TermId(2)),
        });
        let false_guard = NodeId(nodes.len());
        nodes.push(CoreNode {
            id: NodeId(nodes.len()),
            annotations: TermId(0),
            expr: CoreExpr::Literal(TermId(3)),
        });
        let literals = LiteralTypes::build_roots(
            &ErlangTerms {
                terms: terms.clone().into(),
                root: TermId(0),
            },
            &[TermId(0), TermId(1), TermId(2), TermId(3)],
        )
        .unwrap();
        let program = CoreProgram {
            version: crate::core::CoreVersion::V5,
            atom_layout: None,
            contract_closure_profile: None,
            profile: CompilerProfile {
                otp: "28".into(),
                erts: "16.2".into(),
                elixir: "1.19.4".into(),
                dialyzer: "5.4".into(),
                core_options: vec![],
                term_map_order: TermMapOrder::NativeToListWithIterator,
            },
            modules: vec![CoreModule {
                name: "deep".into(),
                source_forms: TermId(0),
                core: pattern,
                terms: terms.into(),
                nodes: nodes.into(),
                map_iterator_orders: BTreeMap::new(),
                contract_variable_maps: BTreeMap::new(),
            }],
            node_count: 50_003,
            term_count: 4,
            kinds: BTreeMap::new(),
        };
        let labels = Labels::build(&program, CoreLabel(0)).unwrap();
        let literal_modules = [literals];
        let under = SafeUnderapprox::new(&program, &labels, &literal_modules);
        assert_eq!(
            under
                .evaluate(ModuleId(0), &[pattern], false_guard)
                .unwrap(),
            Type::none()
        );
        assert!(matches!(
            under.evaluate(ModuleId(0), &[pattern], true_guard),
            Err(UnderapproxError::ResourceLimit)
        ));
    }
}
