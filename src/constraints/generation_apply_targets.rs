use super::*;
impl Generator<'_> {
    pub(super) fn apply_target_constraints(
        &mut self,
        site: &Site,
        destination: &Type,
        args: &[Type],
    ) -> Result<()> {
        let GenerationCallGraph::Indexed {
            program,
            dependencies,
        } = self.callgraph
        else {
            return Ok(());
        };
        let Some(targets) = dependencies.call_targets.get(&site.reference()) else {
            return Ok(());
        };
        if targets.external {
            return Ok(());
        }
        let mut named = Vec::new();
        for id in &targets.local {
            let function = &program.functions[id.0];
            let crate::program::FunctionIdentity::Named(name) = &function.identity else {
                return Ok(());
            };
            let label = self
                .labels
                .label(NodeRef {
                    module: function.module,
                    node: function.node.0,
                })
                .ok_or_else(|| {
                    GenerationError::Boundary("apply target has no native label".into())
                })?;
            named.push((
                label,
                Mfa {
                    module: name.module.clone(),
                    function: name.name.clone(),
                    arity: name.arity,
                },
            ));
        }
        named.sort_by_key(|(label, _)| *label);
        let outer = self.state.clone();
        let mut alternatives = Vec::new();
        let mut unresolved = outer.missing_library_summaries.clone();
        for (_, mfa) in named {
            self.state = outer.clone();
            self.state.context.clear();
            match self.plt_constraints(&mfa, destination, args) {
                Ok(()) => {
                    alternatives.push(self.context()?);
                    unresolved.extend(self.state.missing_library_summaries.iter().cloned());
                }
                Err(GenerationError::ConstructionFailure) => (),
                Err(error) => {
                    self.state = outer;
                    return Err(error);
                }
            }
        }
        self.state = outer;
        if alternatives.is_empty() {
            return Err(GenerationError::ConstructionFailure);
        }
        let joined = self.arena.group(GroupKind::Disjunction, alternatives)?;
        self.state.context.push(joined);
        self.state.missing_library_summaries = unresolved;
        Ok(())
    }
}
