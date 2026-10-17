//! Source-defined dialyzer_utils:massage_type without changing the compiler arena.
use crate::{
    core::{ErlangTerm, ErlangTerms, TermId},
    form_error::ReasonBuilder,
    native_form_environment::RawFormTerm,
};
use std::collections::{BTreeMap, BTreeSet};
fn tuple(a: &ErlangTerms, id: TermId) -> Option<&[TermId]> {
    match &a.terms[id.0] {
        ErlangTerm::Tuple(xs) => Some(xs),
        _ => None,
    }
}
fn atom(a: &ErlangTerms, id: TermId) -> Option<&str> {
    match &a.terms[id.0] {
        ErlangTerm::Atom(s) => Some(s),
        _ => None,
    }
}
fn list(a: &ErlangTerms, mut id: TermId) -> Option<Vec<TermId>> {
    let mut out = Vec::new();
    loop {
        match &a.terms[id.0] {
            ErlangTerm::Nil => return Some(out),
            ErlangTerm::List { items, tail } => {
                out.extend(items);
                id = *tail;
            }
            _ => return None,
        }
    }
}
fn compact_source(raw: &RawFormTerm) -> RawFormTerm {
    let mut seen = BTreeSet::new();
    let mut work = vec![raw.id()];
    while let Some(id) = work.pop() {
        if !seen.insert(id) {
            continue;
        }
        let mut visit = |child: TermId| {
            if child.0 >= id.0 {
                false
            } else {
                work.push(child);
                true
            }
        };
        let ordered = match &raw.arena().terms[id.0] {
            ErlangTerm::Tuple(xs) => xs.iter().all(|id| visit(*id)),
            ErlangTerm::List { items, tail } => items.iter().all(|id| visit(*id)) && visit(*tail),
            ErlangTerm::Map(xs) => xs.iter().all(|(k, v)| visit(*k) && visit(*v)),
            _ => true,
        };
        if !ordered {
            return raw.clone();
        }
    }
    if let Some(nil) = raw
        .arena()
        .terms
        .iter()
        .position(|t| matches!(t, ErlangTerm::Nil))
    {
        seen.insert(TermId(nil));
    }
    let mut ids = BTreeMap::new();
    let mut terms = Vec::with_capacity(seen.len());
    for id in seen {
        let at = |id: &TermId| ids[id];
        let term = match &raw.arena().terms[id.0] {
            ErlangTerm::Tuple(xs) => ErlangTerm::Tuple(xs.iter().map(at).collect()),
            ErlangTerm::List { items, tail } => ErlangTerm::List {
                items: items.iter().map(at).collect(),
                tail: at(tail),
            },
            ErlangTerm::Map(xs) => {
                ErlangTerm::Map(xs.iter().map(|(k, v)| (at(k), at(v))).collect())
            }
            t => t.clone(),
        };
        ids.insert(id, TermId(terms.len()));
        terms.push(term);
    }
    let root = ids[&raw.id()];
    RawFormTerm::new(std::sync::Arc::new(ErlangTerms { root, terms: terms.into() }), root)
        .expect("copied form root")
}
pub(crate) fn massage(raw: &RawFormTerm, definitions: &BTreeSet<(String, usize)>) -> RawFormTerm {
    enum Work {
        Visit(TermId),
        Generic(TermId, Vec<TermId>),
        Function(TermId, TermId, Vec<TermId>, TermId),
    }
    let source = compact_source(raw);
    let a = source.arena();
    let mut b = ReasonBuilder::from_terms(a.terms.clone());
    let mut rewritten = BTreeMap::<TermId, TermId>::new();
    let mut work = vec![Work::Visit(source.id())];
    while let Some(step) = work.pop() {
        match step {
            Work::Visit(id) => {
                if rewritten.contains_key(&id) {
                    continue;
                }
                let Some(t) =
                    tuple(a, id).filter(|t| t.len() == 4 && atom(a, t[0]) == Some("type"))
                else {
                    rewritten.insert(id, id);
                    continue;
                };
                let Some(args) = list(a, t[3]) else {
                    rewritten.insert(id, id);
                    continue;
                };
                if atom(a, t[2]) == Some("fun")
                    && args.len() == 2
                    && let Some(p) = tuple(a, args[0]).filter(|p| {
                        p.len() == 4
                            && atom(a, p[0]) == Some("type")
                            && atom(a, p[2]) == Some("product")
                    })
                    && let Some(elements) = list(a, p[3])
                {
                    work.push(Work::Function(id, args[0], elements.clone(), args[1]));
                    work.push(Work::Visit(args[1]));
                    work.extend(elements.into_iter().rev().map(Work::Visit));
                    continue;
                }
                if atom(a, t[2])
                    .is_some_and(|name| definitions.contains(&(name.into(), args.len())) && {
                        #[cfg(otp_domain = "29")] { is_builtin_type(name, args.len()) }
                        #[cfg(not(otp_domain = "29"))] { true }
                    })
                {
                    let user = b.atom("user_type");
                    let updated = b.tuple(vec![user, t[1], t[2], t[3]]);
                    rewritten.insert(id, updated);
                } else {
                    work.push(Work::Generic(id, args.clone()));
                    work.extend(args.into_iter().rev().map(Work::Visit));
                }
            }
            Work::Generic(id, args) => {
                let t = tuple(a, id).expect("type tuple");
                let args = b.list(args.into_iter().map(|child| rewritten[&child]).collect());
                rewritten.insert(id, b.tuple(vec![t[0], t[1], t[2], args]));
            }
            Work::Function(id, product, args, ret) => {
                let t = tuple(a, id).expect("function tuple");
                let p = tuple(a, product).expect("product tuple");
                let args = b.list(args.into_iter().map(|child| rewritten[&child]).collect());
                let product = b.tuple(vec![p[0], p[1], p[2], args]);
                let args = b.list(vec![product, rewritten[&ret]]);
                rewritten.insert(id, b.tuple(vec![t[0], t[1], t[2], args]));
            }
        }
    }
    b.finish(rewritten[&source.id()])
}

#[cfg(test)]
mod compact_source_tests {
    use super::*;
    use std::sync::Arc;
    fn raw(terms: Vec<ErlangTerm>, root: usize) -> RawFormTerm {
        RawFormTerm::new(
            Arc::new(ErlangTerms {
                root: TermId(root),
                terms: terms.into(),
            }),
            TermId(root),
        )
        .unwrap()
    }
    fn wire(raw: &RawFormTerm) -> serde_json::Value {
        crate::type_exports::arena_to_value(&raw.arena().terms, raw.id())
    }
    #[test]
    fn rewrites_keep_original_first_nil_sharing() {
        let form = raw(
            vec![
                ErlangTerm::Nil,
                ErlangTerm::Atom("type".into()),
                ErlangTerm::Integer("0".into()),
                ErlangTerm::Atom("list".into()),
                ErlangTerm::Nil,
                ErlangTerm::List {
                    items: vec![TermId(4)],
                    tail: TermId(4),
                },
                ErlangTerm::Tuple(vec![TermId(1), TermId(2), TermId(3), TermId(5)]),
            ],
            6,
        );
        assert_eq!(
            wire(&massage(&form, &BTreeSet::new())),
            serde_json::json!({"root":6,"terms":[
            {"kind":"nil"},{"kind":"atom","value":"type"},{"kind":"integer","value":"0"},
            {"kind":"atom","value":"list"},{"kind":"nil"},
            {"kind":"list","items":[4],"tail":0},{"kind":"tuple","items":[1,2,3,5]}]})
        );
    }
    #[test]
    fn forward_reference_sources_keep_existing_rewrite_behavior() {
        let form = raw(
            vec![
                ErlangTerm::Tuple(vec![TermId(1), TermId(2), TermId(3), TermId(4)]),
                ErlangTerm::Atom("type".into()),
                ErlangTerm::Integer("0".into()),
                ErlangTerm::Atom("list".into()),
                ErlangTerm::List {
                    items: vec![TermId(5)],
                    tail: TermId(6),
                },
                ErlangTerm::Atom("argument".into()),
                ErlangTerm::Nil,
            ],
            0,
        );
        assert_eq!(
            wire(&massage(&form, &BTreeSet::new())),
            serde_json::json!({"root":6,"terms":[
            {"kind":"atom","value":"type"},{"kind":"integer","value":"0"},
            {"kind":"atom","value":"list"},{"kind":"atom","value":"argument"},
            {"kind":"nil"},{"kind":"list","items":[3],"tail":4},
            {"kind":"tuple","items":[0,1,2,5]}]})
        );
    }
}

#[cfg(otp_domain = "29")]
pub(crate) fn is_builtin_type(name: &str, arity: usize) -> bool {
    matches!((name, arity), ("any", 0) | ("arity", 0) | ("atom", 0) | ("binary", 0) | ("bitstring", 0) | ("bool", 0) | ("boolean", 0) | ("byte", 0) | ("char", 0) | ("dynamic", 0) | ("float", 0) | ("function", 0) | ("identifier", 0) | ("integer", 0) | ("iodata", 0) | ("iolist", 0) | ("list", 0) | ("list", 1) | ("map", 0) | ("maybe_improper_list", 0) | ("maybe_improper_list", 2) | ("mfa", 0) | ("module", 0) | ("neg_integer", 0) | ("nil", 0) | ("no_return", 0) | ("node", 0) | ("non_neg_integer", 0) | ("none", 0) | ("nonempty_binary", 0) | ("nonempty_bitstring", 0) | ("nonempty_improper_list", 2) | ("nonempty_list", 0) | ("nonempty_list", 1) | ("nonempty_maybe_improper_list", 0) | ("nonempty_maybe_improper_list", 2) | ("nonempty_string", 0) | ("number", 0) | ("pid", 0) | ("port", 0) | ("pos_integer", 0) | ("record", 0) | ("reference", 0) | ("string", 0) | ("term", 0) | ("timeout", 0) | ("tuple", 0))
}
