//! OTP io_lib:write1 unlimited-depth finite-term iolist semantics used by Forms.
use crate::{
    core::{ErlangTerm, TermId},
    form_error::ReasonBuilder,
    lattice::PresentationContext,
    native_form_environment::RawFormTerm,
    term_order::TermOrderArena,
};
use num_bigint::BigInt;
use std::collections::BTreeMap;
#[derive(Clone, Copy)]
pub enum Encoding {
    Latin1,
    Unicode,
}
pub fn write(raw: &RawFormTerm, encoding: Encoding) -> Result<RawFormTerm, String> {
    write_with_map_orders(raw, encoding, &BTreeMap::new())
}
pub fn write_with_map_orders(
    raw: &RawFormTerm,
    encoding: Encoding,
    orders: &BTreeMap<TermId, Vec<usize>>,
) -> Result<RawFormTerm, String> {
    let arena = &raw.arena;
    let normal = TermOrderArena::new(arena)?;
    let mut out = ReasonBuilder::new();
    let context = PresentationContext::default();
    let mut written = vec![None; arena.terms.len()];
    let mut tails = vec![None; arena.terms.len()];
    let mut todo = vec![(raw.id, false)];
    while let Some((id, ready)) = todo.pop() {
        if written[id.0].is_some() {
            continue;
        }
        if !ready {
            todo.push((id, true));
            let children = match &arena.terms[id.0] {
                ErlangTerm::Tuple(xs) => xs.clone(),
                ErlangTerm::List { items, tail } => items.iter().copied().chain([*tail]).collect(),
                ErlangTerm::Map(entries) => entries.iter().flat_map(|(k, v)| [*k, *v]).collect(),
                _ => Vec::new(),
            };
            for child in children.into_iter().rev() {
                if written[child.0].is_none() {
                    todo.push((child, false))
                }
            }
            continue;
        }
        let doc = match &arena.terms[id.0] {
            ErlangTerm::ExternalFun(external_fun_payload) => {
                let crate::core::ExternalFun {
                    module,
                    function,
                    arity,
                } = external_fun_payload.as_ref();

                let atom = |name: &str| match encoding {
                    Encoding::Latin1 => context.atom_latin1(name),
                    Encoding::Unicode => context.atom_to_string(name),
                };
                out.string(&format!(
                    "fun {}:{}/{}",
                    atom(module),
                    atom(function),
                    arity
                ))
            }
            ErlangTerm::Nil => out.string("[]"),
            ErlangTerm::Atom(a) => out.string(&match encoding {
                Encoding::Latin1 => context.atom_latin1(a),
                Encoding::Unicode => context.atom_to_string(a),
            }),
            ErlangTerm::Integer(s) => out.string(
                &s.parse::<BigInt>()
                    .map_err(|_| "invalid integer in raw form")?
                    .to_string(),
            ),
            ErlangTerm::Float(bits) => out.string(&crate::presentation_form::native_float(
                f64::from_bits(*bits),
            )),
            ErlangTerm::Tuple(xs) if xs.is_empty() => out.string("{}"),
            ErlangTerm::Tuple(xs) => {
                let mut body = Vec::new();
                for (i, child) in xs.iter().enumerate() {
                    if i > 0 {
                        body.push(out.integer(44))
                    }
                    body.push(written[child.0].unwrap())
                }
                let body = out.list(body);
                let open = out.integer(123);
                let close = out.integer(125);
                out.list(vec![open, body, close])
            }
            ErlangTerm::List { items, tail } => {
                if items.is_empty() {
                    written[tail.0].unwrap()
                } else {
                    let mut body = Vec::new();
                    for (i, child) in items.iter().enumerate() {
                        if i > 0 {
                            body.push(out.integer(44))
                        }
                        body.push(written[child.0].unwrap())
                    }
                    let body = out.list_tail(body, tails[tail.0].unwrap());
                    let open = out.integer(91);
                    let close = out.integer(93);
                    out.list(vec![open, body, close])
                }
            }
            ErlangTerm::Binary(bytes) => bitstring(&mut out, bytes, bytes.len() as u64 * 8),
            ErlangTerm::Bitstring { bytes, bits } => bitstring(&mut out, bytes, *bits),
            ErlangTerm::Map(entries) => {
                let canonical = normal.canonical_map_pairs(entries)?;
                let pairs = if canonical.len() <= 1 {
                    canonical
                } else {
                    let order = orders
                        .get(&id)
                        .ok_or("native io_lib default map iterator provenance is missing")?;
                    if canonical.len() != entries.len()
                        || order.len() != entries.len()
                        || order
                            .iter()
                            .copied()
                            .collect::<std::collections::BTreeSet<_>>()
                            != (0..entries.len()).collect()
                    {
                        return Err(
                            "native map iterator order is not a permutation of distinct entries"
                                .into(),
                        );
                    }
                    order.iter().map(|i| entries[*i]).collect()
                };
                if pairs.is_empty() {
                    out.string("#{}")
                } else {
                    let mut body = Vec::new();
                    for (i, (k, v)) in pairs.iter().enumerate() {
                        if i > 0 {
                            body.push(out.integer(44))
                        }
                        let sep = out.string(" => ");
                        let pair =
                            out.list(vec![written[k.0].unwrap(), sep, written[v.0].unwrap()]);
                        body.push(pair)
                    }
                    let body = out.list(body);
                    let hash = out.integer(35);
                    let open = out.integer(123);
                    let close = out.integer(125);
                    out.list(vec![hash, open, body, close])
                }
            }
        };
        written[id.0] = Some(doc);
        tails[id.0] = Some(match &arena.terms[id.0] {
            ErlangTerm::Nil => out.nil(),
            ErlangTerm::List { items, tail } => {
                let mut parts = Vec::new();
                for child in items {
                    parts.push(out.integer(44));
                    parts.push(written[child.0].unwrap())
                }
                out.list_tail(parts, tails[tail.0].unwrap())
            }
            _ => {
                let bar = out.integer(124);
                out.list(vec![bar, doc])
            }
        });
    }
    Ok(out.finish(written[raw.id.0].unwrap()))
}
fn bitstring(out: &mut ReasonBuilder, bytes: &[u8], bits: u64) -> TermId {
    let whole = (bits / 8) as usize;
    let rem = (bits % 8) as u8;
    let mut body = Vec::new();
    for b in bytes.iter().take(whole) {
        if !body.is_empty() {
            body.push(out.integer(44))
        }
        body.push(out.string(&b.to_string()))
    }
    if rem > 0 {
        if !body.is_empty() {
            body.push(out.integer(44))
        }
        body.push(out.string(&(bytes[whole] >> (8 - rem)).to_string()));
        body.push(out.integer(58));
        body.push(out.string(&rem.to_string()))
    }
    let body = out.list(body);
    let open = out.integer(60);
    let close = out.integer(62);
    out.list(vec![open, open, body, close, close])
}

/// Unlimited default-field io_lib formatting controls used by native Forms errors.
/// Unsupported controls remain explicit; canonical type rendering is separate.
pub fn format(
    template: &str,
    arguments: &[RawFormTerm],
    orders: &BTreeMap<TermId, Vec<usize>>,
) -> Result<RawFormTerm, crate::form_error::FormError> {
    use crate::{form_error::FormError, native_error::NativeTransferError};
    let badarg = || FormError::Native(NativeTransferError::Badarg);
    let mut output = ReasonBuilder::new();
    let mut items = Vec::new();
    let mut input = template.chars();
    let mut index = 0;
    while let Some(c) = input.next() {
        if c != '~' {
            items.push(output.integer(c as u32));
            continue;
        }
        let mut unicode = false;
        let mut control = input.next().ok_or_else(badarg)?;
        if control == 't' {
            unicode = true;
            control = input.next().ok_or_else(badarg)?;
        }
        if control == 'n' {
            items.push(output.integer(10));
            continue;
        }
        if control == '~' {
            items.push(output.integer(126));
            continue;
        }
        let arg = arguments.get(index).ok_or_else(badarg)?;
        index += 1;
        match control {
            'w' => {
                let doc = write_with_map_orders(
                    arg,
                    if unicode {
                        Encoding::Unicode
                    } else {
                        Encoding::Latin1
                    },
                    orders,
                )
                .map_err(FormError::from)?;
                items.push(output.copy(&doc));
            }
            's' => {
                if let ErlangTerm::Atom(name) = arg.term() {
                    if !unicode && name.chars().any(|c| c as u32 > 255) {
                        return Err(badarg());
                    }
                    // control_small expands atom strings directly into the format stream.
                    items.extend(name.chars().map(|c| output.integer(c as u32)));
                } else {
                    if !matches!(
                        arg.term(),
                        ErlangTerm::Nil | ErlangTerm::List { .. } | ErlangTerm::Binary(_)
                    ) {
                        return Err(badarg());
                    }
                    let doc = string_data(arg, unicode)?;
                    items.push(output.copy(&doc));
                }
            }
            _ => {
                return Err(FormError::Transport(format!(
                    "native formatting control ~{control} is unimplemented"
                )));
            }
        }
    }
    if index != arguments.len() {
        return Err(badarg());
    }
    let root = output.list(items);
    Ok(output.finish(root))
}
fn string_data(
    raw: &RawFormTerm,
    unicode: bool,
) -> Result<RawFormTerm, crate::form_error::FormError> {
    use crate::{form_error::FormError, native_error::NativeTransferError};
    let mut output = ReasonBuilder::new();
    let mut converted = BTreeMap::new();
    let mut work = vec![(raw.id(), false)];
    while let Some((id, ready)) = work.pop() {
        if converted.contains_key(&id) {
            continue;
        }
        let term = &raw.arena().terms[id.0];
        if !ready {
            work.push((id, true));
            if let ErlangTerm::List { items, tail } = term {
                if !matches!(
                    raw.arena().terms[tail.0],
                    ErlangTerm::Nil | ErlangTerm::List { .. } | ErlangTerm::Binary(_)
                ) {
                    return Err(FormError::Native(NativeTransferError::Badarg));
                }
                work.push((*tail, false));
                work.extend(items.iter().rev().map(|id| (*id, false)));
            }
            continue;
        }
        let doc = match term {
            ErlangTerm::Nil => output.nil(),
            ErlangTerm::Integer(n)
                if n.parse::<BigInt>()
                    .is_ok_and(|n| n >= BigInt::from(0) && (unicode || n <= BigInt::from(255))) =>
            {
                output.integer(n)
            }
            ErlangTerm::List { items, tail } => output.list_tail(
                items.iter().map(|id| converted[id]).collect(),
                converted[tail],
            ),
            ErlangTerm::Binary(bytes) => {
                let chars: Vec<u32> = if unicode {
                    std::str::from_utf8(bytes)
                        .map(|s| s.chars().map(|c| c as u32).collect())
                        .unwrap_or_else(|_| bytes.iter().map(|b| *b as u32).collect())
                } else {
                    bytes.iter().map(|b| *b as u32).collect()
                };
                let chars = chars.into_iter().map(|c| output.integer(c)).collect();
                output.list(chars)
            }
            _ => return Err(FormError::Native(NativeTransferError::Badarg)),
        };
        converted.insert(id, doc);
    }
    Ok(output.finish(converted[&raw.id()]))
}
