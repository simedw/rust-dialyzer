use crate::{
    bifs::registry::{Mfa, TransferError},
    binder::{Binder, BinderState, OpaqueMatch},
    core::{CoreExpr, ErlangTerm},
    dataflow_map::DataflowMap,
    lattice::{Endpoint, Known, Type, TypeGetter, TypePredicate, TypeValue},
    native_error::NativeTransferError,
    native_type_operand::NativeTypeOperand,
    patterns::{PatternRef, Patterns},
    program::NodeRef,
};
use crate::{
    guard_control::{
        GuardBound, GuardControl, GuardEnvironment, GuardError, GuardFailureKind,
        GuardFailureMessage, GuardReason,
    },
    guard_literals::Comparison,
    guard_ops::{GuardEval, GuardTypeTest, TypeTestResult, bind_type_test},
    guard_producer::ArgumentRef,
    guard_warning::{GuardOpaque, GuardWarning, OpaqueGuardMessage, OpaqueSides},
};
impl From<crate::binder::BindError> for GuardError {
    fn from(value: crate::binder::BindError) -> Self {
        match value {
            crate::binder::BindError::Native(error) => Self::Native(error.into()),
            crate::binder::BindError::ResourceLimit => Self::ResourceLimit,
            other => Self::Binder(other),
        }
    }
}
impl From<crate::form_error::FormError> for GuardError {
    fn from(value: crate::form_error::FormError) -> Self {
        match value {
            crate::form_error::FormError::Native(error) => Self::Native(error),
            other => Self::Form(other),
        }
    }
}
impl From<crate::guard_producer::ProducerError> for GuardError {
    fn from(value: crate::guard_producer::ProducerError) -> Self {
        Self::Producer(value)
    }
}
fn infix(function: &str, arity: usize) -> bool {
    arity == 2
        && matches!(
            function,
            "==" | "/=" | "=:=" | "=/=" | "<" | "=<" | ">" | ">="
        )
}
impl GuardControl<'_, '_, '_> {
    pub(crate) fn atom(&self, node: NodeRef) -> Result<String, GuardError> {
        let CoreExpr::Literal(id) = self.nodes.expression(node)? else {
            return Err(GuardError::Native(NativeTransferError::FunctionClause));
        };
        let ErlangTerm::Atom(value) = &self.nodes.module(node.module)?.terms[id.0] else {
            return Err(GuardError::Native(NativeTransferError::FunctionClause));
        };
        Ok(value.clone())
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn fail<W>(
        &self,
        guard: NodeRef,
        function: &str,
        args: &[NodeRef],
        types: &[Type],
        eval: GuardEval,
        fatal: bool,
        state: &BinderState<W>,
    ) -> Result<GuardBound<W>, GuardError> {
        let types = types
            .iter()
            .cloned()
            .map(NativeTypeOperand::Canonical)
            .collect::<Vec<_>>();
        let args = args
            .iter()
            .copied()
            .map(ArgumentRef::Original)
            .collect::<Vec<_>>();
        let message = if infix(function, args.len()) {
            GuardFailureMessage::Infix {
                left: self
                    .producer
                    .format_args_1(&args[..1], &types[..1], state)?,
                operator: function.into(),
                right: self
                    .producer
                    .format_args_1(&args[1..], &types[1..], state)?,
            }
        } else {
            GuardFailureMessage::Arguments {
                function: function.into(),
                args: self.producer.format_args(&args, &types, state)?,
            }
        };
        Err(GuardError::Failure {
            fatal,
            reason: Some(GuardReason::GuardFailure {
                guard,
                kind: if eval == GuardEval::Negative {
                    GuardFailureKind::NegGuardFail
                } else {
                    GuardFailureKind::GuardFail
                },
                message,
            }),
        })
    }
    pub(crate) fn opaque<W: Clone + PartialEq + From<OpaqueMatch> + From<GuardWarning>>(
        &self,
        guard: NodeRef,
        function: &str,
        args: &[NodeRef],
        types: &[Type],
        state: &BinderState<W>,
    ) -> Result<BinderState<W>, GuardError> {
        let operands = types
            .iter()
            .cloned()
            .map(NativeTypeOperand::Canonical)
            .collect::<Vec<_>>();
        let args = args
            .iter()
            .copied()
            .map(ArgumentRef::Original)
            .collect::<Vec<_>>();
        self.opaque_operands(guard, function, &args, &operands, state)
    }
    pub(crate) fn opaque_operands<W: Clone + PartialEq + From<OpaqueMatch> + From<GuardWarning>>(
        &self,
        guard: NodeRef,
        function: &str,
        args: &[ArgumentRef],
        operands: &[NativeTypeOperand],
        state: &BinderState<W>,
    ) -> Result<BinderState<W>, GuardError> {
        let any = |operand: &NativeTypeOperand| -> Result<bool, GuardError> {
            match operand {
                NativeTypeOperand::Canonical(typ) => Ok(typ.predicate(TypePredicate::Any)?),
                NativeTypeOperand::Raw(raw) if matches!(raw.term(),ErlangTerm::Tuple(items)if items.len()==3 && matches!(&raw.arena().terms[items[0].0],ErlangTerm::Atom(tag)if tag=="c_literal")) => {
                    Ok(false)
                }
                _ => Err(GuardError::Unported {
                    node: guard,
                    kind: "non-literal raw guard predicate",
                }),
            }
        };
        if matches!(function, "=:=" | "=/=") && operands.len() == 2 {
            if any(&operands[0])? || any(&operands[1])? {
                return Ok(state.clone());
            }
            let same = match (&operands[0], &operands[1]) {
                (NativeTypeOperand::Canonical(a), NativeTypeOperand::Canonical(b)) => {
                    a.predicate(TypePredicate::Opaque)?
                        && b.predicate(TypePredicate::Opaque)?
                        && a.same_opaque(b)?
                }
                _ => false,
            };
            if same {
                return Ok(state.clone());
            }
        }
        let mut positions = Vec::new();
        for (index, operand) in operands.iter().enumerate() {
            if operand.is_opaque(&state.module)? {
                positions.push(index + 1);
            }
        }
        if positions.is_empty() {
            return Ok(state.clone());
        }
        let message = if infix(function, args.len()) {
            let positions = match positions.as_slice() {
                [1] => OpaqueSides::Left,
                [2] => OpaqueSides::Right,
                [1, 2] => OpaqueSides::Both,
                _ => {
                    return Err(GuardError::Boundary(
                        "invalid infix opacity positions".into(),
                    ));
                }
            };
            OpaqueGuardMessage::Infix {
                left: self
                    .producer
                    .format_args_1(&args[..1], &operands[..1], state)?,
                operator: function.into(),
                right: self
                    .producer
                    .format_args_1(&args[1..], &operands[1..], state)?,
                positions,
            }
        } else {
            OpaqueGuardMessage::Arguments {
                function: function.into(),
                args: self.producer.format_args(args, operands, state)?,
            }
        };
        // Native formats eagerly before checking warning mode or generated origin.
        let mut state = state.clone();
        if state.warnings.warning_mode {
            let mut patterns = Patterns::new(self.nodes);
            let binder = Binder {
                patterns: &mut patterns,
            };
            let annotation = binder.annotation(PatternRef::Core(guard))?;
            if !annotation.is_generated() {
                let info = binder.warning_info(PatternRef::Core(guard), &state)?;
                state.warnings.add(
                    annotation,
                    GuardWarning::OpaqueGuard(GuardOpaque {
                        guard,
                        info,
                        message,
                    })
                    .into(),
                    false,
                );
            }
        }
        Ok(state)
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn call<W: Clone + PartialEq + From<OpaqueMatch> + From<GuardWarning>>(
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
        if let Some(test) = GuardTypeTest::lookup(function, args.len()) {
            let actual = self.visit(args[0], map, env, GuardEval::Unknown, state, depth + 1)?;
            let state = self.opaque(
                guard,
                function,
                args,
                std::slice::from_ref(&actual.value),
                &actual.state,
            )?;
            return match bind_type_test(eval, &test.domain()?, &actual.value)? {
                TypeTestResult::Impossible => {
                    self.fail(guard, function, args, &[actual.value], eval, false, &state)
                }
                TypeTestResult::Refined { argument, result } => {
                    let mut map = actual.map;
                    self.nodes.enter_type(args[0], argument, &mut map)?;
                    Ok(GuardBound {
                        map,
                        value: result,
                        state,
                    })
                }
            };
        }
        if args.len() == 2
            && matches!(function, "==" | "=:=")
            && let Some(equal) = self.literals.strict_equal(args[0], args[1])?
        {
            if (equal && eval == GuardEval::Negative) || (!equal && eval == GuardEval::Positive) {
                let mut types = Vec::new();
                for arg in args {
                    let literal = self.producer.folded.root(*arg)?;
                    types.push(self.producer.patterns.nodes.lookup(literal, map)?);
                }
                return self.fail(guard, function, args, &types, eval, false, state);
            }
            return Ok(GuardBound {
                map: map.clone(),
                value: Type::atom(if equal { "true" } else { "false" }),
                state: state.clone(),
            });
        }
        if args.len() == 2 && matches!(function, "==" | "=:=") {
            return self.equality(guard, function, args, map, env, eval, state, depth);
        }
        let operator = match function {
            "<" => Some(Comparison::Less),
            "=<" => Some(Comparison::LessEqual),
            ">" => Some(Comparison::Greater),
            ">=" => Some(Comparison::GreaterEqual),
            _ => None,
        };
        if let Some(operator) = operator
            && args.len() == 2
        {
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
            let state = self.opaque(guard, function, args, &types, &second.state)?;
            let _is_integer_left = types[0].predicate(TypePredicate::Integer)?;
            let _is_integer_right = types[1].predicate(TypePredicate::Integer)?;
            if let Some(value) = self.literals.compare(args[0], args[1], operator)? {
                if !value && eval == GuardEval::Positive {
                    return self.fail(guard, function, args, &types, eval, false, &state);
                }
                return Ok(GuardBound {
                    map: map.clone(),
                    value: Type::atom(if value { "true" } else { "false" }),
                    state,
                });
            }
            if eval == GuardEval::Positive && _is_integer_left && _is_integer_right {
                let candidate = if self.literals.value(args[0])?.is_some()
                    && matches!(self.nodes.expression(args[1])?, CoreExpr::Var(_))
                {
                    Some((args[0], args[1], &types[1], operator))
                } else if self.literals.value(args[1])?.is_some()
                    && matches!(self.nodes.expression(args[0])?, CoreExpr::Var(_))
                {
                    let inverse = match operator {
                        Comparison::Less => Comparison::Greater,
                        Comparison::LessEqual => Comparison::GreaterEqual,
                        Comparison::Greater => Comparison::Less,
                        Comparison::GreaterEqual => Comparison::LessEqual,
                    };
                    Some((args[1], args[0], &types[0], inverse))
                } else {
                    None
                };
                if let Some((literal, variable, actual, operator)) = candidate {
                    let ErlangTerm::Integer(value) = self
                        .literals
                        .value(literal)?
                        .ok_or_else(|| GuardError::Boundary("comparison literal missing".into()))?
                    else {
                        return Err(GuardError::Boundary(
                            "integer predicate accepted non-integer literal".into(),
                        ));
                    };
                    let value: num_bigint::BigInt = value
                        .parse()
                        .map_err(|_| GuardError::Boundary("invalid parsed integer".into()))?;
                    let TypeValue::Integers(values) = actual.get(TypeGetter::NumberValues)? else {
                        return Err(GuardError::Boundary(
                            "number values typed getter result".into(),
                        ));
                    };
                    let refined = match values {
                        Known::Unknown => {
                            let range = match operator {
                                Comparison::LessEqual => Type::range(
                                    Endpoint::Finite(value),
                                    Endpoint::PositiveInfinity,
                                )?,
                                Comparison::Less => Type::range(
                                    Endpoint::Finite(value + 1),
                                    Endpoint::PositiveInfinity,
                                )?,
                                Comparison::GreaterEqual => Type::range(
                                    Endpoint::NegativeInfinity,
                                    Endpoint::Finite(value),
                                )?,
                                Comparison::Greater => Type::range(
                                    Endpoint::NegativeInfinity,
                                    Endpoint::Finite(value - 1),
                                )?,
                            };
                            range.meet(actual)?
                        }
                        Known::Known(values) => {
                            Type::integers(values.into_iter().filter(|n| match operator {
                                Comparison::Less => value < *n,
                                Comparison::LessEqual => value <= *n,
                                Comparison::Greater => value > *n,
                                Comparison::GreaterEqual => value >= *n,
                            }))
                        }
                    };
                    if refined.is_none() {
                        return self.fail(guard, function, args, &types, eval, false, &state);
                    }
                    let mut map = second.map;
                    self.nodes.enter_type(variable, refined, &mut map)?;
                    return Ok(GuardBound {
                        map,
                        value: Type::atom("true"),
                        state,
                    });
                }
            }
            return self.generic(guard, function, args, map, env, eval, &state, depth);
        }
        if matches!((function, args.len()), ("and" | "or", 2) | ("not", 1)) {
            return self.boolean_call(guard, function, args, map, env, eval, state, depth);
        }
        if function == "is_function" && args.len() == 2 {
            return self.function_guard(guard, args, map, env, eval, state, depth);
        }
        if matches!(function, "is_record" | "internal_is_record") && args.len() == 3 {
            return self.record_guard(guard, function, args, map, env, eval, state, depth);
        }
        self.generic(guard, function, args, map, env, eval, state, depth)
    }
    fn native_bif(&self, mfa: &Mfa, args: &[Type]) -> Result<Type, GuardError> {
        if !self.registry.is_known(mfa) {
            return Ok(if args.iter().any(Type::is_impossible) {
                Type::none()
            } else {
                Type::any()
            });
        }
        self.registry
            .transfer(mfa, args)
            .map(|result| result.result)
            .map_err(|error| match error {
                TransferError::NativeException(error) => GuardError::Native(error),
                other => GuardError::Transfer(other),
            })
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn generic<W: Clone + PartialEq + From<OpaqueMatch> + From<GuardWarning>>(
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
        let (mut map, mut state) = (map.clone(), state.clone());
        let mut types = Vec::with_capacity(args.len());
        for &arg in args {
            let bound = self.visit(arg, &map, env, GuardEval::Unknown, &state, depth + 1)?;
            map = bound.map;
            state = bound.state;
            types.push(bound.value);
        }
        state = self.opaque(guard, function, args, &types, &state)?;
        let mfa = Mfa {
            module: "erlang".into(),
            function: function.into(),
            arity: args.len(),
        };
        let result = self.native_bif(&mfa, &types)?;
        if result.is_none() {
            let fatal = !self
                .native_bif(&mfa, &vec![Type::any(); args.len()])?
                .is_none();
            return self.fail(guard, function, args, &types, eval, fatal, &state);
        }
        let domains = self
            .registry
            .admissible_domains(&mfa)
            .map_err(GuardError::Transfer)?
            .unwrap_or_else(|| vec![Type::any(); args.len()]);
        for ((arg, domain), actual) in args.iter().zip(domains).zip(&types) {
            self.nodes
                .enter_type(*arg, domain.meet(actual)?, &mut map)?;
        }
        let result = crate::guard_ops::eval_intersection(eval, &result)?;
        if result.is_none() {
            return if eval == GuardEval::Positive {
                self.fail(guard, function, args, &types, eval, false, &state)
            } else {
                Err(GuardError::Failure {
                    fatal: false,
                    reason: None,
                })
            };
        }
        Ok(GuardBound {
            map,
            value: result,
            state,
        })
    }
}
