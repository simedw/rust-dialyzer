use crate::bifs::{callbacks, predicates, structural};
use crate::lattice::{ListView, TuplesView, Type, TypeView};
use crate::native_error::NativeTransferError;
pub fn transfer(name: &str, args: &[Type]) -> Result<Type, NativeTransferError> {
    Ok(match name {
        "zip" | "zip3" => {
            if args
                .iter()
                .map(structural::is_nil)
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .any(|nil| nil)
            {
                Type::nil()
            } else {
                Type::proper_list(
                    Type::tuple(
                        args.iter()
                            .map(structural::list_elements)
                            .collect::<Result<Vec<_>, _>>()?,
                    )
                    .map_err(NativeTransferError::from)?,
                    false,
                )
            }
        }
        "zipwith" | "zipwith3" => Type::proper_list(callbacks::range(&args[0])?, false)
            .join(&Type::nil())
            .map_err(NativeTransferError::from)?,
        "unzip" | "unzip3" => {
            let arity = if name == "unzip" { 2 } else { 3 };
            if structural::is_nil(&args[0])? {
                Type::tuple(vec![Type::nil(); arity]).map_err(NativeTransferError::from)?
            } else {
                let elements = structural::list_elements(&args[0])?.shallow_structural()?;
                let TypeView::Families(s) = elements.view() else {
                    return Err("unzip tuple_subtypes function_clause".into());
                };
                let Some(TuplesView::Alternatives(alts)) = s.tuples else {
                    return Err("unzip unknown tuple subtypes".into());
                };
                alts.iter().try_fold(Type::none(), |acc, t| {
                    if t.elements.len() != arity {
                        return Err("unzip bad tuple arity".into());
                    };
                    acc.join(
                        &Type::tuple(
                            t.elements
                                .iter()
                                .map(|el| Type::proper_list(el.clone(), false))
                                .collect(),
                        )
                        .map_err(NativeTransferError::from)?,
                    )
                    .map_err(NativeTransferError::from)
                })?
            }
        }
        "keydelete" => {
            let list = &args[2];
            let structural_list = list.shallow_structural()?;
            let term = match structural_list.view() {
                TypeView::Families(s) => match s.list {
                    Some(ListView::Nil) => Type::nil(),
                    Some(ListView::List { tail, .. }) => tail.clone(),
                    _ => return Err("list termination function_clause".into()),
                },
                _ => return Err("list termination function_clause".into()),
            };
            term.join(&Type::list(
                structural::list_elements(list)?,
                term.clone(),
                false,
            ))
            .map_err(NativeTransferError::from)?
        }
        "keyreplace" => Type::proper_list(
            structural::list_elements(&args[2])?
                .join(&args[3])
                .map_err(NativeTransferError::from)?,
            false,
        ),
        "keymap" => {
            if structural::is_nil(&args[2])? {
                args[2].clone()
            } else {
                Type::proper_list(
                    callbacks::range(&args[0])?
                        .join(&structural::list_elements(&args[2])?)
                        .map_err(NativeTransferError::from)?,
                    false,
                )
            }
        }
        "keymerge" => args[1].join(&args[2]).map_err(NativeTransferError::from)?,
        "keyfind" | "keymember" | "keysearch" => {
            let tuple = structural::list_elements(&args[2])?
                .meet(&Type::tuple_any())
                .map_err(NativeTransferError::from)?;
            if tuple.is_none() {
                Type::atom("false")
            } else {
                let structural_tuple = tuple.shallow_structural()?;
                let mut fail = false;
                if !args[0].is_any()
                    && let TypeView::Families(s) = structural_tuple.view()
                    && let Some(TuplesView::Alternatives(alts)) = s.tuples
                {
                    let key = args[0]
                        .widen_to_number()
                        .map_err(NativeTransferError::from)?;
                    fail = true;
                    for t in alts.iter() {
                        if !structural::element(
                            &args[1],
                            &Type::tuple(t.elements.to_vec()).map_err(NativeTransferError::from)?,
                        )?
                        .meet(&key)
                        .map_err(NativeTransferError::from)?
                        .is_none()
                        {
                            fail = false;
                            break;
                        }
                    }
                }
                if fail {
                    Type::atom("false")
                } else if name == "keymember" {
                    predicates::boolean()
                } else {
                    let result = if name == "keysearch" {
                        Type::tuple(vec![Type::atom("value"), tuple])
                            .map_err(NativeTransferError::from)?
                    } else {
                        tuple
                    };
                    result
                        .join(&Type::atom("false"))
                        .map_err(NativeTransferError::from)?
                }
            }
        }
        _ => return Err("unsupported specialized list transfer".into()),
    })
}

pub fn flatten(list: &Type) -> Result<Type, NativeTransferError> {
    let proper = Type::proper_list(Type::any(), false);
    let list = list.meet(&proper).map_err(NativeTransferError::from)?;
    if list.is_impossible() || structural::is_nil(&list)? {
        return Ok(list);
    }
    let elements = structural::list_elements(&list)?;
    if elements.is_any() {
        return Ok(proper);
    }
    let nested = flatten(&elements)?;
    let plain = Type::proper_list(
        elements
            .subtract(&proper)
            .map_err(NativeTransferError::from)?,
        false,
    );
    plain.join(&nested).map_err(NativeTransferError::from)
}
