use crate::folded_core::FoldedCore;
use crate::{
    core::{CoreExpr, ErlangTerm, ErlangTerms, TermId},
    program::NodeRef,
    term_order::TermOrderArena,
};
use std::cmp::Ordering;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Comparison {
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
}
pub struct GuardLiterals<'a> {
    folded: &'a FoldedCore,
    orders: Vec<TermOrderArena>,
    raw_literals: Vec<(std::sync::Arc<ErlangTerms>, Vec<Option<TermId>>)>,
}
impl<'a> GuardLiterals<'a> {
    pub fn new(folded: &'a FoldedCore) -> Result<Self, String> {
        let orders = crate::ordered_parallel::setup_map(&folded.program.modules, |module| {
            TermOrderArena::new(&ErlangTerms {
                root: module.source_forms,
                terms: module.terms.clone(),
            })
        })
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?;
        let raw_literals = crate::ordered_parallel::setup_map(&folded.program.modules, |module| {
            let mut terms = module.terms.clone();
            let tag = TermId(terms.len());
            terms.push(ErlangTerm::Atom("c_literal".into()));
            let roots = module
                .nodes
                .iter()
                .map(|node| {
                    if let CoreExpr::Literal(value) = node.expr {
                        let id = TermId(terms.len());
                        terms.push(ErlangTerm::Tuple(vec![tag, node.annotations, value]));
                        Some(id)
                    } else {
                        None
                    }
                })
                .collect();
            (
                std::sync::Arc::new(ErlangTerms {
                    root: module.source_forms,
                    terms,
                }),
                roots,
            )
        });
        Ok(Self {
            folded,
            orders,
            raw_literals,
        })
    }
    pub fn value(&self, node: NodeRef) -> Result<Option<&ErlangTerm>, String> {
        Ok(self
            .folded
            .literal(node)?
            .map(|id| &self.folded.program.modules[node.module.0].terms[id.0]))
    }
    pub fn raw_operand(
        &self,
        node: NodeRef,
    ) -> Result<crate::native_type_operand::NativeTypeOperand, String> {
        let folded = self.folded.root(node)?;
        let (arena, roots) = &self.raw_literals[folded.module.0];
        let id = roots[folded.node].ok_or("expected folded Core literal operand")?;
        Ok(crate::native_type_operand::NativeTypeOperand::Raw(
            crate::native_form_environment::RawFormTerm::new(arena.clone(), id)?,
        ))
    }
    fn ids(&self, a: NodeRef, b: NodeRef) -> Result<Option<(TermId, TermId)>, String> {
        Ok(match (self.folded.literal(a)?, self.folded.literal(b)?) {
            (Some(a), Some(b)) => Some((a, b)),
            _ => None,
        })
    }
    /// Both native == and =:= guard literal paths explicitly use =:=.
    pub fn strict_equal(&self, a: NodeRef, b: NodeRef) -> Result<Option<bool>, String> {
        let Some((left, right)) = self.ids(a, b)? else {
            return Ok(None);
        };
        Ok(Some(
            self.orders[a.module.0].compare_map_keys(left, &self.orders[b.module.0], right)?
                == Ordering::Equal,
        ))
    }
    pub fn compare(
        &self,
        a: NodeRef,
        b: NodeRef,
        operator: Comparison,
    ) -> Result<Option<bool>, String> {
        let Some((left, right)) = self.ids(a, b)? else {
            return Ok(None);
        };
        let order = self.orders[a.module.0].compare(left, &self.orders[b.module.0], right)?;
        Ok(Some(match operator {
            Comparison::Less => order == Ordering::Less,
            Comparison::LessEqual => order != Ordering::Greater,
            Comparison::Greater => order == Ordering::Greater,
            Comparison::GreaterEqual => order != Ordering::Less,
        }))
    }
}
