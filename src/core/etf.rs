//! Reader for the per-module Core cache written by `prototypes/core_cache/core_cache.exs`.
//!
//! The VM writes one `term_to_binary/1` record per module:
//!
//! ```text
//! {core_module_v1, Name :: binary(), SourceForms :: term id, Core :: node id,
//!  ContractVariableMaps :: ids, Terms :: [term entry], Nodes :: [node entry]}
//! ```
//!
//! `ids` are packed big-endian u32 binaries. Term entries are `{atom, A}`,
//! `{integer, I}`, `{float, <<Bits:64>>}`, `{binary, B}`, `{bitstring, B}`, `nil`,
//! `{tuple, Ids}`, `{list, Ids, Tail}` and `{map, KeyValueIds, IteratorOrder}`.
//! Node entries are `{Kind, Annotations, [Ids], Value, MapArg, MapPattern}`, with
//! `undefined` for absent fields. The reader builds `CoreModule` directly; the
//! caller validates it through `CoreProgram::from_modules`.

use super::{CoreModule, CoreNode, ErlangTerm, NodeId, RawNode, Result, TermId, build_node};
use num_bigint::{BigInt, Sign};
use std::collections::BTreeMap;

/// The subset of the external term format the cache uses.
enum Term<'a> {
    Int(i64),
    Big(BigInt),
    Atom(&'a str),
    Binary(&'a [u8]),
    Bits(&'a [u8], u8),
    Tuple(Vec<Term<'a>>),
    List(Vec<Term<'a>>),
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.at.checked_add(n).filter(|&end| end <= self.bytes.len());
        let end = end.ok_or("Core cache: truncated term")?;
        let slice = &self.bytes[self.at..end];
        self.at = end;
        Ok(slice)
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<usize> {
        Ok(u16::from_be_bytes(self.take(2)?.try_into().unwrap()) as usize)
    }
    fn u32(&mut self) -> Result<usize> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into().unwrap()) as usize)
    }
    fn atom(&mut self, len: usize) -> Result<Term<'a>> {
        let raw = self.take(len)?;
        std::str::from_utf8(raw)
            .map(Term::Atom)
            .map_err(|_| "Core cache: atom is not UTF-8".into())
    }
    fn big(&mut self, len: usize) -> Result<Term<'a>> {
        let sign = if self.u8()? == 0 { Sign::Plus } else { Sign::Minus };
        Ok(Term::Big(BigInt::from_bytes_le(sign, self.take(len)?)))
    }
    fn elements(&mut self, n: usize) -> Result<Vec<Term<'a>>> {
        (0..n).map(|_| self.term()).collect()
    }
    fn term(&mut self) -> Result<Term<'a>> {
        Ok(match self.u8()? {
            97 => Term::Int(self.u8()? as i64),
            98 => Term::Int(i32::from_be_bytes(self.take(4)?.try_into().unwrap()) as i64),
            110 => {
                let n = self.u8()? as usize;
                self.big(n)?
            }
            111 => {
                let n = self.u32()?;
                self.big(n)?
            }
            118 | 100 => {
                let n = self.u16()?;
                self.atom(n)?
            }
            119 | 115 => {
                let n = self.u8()? as usize;
                self.atom(n)?
            }
            104 => {
                let n = self.u8()? as usize;
                Term::Tuple(self.elements(n)?)
            }
            105 => {
                let n = self.u32()?;
                Term::Tuple(self.elements(n)?)
            }
            106 => Term::List(Vec::new()),
            107 => {
                let n = self.u16()?;
                Term::List(self.take(n)?.iter().map(|&b| Term::Int(b as i64)).collect())
            }
            108 => {
                let n = self.u32()?;
                let items = self.elements(n)?;
                match self.term()? {
                    Term::List(tail) if tail.is_empty() => Term::List(items),
                    _ => return Err("Core cache: improper list".into()),
                }
            }
            109 => {
                let n = self.u32()?;
                Term::Binary(self.take(n)?)
            }
            77 => {
                let n = self.u32()?;
                let bits = self.u8()?;
                Term::Bits(self.take(n)?, bits)
            }
            tag => return Err(format!("Core cache: unsupported term tag {tag}")),
        })
    }
}

fn int(t: &Term) -> Result<u64> {
    match t {
        Term::Int(n) if *n >= 0 => Ok(*n as u64),
        _ => Err("Core cache: expected nonnegative integer".into()),
    }
}

fn ids(t: &Term) -> Result<Vec<u64>> {
    let Term::Binary(raw) = t else {
        return Err("Core cache: expected packed ids".into());
    };
    if raw.len() % 4 != 0 {
        return Err("Core cache: packed ids not a multiple of 4 bytes".into());
    }
    Ok(raw
        .chunks_exact(4)
        .map(|c| u32::from_be_bytes(c.try_into().unwrap()) as u64)
        .collect())
}

fn term_ids(t: &Term, before: usize) -> Result<Vec<TermId>> {
    ids(t)?
        .into_iter()
        .map(|id| {
            usize::try_from(id)
                .ok()
                .filter(|&id| id < before)
                .map(TermId)
                .ok_or_else(|| format!("Core cache: term {before} refers to later id {id}"))
        })
        .collect()
}

fn optional(t: &Term) -> Result<Option<u64>> {
    match t {
        Term::Atom("undefined") => Ok(None),
        t => int(t).map(Some),
    }
}

fn erlang_term(entry: &Term, id: usize, orders: &mut BTreeMap<TermId, Vec<usize>>) -> Result<ErlangTerm> {
    let bad = || format!("Core cache: malformed term entry {id}");
    Ok(match entry {
        Term::Atom("nil") => ErlangTerm::Nil,
        Term::Tuple(fields) => match fields.as_slice() {
            [Term::Atom("atom"), Term::Atom(a)] => ErlangTerm::Atom((*a).to_owned()),
            [Term::Atom("integer"), Term::Int(n)] => ErlangTerm::Integer(n.to_string()),
            [Term::Atom("integer"), Term::Big(n)] => ErlangTerm::Integer(n.to_string()),
            [Term::Atom("float"), Term::Binary(b)] if b.len() == 8 => {
                ErlangTerm::Float(u64::from_be_bytes((*b).try_into().unwrap()))
            }
            [Term::Atom("binary"), Term::Binary(b)] => ErlangTerm::Binary(b.to_vec()),
            [Term::Atom("bitstring"), Term::Bits(b, last)] => ErlangTerm::Bitstring {
                bytes: b.to_vec(),
                bits: (b.len() as u64).saturating_sub(1) * 8 + *last as u64,
            },
            [Term::Atom("bitstring"), Term::Binary(b)] => ErlangTerm::Bitstring {
                bytes: b.to_vec(),
                bits: b.len() as u64 * 8,
            },
            [Term::Atom("tuple"), items] => ErlangTerm::Tuple(term_ids(items, id)?),
            [Term::Atom("list"), items, tail] => ErlangTerm::List {
                items: term_ids(items, id)?,
                tail: TermId(
                    usize::try_from(int(tail)?)
                        .ok()
                        .filter(|&t| t < id)
                        .ok_or_else(bad)?,
                ),
            },
            [Term::Atom("map"), pairs, order] => {
                let flat = term_ids(pairs, id)?;
                if flat.len() % 2 != 0 {
                    return Err(bad());
                }
                let order = ids(order)?.into_iter().map(|i| i as usize).collect();
                orders.insert(TermId(id), order);
                ErlangTerm::Map(flat.chunks_exact(2).map(|kv| (kv[0], kv[1])).collect())
            }
            _ => return Err(bad()),
        },
        _ => return Err(bad()),
    })
}

/// Decodes one cached module. Structural validation happens in `build_node` and
/// later in `CoreProgram::from_modules`.
pub fn module_from_etf(bytes: &[u8]) -> Result<CoreModule> {
    let mut reader = Reader { bytes, at: 0 };
    if reader.u8()? != 131 {
        return Err("Core cache: missing external term version byte".into());
    }
    let root = reader.term()?;
    if reader.at != bytes.len() {
        return Err("Core cache: trailing bytes".into());
    }
    let Term::Tuple(fields) = root else {
        return Err("Core cache: expected module tuple".into());
    };
    let [Term::Atom("core_module_v1"), Term::Binary(name), source_forms, core, maps, Term::List(raw_terms), Term::List(raw_nodes)] =
        fields.as_slice()
    else {
        return Err("Core cache: expected core_module_v1 record".into());
    };
    let name = std::str::from_utf8(name)
        .map_err(|_| "Core cache: module name is not UTF-8")?
        .to_owned();
    let path = format!("$.modules[{name}]");

    let mut map_iterator_orders = BTreeMap::new();
    let mut terms = Vec::with_capacity(raw_terms.len());
    for (id, entry) in raw_terms.iter().enumerate() {
        terms.push(erlang_term(entry, id, &mut map_iterator_orders)?);
    }

    let mut nodes: Vec<CoreNode> = Vec::with_capacity(raw_nodes.len());
    for (id, entry) in raw_nodes.iter().enumerate() {
        let Term::Tuple(f) = entry else {
            return Err(format!("{path}.nodes[{id}]: expected node tuple"));
        };
        let [Term::Atom(kind), annotations, Term::List(groups), value, map_arg, map_pattern] = f.as_slice() else {
            return Err(format!("{path}.nodes[{id}]: expected 6-field node"));
        };
        let raw = RawNode {
            kind,
            annotations: int(annotations)?,
            groups: groups.iter().map(ids).collect::<Result<_>>()?,
            value: optional(value)?,
            map_arg: optional(map_arg)?,
            map_pattern: match map_pattern {
                Term::Atom("undefined") => None,
                Term::Atom("true") => Some(true),
                Term::Atom("false") => Some(false),
                _ => return Err(format!("{path}.nodes[{id}].map_pattern: expected boolean")),
            },
        };
        let node = build_node(&raw, format_args!("{path}.nodes[{id}]"), id, &nodes, &terms)?;
        nodes.push(node);
    }

    let pairs = ids(maps)?;
    if pairs.len() % 2 != 0 {
        return Err(format!("{path}.contract_variable_maps: odd length"));
    }
    let contract_variable_maps = pairs
        .chunks_exact(2)
        .map(|p| (TermId(p[0] as usize), TermId(p[1] as usize)))
        .collect();

    Ok(CoreModule {
        name,
        source_forms: TermId(int(source_forms)? as usize),
        core: NodeId(int(core)? as usize),
        terms,
        nodes,
        map_iterator_orders,
        contract_variable_maps,
    })
}
