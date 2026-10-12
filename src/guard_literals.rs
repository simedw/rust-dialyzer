use crate::folded_core::FoldedCore;
use crate::{
    core::{CoreExpr, ErlangTerm, ErlangTerms, TermId},
    program::NodeRef,
    term_order::TermOrderArena,
};
use std::{cmp::Ordering, sync::OnceLock};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Comparison {
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
}
type RawLiterals = (std::sync::Arc<ErlangTerms>, Vec<Option<TermId>>);
/// Per-module term orders and raw literal arenas are built on first use:
/// few modules compare guard literals, and building them for every module
/// copies every term arena.
pub struct GuardLiterals<'a> {
    folded: &'a FoldedCore,
    orders: Vec<OnceLock<Result<TermOrderArena, String>>>,
    raw_literals: Vec<OnceLock<RawLiterals>>,
}
impl<'a> GuardLiterals<'a> {
    pub fn new(folded: &'a FoldedCore) -> Result<Self, String> {
        let modules = folded.program.modules.len();
        Ok(Self {
            folded,
            orders: (0..modules).map(|_| OnceLock::new()).collect(),
            raw_literals: (0..modules).map(|_| OnceLock::new()).collect(),
        })
    }
    fn order(&self, module: usize) -> Result<&TermOrderArena, String> {
        self.orders[module]
            .get_or_init(|| {
                let module = &self.folded.program.modules[module];
                TermOrderArena::new(&ErlangTerms {
                    root: module.source_forms,
                    terms: module.terms.clone(),
                })
            })
            .as_ref()
            .map_err(Clone::clone)
    }
    fn raw_literals(&self, module: usize) -> &RawLiterals {
        self.raw_literals[module].get_or_init(|| {
            let module = &self.folded.program.modules[module];
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
        let (arena, roots) = self.raw_literals(folded.module.0);
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
            self.order(a.module.0)?.compare_map_keys(left, self.order(b.module.0)?, right)?
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
        let order = self.order(a.module.0)?.compare(left, self.order(b.module.0)?, right)?;
        Ok(Some(match operator {
            Comparison::Less => order == Ordering::Less,
            Comparison::LessEqual => order != Ordering::Greater,
            Comparison::Greater => order == Ordering::Greater,
            Comparison::GreaterEqual => order != Ordering::Less,
        }))
    }
}
