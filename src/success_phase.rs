use crate::{
    call_transfer::Signature,
    labels::CoreLabel,
    lattice::{Known, LatticeError, Type, TypeGetter, TypeValue},
};
use std::collections::BTreeMap;

pub type FunctionTypes = BTreeMap<CoreLabel, Signature>;

fn function(signature: &Signature) -> Type {
    Type::function(Some(signature.arguments.clone()), signature.result.clone())
}
fn failed_or_uncalled(signature: &Signature) -> bool {
    signature.result.is_none() || signature.arguments.iter().any(Type::is_none)
}

pub fn reached_fixpoint_strict(
    previous: &FunctionTypes,
    next: &FunctionTypes,
) -> Result<bool, LatticeError> {
    let mut old = previous.iter().peekable();
    for (key, signature) in next {
        while old.peek().is_some_and(|(old_key, _)| *old_key < key) {
            old.next();
        }
        let Some((old_key, old_signature)) = old.next() else {
            return Err(LatticeError::FunctionClause);
        };
        if old_key != key {
            return Err(LatticeError::FunctionClause);
        }
        if !failed_or_uncalled(signature)
            && function(old_signature).limit(4)? != function(signature).limit(4)?
        {
            return Ok(false);
        }
    }
    if old.next().is_some() {
        return Err(LatticeError::FunctionClause);
    }
    Ok(true)
}

pub fn updated_types(
    previous: &FunctionTypes,
    next: &FunctionTypes,
) -> Result<Vec<(CoreLabel, Signature)>, LatticeError> {
    let mut old = previous.iter();
    let mut updated = Vec::new();
    for (key, signature) in next {
        let Some((old_key, old_signature)) = old.next() else {
            return Err(LatticeError::FunctionClause);
        };
        if old_key != key {
            return Err(LatticeError::FunctionClause);
        }
        if !failed_or_uncalled(signature)
            && !function(old_signature)
                .limit(4)?
                .is_subtype(&function(signature).limit(4)?)?
        {
            updated.push((*key, signature.clone()));
        }
    }
    if old.next().is_some() {
        return Err(LatticeError::FunctionClause);
    }
    updated.reverse();
    Ok(updated)
}

pub fn persisted_signature(signature: &Signature) -> Result<Signature, LatticeError> {
    let limited = function(signature).limit(5)?;
    let TypeValue::Types(Known::Known(arguments)) = limited.get(TypeGetter::FunctionArgs)? else {
        return Err(LatticeError::FunctionClause);
    };
    let TypeValue::Type(result) = limited.get(TypeGetter::FunctionRange)? else {
        unreachable!("function range getter returns a type");
    };
    Ok(Signature { arguments, result })
}
