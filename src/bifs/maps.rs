use crate::bifs::structural;
use crate::lattice::{Presence, TuplesView, Type, TypeView};
use crate::native_error::NativeTransferError;
pub fn size(map: &Type) -> Result<Type, NativeTransferError> {
    let map = map.shallow_structural()?;
    let TypeView::Families(s) = map.view() else {
        return Err("map_size function_clause".into());
    };
    let Some(map) = s.maps else {
        return Err("map_size function_clause".into());
    };
    let lower = map
        .entries
        .iter()
        .filter(|e| e.presence == Presence::Mandatory)
        .count();
    Type::range_strings(
        &lower.to_string(),
        &if map.default_key.is_none() {
            map.entries.len().to_string()
        } else {
            "pos_inf".into()
        },
    )
    .map_err(Into::into)
}
pub fn from_keys(list: &Type, value: &Type) -> Result<Type, NativeTransferError> {
    Type::map(vec![], structural::list_elements(list)?, value.clone())
        .map_err(NativeTransferError::from)
}
pub fn from_list(list: &Type) -> Result<Type, NativeTransferError> {
    if structural::is_nil(list)? {
        return Type::map(vec![], Type::none(), Type::none()).map_err(NativeTransferError::from);
    }
    let elements = structural::list_elements(list)?.shallow_structural()?;
    let TypeView::Families(s) = elements.view() else {
        return Err("tuple_subtypes function_clause".into());
    };
    match s.tuples {
        Some(TuplesView::Any) => Ok(Type::map_any()),
        Some(TuplesView::Alternatives(tuples)) => tuples.iter().try_fold(Type::none(), |acc, t| {
            if t.elements.len() != 2 {
                return Err("map pair tuple arity".into());
            }
            let map = Type::map(vec![], t.elements[0].clone(), t.elements[1].clone())
                .map_err(NativeTransferError::from)?;
            acc.join(&map).map_err(NativeTransferError::from)
        }),
        _ => Err("tuple_subtypes function_clause".into()),
    }
}

pub fn merge(left: &Type, right: &Type) -> Result<Type, NativeTransferError> {
    let left = left.shallow_structural()?;
    let right = right.shallow_structural()?;
    let (TypeView::Families(a), TypeView::Families(b)) = (left.view(), right.view()) else {
        return Err("map_pairwise_merge function_clause".into());
    };
    let (Some(a), Some(b)) = (a.maps, b.maps) else {
        return Err("map_pairwise_merge function_clause".into());
    };
    let entries = left
        .map_pairwise_merge(&right, |_, presence, value, rpresence, rvalue| {
            Ok(Some(if rpresence == Presence::Mandatory {
                (Presence::Mandatory, rvalue.clone())
            } else {
                (presence, value.join(rvalue)?)
            }))
        })
        .map_err(NativeTransferError::from)?;
    Type::map(
        entries,
        a.default_key
            .join(b.default_key)
            .map_err(NativeTransferError::from)?,
        a.default_value
            .join(b.default_value)
            .map_err(NativeTransferError::from)?,
    )
    .map_err(NativeTransferError::from)
}
