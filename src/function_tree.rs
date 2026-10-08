use crate::{
    call_transfer::Signature,
    core::{CoreExpr, ErlangTerm},
    dataflow_nodes::DataflowNodes,
    labels::CoreLabel,
    program::{FunctionName, ModuleId, NodeRef},
};
use crate::{
    function_state::{FunctionDefinition, FunctionState},
    worklist::FunctionKey,
};
use std::collections::{BTreeMap, BTreeSet};
/// Callgraph facts are separately phase-owned; no native target result is inferred here.
pub struct GraphInputs {
    pub escaping: BTreeSet<CoreLabel>,
    pub raw_signatures: BTreeMap<CoreLabel, Signature>,
    pub ordered_local_callers: BTreeMap<CoreLabel, Vec<CoreLabel>>,
}
impl GraphInputs {
    pub fn empty() -> Self {
        Self {
            escaping: BTreeSet::new(),
            raw_signatures: BTreeMap::new(),
            ordered_local_callers: BTreeMap::new(),
        }
    }
}
pub fn build_function_state(
    nodes: &DataflowNodes<'_>,
    module: ModuleId,
    graph: &GraphInputs,
) -> Result<FunctionState, String> {
    let m = nodes.module(module)?;
    let root = NodeRef {
        module,
        node: m.core.0,
    };
    let CoreExpr::Module { definitions, .. } = nodes.expression(root)? else {
        return Err("function state requires Core module".into());
    };
    let mut names = BTreeMap::new();
    for (var, fun) in definitions {
        let CoreExpr::Var(term) = nodes.expression(NodeRef {
            module,
            node: var.0,
        })?
        else {
            return Err("module function name is not variable".into());
        };
        let ErlangTerm::Tuple(pair) = &m.terms[term.0] else {
            return Err("module function variable name is not tuple".into());
        };
        if pair.len() != 2 {
            return Err("module function variable name arity".into());
        }
        let ErlangTerm::Atom(name) = &m.terms[pair[0].0] else {
            return Err("module function name is not atom".into());
        };
        let ErlangTerm::Integer(arity) = &m.terms[pair[1].0] else {
            return Err("module function arity is not integer".into());
        };
        let arity = arity
            .parse::<usize>()
            .map_err(|_| "module arity outside platform range")?;
        names.insert(
            nodes.label(NodeRef {
                module,
                node: fun.0,
            })?,
            FunctionName {
                module: m.name.clone(),
                name: name.clone(),
                arity,
            },
        );
    }
    let mut pending = vec![(root, false)];
    let mut found = Vec::new();
    while let Some((node, finish)) = pending.pop() {
        if !finish {
            pending.push((node, true));
            pending.extend(
                nodes
                    .native_subtrees(node)?
                    .into_iter()
                    .rev()
                    .map(|n| (n, false)),
            );
        } else if let CoreExpr::Fun { params, .. } = nodes.expression(node)? {
            let label = nodes.label(node)?;
            found.push(FunctionDefinition {
                key: FunctionKey::Label(label),
                node: Some(node),
                arity: params.len(),
                escaping: graph.escaping.contains(&label),
                name: names.get(&label).cloned(),
                raw_signature: graph.raw_signatures.get(&label).cloned(),
                callers: graph
                    .ordered_local_callers
                    .get(&label)
                    .into_iter()
                    .flatten()
                    .copied()
                    .map(FunctionKey::Label)
                    .collect(),
            });
        }
    }
    found.push(FunctionDefinition {
        key: FunctionKey::Top,
        node: None,
        arity: 0,
        escaping: false,
        name: None,
        raw_signature: None,
        callers: vec![],
    });
    Ok(FunctionState::new(found))
}
