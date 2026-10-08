use crate::bifs::{predicates, structural};
use crate::lattice::{TuplesView, Type, TypeView};
use crate::native_error::NativeTransferError;
struct Function {
    domain: Option<Vec<Type>>,
    range: Type,
}
fn view(fun: &Type) -> Result<Function, NativeTransferError> {
    let fun = fun.shallow_structural()?;
    match fun.view() {
        TypeView::Families(s) => s
            .functions
            .map(|f| Function {
                domain: f.domain.map(|d| d.to_vec()),
                range: f.range.clone(),
            })
            .ok_or(NativeTransferError::FunctionClause),
        _ => Err(NativeTransferError::FunctionClause),
    }
}
pub fn range(fun: &Type) -> Result<Type, NativeTransferError> {
    Ok(view(fun)?.range.clone())
}
pub fn accepts(fun: &Type, args: &[Type]) -> Result<bool, NativeTransferError> {
    let fun = view(fun)?;
    if fun.range.is_impossible() {
        return Ok(false);
    }
    match fun.domain {
        None => Ok(true),
        Some(domain) if domain.len() == args.len() => {
            for (d, a) in domain.iter().zip(args) {
                if d.meet(a)
                    .map_err(NativeTransferError::from)?
                    .is_impossible()
                {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        _ => Ok(false),
    }
}
pub fn transfer(name: &str, args: &[Type]) -> Result<Type, NativeTransferError> {
    if name == "takewhile" {
        let proper = Type::proper_list(Type::any(), false);
        let list = if args[1]
            .meet(&proper)
            .map_err(NativeTransferError::from)?
            .is_none()
        {
            Type::proper_list(structural::list_elements(&args[1])?, false)
        } else {
            args[1].meet(&proper).map_err(NativeTransferError::from)?
        };
        return transfer("filter", &[args[0].clone(), list]);
    }
    if name == "splitwith" {
        let left = transfer("takewhile", args)?;
        let right = transfer("dropwhile", args)?;
        return Ok(if left.is_none() || right.is_none() {
            Type::none()
        } else {
            Type::tuple(vec![left, right]).map_err(NativeTransferError::from)?
        });
    }
    let fun = &args[0];
    let list = args.last().ok_or("list arg missing")?;
    let nil = structural::is_nil(list)?;
    let nonempty = structural::nonempty(list)?;
    let el = structural::list_elements(list)?;
    if matches!(name, "sort" | "usort") {
        return Ok(
            if !range(fun)?.is_impossible()
                && range(fun)?
                    .is_subtype(&predicates::boolean())
                    .map_err(NativeTransferError::from)?
            {
                list.clone()
            } else if nil {
                Type::nil()
            } else {
                Type::none()
            },
        );
    }
    let default = match name {
        "all" => Type::atom("true"),
        "any" => Type::atom("false"),
        "foreach" => Type::atom("ok"),
        "partition" => {
            Type::tuple(vec![Type::nil(), Type::nil()]).map_err(NativeTransferError::from)?
        }
        "foldl" | "foldr" => args[1].clone(),
        "mapfoldl" | "mapfoldr" => {
            Type::tuple(vec![Type::nil(), args[1].clone()]).map_err(NativeTransferError::from)?
        }
        "map" => Type::nil(),
        _ => Type::nil(),
    };
    if nil {
        return Ok(if name == "map" {
            list.clone()
        } else if matches!(name, "mapfoldl" | "mapfoldr") {
            Type::tuple(vec![list.clone(), args[1].clone()])?
        } else {
            default
        });
    }
    let application = if matches!(name, "foldl" | "foldr" | "mapfoldl" | "mapfoldr") {
        vec![el.clone(), args[1].clone()]
    } else {
        vec![el.clone()]
    };
    let ok = accepts(fun, &application)?;
    let result = range(fun)?;
    if !ok {
        return Ok(if nonempty {
            Type::none()
        } else if matches!(name, "all" | "any") {
            result
        } else {
            default
        });
    }
    let selector = result.shallow_structural()?;
    Ok(match name {
        "all" | "any" => {
            if nonempty {
                result
            } else {
                default.join(&result).map_err(NativeTransferError::from)?
            }
        }
        "foreach" => Type::atom("ok"),
        "foldl" | "foldr" => {
            if nonempty {
                result
            } else {
                result.join(&args[1]).map_err(NativeTransferError::from)?
            }
        }
        "map" => Type::proper_list(result, nonempty),
        "flatmap" => {
            if structural::is_nil(&result)? {
                Type::nil()
            } else {
                Type::proper_list(
                    structural::list_elements(&result)?,
                    nonempty
                        && !Type::nil()
                            .is_subtype(&result)
                            .map_err(NativeTransferError::from)?,
                )
            }
        }
        "dropwhile" => {
            if selector == Type::atom("true") || selector == Type::atom("false") {
                if list
                    .meet(&Type::proper_list(Type::any(), false))
                    .map_err(NativeTransferError::from)?
                    .is_none()
                {
                    Type::none()
                } else if selector == Type::atom("true") {
                    Type::nil()
                } else {
                    list.clone()
                }
            } else {
                let cons = list
                    .meet(&Type::list(Type::any(), Type::any(), true))
                    .map_err(NativeTransferError::from)?;
                structural::tail(&cons)?
                    .meet(&Type::list(Type::any(), Type::any(), false))
                    .map_err(NativeTransferError::from)?
            }
        }
        "filter" => {
            if selector == Type::atom("false") {
                Type::nil()
            } else if selector == Type::atom("true") {
                list.clone()
            } else {
                Type::proper_list(el, false)
            }
        }
        "partition" => {
            if selector == Type::atom("true") {
                Type::tuple(vec![list.clone(), Type::nil()]).map_err(NativeTransferError::from)?
            } else if selector == Type::atom("false") {
                Type::tuple(vec![Type::nil(), list.clone()]).map_err(NativeTransferError::from)?
            } else {
                let l = Type::proper_list(el, false);
                Type::tuple(vec![l.clone(), l]).map_err(NativeTransferError::from)?
            }
        }
        "mapfoldl" | "mapfoldr" => {
            let result = result.shallow_structural()?;
            let TypeView::Families(s) = result.view() else {
                return Err("tuple_subtypes range function_clause".into());
            };
            let Some(TuplesView::Alternatives(alts)) = s.tuples else {
                return Err("mapfold unknown tuple range".into());
            };
            alts.iter().try_fold(Type::none(), |acc, t| {
                if t.elements.len() != 2 {
                    return Err("mapfold pair range badmatch".into());
                }
                let value = Type::tuple(vec![
                    Type::proper_list(t.elements[0].clone(), nonempty),
                    if nonempty {
                        t.elements[1].clone()
                    } else {
                        args[1]
                            .join(&t.elements[1])
                            .map_err(NativeTransferError::from)?
                    },
                ])
                .map_err(NativeTransferError::from)?;
                acc.join(&value).map_err(NativeTransferError::from)
            })?
        }
        _ => return Err("unimplemented callback transfer".into()),
    })
}
