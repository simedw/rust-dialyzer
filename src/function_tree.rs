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
    Ok(build_function_layout(nodes, module, graph)?.instantiate(&graph.raw_signatures))
}

pub(crate) struct FunctionLayout {
    definitions: Vec<FunctionDefinition>,
}
impl FunctionLayout {
    pub(crate) fn instantiate(&self, signatures: &BTreeMap<CoreLabel, Signature>) -> FunctionState {
        FunctionState::new(self.definitions.iter().cloned().map(|mut definition| {
            if let FunctionKey::Label(label) = definition.key {
                definition.raw_signature = signatures.get(&label).cloned();
            }
            definition
        }).collect())
    }
}

pub(crate) fn build_function_layout(
    nodes: &DataflowNodes<'_>,
    module: ModuleId,
    graph: &GraphInputs,
) -> Result<FunctionLayout, String> {
    let m = nodes.module(module)?;
    let root = NodeRef {
        module,
        node: m.core.0,
    };
    let CoreExpr::Module(module_payload) = nodes.expression(root)? else {
        return Err("function state requires Core module".into());
    };
    let crate::core::ModuleExpr { definitions, .. } = module_payload.as_ref();
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
                raw_signature: None,
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
    Ok(FunctionLayout { definitions: found })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lattice::Type;

    #[test]
    fn cached_layout_uses_current_signatures_and_fresh_mutable_state() {
        let label = CoreLabel(17);
        let key = FunctionKey::Label(label);
        let layout = FunctionLayout { definitions: vec![FunctionDefinition {
            key, node: None, arity: 1, escaping: true, name: None,
            raw_signature: None, callers: vec![],
        }] };
        let failed = Signature { arguments: vec![Type::any()], result: Type::none() };
        let returning = Signature { arguments: vec![Type::integer(3.into())], result: Type::atom("ok") };
        let mut first = layout.instantiate(&BTreeMap::from([(label, failed.clone())]));
        first.update_return(key, &[Type::any()], &Type::atom("changed")).unwrap();
        first.set_warning_mode();
        let second = layout.instantiate(&BTreeMap::from([(label, returning.clone())]));
        assert_eq!(second.definitions()[&key].raw_signature.as_ref(), Some(&returning));
        assert_eq!(second.entries()[&key].signature().result, Type::unit());
        assert!(!second.warning_mode());
        let third = layout.instantiate(&BTreeMap::from([(label, failed)]));
        assert_eq!(third.entries()[&key].signature().result, Type::none());
        assert!(third.reachable().is_empty());
        assert!(layout.definitions[0].raw_signature.is_none());
    }
}
