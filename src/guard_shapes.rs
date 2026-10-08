use crate::{
    binder::{Binder, BinderState, OpaqueMatch},
    core::{CoreExpr, ErlangTerm},
    core_print::CorePrinter,
    dataflow_map::DataflowMap,
    lattice::{BuiltinType, Known, OpacityConflict, Type, TypeGetter, TypeValue},
    native_error::NativeTransferError,
    patterns::{PatternRef, Patterns},
    program::NodeRef,
};
use crate::{
    guard_control::{GuardBound, GuardControl, GuardEnvironment, GuardError, GuardReason},
    guard_ops::GuardEval,
    guard_producer::ProducerError,
    guard_warning::GuardWarning,
};
use num_bigint::BigInt;
use num_traits::ToPrimitive;
fn duplicate_count(value: &BigInt) -> Result<usize, GuardError> {
    if value < &BigInt::from(0) {
        return Err(GuardError::Native(NativeTransferError::FunctionClause));
    }
    let count = value.to_usize().ok_or(GuardError::ResourceLimit)?;
    if count > 1_000_000 {
        return Err(GuardError::ResourceLimit);
    }
    Ok(count)
}
impl GuardControl<'_, '_, '_> {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn function_guard<W: Clone + PartialEq + From<OpaqueMatch> + From<GuardWarning>>(
        &self,
        guard: NodeRef,
        args: &[NodeRef],
        map: &DataflowMap,
        env: &GuardEnvironment,
        eval: GuardEval,
        state: &BinderState<W>,
        depth: usize,
    ) -> Result<GuardBound<W>, GuardError> {
        let fun = self.visit(args[0], map, env, GuardEval::Unknown, state, depth + 1)?;
        let arity = self.visit(
            args[1],
            &fun.map,
            env,
            GuardEval::Unknown,
            &fun.state,
            depth + 1,
        )?;
        let actual = [fun.value, arity.value];
        let state = self.opaque(guard, "is_function", args, &actual, &arity.state)?;
        let arity_type = actual[1].meet(&Type::builtin(BuiltinType::Integer)?)?;
        if arity_type.is_none() {
            return self.fail(guard, "is_function", args, &actual, eval, false, &state);
        }
        let TypeValue::Integers(values) = arity_type.get(TypeGetter::NumberValues)? else {
            return Err(GuardError::Boundary(
                "function guard number getter shape".into(),
            ));
        };
        let constraint = match values {
            Known::Unknown => Type::builtin(BuiltinType::Function)?,
            Known::Known(values) => {
                let mut result = Type::none();
                for value in values {
                    let count = duplicate_count(&value)?;
                    result = result
                        .join(&Type::function(Some(vec![Type::any(); count]), Type::any()))?;
                }
                result
            }
        };
        let fun_type = actual[0].meet(&constraint)?;
        if fun_type.is_none() {
            return if eval == GuardEval::Positive {
                self.fail(guard, "is_function", args, &actual, eval, false, &state)
            } else {
                Ok(GuardBound {
                    map: arity.map,
                    value: Type::atom("false"),
                    state,
                })
            };
        }
        let mut map = arity.map;
        let value = match eval {
            GuardEval::Positive => {
                self.nodes.enter_type(args[0], fun_type, &mut map)?;
                self.nodes.enter_type(args[1], arity_type, &mut map)?;
                Type::atom("true")
            }
            GuardEval::Negative => Type::atom("false"),
            GuardEval::Unknown => Type::builtin(BuiltinType::Boolean)?,
        };
        Ok(GuardBound { map, value, state })
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn record_guard<W: Clone + PartialEq + From<OpaqueMatch> + From<GuardWarning>>(
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
        let tag = self.atom(args[1])?;
        let CoreExpr::Literal(id) = self.nodes.expression(args[2])? else {
            return Err(GuardError::Unported {
                node: args[2],
                kind: "cerl int_val badrecord payload",
            });
        };
        let ErlangTerm::Integer(arity) = &self.nodes.module(args[2].module)?.terms[id.0] else {
            return Err(GuardError::Native(NativeTransferError::FunctionClause));
        };
        let arity = arity
            .parse::<BigInt>()
            .map_err(|e| GuardError::Boundary(e.to_string()))?;
        let bound = self.visit(args[0], map, env, GuardEval::Unknown, state, depth + 1)?;
        let mut state = self.opaque(
            guard,
            "is_record",
            &args[..1],
            std::slice::from_ref(&bound.value),
            &bound.state,
        )?;
        let count = duplicate_count(&(&arity - 1))?;
        let mut fields = vec![Type::any(); count + 1];
        fields[0] = Type::atom(tag.clone());
        let tuple = Type::tuple(fields)?;
        let intersection = tuple.meet(&bound.value)?;
        if bound.value.opacity_conflict(&tuple, &state.module)? != OpacityConflict::None {
            let mut patterns = Patterns::new(self.nodes);
            let pattern = PatternRef::Core(guard);
            let text = CorePrinter {
                patterns: &patterns,
            }
            .format_patterns(&[pattern])
            .map_err(ProducerError::Core)?;
            let context = state.presentation_context();
            let message = [
                text,
                context.type_to_string(&tuple)?,
                context.type_to_string(&intersection)?,
            ];
            let binder = Binder {
                patterns: &mut patterns,
            };
            let annotation = binder.annotation(pattern)?;
            if state.warnings.warning_mode && !annotation.is_generated() {
                let info = binder.warning_info(pattern, &state)?;
                state.warnings.add(
                    annotation,
                    OpaqueMatch {
                        pattern,
                        actual: bound.value.clone(),
                        expected: tuple.clone(),
                        intersection: intersection.clone(),
                        message,
                        info,
                    }
                    .into(),
                    false,
                );
            }
        }
        if intersection.is_none() {
            return if eval == GuardEval::Positive {
                self.fail(
                    guard,
                    function,
                    args,
                    &[bound.value, Type::atom(tag), Type::integer(arity)],
                    eval,
                    false,
                    &state,
                )
            } else {
                Ok(GuardBound {
                    map: bound.map,
                    value: Type::atom("false"),
                    state,
                })
            };
        }
        let prototype = state
            .records
            .get(&(tag.clone(), count))
            .map(|record| &record.prototype)
            .unwrap_or(&tuple);
        let typ = prototype.meet(&bound.value)?;
        if typ.is_none() {
            return Err(GuardError::Failure {
                fatal: false,
                reason: Some(GuardReason::RecordMatching {
                    guard,
                    actual: format!(
                        "record {}",
                        state.presentation_context().type_to_string(&bound.value)?
                    ),
                    tag,
                }),
            });
        }
        let mut map = bound.map;
        let value = match eval {
            GuardEval::Positive => {
                self.nodes.enter_type(args[0], typ, &mut map)?;
                Type::atom("true")
            }
            GuardEval::Negative => Type::atom("false"),
            GuardEval::Unknown => Type::builtin(BuiltinType::Boolean)?,
        };
        Ok(GuardBound { map, value, state })
    }
}
