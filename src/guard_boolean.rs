use crate::{
    binder::{BinderState, OpaqueMatch},
    dataflow_map::DataflowMap,
    lattice::{BuiltinType, Known, Type, TypeGetter, TypePredicate, TypeValue},
    program::NodeRef,
};
use crate::{
    guard_control::{GuardBound, GuardControl, GuardEnvironment, GuardError},
    guard_ops::GuardEval,
    guard_warning::GuardWarning,
};
impl GuardControl<'_, '_, '_> {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn boolean_call<W: Clone + PartialEq + From<OpaqueMatch> + From<GuardWarning>>(
        &self,
        guard: NodeRef,
        function: &str,
        args: &[NodeRef],
        map: &DataflowMap,
        env: &GuardEnvironment,
        eval: GuardEval,
        state: &BinderState<W>,
        depth: usize,
    ) -> Result<GuardBound<W>, GuardError> {
        if function == "not" {
            if eval != GuardEval::Unknown {
                let opposite = if eval == GuardEval::Positive {
                    GuardEval::Negative
                } else {
                    GuardEval::Positive
                };
                let first = self.visit(args[0], map, env, opposite, state, depth + 1)?;
                if first
                    .value
                    .is_atom_literal(if opposite == GuardEval::Positive {
                        "true"
                    } else {
                        "false"
                    })?
                {
                    return Ok(GuardBound {
                        map: first.map,
                        value: Type::atom(if eval == GuardEval::Positive {
                            "true"
                        } else {
                            "false"
                        }),
                        state: first.state,
                    });
                }
                let second = self.visit(args[0], map, env, eval, &first.state, depth + 1)?;
                return self.fail(
                    guard,
                    function,
                    args,
                    &[second.value],
                    eval,
                    false,
                    &second.state,
                );
            }
            let bound = self.visit(args[0], map, env, eval, state, depth + 1)?;
            let state = self.opaque(
                guard,
                function,
                args,
                std::slice::from_ref(&bound.value),
                &bound.state,
            )?;
            let boolean = bound.value.meet(&Type::builtin(BuiltinType::Boolean)?)?;
            if boolean.is_none() {
                return Err(GuardError::Failure {
                    fatal: true,
                    reason: None,
                });
            }
            let TypeValue::Atoms(values) = boolean.get(TypeGetter::AtomValues)? else {
                return Err(GuardError::Boundary("boolean atom getter shape".into()));
            };
            let value = match values {
                Known::Known(values) if values == ["true"] => Type::atom("false"),
                Known::Known(values) if values == ["false"] => Type::atom("true"),
                Known::Known(values) if values.len() == 2 => boolean,
                Known::Unknown => {
                    return self.fail(guard, function, args, &[bound.value], eval, false, &state);
                }
                _ => {
                    return Err(GuardError::Unported {
                        node: guard,
                        kind: "malformed not boolean values",
                    });
                }
            };
            return Ok(GuardBound {
                map: bound.map,
                value,
                state,
            });
        }
        let sequential = (function == "and" && eval == GuardEval::Positive)
            || (function == "or" && eval == GuardEval::Negative);
        if sequential {
            let desired = if eval == GuardEval::Positive {
                "true"
            } else {
                "false"
            };
            let first = self.visit(args[0], map, env, eval, state, depth + 1)?;
            if !first.value.is_atom_literal(desired)? {
                return self.fail(
                    guard,
                    function,
                    args,
                    &[first.value, Type::any()],
                    eval,
                    false,
                    &first.state,
                );
            }
            let second = self.visit(args[1], &first.map, env, eval, &first.state, depth + 1)?;
            if !second.value.is_atom_literal(desired)? {
                return self.fail(
                    guard,
                    function,
                    args,
                    &[first.value, second.value],
                    eval,
                    false,
                    &second.state,
                );
            }
            return Ok(GuardBound {
                map: second.map,
                value: Type::atom(desired),
                state: second.state,
            });
        }
        let join = map.begin_join();
        let baseline = join.branch();
        let first = match self.visit(args[0], &baseline, env, eval, state, depth + 1) {
            Err(GuardError::Failure { fatal: false, .. }) if eval != GuardEval::Unknown => {
                let (fallback, mode) = if function == "and" {
                    (args[1], GuardEval::Positive)
                } else {
                    (args[0], GuardEval::Unknown)
                };
                self.visit(fallback, &baseline, env, mode, state, depth + 1)?
            }
            other => other?,
        };
        let second = match self.visit(args[1], &baseline, env, eval, &first.state, depth + 1) {
            Err(GuardError::Failure { fatal: false, .. }) if eval != GuardEval::Unknown => {
                let (fallback, mode) = if function == "and" {
                    (args[0], GuardEval::Positive)
                } else {
                    (args[1], GuardEval::Unknown)
                };
                self.visit(fallback, &baseline, env, mode, &first.state, depth + 1)?
            }
            other => other?,
        };
        let types = [first.value, second.value];
        if eval == GuardEval::Negative {
            if types[0].is_atom_literal("false")? || types[1].is_atom_literal("false")? {
                return Ok(GuardBound {
                    map: join.finish(&[first.map, second.map])?,
                    value: Type::atom("false"),
                    state: second.state,
                });
            }
            return self.fail(guard, function, args, &types, eval, false, &second.state);
        }
        if eval == GuardEval::Positive {
            if (types[0].is_atom_literal("true")? && types[1].predicate(TypePredicate::Boolean)?)
                || (types[1].is_atom_literal("true")?
                    && types[0].predicate(TypePredicate::Boolean)?)
            {
                return Ok(GuardBound {
                    map: join.finish(&[first.map, second.map])?,
                    value: Type::atom("true"),
                    state: second.state,
                });
            }
            return self.fail(guard, function, args, &types, eval, false, &second.state);
        }
        let state = self.opaque(guard, function, args, &types, &second.state)?;
        let boolean = Type::builtin(BuiltinType::Boolean)?;
        let left = types[0].meet(&boolean)?;
        let right = types[1].meet(&boolean)?;
        if left.is_none() || right.is_none() {
            return Err(GuardError::Failure {
                fatal: true,
                reason: None,
            });
        }
        let map = join.finish(&[first.map, second.map])?;
        let TypeValue::Atoms(left) = left.get(TypeGetter::AtomValues)? else {
            return Err(GuardError::Boundary("left boolean getter".into()));
        };
        let TypeValue::Atoms(right) = right.get(TypeGetter::AtomValues)? else {
            return Err(GuardError::Boundary("right boolean getter".into()));
        };
        let singleton = |value: &Known<Vec<String>>, s: &str| matches!(value,Known::Known(v)if v.len()==1&&v[0]==s);
        let (identity, absorbing) = if function == "and" {
            ("true", "false")
        } else {
            ("false", "true")
        };
        let value = if singleton(&left, identity) && singleton(&right, identity) {
            Type::atom(identity)
        } else if singleton(&left, absorbing) || singleton(&right, absorbing) {
            Type::atom(absorbing)
        } else if matches!(left, Known::Unknown) || matches!(right, Known::Unknown) {
            return self.fail(guard, function, args, &types, eval, false, &state);
        } else {
            boolean
        };
        Ok(GuardBound { map, value, state })
    }
}
