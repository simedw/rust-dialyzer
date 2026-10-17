use super::*;
use crate::lattice::{NativeRecordName, NativeValue};

impl BodyFlow<'_, '_, '_> {
    pub(super) fn native_record_identity_value(identity: &NativeRecordName) -> NativeValue {
        match identity {
            NativeRecordName::Any => NativeValue::Nil,
            NativeRecordName::Named { module,name } => NativeValue::Tuple(vec![
                NativeValue::Atom(module.to_string()),NativeValue::Atom(name.to_string())]),
            NativeRecordName::Atom(name) => NativeValue::Atom(name.to_string()),
        }
    }

    pub(super) fn native_record_variable(&self, reference: NodeRef) -> Result<NativeValue> {
        let CoreExpr::Var(id) = self.patterns.nodes.expression(reference)? else {
            return Err(LatticeError::FunctionClause.into());
        };
        Ok(match &self.patterns.nodes.module(reference.module)?.terms[id.0] {
            ErlangTerm::Integer(n) => NativeValue::Integer(n.parse().map_err(|_| LatticeError::FunctionClause)?),
            ErlangTerm::Atom(a) => NativeValue::Atom(a.clone()),
            _ => return Err(LatticeError::FunctionClause.into()),
        })
    }

    pub(super) fn native_record_inf<W>(
        &mut self, node: NodeRef, identity: &NativeRecordName, mut value: Value,
        state: &mut BinderState<W>,
    ) -> Result<Value>
    where W: Clone + PartialEq + From<OpaqueMatch> + From<BodyWarning>
        + From<crate::guard_warning::GuardWarning> + From<crate::call_warning::CallWarning> + From<CaseWarning>,
    {
        let NativeRecordName::Named { module,name } = identity else {
            if matches!(identity,NativeRecordName::Any) { value.typ = Type::record_any(); }
            return Ok(value);
        };
        if let Some(record) = state.native_records.get(&(module.to_string(),name.to_string())).cloned()
            && record.prototype.meet(&value.typ)?.is_none()
        {
            let context = state.presentation_context();
            let actual = context.type_to_string(&value.typ)?;
            let fields = record.prototype.native_record_field_diffs(&value.typ,&context)?;
            self.emit(node,BodyMessage::RecordConstruction { actual,fields },state)?;
            value.typ = Type::none();
        }
        Ok(value)
    }
}
