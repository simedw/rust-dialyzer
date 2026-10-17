//! Native record guard paths from OTP29 dialyzer_dataflow.erl.
use super::*;
use crate::{core::CoreExpr, guard_ops::eval_intersection, lattice::NativeRecordName};
impl GuardControl<'_, '_, '_> {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn native_record_guard<W: Clone + PartialEq + From<OpaqueMatch> + From<GuardWarning>>(
        &self, guard: NodeRef, function: &str, args: &[NodeRef], map: &DataflowMap,
        env: &GuardEnvironment, eval: GuardEval, state: &BinderState<W>, depth: usize,
    ) -> Result<GuardBound<W>, GuardError> {
        let bound = self.visit(args[0], map, env, GuardEval::Unknown, state, depth + 1)?;
        let module = self.atom(args[1])?;
        let name = self.atom(args[2])?;
        let prototype = Type::record(NativeRecordName::Named { module: module.as_str().into(), name: name.as_str().into() }, Default::default());
        let intersection = bound.value.meet(&prototype)?;
        if intersection.is_none() {
            if eval == GuardEval::Positive {
                return self.fail(guard, function, args, &[bound.value, Type::atom(module), Type::atom(name)], eval, false, &bound.state);
            }
            return Ok(GuardBound { map: bound.map, value: Type::atom("false"), state: bound.state });
        }
        let mut map = bound.map;
        let value = match eval {
            GuardEval::Positive => {
                // Upstream deliberately re-enters RecType, not its intersection.
                self.nodes.enter_type(args[0], bound.value, &mut map)?;
                Type::atom("true")
            }
            GuardEval::Negative => Type::atom("false"),
            GuardEval::Unknown => Type::builtin(BuiltinType::Boolean)?,
        };
        Ok(GuardBound { map, value, state: bound.state })
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn native_record_field<W: Clone + PartialEq + From<OpaqueMatch> + From<GuardWarning>>(
        &self, args: &[NodeRef], map: &DataflowMap, env: &GuardEnvironment,
        eval: GuardEval, state: &BinderState<W>, depth: usize,
    ) -> Result<GuardBound<W>, GuardError> {
        let [source, identity, field] = args else { return Err(GuardError::Native(NativeTransferError::FunctionClause)); };
        let bound = self.visit(*source, map, env, GuardEval::Unknown, state, depth + 1)?;
        let field = self.atom(*field)?;
        let CoreExpr::Literal(identity_term) = self.nodes.expression(*identity)? else { return Err(GuardError::Native(NativeTransferError::FunctionClause)); };
        let name = bound.state.resolve_native_record_id(&self.nodes.module(identity.module)?.terms, *identity_term)?;
        let constraint = Type::record(name, Default::default());
        let source_type = bound.value.meet(&constraint)?;
        if source_type.is_none() { return Err(GuardError::Failure { fatal: false, reason: None }); }
        let mut map = bound.map;
        self.nodes.enter_type(*source, source_type, &mut map)?;
        let value = eval_intersection(eval, &source_type.record_get(&field)?)?;
        if value.is_none() { return Err(GuardError::Failure { fatal: false, reason: None }); }
        Ok(GuardBound { map, value, state: bound.state })
    }
}
