use crate::bifs::arithmetic::{self, Bound};
use crate::lattice::{AtomsView, Type, TypeView};
use crate::native_error::NativeTransferError;
use num_traits::ToPrimitive;

pub fn boolean() -> Type {
    Type::atoms(["false", "true"])
}
fn truth(value: bool) -> Type {
    Type::atom(if value { "true" } else { "false" })
}
pub fn atom_values(value: &Type) -> Result<Option<Vec<String>>, NativeTransferError> {
    let value = value.shallow_structural()?;
    #[cfg(otp_domain = "29")]
    if matches!(value.view(), TypeView::Families(s) if s.records.is_some()) { return Ok(None); }
    Ok(match value.view() {
        TypeView::Families(s) => match s.atoms {
            Some(AtomsView::Set(x))
                if s.number.is_none()
                    && s.list.is_none()
                    && s.tuples.is_none()
                    && s.identifiers.is_none()
                    && s.bitstrings.is_none()
                    && s.functions.is_none()
                    && s.maps.is_none() =>
            {
                Some(x.iter().map(|a| a.to_string()).collect())
            }
            _ => None,
        },
        _ => None,
    })
}
pub fn test(value: &Type, class: &Type) -> Result<Type, NativeTransferError> {
    Ok({
        if !value.is_impossible() && value.is_subtype(class).map_err(NativeTransferError::from)? {
            truth(true)
        } else if value
            .meet(class)
            .map_err(NativeTransferError::from)?
            .is_none()
        {
            truth(false)
        } else {
            boolean()
        }
    })
}
fn equal(a: &Type, b: &Type) -> Result<Type, NativeTransferError> {
    Ok({
        if a.meet(b).map_err(NativeTransferError::from)?.is_none() {
            return Ok(truth(false));
        }
        if let (Some(left), Some(right)) = (atom_values(a)?, atom_values(b)?) {
            if left.len() == 1 && left == right {
                return Ok(truth(true));
            }
            return Ok(boolean());
        }
        if let (Some((lo1, hi1)), Some((lo2, hi2))) =
            (arithmetic::bounds(a)?, arithmetic::bounds(b)?)
        {
            if matches!((&lo1, &hi2), (Bound::Finite(_), Bound::Finite(_))) && lo1 > hi2
                || matches!((&hi1, &lo2), (Bound::Finite(_), Bound::Finite(_))) && lo2 > hi1
            {
                return Ok(truth(false));
            }
            if lo1 == hi1 && lo2 == hi2 && lo1 == lo2 {
                return Ok(truth(true));
            }
        }
        boolean()
    })
}
pub fn type_ranks(value: &Type) -> Result<(usize, usize), NativeTransferError> {
    use crate::lattice::BuiltinType;
    let mut min = 0;
    let mut max = 0;
    for (index, class) in [
        BuiltinType::Number,
        BuiltinType::Atom,
        BuiltinType::Reference,
        BuiltinType::Function,
        BuiltinType::Port,
        BuiltinType::Pid,
        BuiltinType::Tuple,
        BuiltinType::Map,
        BuiltinType::List,
        BuiltinType::Bitstring,
    ]
    .into_iter()
    .enumerate()
    {
        let class = Type::builtin(class)?;
        if !value.meet(&class)?.is_none() {
            if min == 0 {
                min = index + 1;
            }
            max = index + 1;
        }
    }
    Ok((min, max))
}
pub fn compare(op: &str, a: &Type, b: &Type) -> Result<Type, NativeTransferError> {
    Ok({
        let exact = matches!(op, "=:=" | "=/=")
            || (a
                .is_subtype(&Type::all_atoms())
                .map_err(NativeTransferError::from)?
                && b.is_subtype(&Type::all_atoms())
                    .map_err(NativeTransferError::from)?)
            || (a
                .is_subtype(&Type::all_integers())
                .map_err(NativeTransferError::from)?
                && b.is_subtype(&Type::all_integers())
                    .map_err(NativeTransferError::from)?);
        if matches!(op, "==" | "/=" | "=:=" | "=/=") {
            if !exact {
                return Ok(boolean());
            }
            let equal = equal(a, b)?;
            return Ok(if matches!(op, "/=" | "=/=") {
                if equal == truth(true) {
                    truth(false)
                } else if equal == truth(false) {
                    truth(true)
                } else {
                    equal
                }
            } else {
                equal
            });
        }
        if let (Some((lo1, hi1)), Some((lo2, hi2))) =
            (arithmetic::bounds(a)?, arithmetic::bounds(b)?)
        {
            let finite =
                |a: &Bound, b: &Bound| matches!((a, b), (Bound::Finite(_), Bound::Finite(_)));
            let (yes, no) = match op {
                ">" => (
                    finite(&lo1, &hi2) && lo1 > hi2,
                    finite(&hi1, &lo2) && lo2 >= hi1,
                ),
                ">=" => (
                    finite(&lo1, &hi2) && lo1 >= hi2,
                    finite(&hi1, &lo2) && lo2 > hi1,
                ),
                "<" => (
                    finite(&hi1, &lo2) && hi1 < lo2,
                    finite(&lo1, &hi2) && hi2 <= lo1,
                ),
                "=<" => (
                    finite(&hi1, &lo2) && hi1 <= lo2,
                    finite(&lo1, &hi2) && hi2 < lo1,
                ),
                _ => unreachable!(),
            };
            return Ok(if yes {
                truth(true)
            } else if no {
                truth(false)
            } else {
                boolean()
            });
        }
        if !a.meet(b).map_err(NativeTransferError::from)?.is_none() {
            return Ok(boolean());
        }
        let (left, right) = if matches!(op, ">" | ">=") {
            (type_ranks(b)?, type_ranks(a)?)
        } else {
            (type_ranks(a)?, type_ranks(b)?)
        };
        if left.1 < right.0 {
            truth(true)
        } else if left.0 > right.1 {
            truth(false)
        } else {
            boolean()
        }
    })
}
pub fn class(name: &str) -> Result<Type, NativeTransferError> {
    Ok(match name {
        "is_map" => Type::map_any(),
        "is_atom" => Type::all_atoms(),
        "is_float" => Type::float(),
        "is_integer" => Type::all_integers(),
        "is_number" => Type::number(),
        "is_boolean" => boolean(),
        "is_binary" => Type::binary(),
        "is_bitstring" => Type::bitstring(1.into(), 0.into()).expect("nonnegative bitstring class"),
        "is_function" => Type::function(None, Type::any()),
        "is_list" => Type::list(Type::any(), Type::any(), false),
        "is_tuple" => Type::tuple_any(),
        "is_pid" => Type::identifier(Some(vec!["pid".into()]))?,
        "is_port" => Type::identifier(Some(vec!["port".into()]))?,
        "is_reference" => Type::identifier(Some(vec!["reference".into()]))?,
        other => return Err(format!("predicate domain not available {other}").into()),
    })
}
pub fn function_arity(fun: &Type, arity: &Type) -> Result<Type, NativeTransferError> {
    Ok({
        if let Some(values) = arithmetic::values(arity)?
            && values.len() == 1
            && let Some(n) = values[0].to_usize()
        {
            return test(
                fun,
                &Type::function(Some(vec![Type::any(); n]), Type::any()),
            );
        }
        boolean()
    })
}

pub fn ordering_refinement(
    op: &str,
    left: &Type,
    right: &Type,
    truth: bool,
) -> Result<Option<Vec<Type>>, NativeTransferError> {
    let (Some((lo1, hi1)), Some((lo2, hi2))) =
        (arithmetic::bounds(left)?, arithmetic::bounds(right)?)
    else {
        return Ok(None);
    };
    let op = if truth {
        op
    } else {
        match op {
            "<" => ">=",
            "=<" => ">",
            ">" => "=<",
            ">=" => "<",
            _ => return Ok(None),
        }
    };
    let successor = |x: Bound| match x {
        Bound::Finite(n) => Bound::Finite(n + 1),
        x => x,
    };
    let predecessor = |x: Bound| match x {
        Bound::Finite(n) => Bound::Finite(n - 1),
        x => x,
    };
    let (a, b) = match op {
        "<" => (
            arithmetic::range(lo1.clone(), hi1.min(predecessor(hi2.clone())))?,
            arithmetic::range(lo2.max(successor(lo1.clone())), hi2)?,
        ),
        "=<" => (
            arithmetic::range(lo1.clone(), hi1.min(hi2.clone()))?,
            arithmetic::range(lo2.max(lo1), hi2)?,
        ),
        ">" => {
            return ordering_refinement("<", right, left, true).map(|v| {
                v.map(|mut v| {
                    v.reverse();
                    v
                })
            });
        }
        ">=" => {
            return ordering_refinement("=<", right, left, true).map(|v| {
                v.map(|mut v| {
                    v.reverse();
                    v
                })
            });
        }
        _ => return Ok(None),
    };
    Ok(Some(vec![
        left.meet(&a).map_err(NativeTransferError::from)?,
        right.meet(&b).map_err(NativeTransferError::from)?,
    ]))
}
