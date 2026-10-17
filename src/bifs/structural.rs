use crate::bifs::arithmetic;
use crate::bifs::predicates::boolean;
use crate::lattice::{ListView, TuplesView, Type, TypeView};
use crate::native_error::NativeTransferError;
use num_traits::ToPrimitive;

pub fn list_elements(value: &Type) -> Result<Type, NativeTransferError> {
    let value = value.shallow_structural()?;
    Ok(match value.view() {
        TypeView::Families(s) => match s.list {
            Some(ListView::List { elements, .. }) => elements.clone(),
            _ => Type::none(),
        },
        _ => Type::none(),
    })
}
pub fn nonempty(value: &Type) -> Result<bool, NativeTransferError> {
    let value = value.shallow_structural()?;
    Ok(
        matches!(value.view(),TypeView::Families(s) if matches!(s.list,Some(ListView::List{nonempty:true,..}))),
    )
}
pub fn tail(value: &Type) -> Result<Type, NativeTransferError> {
    let value = value.shallow_structural()?;

    match value.view() {
        TypeView::Families(s) => match s.list {
            Some(ListView::List { tail, .. }) => {
                Ok(tail.join(&value).map_err(NativeTransferError::from)?)
            }
            _ => Err("native cons_tl function_clause".into()),
        },
        _ => Err("native cons_tl function_clause".into()),
    }
}
fn join(values: impl IntoIterator<Item = Type>) -> Result<Type, NativeTransferError> {
    Ok({
        values.into_iter().try_fold(Type::none(), |a, b| {
            a.join(&b).map_err(NativeTransferError::from)
        })?
    })
}
pub fn element(index: &Type, tuple: &Type) -> Result<Type, NativeTransferError> {
    let tuple = tuple.shallow_structural()?;

    match tuple.view() {
        TypeView::Families(s) => match s.tuples {
            Some(TuplesView::Any) => Ok(Type::any()),
            Some(TuplesView::Alternatives(alts)) => {
                let mut output = Type::none();
                for variant in alts.iter() {
                    let positions = arithmetic::values(index)?;
                    if let Some(ns) = positions {
                        for n in ns {
                            if let Some(n) = n.to_usize()
                                && n >= 1
                                && n <= variant.elements.len()
                            {
                                output = output
                                    .join(&variant.elements[n - 1])
                                    .map_err(NativeTransferError::from)?
                            }
                        }
                    } else {
                        output = output
                            .join(&join(variant.elements.iter().cloned())?)
                            .map_err(NativeTransferError::from)?
                    }
                }
                Ok(output)
            }
            _ => Err("native tuple_subtypes function_clause".into()),
        },
        _ => Err("native tuple_subtypes function_clause".into()),
    }
}
pub fn setelement(index: &Type, tuple: &Type, value: &Type) -> Result<Type, NativeTransferError> {
    let tuple = tuple.shallow_structural()?;

    match tuple.view() {
        TypeView::Families(s) => match s.tuples {
            Some(TuplesView::Any) => Ok(Type::tuple_any()),
            Some(TuplesView::Alternatives(alts)) => {
                let mut output = Type::none();
                for variant in alts.iter() {
                    let mut elements = variant.elements.to_vec();
                    match arithmetic::values(index)? {
                        None => {
                            for field in &mut elements {
                                *field = field.join(value).map_err(NativeTransferError::from)?
                            }
                        }
                        Some(ns) if ns.len() == 1 => {
                            let pos = ns[0].to_usize();
                            if !matches!(pos,Some(n) if n>=1&&n<=elements.len()) {
                                continue;
                            }
                            elements[pos.unwrap() - 1] = value.clone()
                        }
                        Some(ns) => {
                            for n in ns {
                                if let Some(n) = n.to_usize()
                                    && n >= 1
                                    && n <= elements.len()
                                {
                                    elements[n - 1] = elements[n - 1]
                                        .join(value)
                                        .map_err(NativeTransferError::from)?
                                }
                            }
                        }
                    }
                    output = output
                        .join(&Type::tuple(elements).map_err(NativeTransferError::from)?)
                        .map_err(NativeTransferError::from)?
                }
                Ok(output)
            }
            _ => Err("native tuple_subtypes function_clause".into()),
        },
        _ => Err("native tuple_subtypes function_clause".into()),
    }
}
pub fn tuple_list(tuple: &Type) -> Result<Type, NativeTransferError> {
    let tuple = tuple.shallow_structural()?;

    match tuple.view() {
        TypeView::Families(s) => match s.tuples {
            Some(TuplesView::Any) => Ok(Type::proper_list(Type::any(), false)),
            Some(TuplesView::Alternatives(alts)) => {
                let mut elements = Type::none();
                let mut empty = false;
                for variant in alts.iter() {
                    empty |= variant.elements.is_empty();
                    elements = elements
                        .join(&join(variant.elements.iter().cloned())?)
                        .map_err(NativeTransferError::from)?
                }
                let result = Type::proper_list(elements, true);
                Ok(if empty {
                    result
                        .join(&Type::nil())
                        .map_err(NativeTransferError::from)?
                } else {
                    result
                })
            }
            _ => Err("native tuple_subtypes function_clause".into()),
        },
        _ => Err("native tuple_subtypes function_clause".into()),
    }
}
pub fn append(left: &Type, right: &Type) -> Result<Type, NativeTransferError> {
    Ok({
        if is_nil(left)? {
            right.clone()
        } else if is_nil(right)? {
            left.clone()
        } else {
            let cons = Type::cons(list_elements(left)?, right.clone())
                .map_err(NativeTransferError::from)?;
            if nonempty(left)? {
                cons
            } else {
                right.join(&cons).map_err(NativeTransferError::from)?
            }
        }
    })
}
pub fn subtract(left: &Type, right: &Type) -> Result<Type, NativeTransferError> {
    Ok({
        if is_nil(left)? {
            Type::nil()
        } else if is_nil(right)? {
            left.clone()
        } else {
            Type::proper_list(list_elements(left)?, false)
        }
    })
}
pub fn member(key: &Type, list: &Type) -> Result<Type, NativeTransferError> {
    Ok({
        if key
            .meet(&list_elements(list)?)
            .map_err(NativeTransferError::from)?
            .is_none()
        {
            Type::atom("false")
        } else {
            boolean()
        }
    })
}
pub fn make_tuple(size: &Type) -> Result<Type, NativeTransferError> {
    Ok({
        if let Some(values) = arithmetic::values(size)?
            && values.len() == 1
            && let Some(n) = values[0].to_usize()
        {
            return Ok(Type::tuple_arity(n));
        }
        Type::tuple_any()
    })
}
pub fn record(tuple: &Type, tag: &Type, size: Option<&Type>) -> Result<Type, NativeTransferError> {
    let tuple_structural = tuple.shallow_structural()?;
    let arity = size
        .map(arithmetic::values)
        .transpose()?
        .flatten()
        .and_then(|x| if x.len() == 1 { x[0].to_usize() } else { None });
    if !tuple
        .is_subtype(&Type::tuple_any())
        .map_err(NativeTransferError::from)?
    {
        let class = arity.map(Type::tuple_arity).unwrap_or_else(Type::tuple_any);
        return Ok(
            if tuple
                .meet(&class)
                .map_err(NativeTransferError::from)?
                .is_none()
            {
                Type::atom("false")
            } else {
                boolean()
            },
        );
    }
    match tuple_structural.view() {
        TypeView::Families(s) => match s.tuples {
            Some(TuplesView::Any) => Ok(boolean()),
            Some(TuplesView::Alternatives(alts)) => {
                let mut result = Type::none();
                for variant in alts.iter() {
                    let out = if size.is_some() && arity.is_none() {
                        boolean()
                    } else if matches!(arity,Some(n) if n!=variant.elements.len()) {
                        Type::atom("false")
                    } else {
                        let Some(head) = variant.elements.first() else {
                            return Err(NativeTransferError::CaseClauseNil);
                        };
                        if !head
                            .is_subtype(&Type::all_atoms())
                            .map_err(NativeTransferError::from)?
                        {
                            if head
                                .meet(&Type::all_atoms())
                                .map_err(NativeTransferError::from)?
                                .is_none()
                            {
                                Type::atom("false")
                            } else {
                                boolean()
                            }
                        } else {
                            let structural_head = head.shallow_structural()?;
                            let exact = match structural_head.view() {
                                TypeView::Families(h) => {
                                    matches!(h.atoms,Some(crate::lattice::AtomsView::Set(v)) if v.len()==1)
                                }
                                _ => false,
                            };
                            if exact && structural_head == tag.shallow_structural()? {
                                Type::atom("true")
                            } else {
                                boolean()
                            }
                        }
                    };
                    result = result.join(&out).map_err(NativeTransferError::from)?
                }
                Ok(result)
            }
            _ => unreachable!(),
        },
        _ => unreachable!(),
    }
}

#[cfg(otp_domain = "29")]
pub fn record29(args: &[Type]) -> Result<Type, NativeTransferError> {
    use crate::lattice::{RecordTest, TypePredicate};
    let value = &args[0];
    if args.len() == 1 {
        return Ok(if value.is_native_record() { Type::atom("true") }
            else if value.meet(&Type::record_any())?.is_none() { Type::atom("false") }
            else { boolean() });
    }
    let native = if args.len() == 2 {
        value.is_native_record_named(&args[1].to_native_value())
    } else {
        value.is_native_record_qualified(&args[1].to_native_value(),&args[2].to_native_value())
    };
    match native {
        RecordTest::True => return Ok(Type::atom("true")),
        RecordTest::Unknown => return Ok(boolean()),
        RecordTest::False => {}
    }
    if !value.predicate(TypePredicate::Tuple)? {
        return Ok(if value.meet(&Type::tuple_any())?.is_none() { Type::atom("false") } else { boolean() });
    }
    // In OTP 29 erl_bif_types, is_integer(Z) receives the native type term,
    // so the tuple arity branch cannot run for canonical type arguments.
    if args.len() == 3 { return Ok(boolean()); }
    record(value,&args[1],None)
}

pub fn is_nil(value: &Type) -> Result<bool, NativeTransferError> {
    Ok(value.shallow_structural()? == Type::nil())
}
