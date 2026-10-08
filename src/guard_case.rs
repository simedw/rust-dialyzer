use crate::{
    binder::{BindError, Binder, BinderState, Direction, OpaqueMatch},
    core::CoreExpr,
    dataflow_map::DataflowMap,
    lattice::{Known, Type, TypeGetter, TypePredicate, TypeValue},
    patterns::{PatternRef, Patterns},
    program::NodeRef,
    underapprox::{SafeUnderapprox, UnderapproxError},
};
use crate::{
    guard_control::{GuardBound, GuardControl, GuardEnvironment, GuardError},
    guard_ops::GuardEval,
    guard_warning::GuardWarning,
};
impl GuardControl<'_, '_, '_> {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn case_guard<W: Clone + PartialEq + From<OpaqueMatch> + From<GuardWarning>>(
        &self,
        node: NodeRef,
        argument: NodeRef,
        clauses: &[NodeRef],
        map: &DataflowMap,
        env: &GuardEnvironment,
        eval: GuardEval,
        state: &BinderState<W>,
        depth: usize,
    ) -> Result<GuardBound<W>, GuardError> {
        let scope = map.begin_join();
        let baseline = scope.branch();
        let general = self.visit(
            argument,
            &baseline,
            env,
            GuardEval::Unknown,
            state,
            depth + 1,
        )?;
        let mut remaining = general.value;
        let mut state = general.state;
        let mut accumulated = Type::none();
        let mut maps = Vec::new();
        let mut throws = Vec::new();
        let safe = SafeUnderapprox::new(
            self.nodes.core(),
            self.nodes.labels(),
            self.nodes.literal_types(),
        );
        for &clause in clauses {
            let CoreExpr::Clause {
                patterns,
                guard,
                body,
            } = self.nodes.expression(clause)?
            else {
                return Err(GuardError::Unported {
                    node: clause,
                    kind: "non-clause guard Case child",
                });
            };
            let child = |id| crate::dataflow_nodes::DataflowNodes::node(node.module, id);
            let guard = child(*guard);
            let body = child(*body);
            if patterns.is_empty() {
                if let CoreExpr::Literal(id) = self.nodes.expression(body)?
                    && matches!(&self.nodes.module(body.module)?.terms[id.0],crate::core::ErlangTerm::Atom(atom)if atom=="fail")
                {
                    continue;
                }
                if let CoreExpr::Primop { name, .. } = self.nodes.expression(body)?
                    && self.atom(child(*name))? == "match_fail"
                {
                    continue;
                }
            }
            let mut current_map = Some(general.map.clone());
            let mut arg_type = remaining.clone();
            if let [pattern] = patterns.as_slice()
                && let CoreExpr::Literal(id) = self.nodes.expression(child(*pattern))?
                && let crate::core::ErlangTerm::Atom(atom) =
                    &self.nodes.module(node.module)?.terms[id.0]
            {
                let mode = match atom.as_str() {
                    "true" => Some(GuardEval::Positive),
                    "false" => Some(GuardEval::Negative),
                    _ => None,
                };
                if let Some(mode) = mode {
                    match self.visit(argument, &baseline, env, mode, &state, depth + 1) {
                        Ok(bound) => {
                            current_map = Some(bound.map);
                            arg_type = bound.value;
                            state = bound.state
                        }
                        Err(GuardError::Failure { fatal: false, .. }) => {
                            current_map = None;
                        }
                        Err(error) => return Err(error),
                    }
                }
            }
            if !patterns.is_empty() {
                if arg_type.is_none() {
                    current_map = None;
                } else {
                    let types = if arg_type.predicate(TypePredicate::Any)? {
                        vec![Type::any(); patterns.len()]
                    } else {
                        let TypeValue::Types(Known::Known(types)) =
                            arg_type.get(TypeGetter::ToTypeList)?
                        else {
                            return Err(GuardError::Boundary("Case type list getter".into()));
                        };
                        types
                    };
                    let mut arena = Patterns::new(self.nodes);
                    let mut binder = Binder {
                        patterns: &mut arena,
                    };
                    let refs = patterns
                        .iter()
                        .map(|id| PatternRef::Core(child(*id)))
                        .collect::<Vec<_>>();
                    match binder.bind(
                        &refs,
                        &types,
                        current_map.as_ref().unwrap_or(&general.map),
                        &state,
                        Direction::Forward,
                    ) {
                        Ok(bound) => {
                            if current_map.is_some() {
                                current_map = Some(bound.map);
                            }
                            state = bound.state;
                        }
                        Err(BindError::Failure(_)) => current_map = None,
                        Err(error) => return Err(error.into()),
                    }
                }
            }
            let under = safe
                .evaluate(node.module, patterns, crate::core::NodeId(guard.node))
                .map_err(|error| match error {
                    UnderapproxError::Native(error) => GuardError::from(error),
                    UnderapproxError::NativeCaseClauseAtom(atom) => {
                        GuardError::UnderapproxCaseClauseAtom(atom)
                    }
                    UnderapproxError::ResourceLimit => GuardError::ResourceLimit,
                    UnderapproxError::Boundary(text) => GuardError::Boundary(text),
                })?;
            let next = remaining.subtract(&under)?;
            if let Some(current_map) = current_map.filter(|_| !remaining.is_none()) {
                let saved = state.clone();
                let result = (|| {
                    let guard = self.visit(
                        guard,
                        &current_map,
                        env,
                        GuardEval::Positive,
                        &saved,
                        depth + 1,
                    )?;
                    if Type::atom("true").meet(&guard.value)?.is_none() {
                        return Ok(None);
                    }
                    let body = self.visit(body, &guard.map, env, eval, &guard.state, depth + 1)?;
                    let valid = match eval {
                        GuardEval::Positive => body.value.is_atom_literal("true")?,
                        GuardEval::Negative => body.value.is_atom_literal("false")?,
                        GuardEval::Unknown => true,
                    };
                    Ok(if valid { Some(body) } else { None })
                })();
                match result {
                    Ok(Some(bound)) => {
                        accumulated = accumulated.join(&bound.value)?;
                        maps.insert(0, bound.map);
                        state = bound.state;
                    }
                    Ok(None) => state = saved,
                    Err(GuardError::Failure {
                        fatal: false,
                        reason,
                    }) => {
                        throws.push(reason);
                        state = saved
                    }
                    Err(error) => return Err(error),
                }
            }
            remaining = next;
        }
        if accumulated.is_none() {
            return Err(GuardError::Failure {
                fatal: false,
                reason: throws.into_iter().next().flatten(),
            });
        }
        Ok(GuardBound {
            map: scope.finish(&maps)?,
            value: accumulated,
            state,
        })
    }
}
