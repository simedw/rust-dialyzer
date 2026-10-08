use crate::{
    contract_runtime::ContractSummary,
    lattice::{LatticeError, ListView, OpacityConflict, Type, TypeView},
};
use serde::Serialize;
type Result<T> = std::result::Result<T, LatticeError>;
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Validation {
    Valid,
    Overlapping,
    Invalid {
        arguments: Option<Vec<usize>>,
        return_invalid: Option<bool>,
    },
    OpaqueMismatch {
        range: Type,
    },
    RangeWarnings {
        warnings: Vec<RangeWarning>,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RangeWarning {
    Extra { extra: Type, success: Type },
    Missing { missing: Type, contract: Type },
}
fn parts(value: &Type) -> Result<(Vec<Type>, Type)> {
    let value = value.shallow_structural()?;
    match value.view() {
        TypeView::Families(s) => s
            .functions
            .and_then(|f| f.domain.map(|d| (d.to_vec(), f.range.clone())))
            .ok_or(LatticeError::FunctionClause),
        _ => Err(LatticeError::FunctionClause),
    }
}
fn join(values: impl IntoIterator<Item = Type>) -> Result<Type> {
    values
        .into_iter()
        .try_fold(Type::none(), |acc, t| acc.join(&t))
}
fn invalid(signatures: &[Type], success: &Type) -> Result<Validation> {
    if signatures.len() != 1 {
        return Ok(Validation::Invalid {
            arguments: None,
            return_invalid: None,
        });
    }
    let (args, range) = parts(&signatures[0])?;
    let (success_args, success_range) = parts(success)?;
    if args.len() != success_args.len() {
        return Err(LatticeError::FunctionClause);
    }
    let mut bad = Vec::new();
    for (i, (arg, success)) in args.iter().zip(success_args).enumerate() {
        if arg.meet(&success)?.is_none() {
            bad.push(i + 1)
        }
    }
    let bad_range = range.meet(&success_range)?.is_impossible() != range.is_impossible();
    Ok(Validation::Invalid {
        arguments: Some(bad),
        return_invalid: Some(bad_range),
    })
}
fn list_elements(value: &Type) -> Result<Option<Type>> {
    let value = value.shallow_structural()?;
    if let TypeView::Families(s) = value.view()
        && let Some(ListView::List { elements, .. }) = s.list
    {
        return Ok(Some(elements.clone()));
    }
    Ok(None)
}
fn bad_extraneous(range: &Type, success: &Type) -> Result<bool> {
    let list = Type::proper_list(Type::any(), false);
    let a = range.meet(&list)?;
    let b = success.meet(&list)?;
    if let (Some(a), Some(b)) = (list_elements(&a)?, list_elements(&b)?)
        && a.meet(&b)?.is_none()
    {
        return Ok(true);
    }
    let a = range.meet(&Type::map_any())?;
    let b = success.meet(&Type::map_any())?;
    let empty = Type::map(vec![], Type::none(), Type::none())?;
    Ok(a != empty && b != empty && a.meet(&b)? == empty)
}
fn missing(signatures: &[Type], success_range: &Type) -> Result<Option<RangeWarning>> {
    let ranges = signatures
        .iter()
        .map(parts)
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .map(|(_, range)| range);
    let all = join(ranges)?;
    let mut extra = Vec::new();
    for item in success_range.elements()? {
        if item.meet(&all)?.is_none() {
            extra.push(item)
        }
    }
    Ok(if extra.is_empty() {
        None
    } else {
        Some(RangeWarning::Missing {
            missing: join(extra)?,
            contract: all,
        })
    })
}
pub fn validate(summary: &ContractSummary, success: &Type, module: &str) -> Result<Validation> {
    let signatures = summary
        .alternatives
        .iter()
        .map(|a| a.signature.substitute(&a.constraints))
        .collect::<Result<Vec<_>>>()?;
    let mut domains = Vec::new();
    for signature in &signatures {
        domains.push(parts(signature)?.0)
    }
    for (i, a) in domains.iter().enumerate() {
        for b in &domains[i + 1..] {
            if a.len() != b.len() {
                return Err(LatticeError::FunctionClause);
            }
            let mut disjoint = false;
            for (x, y) in a.iter().zip(b) {
                disjoint |= x.meet(y)?.is_impossible();
            }
            if !disjoint {
                return Ok(Validation::Overlapping);
            }
        }
    }
    let (_, success_range) = parts(success)?;
    let mut valid = false;
    for signature in &signatures {
        let (_, range) = parts(signature)?;
        if success_range.opacity_conflict(&range, module)? != OpacityConflict::None {
            return Ok(Validation::OpaqueMismatch { range });
        }
        let inf = signature.meet(success)?;
        if !valid && !inf.is_impossible() {
            let (args, inf_range) = parts(&inf)?;
            valid = args.iter().all(|arg| !arg.is_none())
                && range.is_impossible() == inf_range.is_impossible();
        }
    }
    if !valid {
        return invalid(&signatures, success);
    }
    for signature in &signatures {
        let (_, range) = parts(signature)?;
        let mut extra = Vec::new();
        for item in range.elements()? {
            if item.meet(&success_range)?.is_none() {
                extra.push(item)
            }
        }
        if !extra.is_empty() {
            let mut warnings = vec![RangeWarning::Extra {
                extra: join(extra)?,
                success: success_range.clone(),
            }];
            if let Some(missing) = missing(&signatures, &success_range)? {
                warnings.push(missing)
            }
            return Ok(Validation::RangeWarnings { warnings });
        }
        if bad_extraneous(&range, &success_range)? {
            return Ok(Validation::Invalid {
                arguments: Some(Vec::new()),
                return_invalid: Some(true),
            });
        }
    }
    Ok(
        if let Some(missing) = missing(&signatures, &success_range)? {
            Validation::RangeWarnings {
                warnings: vec![missing],
            }
        } else {
            Validation::Valid
        },
    )
}
