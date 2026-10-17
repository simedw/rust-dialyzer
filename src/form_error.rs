use crate::{
    core::{ErlangTerm, ErlangTerms, TermId},
    lattice::{BitstringDomainError, LatticeError},
    native_error::NativeTransferError,
    native_form_environment::RawFormTerm,
};
use std::sync::Arc;
#[derive(Clone, Debug)]
pub enum FormError {
    Native(NativeTransferError),
    ErrorReason(RawFormTerm),
    CapturedError(crate::term_order::CaptureEnvironment),
    MissingClosureProfile(&'static str),
    Throw(RawFormTerm),
    Transport(String),
}
impl From<String> for FormError {
    fn from(v: String) -> Self {
        Self::Transport(v)
    }
}
impl From<&str> for FormError {
    fn from(v: &str) -> Self {
        Self::Transport(v.into())
    }
}
impl From<LatticeError> for FormError {
    fn from(v: LatticeError) -> Self {
        Self::Native(v.into())
    }
}
impl From<BitstringDomainError> for FormError {
    fn from(v: BitstringDomainError) -> Self {
        Self::Transport(v.to_string())
    }
}
impl From<FormError> for String {
    fn from(v: FormError) -> Self {
        v.to_string()
    }
}
impl std::fmt::Display for FormError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Native(v) => v.fmt(f),
            Self::Transport(v) => f.write_str(v),
            Self::CapturedError(_) => f.write_str("native captured error reason"),
            Self::MissingClosureProfile(module) => {
                write!(f, "missing original closure code profile for {module}")
            }
            Self::ErrorReason(v) => write!(f, "native error reason at term{}", v.id.0),
            Self::Throw(v) => write!(f, "native throw reason at term{}", v.id.0),
        }
    }
}
impl std::error::Error for FormError {}
impl FormError {
    /// Native msg_with_position wrapping preserves the original iolist nesting.
    pub(crate) fn at_position(self, source: &crate::program::SourceOrigin) -> Self {
        let Self::Throw(ref raw) = self else {
            return self;
        };
        let ErlangTerm::Tuple(parts) = raw.term() else {
            return self;
        };
        if parts.len() != 2
            || !matches!(&raw.arena().terms[parts[0].0], ErlangTerm::Atom(tag) if tag=="error")
        {
            return self;
        }
        let message = RawFormTerm::new(raw.arena().clone(), parts[1]).expect("throw message");
        let mut b = ReasonBuilder::new();
        let file = source.file.as_deref().unwrap_or("nofile");
        let basename = std::path::Path::new(file)
            .file_name()
            .and_then(|x| x.to_str())
            .unwrap_or("");
        let file = b.string(basename);
        let line = b.string(
            &source
                .line
                .map(|x| x.to_string())
                .unwrap_or("undefined".into()),
        );
        let colon = b.integer(58);
        let mut position = vec![line];
        if let Some(column) = source.column {
            position.push(colon);
            position.push(b.string(&column.to_string()));
        }
        let position = b.list(position);
        let space = b.integer(32);
        let message = b.copy(&message);
        let message = b.list(vec![file, colon, position, colon, space, message]);
        let error = b.atom("error");
        let reason = b.tuple(vec![error, message]);
        Self::Throw(b.finish(reason))
    }
    pub fn report(&self) -> serde_json::Value {
        use serde_json::json;
        match self {
            Self::Native(v) => {
                let mut report = v.report();
                if let Some(reason) = native_reason(v) {
                    report["reason"] =
                        crate::type_exports::arena_to_value(&reason.arena.terms, reason.id);
                }
                report
            }
            Self::Transport(v) => json!({"kind":"unsupported","detail":v}),
            Self::ErrorReason(v) => {
                json!({"kind":"error","reason":crate::type_exports::arena_to_value(&v.arena.terms,v.id)})
            }
            Self::CapturedError(v) => json!({"kind":"error","captures":v.to_value()}),
            Self::MissingClosureProfile(module) => {
                json!({"kind":"unsupported","boundary":"missing_closure_profile","module":module})
            }
            Self::Throw(v) => {
                json!({"kind":"throw","reason":crate::type_exports::arena_to_value(&v.arena.terms,v.id)})
            }
        }
    }
}
/// Flat reason construction retains raw handles without recursive owned terms.
pub(crate) struct ReasonBuilder {
    terms: crate::shared_vec::SharedVec<ErlangTerm>,
    nil: TermId,
    /// Set by `interning`: equal terms share one id.
    seen: Option<crate::fast_hash::FastHashMap<ErlangTerm, TermId>>,
}
impl ReasonBuilder {
    pub fn new() -> Self {
        Self {
            terms: vec![ErlangTerm::Nil].into(),
            nil: TermId(0),
            seen: None,
        }
    }
    /// A builder whose equal subterms share storage. Long-lived captures
    /// repeat the same types many times (each argument appears in the
    /// signature, the argument list and constraints); readers only walk
    /// the structure, so sharing is invisible to them.
    pub fn interning() -> Self {
        let mut seen = crate::fast_hash::FastHashMap::default();
        seen.insert(ErlangTerm::Nil, TermId(0));
        Self {
            seen: Some(seen),
            ..Self::new()
        }
    }
    pub(crate) fn from_terms(mut terms: crate::shared_vec::SharedVec<ErlangTerm>) -> Self {
        let nil = if let Some(index) = terms.iter().position(|t| matches!(t, ErlangTerm::Nil)) {
            TermId(index)
        } else {
            let id = TermId(terms.len());
            terms.push(ErlangTerm::Nil);
            id
        };
        Self {
            terms,
            nil,
            seen: None,
        }
    }
    pub fn push(&mut self, t: ErlangTerm) -> TermId {
        let id = TermId(self.terms.len());
        if let Some(seen) = &mut self.seen {
            match seen.entry(t) {
                std::collections::hash_map::Entry::Occupied(entry) => return *entry.get(),
                std::collections::hash_map::Entry::Vacant(entry) => {
                    self.terms.push(entry.key().clone());
                    entry.insert(id);
                }
            }
        } else {
            self.terms.push(t);
        }
        id
    }
    pub fn atom(&mut self, s: &str) -> TermId {
        self.push(ErlangTerm::Atom(s.into()))
    }
    pub fn integer(&mut self, n: impl ToString) -> TermId {
        self.push(ErlangTerm::Integer(n.to_string()))
    }
    pub fn list(&mut self, items: Vec<TermId>) -> TermId {
        self.list_tail(items, self.nil)
    }
    pub fn list_tail(&mut self, items: Vec<TermId>, tail: TermId) -> TermId {
        if items.is_empty() {
            tail
        } else {
            self.push(ErlangTerm::List { items, tail })
        }
    }
    pub fn nil(&self) -> TermId {
        self.nil
    }
    pub fn string(&mut self, s: &str) -> TermId {
        let items = s.chars().map(|c| self.integer(c as u32)).collect();
        self.list(items)
    }
    pub fn tuple(&mut self, items: Vec<TermId>) -> TermId {
        self.push(ErlangTerm::Tuple(items))
    }
    pub fn copy(&mut self, raw: &RawFormTerm) -> TermId {
        if self.seen.is_none() {
            return crate::form_cache_wire::copy_arena(&mut self.terms, &raw.arena.terms, raw.id);
        }
        // Children before parents, so each child's new id is known.
        let terms = &raw.arena.terms;
        let mut ids = crate::fast_hash::FastHashMap::<usize, TermId>::default();
        let mut work = vec![(raw.id, false)];
        while let Some((id, expanded)) = work.pop() {
            if ids.contains_key(&id.0) {
                continue;
            }
            let children: Vec<TermId> = match &terms[id.0] {
                #[cfg(otp_domain = "29")]
                ErlangTerm::Record(record) => record.fields.iter().map(|(_, value)| *value).collect(),
                ErlangTerm::Tuple(v) => v.clone(),
                ErlangTerm::List { items, tail } => items.iter().copied().chain([*tail]).collect(),
                ErlangTerm::Map(v) => v.iter().flat_map(|(k, v)| [*k, *v]).collect(),
                _ => Vec::new(),
            };
            if !expanded && children.iter().any(|c| !ids.contains_key(&c.0)) {
                work.push((id, true));
                work.extend(children.into_iter().map(|c| (c, false)));
                continue;
            }
            let at = |c: &TermId| ids[&c.0];
            let term = match &terms[id.0] {
                #[cfg(otp_domain = "29")]
                ErlangTerm::Record(record) => {
                    let mut record = record.clone();
                    record.fields.iter_mut().for_each(|(_, value)| *value = at(value));
                    ErlangTerm::Record(record)
                }
                ErlangTerm::Tuple(v) => ErlangTerm::Tuple(v.iter().map(at).collect()),
                ErlangTerm::List { items, tail } => ErlangTerm::List {
                    items: items.iter().map(at).collect(),
                    tail: at(tail),
                },
                ErlangTerm::Map(v) => ErlangTerm::Map(v.iter().map(|(k, v)| (at(k), at(v))).collect()),
                v => v.clone(),
            };
            let new = self.push(term);
            ids.insert(id.0, new);
        }
        ids[&raw.id.0]
    }
    pub(crate) fn native(&mut self, value: &crate::lattice::NativeValue) -> TermId {
        use crate::lattice::NativeValue as N;
        enum Work<'a> {
            Visit(&'a N),
            Tuple(usize),
            List(usize),
            #[cfg(otp_domain = "29")]
            Map(usize),
        }
        let mut work = vec![Work::Visit(value)];
        let mut values = Vec::new();
        while let Some(item) = work.pop() {
            match item {
                Work::Visit(v) => match v {
                    N::Atom(s) => values.push(self.atom(s)),
                    N::Integer(n) => values.push(self.integer(n)),
                    N::Nil => values.push(self.nil()),
                    N::Tuple(xs) => {
                        work.push(Work::Tuple(xs.len()));
                        work.extend(xs.iter().rev().map(Work::Visit));
                    }
                    N::List(xs) => {
                        work.push(Work::List(xs.len()));
                        work.extend(xs.iter().rev().map(Work::Visit));
                    }
                    #[cfg(otp_domain = "29")]
                    N::Map(entries) => {
                        work.push(Work::Map(entries.len()));
                        work.extend(entries.iter().rev().flat_map(|(k,v)| [Work::Visit(v),Work::Visit(k)]));
                    }
                },
                Work::Tuple(n) => {
                    let children = values.split_off(values.len() - n);
                    values.push(self.tuple(children));
                }
                Work::List(n) => {
                    let children = values.split_off(values.len() - n);
                    values.push(self.list(children));
                }
                #[cfg(otp_domain = "29")]
                Work::Map(n) => {
                    let children = values.split_off(values.len() - n * 2);
                    values.push(self.push(ErlangTerm::Map(children.chunks_exact(2).map(|p| (p[0],p[1])).collect())));
                }
            }
        }
        values.pop().expect("native value traversal emits one root")
    }
    pub fn finish(self, root: TermId) -> RawFormTerm {
        let terms = if self.seen.is_some() {
            let mut terms = self.terms.to_vec();
            terms.shrink_to_fit();
            terms.into()
        } else {
            self.terms
        };
        RawFormTerm {
            arena: Arc::new(ErlangTerms { root, terms }),
            id: root,
        }
    }
    pub fn message(mut self, pieces: Vec<MessagePiece>) -> RawFormTerm {
        let mut items = Vec::new();
        for p in pieces {
            match p {
                MessagePiece::Text(s) => items.extend(s.chars().map(|c| self.integer(c as u32))),
                MessagePiece::Printed(s) => items.push(self.string(&s)),
                MessagePiece::RawWritten(v) => items.push(self.copy(&v)),
            }
        }
        let msg = self.list(items);
        let error = self.atom("error");
        let root = self.tuple(vec![error, msg]);
        self.finish(root)
    }
}
pub(crate) enum MessagePiece {
    Text(String),
    Printed(String),
    RawWritten(RawFormTerm),
}

impl serde::Serialize for FormError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.report().serialize(serializer)
    }
}

fn native_reason(error: &NativeTransferError) -> Option<RawFormTerm> {
    use crate::lattice::NativeValue as N;
    let atom = |s: &str| N::Atom(s.into());
    let pair = |tag: &str, value: N| N::Tuple(vec![atom(tag), value]);
    let value = match error {
        NativeTransferError::FunctionClause => atom("function_clause"),
        NativeTransferError::Badarg => atom("badarg"),
        NativeTransferError::SystemLimit => atom("system_limit"),
        NativeTransferError::CaseClauseNil => pair("case_clause", N::Nil),
        NativeTransferError::CaseClauseType(t) => pair("case_clause", t.to_native_value()),
        NativeTransferError::BadMatchType(t) => pair("badmatch", t.to_native_value()),
        NativeTransferError::BadMatchBoolean(v) => {
            pair("badmatch", atom(if *v { "true" } else { "false" }))
        }
        NativeTransferError::BadMatchNative(v) => pair("badmatch", *v.clone()),
        NativeTransferError::BadGenerator(t) => pair("bad_generator", t.to_native_value()),
        NativeTransferError::BadGenerators(tails) => pair(
            "bad_generators",
            N::Tuple(vec![
                N::List(tails.0.iter().map(|t| t.to_native_value()).collect()),
                N::List(tails.1.iter().map(|t| t.to_native_value()).collect()),
            ]),
        ),
        NativeTransferError::NativeNonTerminating | NativeTransferError::Detail(_) => return None,
    };
    let mut builder = ReasonBuilder::new();
    let root = builder.native(&value);
    Some(builder.finish(root))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The term as a tree, independent of arena layout and sharing.
    fn tree(raw: &RawFormTerm) -> String {
        fn go(terms: &crate::shared_vec::SharedVec<ErlangTerm>, id: TermId, out: &mut String) {
            let children: Vec<TermId> = match &terms[id.0] {
                ErlangTerm::Tuple(items) => items.clone(),
                ErlangTerm::List { items, tail } => items.iter().copied().chain([*tail]).collect(),
                other => return out.push_str(&format!("{other:?}")),
            };
            out.push_str(if matches!(terms[id.0], ErlangTerm::Tuple(_)) { "{" } else { "[" });
            for child in children {
                go(terms, child, out);
                out.push(',');
            }
            out.push(')');
        }
        let mut out = String::new();
        go(&raw.arena.terms, raw.id, &mut out);
        out
    }

    fn build(mut b: ReasonBuilder, source: &RawFormTerm) -> RawFormTerm {
        let pair = {
            let x = b.atom("x");
            let n = b.integer(7);
            b.tuple(vec![x, n])
        };
        let again = {
            let x = b.atom("x");
            let n = b.integer(7);
            b.tuple(vec![x, n])
        };
        let copied = b.copy(source);
        let copied_again = b.copy(source);
        let text = b.string("ab");
        let items = b.list(vec![pair, again, copied, copied_again, text]);
        b.finish(items)
    }

    #[test]
    fn interning_shares_equal_subterms_without_changing_the_term() {
        let mut source = ReasonBuilder::new();
        let leaf = source.atom("leaf");
        let shared = source.tuple(vec![leaf, leaf]);
        let root = source.list(vec![shared, shared, leaf]);
        let source = source.finish(root);
        let plain = build(ReasonBuilder::new(), &source);
        let interned = build(ReasonBuilder::interning(), &source);
        assert_eq!(tree(&plain), tree(&interned));
        assert!(interned.arena.terms.len() < plain.arena.terms.len());
        let distinct = interned.arena.terms.iter().collect::<std::collections::HashSet<_>>();
        assert_eq!(distinct.len(), interned.arena.terms.len());
    }
}
