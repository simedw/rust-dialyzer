use super::*;
use crate::{
    body_calls::CallWorld,
    emission::{FunctionEvidence, ParentEvidence, function_warning},
    source_callgraph::{GraphVertex, SourceCallGraph},
};

impl BodyFlow<'_, '_, '_> {
    pub fn collect_function_warnings<
        W: Clone + PartialEq + From<BodyWarning> + From<OpaqueMatch>,
    >(
        &mut self,
        graph: &SourceCallGraph,
        world: &CallWorld<'_>,
        state: &BinderState<W>,
    ) -> Result<BinderState<W>> {
        let state_before_current = state.current_fun.clone();
        let mut state = state.clone();
        for key in self.functions.order() {
            let FunctionKey::Label(label) = key else {
                continue;
            };
            let definition = self
                .functions
                .definitions()
                .get(key)
                .ok_or(StateError::Missing(*key))?;
            let entry = self
                .functions
                .entries()
                .get(key)
                .ok_or(StateError::Missing(*key))?;
            let node = definition
                .node
                .ok_or_else(|| BodyError::Boundary("function warning node absent".into()))?;
            state.current_fun = self
                .functions
                .homes()
                .get(label)
                .cloned()
                .map(WarningFunction::Named)
                .ok_or_else(|| BodyError::Boundary("function warning home absent".into()))?;
            let mfa = definition.name.as_ref().map(|name| Mfa {
                module: name.module.clone().into(),
                function: name.name.clone().into(),
                arity: name.arity,
            });
            let vertex =
                mfa.as_ref()
                    .map_or(GraphVertex::Label(*label), |mfa| GraphVertex::Named {
                        module: mfa.module.clone().to_string(),
                        function: mfa.function.clone().to_string(),
                        arity: mfa.arity.into(),
                    });
            let incoming = graph.in_neighbours(&vertex).unwrap_or(&[]);
            let parent = match incoming {
                [
                    GraphVertex::Named {
                        module,
                        function,
                        arity,
                    },
                ] => Some(Mfa {
                    module: module.clone().into(),
                    function: function.clone().into(),
                    arity: num_traits::ToPrimitive::to_usize(arity).ok_or(LatticeError::BadArg)?,
                }),
                [GraphVertex::Label(label)] => graph.function_name(*label),
                _ => None,
            };
            let needs_return = entry.called() && entry.signature().result.is_none();
            let own_return = if needs_return {
                mfa.as_ref()
                    .map(|mfa| world.active_contract(mfa))
                    .transpose()
                    .map_err(BodyError::Boundary)?
                    .flatten()
                    .map(|contract| {
                        contract
                            .summary
                            .return_for(&vec![Type::any(); contract.summary.arguments.len()])
                    })
                    .transpose()
                    .map_err(BodyError::Contract)?
            } else {
                None
            };
            let parent_return = if needs_return
                && own_return.is_none()
                && mfa.is_none()
                && definition.escaping
                && incoming.len() == 1
            {
                parent
                    .as_ref()
                    .map(|mfa| world.active_contract(mfa))
                    .transpose()
                    .map_err(BodyError::Boundary)?
                    .flatten()
                    .map(|contract| {
                        contract
                            .summary
                            .return_for(&vec![Type::any(); contract.summary.arguments.len()])
                    })
                    .transpose()
                    .map_err(BodyError::Contract)?
            } else {
                None
            };
            let kind = crate::returns::ReturnKind::OnlyNormal;
            let evidence = FunctionEvidence {
                named: mfa.is_some(),
                handled: entry.called(),
                reachable: self.functions.reachable().contains(label),
                result: &entry.signature().result,
                validated_own_contract_return: own_return.as_ref(),
                parent: ParentEvidence {
                    escaping: definition.escaping,
                    incoming_count: incoming.len(),
                    parent_named: parent.is_some(),
                    validated_parent_contract_return: parent_return.as_ref(),
                },
                return_kind: kind,
            };
            if let Some(mut warning) = function_warning(&evidence)? {
                if !matches!(warning, crate::emission::FunctionWarning::Unused) {
                    let return_kind = crate::returns::classify(
                        self.patterns.nodes.module(node.module)?,
                        crate::core::NodeId(node.node),
                        self.bifs,
                    )?;
                    warning = function_warning(&FunctionEvidence {
                        return_kind,
                        ..evidence
                    })?
                    .expect("return classification does not suppress a warning");
                }
                let binder = Binder {
                    patterns: self.patterns,
                };
                let pattern = PatternRef::Core(node);
                let annotation = binder.annotation(pattern)?;
                let info = binder.warning_info(pattern, &state)?;
                state.warnings.add(
                    annotation,
                    BodyWarning {
                        source: node,
                        info,
                        message: BodyMessage::Function {
                            warning,
                            name: mfa.map(|mfa| (mfa.function.to_string(), mfa.arity)),
                        },
                    }
                    .into(),
                    false,
                );
            }
        }
        state.current_fun = state_before_current;
        Ok(state)
    }
}
