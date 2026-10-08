use super::*;
use crate::{
    dataflow_key::DataflowKey,
    patterns::{LiteralRef, PatternRef, Shape},
};
impl Generator<'_> {
    pub(super) fn pattern_variable(&self, pattern: PatternRef) -> Result<Type> {
        let key = match pattern {
            PatternRef::Core(node) => self
                .patterns
                .nodes
                .optional_label(node)
                .map_err(GenerationError::Boundary)?
                .map(DataflowKey::Core),
            PatternRef::Refold(id) => {
                self.patterns
                    .refolded
                    .get(id)
                    .ok_or_else(|| GenerationError::Boundary("refold node outside arena".into()))?
                    .key
            }
        }
        .ok_or_else(|| GenerationError::Boundary("constraint node has no label".into()))?;
        Ok(Type::variable_id(VariableId::Integer(match key {
            DataflowKey::Core(n) => n.0.into(),
            DataflowKey::Synthetic(n) => (-(n.magnitude() as i128)).into(),
        })))
    }
    pub(super) fn pattern_foldable(&self, pattern: PatternRef) -> Result<bool> {
        let mut todo = vec![pattern];
        while let Some(pattern) = todo.pop() {
            match self
                .patterns
                .shape(pattern)
                .map_err(GenerationError::Boundary)?
            {
                Shape::Literal(lit) => {
                    if !self.state.in_match {
                        continue;
                    }
                    let map = self
                        .patterns
                        .contains_map(lit)
                        .map_err(GenerationError::Boundary)?;
                    if map {
                        return Ok(false);
                    }
                }
                Shape::Tuple(xs) => todo.extend(xs),
                Shape::Cons { head, tail } => todo.extend([head, tail]),
                _ => return Ok(false),
            }
        }
        Ok(true)
    }
    pub(super) fn pattern_record(
        &mut self,
        pattern: PatternRef,
        module: ModuleId,
    ) -> Result<Option<Type>> {
        if !self
            .patterns
            .has_record_annotation(pattern)
            .map_err(GenerationError::Boundary)?
        {
            return Ok(None);
        }
        let Shape::Tuple(elements) = self
            .patterns
            .shape(pattern)
            .map_err(GenerationError::Boundary)?
        else {
            return Ok(None);
        };
        let Some(first) = elements.first() else {
            return Ok(None);
        };
        let Shape::Literal(LiteralRef::Term {
            module: tag_module,
            term,
        }) = self
            .patterns
            .shape(*first)
            .map_err(GenerationError::Boundary)?
        else {
            return Ok(None);
        };
        let ErlangTerm::Atom(tag) = self
            .patterns
            .term(tag_module, term)
            .map_err(GenerationError::Boundary)?
        else {
            return Ok(None);
        };
        let tag = tag.clone();
        self.lookup_record(module, &tag, elements.len() - 1)
    }
    pub(super) fn visit_pattern(
        &mut self,
        pattern: PatternRef,
        origin: NodeRef,
        defined: Defined,
        todo: &mut Vec<Frame>,
        values: &mut Vec<Type>,
    ) -> Result<()> {
        if let PatternRef::Core(node) = pattern {
            todo.push(Frame::Visit(
                Site {
                    module: node.module,
                    node: NodeId(node.node),
                },
                defined,
            ));
            return Ok(());
        }
        let shape = self
            .patterns
            .shape(pattern)
            .map_err(GenerationError::Boundary)?;
        let (children, kind) = match shape {
            Shape::Literal(literal) => {
                values.push(
                    self.patterns
                        .literal_type(literal)
                        .map_err(GenerationError::Boundary)?,
                );
                return Ok(());
            }
            Shape::Tuple(xs) => (xs, PatternFinish::Tuple),
            Shape::Values(xs) => (xs, PatternFinish::Values),
            Shape::Cons { head, tail } => (vec![head, tail], PatternFinish::Cons),
            Shape::Map(entries) => {
                let context = MapContext {
                    pattern,
                    origin,
                    entries,
                    pairs: vec![],
                    next: 0,
                    defined,
                    in_match: self.state.in_match,
                    base: values.len(),
                };
                self.next_map_pair(context, todo, values)?;
                return Ok(());
            }
            _ => {
                return Err(GenerationError::Boundary(
                    "unexpected synthetic pattern shape".into(),
                ));
            }
        };
        todo.push(Frame::PatternFinish {
            pattern,
            origin,
            kind,
            count: children.len(),
        });
        for pattern in children.into_iter().rev() {
            todo.push(Frame::PatternVisit {
                pattern,
                origin,
                defined: defined.clone(),
            });
        }
        Ok(())
    }
}
