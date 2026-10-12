use crate::{
    bifs::registry::Mfa,
    binder::{Binder, BinderState, OpaqueMatch, WarningFunction, WarningInfo, WarningLocation},
    core::{CoreExpr, ErlangTerm, NodeId},
    dependencies::OriginalPltMembership,
    lattice::NativeValue,
    native_error::NativeTransferError,
    native_runtime_hash::NativeHashKey,
    patterns::PatternRef,
    program::{FunctionName, NodeRef, Program},
    source_callgraph::{CallGraphError, GraphVertex, SourceCallGraph},
    source_digraph::RuntimeGraphKey,
    warning_payload::NativeWarning,
    warning_policy::Policy,
    warnings::Tag,
};
use num_traits::ToPrimitive;
use std::collections::{BTreeMap, BTreeSet};

type Result<T> = std::result::Result<T, CallGraphError>;
pub type Call = (GraphVertex, GraphVertex);
#[derive(Clone, Debug)]
pub struct ExternalCall {
    pub target: Mfa,
    pub origin: WarningInfo,
}
pub struct CleanupOutcome {
    /// Incoming module dependencies computed before removal of external vertices.
    pub module_dependencies: BTreeMap<String, BTreeSet<String>>,
    pub missing_calls: Vec<NativeWarning>,
    pub external_calls: Vec<ExternalCall>,
}
pub struct CleanupContext<'a, 'b, 'c, 'd> {
    pub program: &'a Program,
    pub binder: &'a Binder<'b, 'c, 'd>,
    pub module_states: &'a [BinderState<OpaqueMatch>],
    pub policy: &'a Policy,
    pub original_plt: &'a OriginalPltMembership,
}
fn bind_error(error: crate::binder::BindError) -> CallGraphError {
    match error {
        crate::binder::BindError::Native(e) => CallGraphError::Native(e.into()),
        crate::binder::BindError::Boundary(s) => CallGraphError::Boundary(s),
        e => CallGraphError::Boundary(format!("cleanup location helper: {e:?}")),
    }
}
fn named(v: &GraphVertex) -> Result<Mfa> {
    match v {
        GraphVertex::Named {
            module,
            function,
            arity,
        } => Ok(Mfa {
            module: module.clone().into(),
            function: function.clone().into(),
            arity: arity
                .to_usize()
                .ok_or(CallGraphError::Native(NativeTransferError::FunctionClause))?,
        }),
        other => Err(CallGraphError::Boundary(format!(
            "expected a named call graph vertex, found {other:?}"
        ))),
    }
}
fn function(mfa: &Mfa) -> FunctionName {
    FunctionName {
        module: mfa.module.clone().to_string(),
        name: mfa.function.clone().to_string(),
        arity: mfa.arity,
    }
}
fn mfa_value(mfa: &Mfa) -> NativeValue {
    NativeValue::Tuple(vec![
        NativeValue::Atom(mfa.module.clone().to_string()),
        NativeValue::Atom(mfa.function.clone().to_string()),
        NativeValue::Integer(mfa.arity.into()),
    ])
}
/// Source version2 sets enumeration of intermodule named call edges.
pub fn non_local_calls(graph: &SourceCallGraph) -> Result<Vec<Call>> {
    let g = graph.source_graph();
    // Collected into a map by key and then ordered by key, so the edges are
    // read in no particular order (runtime order copies every edge key).
    g.layout().map_err(CallGraphError::RuntimeOrdering)?;
    let mut by_key = BTreeMap::new();
    for edge in g.unordered_edges() {
        if let (GraphVertex::Named { module: a, .. }, GraphVertex::Named { module: b, .. }) = edge
            && a != b
        {
            let key = NativeHashKey::Tuple(vec![
                edge.0
                    .runtime_key()
                    .map_err(CallGraphError::RuntimeOrdering)?,
                edge.1
                    .runtime_key()
                    .map_err(CallGraphError::RuntimeOrdering)?,
            ]);
            by_key.insert(key, edge.clone());
        }
    }
    if crate::lattice::verify_fast_paths() {
        let mut ordered = BTreeMap::new();
        for id in g.ordered_edges().map_err(CallGraphError::RuntimeOrdering)? {
            let edge = g.edge(&id).ok_or("canonical graph edge missing")?;
            if let (GraphVertex::Named { module: a, .. }, GraphVertex::Named { module: b, .. }) =
                edge
                && a != b
            {
                let key = NativeHashKey::Tuple(vec![
                    edge.0.runtime_key().map_err(CallGraphError::RuntimeOrdering)?,
                    edge.1.runtime_key().map_err(CallGraphError::RuntimeOrdering)?,
                ]);
                ordered.insert(key, edge.clone());
            }
        }
        assert!(ordered == by_key, "non-local call edges");
    }
    crate::native_map_order::ordered_keys(
        g.layout().map_err(CallGraphError::RuntimeOrdering)?,
        by_key.keys().cloned(),
    )
    .map_err(CallGraphError::RuntimeOrdering)?
    .into_iter()
    .map(|k| {
        by_key
            .remove(&k)
            .ok_or_else(|| "set key absent from canonical edge projection".into())
    })
    .collect()
}
pub fn module_dependencies(graph: &SourceCallGraph) -> Result<BTreeMap<String, BTreeSet<String>>> {
    let g = graph.source_graph();
    // Maps of sets: vertices and edges are read in no particular order.
    g.layout().map_err(CallGraphError::RuntimeOrdering)?;
    let mut result = BTreeMap::new();
    for v in g.unordered_vertices() {
        if let GraphVertex::Named { module, .. } = v {
            result.entry(module.to_string()).or_insert_with(BTreeSet::new);
        }
    }
    for edge in g.unordered_edges() {
        if let (GraphVertex::Named { module: from, .. }, GraphVertex::Named { module: to, .. }) =
            edge
            && from != to
        {
            result
                .get_mut(to.as_str())
                .ok_or("module vertex missing")?
                .insert(from.to_string());
        }
    }
    if crate::lattice::verify_fast_paths() {
        let mut ordered = BTreeMap::new();
        for v in g.ordered_vertices().map_err(CallGraphError::RuntimeOrdering)? {
            if let GraphVertex::Named { module, .. } = v {
                ordered.entry(module.to_string()).or_insert_with(BTreeSet::new);
            }
        }
        for id in g.ordered_edges().map_err(CallGraphError::RuntimeOrdering)? {
            if let Some((
                GraphVertex::Named { module: from, .. },
                GraphVertex::Named { module: to, .. },
            )) = g.edge(&id)
                && from != to
                && let Some(callers) = ordered.get_mut(to.as_str())
            {
                callers.insert(from.to_string());
            }
        }
        assert_eq!(ordered, result, "module call dependencies");
    }
    Ok(result)
}
impl CleanupContext<'_, '_, '_, '_> {
    fn atom(&self, node: NodeRef) -> Result<Option<&str>> {
        let module = self.binder.patterns.nodes.module(node.module)?;
        Ok(match self.binder.patterns.nodes.expression(node)? {
            CoreExpr::Literal(id) => match &module.terms[id.0] {
                ErlangTerm::Atom(a) => Some(a.as_str()),
                _ => None,
            },
            _ => None,
        })
    }
    fn matches_call(&self, node: NodeRef, target: &Mfa) -> Result<bool> {
        let CoreExpr::Call { module, name, args } = self.binder.patterns.nodes.expression(node)?
        else {
            return Ok(false);
        };
        let site = |id: NodeId| NodeRef {
            module: node.module,
            node: id.0,
        };
        let (Some(m), Some(f)) = (self.atom(site(*module))?, self.atom(site(*name))?) else {
            return Ok(false);
        };
        if *m == *target.module && *f == *target.function && args.len() == target.arity {
            return Ok(true);
        }
        if m != "erlang" || f != "make_fun" || args.len() != 3 {
            return Ok(false);
        }
        let arity = match self.binder.patterns.nodes.expression(site(args[2]))? {
            CoreExpr::Literal(id) => {
                match &self.binder.patterns.nodes.module(node.module)?.terms[id.0] {
                    ErlangTerm::Integer(value) => value.parse::<num_bigint::BigInt>().ok(),
                    _ => None,
                }
            }
            _ => None,
        };
        Ok(self.atom(site(args[0]))? == Some(target.module.as_str())
            && self.atom(site(args[1]))? == Some(target.function.as_str())
            && arity == Some(target.arity.into()))
    }
    pub fn find_call_origin(&self, from: &Mfa, to: &Mfa) -> Result<WarningInfo> {
        let id = self
            .program
            .named
            .get(&function(from))
            .ok_or("caller absent from compiler code server")?;
        let fun = &self.program.functions[id.0];
        let root = NodeRef {
            module: fun.module,
            node: fun.node.0,
        };
        let mut stack = vec![(root, false)];
        let mut latest = None;
        while let Some((node, visited)) = stack.pop() {
            if visited {
                if self.matches_call(node, to)? {
                    // Source computes each matching origin during fold, even if a later match replaces it.
                    latest = Some(self.call_origin(node, from)?);
                }
            } else {
                stack.push((node, true));
                stack.extend(
                    self.binder
                        .patterns
                        .nodes
                        .native_subtrees(node)?
                        .into_iter()
                        .rev()
                        .map(|child| (child, false)),
                );
            }
        }
        latest.ok_or(CallGraphError::Native(NativeTransferError::Badarg))
    }
    fn call_origin(&self, node: NodeRef, caller: &Mfa) -> Result<WarningInfo> {
        let module = self.binder.patterns.nodes.module(node.module)?;
        let annotation = module.nodes[node.node].annotations;
        let mut cursor = annotation;
        let mut has_file = false;
        while let ErlangTerm::List { items, tail } = &module.terms[cursor.0] {
            for id in items {
                if let ErlangTerm::Tuple(xs) = &module.terms[id.0]
                    && xs.len() == 2
                    && matches!(&module.terms[xs[0].0], ErlangTerm::Atom(a) if a == "file")
                {
                    has_file = true;
                    break;
                }
            }
            if has_file {
                break;
            }
            cursor = *tail;
        }
        if !has_file {
            return Err(CallGraphError::Native(NativeTransferError::FunctionClause));
        }
        let state = self
            .module_states
            .get(node.module.0)
            .ok_or("module BinderState unavailable")?;
        let pat = PatternRef::Core(node);
        let mut info = self.binder.warning_info(pat, state).map_err(bind_error)?;
        info.location = self
            .binder
            .warning_location(pat, WarningLocation::Line(0))
            .map_err(bind_error)?;
        info.current_fun = WarningFunction::Named(function(caller));
        Ok(info)
    }
    pub fn unknown_functions(&self, calls: &[ExternalCall]) -> Vec<(String, NativeWarning)> {
        calls
            .iter()
            .filter_map(|call| {
                let WarningFunction::Named(caller) = &call.origin.current_fun else {
                    return None;
                };
                let is_member = self
                    .policy
                    .modules
                    .iter()
                    .find(|m| m.module == caller.module)
                    .is_some_and(|m| m.functions.iter().any(|f| &f.function == caller));
                if is_member {
                    return None;
                }
                Some((
                    caller.module.clone(),
                    NativeWarning::from_value(
                        "warn_unknown",
                        &call.origin,
                        &NativeValue::Tuple(vec![
                            NativeValue::Atom("unknown_function".into()),
                            mfa_value(&call.target),
                        ]),
                    ),
                ))
            })
            .collect()
    }
}
pub fn cleanup_callgraph(
    graph: &mut SourceCallGraph,
    context: &CleanupContext<'_, '_, '_, '_>,
) -> Result<CleanupOutcome> {
    let module_dependencies = module_dependencies(graph)?;
    let exports = Program::compiler_exports(context.binder.patterns.nodes.core())?;
    let external = graph.remove_external()?;
    let known_modules = context.original_plt.modules();
    let mut bad = Vec::new();
    let mut unknown = Vec::new();
    for (from, to) in external {
        let target = named(&to)?;
        if context.original_plt.contains(&target) {
            continue;
        }
        if known_modules.contains(target.module.as_str())
            || context
                .program
                .modules
                .iter()
                .any(|m| *m.name == *target.module)
        {
            bad.push((from, to));
        } else {
            unknown.push((from, target));
        }
    }
    for (from, to) in non_local_calls(graph)? {
        let target = named(&to)?;
        let exported = exports.contains(&function(&target));
        if !exported {
            bad.push((from, to));
        }
    }
    let mut missing_calls = Vec::new();
    for (from, to) in bad {
        let target = named(&to)?;
        if matches!(&from, GraphVertex::Named { .. })
            && target.function == "module_info"
            && target.arity <= 1
        {
            continue;
        }
        let caller = named(&from)?;
        let origin = context.find_call_origin(&caller, &target).map_err(|error| {
            CallGraphError::Boundary(format!(
                "module {} call origin {}/{} -> {}:{}/{}: {error:?}",
                caller.module,
                caller.function,
                caller.arity,
                target.module,
                target.function,
                target.arity
            ))
        })?;
        let message = NativeValue::Tuple(vec![
            NativeValue::Atom("call_to_missing".into()),
            NativeValue::List(vec![
                NativeValue::Atom(target.module.to_string()),
                NativeValue::Atom(target.function.to_string()),
                NativeValue::Integer(target.arity.into()),
            ]),
        ]);
        // Formatting precedes filtering in Source, so suppressed malformed origins still fail.
        if context
            .policy
            .allows(&function(&caller), Tag::WarnCallgraph)
        {
            missing_calls.push(NativeWarning::from_value(
                "warn_callgraph",
                &origin,
                &message,
            ));
        }
    }
    missing_calls.reverse();
    let external_calls = unknown
        .into_iter()
        .map(|(from, target)| {
            let caller = named(&from)?;
            let origin = context.find_call_origin(&caller, &target).map_err(|error| {
                CallGraphError::Boundary(format!(
                    "module {} unknown call origin {}/{} -> {}:{}/{}: {error:?}",
                    caller.module,
                    caller.function,
                    caller.arity,
                    target.module,
                    target.function,
                    target.arity
                ))
            })?;
            Ok(ExternalCall { target, origin })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(CleanupOutcome {
        module_dependencies,
        missing_calls,
        external_calls,
    })
}
