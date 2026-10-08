use crate::{
    core::{ErlangTerm, TermId},
    lattice::NativeValue,
    resolver::{FormCache, Unfold},
    type_exports::arena_to_value,
};
use num_bigint::BigInt;
use std::collections::{BTreeMap, BTreeSet};

pub fn cache_to_value(cache: &FormCache) -> Result<serde_json::Value, String> {
    if !cache.interop_complete() {
        return Err("cache was created by a resolver without original metadata handles".into());
    }
    let mut out = Vec::new();
    let mut entries = Vec::new();
    for entry in cache.entries() {
        let names = entry
            .enclosing
            .iter()
            .map(|scope| match scope {
                Unfold::Named(n) => Ok(NativeValue::Tuple(vec![
                    atom("type"),
                    NativeValue::Tuple(vec![atom(&n.module), atom(&n.name), int(n.arity)]),
                ])),
                Unfold::Record(name) => Ok(NativeValue::Tuple(vec![atom("record"), atom(name)])),
            })
            .collect::<Result<Vec<_>, String>>()?;
        let key = NativeValue::Tuple(vec![
            atom(&entry.name.module),
            atom(&entry.name.name),
            NativeValue::Integer(entry.depth.into()),
            list(
                entry
                    .arguments
                    .iter()
                    .map(|t| t.to_native_value())
                    .collect(),
            ),
            list(names),
        ]);
        let value = NativeValue::Tuple(vec![
            entry.value.to_native_value(),
            NativeValue::Integer(entry.cost.into()),
        ]);
        entries.push((native(&mut out, &key), native(&mut out, &value)));
    }
    let types = push(&mut out, ErlangTerm::Map(entries));
    let mut entries = Vec::new();
    for (name, handle) in cache.cached_tables() {
        let arena = handle.arena();
        let module = native(&mut out, &atom(name));
        let table = copy_arena(&mut out, &arena.terms, handle.root());
        entries.push((module, table));
    }
    let modules = push(&mut out, ErlangTerm::Map(entries));
    let tag = native(&mut out, &atom("cache"));
    let root = push(&mut out, ErlangTerm::Tuple(vec![tag, types, modules]));
    Ok(arena_to_value(&out, root))
}
fn atom(s: &str) -> NativeValue {
    NativeValue::Atom(s.into())
}
fn int(n: usize) -> NativeValue {
    NativeValue::Integer(BigInt::from(n))
}
fn list(v: Vec<NativeValue>) -> NativeValue {
    if v.is_empty() {
        NativeValue::Nil
    } else {
        NativeValue::List(v)
    }
}
pub(crate) fn push(out: &mut Vec<ErlangTerm>, term: ErlangTerm) -> TermId {
    let id = TermId(out.len());
    out.push(term);
    id
}
fn native(out: &mut Vec<ErlangTerm>, v: &NativeValue) -> TermId {
    let term = match v {
        NativeValue::Atom(v) => ErlangTerm::Atom(v.clone()),
        NativeValue::Integer(v) => ErlangTerm::Integer(v.to_string()),
        NativeValue::Nil => ErlangTerm::Nil,
        NativeValue::Tuple(v) => ErlangTerm::Tuple(v.iter().map(|v| native(out, v)).collect()),
        NativeValue::List(v) => {
            let items = v.iter().map(|v| native(out, v)).collect();
            let tail = push(out, ErlangTerm::Nil);
            ErlangTerm::List { items, tail }
        }
    };
    push(out, term)
}
pub(crate) fn copy_arena(out: &mut Vec<ErlangTerm>, terms: &[ErlangTerm], root: TermId) -> TermId {
    let mut reachable = BTreeSet::new();
    let mut work = vec![root];
    while let Some(id) = work.pop() {
        if !reachable.insert(id.0) {
            continue;
        }
        match &terms[id.0] {
            ErlangTerm::Tuple(v) => work.extend(v),
            ErlangTerm::List { items, tail } => {
                work.extend(items);
                work.push(*tail)
            }
            ErlangTerm::Map(v) => {
                for (k, v) in v {
                    work.extend([*k, *v]);
                }
            }
            _ => {}
        }
    }
    let mut ids = BTreeMap::new();
    for id in reachable {
        let at = |id: &TermId| ids[&id.0];
        let term = match &terms[id] {
            ErlangTerm::Tuple(v) => ErlangTerm::Tuple(v.iter().map(at).collect()),
            ErlangTerm::List { items, tail } => ErlangTerm::List {
                items: items.iter().map(at).collect(),
                tail: at(tail),
            },
            ErlangTerm::Map(v) => ErlangTerm::Map(v.iter().map(|(k, v)| (at(k), at(v))).collect()),
            v => v.clone(),
        };
        ids.insert(id, push(out, term));
    }
    ids[&root.0]
}
