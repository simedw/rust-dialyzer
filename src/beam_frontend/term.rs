use crate::core::{ErlangTerm, ErlangTerms, TermId};

/// Owned flat term arena shared by the compiler stages. Child IDs remain stable.
#[derive(Clone, Default)]
pub struct Arena {
    pub terms: crate::shared_vec::SharedVec<ErlangTerm>,
    atoms: crate::fast_hash::FastHashMap<String, TermId>,
    small_integers: crate::fast_hash::FastHashMap<u8, TermId>,
    nil: Option<TermId>,
}
impl Arena {
    pub fn from_terms(input: &ErlangTerms) -> Self {
        Self {
            terms: input.terms.clone(),
            ..Self::default()
        }
    }
    pub fn finish(mut self, root: TermId) -> ErlangTerms {
        self.terms.freeze_shared();
        ErlangTerms {
            root,
            terms: self.terms,
        }
    }
    pub fn snapshot(&mut self, root: TermId) -> ErlangTerms {
        self.terms.freeze_shared();
        ErlangTerms {
            root,
            terms: self.terms.clone(),
        }
    }
    pub fn push(&mut self, value: ErlangTerm) -> TermId {
        let id = TermId(self.terms.len());
        self.terms.push(value);
        id
    }
    pub fn atom(&mut self, value: impl AsRef<str>) -> TermId {
        let value = value.as_ref();
        if let Some(&id) = self.atoms.get(value) {
            return id;
        }
        let id = self.push(ErlangTerm::Atom(value.to_owned()));
        self.atoms.insert(value.to_owned(), id);
        id
    }
    pub fn integer(&mut self, value: impl ToString) -> TermId {
        let value = value.to_string();
        let small = value
            .parse::<u8>()
            .ok()
            .filter(|_| !value.starts_with('+') && (value.len() == 1 || !value.starts_with('0')));
        if let Some(id) = small.and_then(|value| self.small_integers.get(&value)) {
            return *id;
        }
        let id = self.push(ErlangTerm::Integer(value));
        if let Some(value) = small {
            self.small_integers.insert(value, id);
        }
        id
    }
    pub fn tuple(&mut self, values: Vec<TermId>) -> TermId {
        self.push(ErlangTerm::Tuple(values))
    }
    pub fn rebuild_tuple(&mut self, original: TermId, values: Vec<TermId>) -> TermId {
        if matches!(self.get(original), ErlangTerm::Tuple(previous) if previous == &values) {
            original
        } else {
            self.tuple(values)
        }
    }
    pub fn rebuild_list(&mut self, original: TermId, items: Vec<TermId>, tail: TermId) -> TermId {
        if matches!(self.get(original), ErlangTerm::List { items: previous, tail: previous_tail }
            if previous == &items && *previous_tail == tail) {
            original
        } else {
            self.push(ErlangTerm::List { items, tail })
        }
    }
    pub fn nil(&mut self) -> TermId {
        if let Some(id) = self.nil {
            return id;
        }
        let id = self.push(ErlangTerm::Nil);
        self.nil = Some(id);
        id
    }
    pub fn list(&mut self, values: Vec<TermId>) -> TermId {
        let tail = self.nil();
        if values.is_empty() {
            tail
        } else {
            self.push(ErlangTerm::List {
                items: values,
                tail,
            })
        }
    }
    pub fn tagged(&mut self, tag: &str, mut values: Vec<TermId>) -> TermId {
        let tag = self.atom(tag);
        values.insert(0, tag);
        self.tuple(values)
    }
    pub fn get(&self, id: TermId) -> &ErlangTerm {
        &self.terms[id.0]
    }
    pub fn as_atom(&self, id: TermId) -> Option<&str> {
        match self.get(id) {
            ErlangTerm::Atom(x) => Some(x),
            _ => None,
        }
    }
    pub fn as_integer(&self, id: TermId) -> Option<i64> {
        match self.get(id) {
            ErlangTerm::Integer(x) => x.parse().ok(),
            _ => None,
        }
    }
    pub fn tuple_fields(&self, id: TermId) -> Result<&[TermId], String> {
        match self.get(id) {
            ErlangTerm::Tuple(x) => Ok(x),
            _ => Err(format!("expected tuple at term {}", id.0)),
        }
    }
    pub fn tag(&self, id: TermId) -> Option<&str> {
        self.tuple_fields(id)
            .ok()
            .and_then(|x| x.first())
            .and_then(|x| self.as_atom(*x))
    }
    pub fn list_items(&self, mut id: TermId) -> Result<Vec<TermId>, String> {
        let mut out = Vec::new();
        loop {
            match self.get(id) {
                ErlangTerm::Nil => return Ok(out),
                ErlangTerm::List { items, tail } => {
                    out.extend(items.iter().copied());
                    id = *tail;
                }
                _ => return Err(format!("expected proper list at term {}", id.0)),
            }
        }
    }
    pub fn map_get(&self, id: TermId, key: &str) -> Option<TermId> {
        match self.get(id) {
            ErlangTerm::Map(x) => x
                .iter()
                .find(|(k, _)| self.as_atom(*k) == Some(key))
                .map(|(_, v)| *v),
            _ => None,
        }
    }
    pub fn keyword(&self, id: TermId, key: &str) -> Option<TermId> {
        self.list_items(id).ok()?.into_iter().find_map(|x| {
            let t = self.tuple_fields(x).ok()?;
            if t.len() == 2 && self.as_atom(t[0]) == Some(key) {
                Some(t[1])
            } else {
                None
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rebuilding_retains_unchanged_nodes_without_merging_new_child_ids() {
        let mut arena = Arena::default();
        let first = arena.integer(1000);
        let tuple = arena.tuple(vec![first]);
        let nil = arena.nil();
        let list = arena.push(ErlangTerm::List { items: vec![first], tail: nil });
        let count = arena.terms.len();
        assert_eq!(arena.rebuild_tuple(tuple, vec![first]), tuple);
        assert_eq!(arena.rebuild_list(list, vec![first], nil), list);
        assert_eq!(arena.terms.len(), count);
        let second = arena.integer(1000);
        assert_ne!(first, second);
        assert_ne!(arena.rebuild_tuple(tuple, vec![second]), tuple);
        assert_ne!(arena.rebuild_list(list, vec![second], nil), list);
    }
    #[test]
    fn shared_primitives_keep_node_identity_and_integer_spelling() {
        let mut a = Arena::default();
        let name = a.atom("x");
        assert_eq!(name, a.atom("x"));
        let number = a.integer(17);
        assert_eq!(number, a.integer(17));
        let empty = a.nil();
        assert_eq!(empty, a.nil());
        let first = a.tagged("c_var", vec![empty, name]);
        let second = a.tagged("c_var", vec![empty, name]);
        assert_ne!(first, second);
        assert_eq!(a.get(first), a.get(second));
        let one = a.integer(1);
        let padded = a.integer("01");
        assert_ne!(one, padded);
        assert_eq!(a.get(padded), &ErlangTerm::Integer("01".into()));
    }
    #[test]
    fn stages_share_input_and_snapshots_keep_stable_ids() {
        let mut original = Arena::default();
        let root = original.atom("input");
        let input = original.finish(root);
        let mut next = Arena::from_terms(&input);
        assert_eq!(
            input.terms.as_contiguous().unwrap().as_ptr(),
            next.terms.as_contiguous().unwrap().as_ptr()
        );
        let first = next.atom("first");
        let snapshot = next.snapshot(first);
        let second = next.atom("second");
        assert_eq!(snapshot.terms.len(), 2);
        assert_eq!(next.as_atom(second), Some("second"));
        assert_eq!(next.as_atom(root), Some("input"));
        assert_eq!(snapshot.terms[first.0], ErlangTerm::Atom("first".into()));
    }
    #[test]
    fn malformed_keyword_tuple_is_rejected_without_indexing_missing_field() {
        let mut a = Arena::default();
        let key = a.atom("line");
        let malformed = a.tuple(vec![key]);
        let list = a.list(vec![malformed]);
        assert_eq!(a.keyword(list, "line"), None);
    }
}

/// Preserve the reachable term graph and its relative arena order while
/// dropping compiler drafts that no later stage can observe.
pub fn compact(input: ErlangTerms) -> Result<ErlangTerms, String> {
    let mut needed = vec![false; input.terms.len()];
    let mut pending = vec![input.root];
    while let Some(id) = pending.pop() {
        let active = needed.get_mut(id.0).ok_or("compact root outside arena")?;
        if std::mem::replace(active, true) {
            continue;
        }
        match &input.terms[id.0] {
            ErlangTerm::Tuple(items) => pending.extend(items.iter().copied()),
            ErlangTerm::List { items, tail } => {
                pending.extend(items.iter().copied());
                pending.push(*tail);
            }
            ErlangTerm::Map(pairs) => pending.extend(pairs.iter().flat_map(|(k, v)| [*k, *v])),
            #[cfg(otp_domain = "29")]
            ErlangTerm::Record(record) => pending.extend(record.fields.iter().map(|(_, value)| *value)),
            _ => (),
        }
    }
    let mut remap = vec![usize::MAX; needed.len()];
    let mut terms = Vec::with_capacity(needed.iter().filter(|&&n| n).count());
    let root = input.root;
    // Kept terms move out of the input and have their children renumbered
    // in place.
    input.terms.consume_kept(
        |i| needed[i],
        |i, mut term| {
            let mapped = |id: &mut TermId| -> Result<(), String> {
                let value = *remap.get(id.0).ok_or("compact child outside arena")?;
                if value == usize::MAX {
                    return Err("compact child must precede parent".into());
                }
                *id = TermId(value);
                Ok(())
            };
            match &mut term {
                ErlangTerm::Tuple(items) => items.iter_mut().try_for_each(mapped)?,
                ErlangTerm::List { items, tail } => {
                    items.iter_mut().try_for_each(mapped)?;
                    mapped(tail)?;
                }
                ErlangTerm::Map(pairs) => pairs
                    .iter_mut()
                    .try_for_each(|(k, v)| mapped(k).and_then(|()| mapped(v)))?,
                #[cfg(otp_domain = "29")]
                ErlangTerm::Record(record) => record.fields.iter_mut().try_for_each(|(_, value)| mapped(value))?,
                _ => (),
            }
            remap[i] = terms.len();
            terms.push(term);
            Ok::<_, String>(())
        },
    )?;
    Ok(ErlangTerms {
        root: TermId(remap[root.0]),
        terms: terms.into(),
    })
}

#[cfg(test)]
mod compaction_tests {
    use super::*;
    #[cfg(otp_domain = "29")]
    #[test]
    fn compact_native_record_preserves_nested_and_shared_field_references() {
        let mut a = Arena::default();
        a.atom("discarded draft");
        let key = a.atom("key");
        let value = a.integer(42);
        let pair = a.tuple(vec![key, value]);
        let nested = a.push(ErlangTerm::Map(vec![(key, pair)]));
        let inner = a.push(ErlangTerm::Record(Box::new(crate::core::NativeRecord {
            module: "records".into(), name: "inner".into(), exported: false,
            fields: vec![("nested".into(), nested)],
        })));
        let root = a.push(ErlangTerm::Record(Box::new(crate::core::NativeRecord {
            module: "records".into(), name: "outer".into(), exported: true,
            fields: vec![("second".into(), inner), ("first".into(), nested), ("shared".into(), inner)],
        })));
        let input = a.finish(root);
        let output = compact(input.clone()).unwrap();
        let limits = crate::external_term::Limits::default();
        assert_eq!(crate::external_term::encode(&input, limits, false).unwrap(), crate::external_term::encode(&output, limits, false).unwrap());
        assert_eq!(output.terms.len(), input.terms.len() - 1);
        let ErlangTerm::Record(record) = &output.terms[output.root.0] else { panic!("record lost") };
        assert_eq!(record.fields[0].1, record.fields[2].1);
        assert!(record.fields.iter().all(|(_, value)| value.0 < output.root.0));
    }
    #[test]
    fn compaction_preserves_complete_terms_and_shared_children() {
        let mut a = Arena::default();
        let dead = a.atom("unused compiler draft");
        let key = a.atom("key");
        let one = a.integer(1);
        let pair = a.tuple(vec![key, one]);
        let list = a.list(vec![pair, pair]);
        let root = a.push(ErlangTerm::Map(vec![(key, list)]));
        let input = a.finish(root);
        let output = compact(input.clone()).unwrap();
        let limits = crate::external_term::Limits::default();
        assert_eq!(
            crate::external_term::encode(&input, limits, false).unwrap(),
            crate::external_term::encode(&output, limits, false).unwrap()
        );
        assert_eq!(output.terms.len(), input.terms.len() - 1);
        assert!(!output.terms.iter().any(|t| t == &input.terms[dead.0]));
        let ErlangTerm::List { items, .. } = &output.terms[output.terms.len() - 2] else {
            panic!("list missing")
        };
        assert_eq!(items[0], items[1]);
    }
}
