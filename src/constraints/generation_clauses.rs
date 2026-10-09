use super::*;
use crate::underapprox::SafeUnderapprox;
impl Generator<'_> {
    pub(super) fn next_clause(
        &mut self,
        mut context: CaseContext,
        todo: &mut Vec<Frame>,
        values: &mut Vec<Type>,
    ) -> Result<()> {
        values.truncate(context.base);
        if context.next == context.clauses.len() {
            if context.branches.is_empty() {
                return Err(GenerationError::ConstructionFailure);
            }
            let disjunction = self.arena.group(
                GroupKind::Disjunction,
                context.branches.into_iter().rev().collect(),
            )?;
            let outer = self.arena.group(
                GroupKind::Conjunction,
                context.outer_context.into_iter().rev().collect(),
            )?;
            self.state = context.previous;
            self.state.context = vec![
                self.arena
                    .group(GroupKind::Conjunction, vec![outer, disjunction])?,
            ];
            values.push(self.var(&context.site)?);
            return Ok(());
        }
        let clause = context.clauses[context.next];
        context.next += 1;
        let CoreExpr::Clause {
            patterns,
            guard,
            body,
        } = self.core.modules[context.site.module.0].nodes[clause.0]
            .expr
            .clone()
        else {
            return Err(GenerationError::Boundary("case child is not clause".into()));
        };
        context.next_subtractions = if let Some(previous) = &context.subtractions {
            let under = SafeUnderapprox::new(self.core, self.labels, self.literal_types);
            let approximation = under
                .evaluate_with_refold_events(context.site.module, &patterns, guard, |node| {
                    self.patterns
                        .refold(crate::patterns::PatternRef::Core(node))
                        .map(|_| ())
                        .map_err(|e| {
                            crate::underapprox::UnderapproxError::Boundary(format!(
                                "underapprox refold: {e:?}"
                            ))
                        })
                })
                .map_err(GenerationError::Underapprox)?;
            let mut ordered = previous
                .iter()
                .cloned()
                .map(|ty| (ty.native_order_key(), ty))
                .collect::<BTreeMap<_, _>>();
            ordered.insert(approximation.native_order_key(), approximation);
            Some(ordered.into_values().collect())
        } else {
            None
        };
        let module = context.site.module;
        let mut defined = context.defined.clone();
        let mut nodes = patterns.clone();
        while let Some(node) = nodes.pop() {
            let expression = &self.core.modules[module.0].nodes[node.0].expr;
            if matches!(expression, CoreExpr::Var(_)) {
                defined.insert(self.variable_id(&Site { module, node })?);
            }
            nodes.extend(expression.native_children());
        }
        self.state = context.previous.clone();
        self.state.context.clear();
        self.state.in_match = true;
        let count = patterns.len();
        todo.push(Frame::ClausePatterns {
            context,
            guard,
            body,
            defined: defined.clone(),
            count,
        });
        for node in patterns.into_iter().rev() {
            todo.push(Frame::Visit(Site { module, node }, defined.clone()));
        }
        Ok(())
    }
}
