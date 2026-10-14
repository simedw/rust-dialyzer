//! Source cerl:fold_literal view: only tuple and cons edges are traversed.
use crate::{
    core::{CoreExpr, CoreNode, CoreProgram, ErlangTerm, NodeId, TermId},
    program::NodeRef,
};
pub struct FoldedCore {
    pub program: CoreProgram,
    roots: Vec<Vec<NodeId>>,
}
impl FoldedCore {
    pub fn build(source: &CoreProgram) -> Self {
        // Modules fold independently, on all cores.
        let folded = crate::ordered_parallel::setup_map(&source.modules, |module| {
            let mut module = module.clone();
            let roots = fold_module(&mut module);
            let mut kinds = std::collections::BTreeMap::<&'static str, usize>::new();
            for node in &module.nodes {
                *kinds.entry(node.expr.kind()).or_default() += 1;
            }
            (module, roots, kinds)
        });
        let mut program = source.clone();
        program.modules = Vec::with_capacity(folded.len());
        program.kinds.clear();
        let mut roots = Vec::with_capacity(folded.len());
        for (module, folded, kinds) in folded {
            for (kind, count) in kinds {
                *program.kinds.entry(kind.into()).or_default() += count;
            }
            program.modules.push(module);
            roots.push(folded);
        }
        program.node_count = program.modules.iter().map(|m| m.nodes.len()).sum();
        program.term_count = program.modules.iter().map(|m| m.terms.len()).sum();
        Self { program, roots }
    }
    pub fn root(&self, original: NodeRef) -> Result<NodeRef, String> {
        let node = self
            .roots
            .get(original.module.0)
            .and_then(|m| m.get(original.node))
            .ok_or("fold source node outside arena")?;
        Ok(NodeRef {
            module: original.module,
            node: node.0,
        })
    }
    pub fn literal(&self, original: NodeRef) -> Result<Option<TermId>, String> {
        let folded = self.root(original)?;
        Ok(
            match self.program.modules[folded.module.0].nodes[folded.node].expr {
                CoreExpr::Literal(id) => Some(id),
                _ => None,
            },
        )
    }
}

/// Folds literal tuples and conses of `module` into new literal nodes; the
/// folded node of each original node, by index.
fn fold_module(module: &mut crate::core::CoreModule) -> Vec<NodeId> {
    let count = module.nodes.len();
    let mut folded: Vec<NodeId> = Vec::with_capacity(count);
    for index in 0..count {
        let original = &module.nodes[index];
        let expr = match &original.expr {
            CoreExpr::Tuple(children) => {
                let children: Vec<_> = children.iter().map(|id| folded[id.0]).collect();
                let literal: Option<Vec<_>> = children
                    .iter()
                    .map(|id| match module.nodes[id.0].expr {
                        CoreExpr::Literal(term) => Some(term),
                        _ => None,
                    })
                    .collect();
                match literal {
                    Some(items) => {
                        let id = TermId(module.terms.len());
                        module.terms.push(ErlangTerm::Tuple(items));
                        CoreExpr::Literal(id)
                    }
                    None => CoreExpr::Tuple(children),
                }
            }
            CoreExpr::Cons { head, tail } => {
                let head = folded[head.0];
                let tail = folded[tail.0];
                match (&module.nodes[head.0].expr, &module.nodes[tail.0].expr) {
                    (CoreExpr::Literal(h), CoreExpr::Literal(t)) => {
                        let id = TermId(module.terms.len());
                        let mut items = vec![*h];
                        let tail = match &module.terms[t.0] {
                            ErlangTerm::List { items: rest, tail } => {
                                items.extend(rest.iter().copied());
                                *tail
                            }
                            _ => *t,
                        };
                        module.terms.push(ErlangTerm::List { items, tail });
                        CoreExpr::Literal(id)
                    }
                    _ => CoreExpr::Cons { head, tail },
                }
            }
            _ => {
                folded.push(NodeId(index));
                continue;
            }
        };
        let annotations = module.nodes[index].annotations;
        let id = NodeId(module.nodes.len());
        module.nodes.push(CoreNode {
            id,
            annotations,
            expr,
        });
        folded.push(id);
    }
    folded
}
