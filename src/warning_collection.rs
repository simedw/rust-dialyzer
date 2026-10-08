use crate::{
    core::{ErlangTerm, ErlangTerms, TermId},
    program::FunctionName,
    term_order::TermOrderArena,
};
use std::{cmp::Ordering, collections::BTreeMap};
#[derive(Clone, Copy)]
pub struct ContractOrigin {
    pub file: TermId,
    pub location: TermId,
}
pub type ContractOrigins = BTreeMap<FunctionName, ContractOrigin>;
#[derive(Debug)]
pub enum PostprocessError {
    Malformed(&'static str),
    TermOrder(String),
}
type Result<T> = std::result::Result<T, PostprocessError>;
fn tuple<const N: usize>(arena: &ErlangTerms, id: TermId) -> Result<[TermId; N]> {
    let Some(ErlangTerm::Tuple(items)) = arena.terms.get(id.0) else {
        return Err(PostprocessError::Malformed("expected warning tuple"));
    };
    items
        .as_slice()
        .try_into()
        .map_err(|_| PostprocessError::Malformed("warning tuple arity"))
}
fn atom(arena: &ErlangTerms, id: TermId) -> Result<&str> {
    match arena.terms.get(id.0) {
        Some(ErlangTerm::Atom(s)) => Ok(s),
        _ => Err(PostprocessError::Malformed("expected atom")),
    }
}
fn arguments(arena: &ErlangTerms, mut id: TermId) -> Result<[TermId; 6]> {
    let mut output = Vec::new();
    loop {
        match arena.terms.get(id.0) {
            Some(ErlangTerm::Nil) => break,
            Some(ErlangTerm::List { items, tail }) => {
                if output.len() + items.len() > 6 {
                    return Err(PostprocessError::Malformed("contract range arguments"));
                }
                output.extend(items);
                id = *tail;
            }
            _ => return Err(PostprocessError::Malformed("contract range arguments")),
        }
    }
    output
        .as_slice()
        .try_into()
        .map_err(|_| PostprocessError::Malformed("contract range arguments"))
}
struct RangeWarning {
    fields: [TermId; 3],
    info: [TermId; 3],
    message_tag: TermId,
    args: [TermId; 6],
    origin: Option<ContractOrigin>,
}
fn push(arena: &mut ErlangTerms, term: ErlangTerm) -> TermId {
    let id = TermId(arena.terms.len());
    arena.terms.push(term);
    id
}

pub fn postprocess(
    arena: &mut ErlangTerms,
    warnings: &[TermId],
    contracts: &ContractOrigins,
) -> Result<Vec<TermId>> {
    let order = TermOrderArena::new(arena).map_err(PostprocessError::TermOrder)?;
    let equal = |a, b| {
        order
            .compare_map_keys(a, &order, b)
            .map(|o| o == Ordering::Equal)
            .map_err(PostprocessError::TermOrder)
    };
    let mut ranges = Vec::new();
    let mut others = Vec::new();
    for id in warnings {
        let fields = tuple::<3>(arena, *id)?;
        if !matches!(arena.terms.get(fields[0].0), Some(ErlangTerm::Atom(tag)) if tag == "warn_contract_range")
        {
            others.push(*id);
            continue;
        }
        let info = tuple::<3>(arena, fields[1])?;
        let message = tuple::<2>(arena, fields[2])?;
        if atom(arena, message[0])? != "contract_range" {
            return Err(PostprocessError::Malformed("contract range message"));
        }
        let args = arguments(arena, message[1])?;
        let origin = match (
            &arena.terms[args[1].0],
            &arena.terms[args[2].0],
            &arena.terms[args[3].0],
        ) {
            (ErlangTerm::Atom(module), ErlangTerm::Atom(function), ErlangTerm::Integer(arity)) => {
                arity.parse().ok().and_then(|arity| {
                    contracts
                        .get(&FunctionName {
                            module: module.clone(),
                            name: function.clone(),
                            arity,
                        })
                        .copied()
                })
            }
            _ => None,
        };
        ranges.push(RangeWarning {
            fields,
            info,
            message_tag: message[0],
            args,
            origin,
        });
    }
    let mut rewritten = Vec::new();
    for range in ranges {
        if let Some(origin) = range.origin {
            if !equal(range.info[0], origin.file)? {
                continue;
            }
            let mut retained = Vec::with_capacity(others.len());
            for id in others {
                let fields = tuple::<3>(arena, id)?;
                let discard = if matches!(arena.terms.get(fields[0].0), Some(ErlangTerm::Atom(tag)) if tag == "warn_contract_types")
                {
                    let info = tuple::<3>(arena, fields[1])?;
                    if equal(info[0], origin.file)? && equal(info[1], origin.location)? {
                        match arena.terms.get(info[2].0) {
                            Some(ErlangTerm::Tuple(mfa)) if mfa.len() == 3 => {
                                equal(mfa[0], range.args[1])?
                                    && equal(mfa[1], range.args[2])?
                                    && equal(mfa[2], range.args[3])?
                            }
                            _ => false,
                        }
                    } else {
                        false
                    }
                } else {
                    false
                };
                if !discard {
                    retained.push(id);
                }
            }
            others = retained;
        }
        let nil = push(arena, ErlangTerm::Nil);
        let args = push(
            arena,
            ErlangTerm::List {
                items: vec![
                    range.args[0],
                    range.args[1],
                    range.args[2],
                    range.args[4],
                    range.info[1],
                    range.args[5],
                ],
                tail: nil,
            },
        );
        let msg = push(arena, ErlangTerm::Tuple(vec![range.message_tag, args]));
        let info = if let Some(origin) = range.origin {
            let mfa = push(
                arena,
                ErlangTerm::Tuple(vec![range.args[1], range.args[2], range.args[3]]),
            );
            push(
                arena,
                ErlangTerm::Tuple(vec![origin.file, origin.location, mfa]),
            )
        } else {
            range.fields[1]
        };
        rewritten.push(push(
            arena,
            ErlangTerm::Tuple(vec![range.fields[0], info, msg]),
        ));
    }
    rewritten.extend(others);
    Ok(rewritten)
}
