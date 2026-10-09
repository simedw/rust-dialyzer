use super::*;
impl Generator<'_> {
    pub(super) fn begin_handler(
        &mut self,
        context: Box<TryContext>,
        body_result: Type,
        todo: &mut Vec<Frame>,
        values: &mut Vec<Type>,
    ) -> Result<()> {
        values.truncate(context.base);
        let body_state = self.state.clone();
        self.state.context.clear();
        let vars = context
            .exception_vars
            .iter()
            .copied()
            .filter(|n| {
                matches!(
                    self.core.modules[context.site.module.0].nodes[n.0].expr,
                    CoreExpr::Var(_)
                )
            })
            .collect::<Vec<_>>();
        let defined = self.defined(context.site.module, &vars, context.defined)?;
        todo.push(Frame::TryHandler {
            site: context.site,
            body_state: Box::new(body_state),
            body_result,
            outer: Box::new(context.outer),
            base: context.base,
        });
        todo.push(Frame::Visit(
            Site {
                module: context.site.module,
                node: context.handler,
            },
            defined,
        ));
        Ok(())
    }
    pub(super) fn finish_try(
        &mut self,
        site: Site,
        body_state: State,
        body_result: Type,
        outer: State,
        handler: Type,
    ) -> Result<Type> {
        let body_cs = self.arena.group(
            GroupKind::Conjunction,
            body_state.context.iter().rev().copied().collect(),
        )?;
        let handler_cs = self.context()?;
        let old = self.arena.group(
            GroupKind::Conjunction,
            outer.context.iter().rev().copied().collect(),
        )?;
        let dst = self.var(&site)?;
        let equal = |arena: &mut NativeConstraintArena, left: Type, right: Type| {
            arena
                .atomic(
                    left,
                    Relation::Equal,
                    NativeOperand::Type(right),
                    self.provider,
                )
                .map_err(GenerationError::from)
        };
        let (new, returned) = if outer.in_guard {
            let body_eq = equal(&mut self.arena, body_result, dst.clone())?;
            let conjunction = self
                .arena
                .group(GroupKind::Conjunction, vec![body_cs, body_eq])?;
            let handler_eq = equal(&mut self.arena, handler, dst.clone())?;
            let disjunction = self
                .arena
                .group(GroupKind::Disjunction, vec![conjunction, handler_eq])?;
            (disjunction, dst)
        } else {
            use crate::lattice::TypePredicate;
            match (
                body_result.predicate(TypePredicate::None)?,
                handler.predicate(TypePredicate::None)?,
            ) {
                (false, false) => {
                    let body_eq = equal(&mut self.arena, dst.clone(), body_result)?;
                    let body = self
                        .arena
                        .group(GroupKind::Conjunction, vec![body_cs, body_eq])?;
                    let handler_eq = equal(&mut self.arena, dst.clone(), handler)?;
                    let handler = self
                        .arena
                        .group(GroupKind::Conjunction, vec![handler_cs, handler_eq])?;
                    (
                        self.arena
                            .group(GroupKind::Disjunction, vec![body, handler])?,
                        dst,
                    )
                }
                (false, true) => {
                    let body_eq = equal(&mut self.arena, dst, body_result.clone())?;
                    (
                        self.arena
                            .group(GroupKind::Conjunction, vec![body_cs, body_eq])?,
                        body_result,
                    )
                }
                (true, false) => {
                    let handler_eq = equal(&mut self.arena, dst, handler.clone())?;
                    (
                        self.arena
                            .group(GroupKind::Conjunction, vec![handler_cs, handler_eq])?,
                        handler,
                    )
                }
                (true, true) => return Err(GenerationError::ConstructionFailure),
            }
        };
        let joined = self.arena.group(GroupKind::Conjunction, vec![old, new])?;
        self.state.context = vec![joined];
        Ok(returned)
    }
}
