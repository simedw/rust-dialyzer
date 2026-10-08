use crate::{
    binder::{OpaqueMatch, WarningFunction, WarningInfo, WarningLocation},
    core::{ErlangTerm, ErlangTerms, TermId},
    form_error::ReasonBuilder,
    lattice::NativeValue,
    native_form_environment::RawFormTerm,
    term_order::compare_map_keys,
};
use std::cmp::Ordering;
#[derive(Clone, Debug)]
pub struct NativeWarning {
    pub raw: RawFormTerm,
}
impl NativeWarning {
    pub fn from_raw_origin(category: &str, origin: &RawFormTerm, message: &RawFormTerm) -> Self {
        let mut b = ReasonBuilder::new();
        let category = b.atom(category);
        let origin = b.copy(origin);
        let message = b.copy(message);
        let root = b.tuple(vec![category, origin, message]);
        Self {
            raw: b.finish(root),
        }
    }
    pub fn from_parts(category: &str, info: &WarningInfo, message: &RawFormTerm) -> Self {
        let mut b = ReasonBuilder::new();
        let tag = b.atom(category);
        let file = b.string(&info.filename);
        let location = match info.location {
            WarningLocation::Line(n) => b.integer(n),
            WarningLocation::Position { line, column } => {
                let line = b.integer(line);
                let column = b.integer(column);
                b.tuple(vec![line, column])
            }
        };
        let function = match &info.current_fun {
            WarningFunction::Top => b.atom("top"),
            WarningFunction::Undefined => b.atom("undefined"),
            WarningFunction::Label(label) => b.integer(label.0),
            WarningFunction::Named(name) => {
                let module = b.atom(&name.module);
                let function = b.atom(&name.name);
                let arity = b.integer(name.arity);
                b.tuple(vec![module, function, arity])
            }
        };
        let origin = b.tuple(vec![file, location, function]);
        let message = b.copy(message);
        let root = b.tuple(vec![tag, origin, message]);
        Self {
            raw: b.finish(root),
        }
    }
    pub fn from_value(category: &str, info: &WarningInfo, message: &NativeValue) -> Self {
        let mut b = ReasonBuilder::new();
        let root = b.native(message);
        Self::from_parts(category, info, &b.finish(root))
    }
    pub fn list(warnings: &[Self]) -> RawFormTerm {
        let mut b = ReasonBuilder::new();
        let terms = warnings.iter().map(|w| b.copy(&w.raw)).collect();
        let root = b.list(terms);
        b.finish(root)
    }
    pub fn matches(&self, raw: &ErlangTerms) -> Result<bool, String> {
        Ok(compare_map_keys(self.raw.arena(), self.raw.id(), raw, raw.root)? == Ordering::Equal)
    }
}
impl PartialEq for NativeWarning {
    fn eq(&self, other: &Self) -> bool {
        // Body analysis compares accumulated warning lists between call
        // alternatives, so equality must not rebuild term-order arenas.
        match structurally_equal(self.raw.arena(), self.raw.id(), other.raw.arena(), other.raw.id()) {
            Some(equal) => equal,
            None => {
                compare_map_keys(
                    self.raw.arena(),
                    self.raw.id(),
                    other.raw.arena(),
                    other.raw.id(),
                ) == Ok(Ordering::Equal)
            }
        }
    }
}

/// Exact term equality (`=:=`) by walking both arenas. Returns `None` when a
/// map is reached, because map equality ignores pair order; the caller then
/// uses the full term order.
fn structurally_equal(a: &ErlangTerms, ai: TermId, b: &ErlangTerms, bi: TermId) -> Option<bool> {
    let mut pending = vec![(ai, bi)];
    while let Some((x, y)) = pending.pop() {
        if std::ptr::eq(a, b) && x == y {
            continue;
        }
        match (&a.terms[x.0], &b.terms[y.0]) {
            (ErlangTerm::Map(_), _) | (_, ErlangTerm::Map(_)) => return None,
            (ErlangTerm::Tuple(xs), ErlangTerm::Tuple(ys)) => {
                if xs.len() != ys.len() {
                    return Some(false);
                }
                pending.extend(xs.iter().copied().zip(ys.iter().copied()));
            }
            (ErlangTerm::List { items: xs, tail: xt }, ErlangTerm::List { items: ys, tail: yt }) => {
                if xs.len() != ys.len() {
                    // Same list may be split differently between items and tail.
                    return None;
                }
                pending.extend(xs.iter().copied().zip(ys.iter().copied()));
                pending.push((*xt, *yt));
            }
            (ErlangTerm::List { .. }, _) | (_, ErlangTerm::List { .. }) => return None,
            (x, y) => {
                if x != y {
                    return Some(false);
                }
            }
        }
    }
    Some(true)
}
impl Eq for NativeWarning {}
pub fn string(value: &str) -> NativeValue {
    NativeValue::List(
        value
            .chars()
            .map(|c| NativeValue::Integer((c as u32).into()))
            .collect(),
    )
}
impl OpaqueMatch {
    pub fn native_warning(&self) -> NativeWarning {
        NativeWarning::from_value(
            "warn_opaque",
            &self.info,
            &NativeValue::Tuple(vec![
                NativeValue::Atom("opaque_match".into()),
                NativeValue::List(self.message.iter().map(|s| string(s)).collect()),
            ]),
        )
    }
}
