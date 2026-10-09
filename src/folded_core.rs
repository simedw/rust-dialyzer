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
        let mut program = source.clone();
        let mut roots = Vec::with_capacity(source.modules.len());
        for module in &mut program.modules {
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
            // Folding appends to copies of full arenas, doubling them.
            module.terms = crate::core::exact_capacity(std::mem::take(&mut module.terms));
            module.nodes = crate::core::exact_capacity(std::mem::take(&mut module.nodes));
            roots.push(folded);
        }
        program.node_count = program.modules.iter().map(|m| m.nodes.len()).sum();
        program.term_count = program.modules.iter().map(|m| m.terms.len()).sum();
        program.kinds.clear();
        for module in &program.modules {
            for node in &module.nodes {
                *program.kinds.entry(node.expr.kind().into()).or_default() += 1;
            }
        }
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
