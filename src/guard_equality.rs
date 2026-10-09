use crate::{
    binder::{BinderState, OpaqueMatch},
    core::{CoreExpr, ErlangTerm},
    dataflow_map::DataflowMap,
    lattice::{BuiltinType, OpacityConflict, Type, TypePredicate},
    native_type_operand::NativeTypeOperand,
    program::NodeRef,
};
use crate::{
    guard_control::{GuardBound, GuardControl, GuardEnvironment, GuardError},
    guard_ops::GuardEval,
    guard_producer::ArgumentRef,
    guard_warning::GuardWarning,
};
impl GuardControl<'_, '_, '_> {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn equality<W: Clone + PartialEq + From<OpaqueMatch> + From<GuardWarning>>(
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
        if eval == GuardEval::Positive {
            let literal = if self.literals.value(args[0])?.is_some() {
                Some((args[0], args[1]))
            } else if self.literals.value(args[1])?.is_some() {
                Some((args[1], args[0]))
            } else {
                None
            };
            if let Some((literal, other)) = literal
                && (function == "=:="
                    || matches!(
                        self.literals.value(literal)?,
                        Some(ErlangTerm::Atom(_) | ErlangTerm::Nil)
                    ))
            {
                return self.literal_other(
                    guard, function, args, literal, other, map, env, state, depth,
                );
            }
        }
        if function == "=:=" {
            return self.strict_general(guard, function, args, map, env, eval, state, depth);
        }
        let first = self.visit(args[0], map, env, GuardEval::Unknown, state, depth + 1)?;
        let second = self.visit(
            args[1],
            &first.map,
            env,
            GuardEval::Unknown,
            &first.state,
            depth + 1,
        )?;
        let types = [first.value, second.value];
        let state = self.opaque(guard, "==", args, &types, &second.state)?;
        if types[0].predicate(TypePredicate::Nil)?
            || types[1].predicate(TypePredicate::Nil)?
            || types[0].predicate(TypePredicate::Atom)?
            || types[1].predicate(TypePredicate::Atom)?
        {
            return self.strict_general(guard, function, args, map, env, eval, &state, depth);
        }
        if crate::binder::opacity_conflict(&types[0], &types[1], &state.module)? != OpacityConflict::None {
            return self.fail(guard, function, args, &types, eval, false, &state);
        }
        Ok(GuardBound {
            map: second.map,
            value: crate::guard_ops::eval_intersection(
                eval,
                &Type::builtin(BuiltinType::Boolean)?,
            )?,
            state,
        })
    }
    #[allow(clippy::too_many_arguments)]
    fn strict_general<W: Clone + PartialEq + From<OpaqueMatch> + From<GuardWarning>>(
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
        let first = self.visit(args[0], map, env, GuardEval::Unknown, state, depth + 1)?;
        let second = self.visit(
            args[1],
            &first.map,
            env,
            GuardEval::Unknown,
            &first.state,
            depth + 1,
        )?;
        let types = [first.value, second.value];
        let state = self.opaque(guard, "=:=", args, &types, &second.state)?;
        let intersection = types[0].meet(&types[1])?;
        if intersection.is_none() {
            return if eval == GuardEval::Positive {
                self.fail(guard, function, args, &types, eval, false, &state)
            } else {
                Ok(GuardBound {
                    map: second.map,
                    value: Type::atom("false"),
                    state,
                })
            };
        }
        let mut map = second.map;
        let value = match eval {
            GuardEval::Positive => {
                match (
                    self.nodes.expression(args[0])?,
                    self.nodes.expression(args[1])?,
                ) {
                    (CoreExpr::Var(_), CoreExpr::Var(_)) => {
                        self.nodes.enter_substitution(args[0], args[1], &mut map)?;
                        self.nodes.enter_type(args[1], intersection, &mut map)?;
                    }
                    (CoreExpr::Var(_), _) => {
                        self.nodes.enter_type(args[0], intersection, &mut map)?
                    }
                    (_, CoreExpr::Var(_)) => {
                        self.nodes.enter_type(args[1], intersection, &mut map)?
                    }
                    _ => {}
                }
                Type::atom("true")
            }
            GuardEval::Negative => Type::atom("false"),
            GuardEval::Unknown => Type::builtin(BuiltinType::Boolean)?,
        };
        Ok(GuardBound { map, value, state })
    }
    #[allow(clippy::too_many_arguments)]
    fn literal_other<W: Clone + PartialEq + From<OpaqueMatch> + From<GuardWarning>>(
        &self,
        guard: NodeRef,
        function: &str,
        original_args: &[NodeRef],
        literal: NodeRef,
        other: NodeRef,
        map: &DataflowMap,
        env: &GuardEnvironment,
        state: &BinderState<W>,
        depth: usize,
    ) -> Result<GuardBound<W>, GuardError> {
        let value = self
            .literals
            .value(literal)?
            .ok_or_else(|| GuardError::Boundary("expected literal equality operand".into()))?;
        let mode = match value {
            ErlangTerm::Atom(atom) if atom == "true" => GuardEval::Positive,
            ErlangTerm::Atom(atom) if atom == "false" => GuardEval::Negative,
            _ => GuardEval::Unknown,
        };
        let bound = self.visit(other, map, env, mode, state, depth + 1)?;
        let operands = [
            self.literals.raw_operand(literal)?,
            NativeTypeOperand::Canonical(bound.value.clone()),
        ];
        let state = self.opaque_operands(
            guard,
            "=:=",
            &[ArgumentRef::Folded(literal), ArgumentRef::Original(other)],
            &operands,
            &bound.state,
        )?;
        if mode != GuardEval::Unknown {
            let desired = if mode == GuardEval::Positive {
                "true"
            } else {
                "false"
            };
            if bound.value.is_atom_literal(desired)? {
                return Ok(GuardBound {
                    map: bound.map,
                    value: if mode == GuardEval::Positive {
                        bound.value
                    } else {
                        Type::atom("true")
                    },
                    state,
                });
            }
            let reread = self.visit(other, map, env, GuardEval::Unknown, &state, depth + 1)?;
            return self.fail(
                guard,
                function,
                original_args,
                &[reread.value, Type::atom(desired)],
                GuardEval::Unknown,
                false,
                &reread.state,
            );
        }
        let folded = self.producer.folded.root(literal)?;
        let literal_type = self.producer.patterns.nodes.lookup(folded, map)?;
        if literal_type.meet(&bound.value)?.is_none() {
            return self.fail(
                guard,
                function,
                original_args,
                &[bound.value, literal_type],
                GuardEval::Unknown,
                false,
                &state,
            );
        }
        let mut map = bound.map;
        if matches!(self.nodes.expression(other)?, CoreExpr::Var(_)) {
            self.nodes.enter_type(other, literal_type, &mut map)?;
        }
        Ok(GuardBound {
            map,
            value: Type::atom("true"),
            state,
        })
    }
}
