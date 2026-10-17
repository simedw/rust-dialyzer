//! OTP29 dialyzer_dataflow bind_record and bind_record_pairs.
use super::*;
use crate::{lattice::NativeRecordName, patterns::NativeRecordIdentity};
impl Binder<'_, '_, '_> {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn native_record<W: Clone + PartialEq + From<OpaqueMatch>>(
        &mut self, pat: PatternRef, identity: NativeRecordIdentity, pairs: Vec<PatternRef>,
        actual: &Type, map: &mut DataflowMap, state: &mut BinderState<W>,
        dir: Direction, depth: usize,
    ) -> Result<Type, BindError> {
        if identity == NativeRecordIdentity::Any {
            // Upstream's anonymous-record branch deliberately ignores fields
            // and does not intersect the incoming type.
            return Ok(Type::record_any());
        }
        let name = state.resolve_native_record_identity(&identity);
        let NativeRecordName::Named { module, name: record_name } = &name else { return Err(LatticeError::FunctionClause.into()); };
        let prototype = state.native_records.get(&(module.to_string(), record_name.to_string()))
            .map(|record| record.prototype).unwrap_or_else(|| Type::record(name.clone(), BTreeMap::new()));
        let checked_pattern = if matches!(identity, NativeRecordIdentity::Local(_)) {
            self.patterns.qualified_native_record(pat, NativeRecordIdentity::Qualified { module: module.to_string(), name: record_name.to_string() }, pairs.clone())?
        } else { pat };
        let record = self.checked(checked_pattern, &prototype, actual, state)?;
        for pair in pairs {
            let result = (|| {
                let Shape::RecordPair { key, value } = self.patterns.shape(pair)? else { return Err(LatticeError::FunctionClause.into()); };
                let key = self.atom(key)?;
                let value_type = record.record_get(&key)?;
                let _bound = self.single(value, value_type, map, state, dir, depth)?;
                // t_record_replace([], {Key, ValType}, Record) returns Record.
                // Refining the returned record's field here would diverge.
                Ok::<_, BindError>(())
            })();
            match result {
                Ok(()) => (),
                Err(BindError::Failure(_)) => return Err(self.failure(pat, &prototype, FailureKind::Record)),
                Err(error) => return Err(error),
            }
        }
        Ok(record)
    }
}

#[cfg(test)]
#[path = "native29_tests.rs"]
mod tests;
