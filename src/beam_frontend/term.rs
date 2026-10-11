use crate::core::{ErlangTerm, ErlangTerms, TermId};

/// Owned flat term arena shared by the compiler stages. Child IDs remain stable.
#[derive(Clone)]
pub struct Arena {
    pub terms: crate::shared_vec::SharedVec<ErlangTerm>,
}
impl Arena {
    pub fn from_terms(input: &ErlangTerms) -> Self {
        Self {
            terms: input.terms.clone(),
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
    pub fn atom(&mut self, value: impl Into<String>) -> TermId {
        self.push(ErlangTerm::Atom(value.into()))
    }
    pub fn integer(&mut self, value: impl ToString) -> TermId {
        self.push(ErlangTerm::Integer(value.to_string()))
    }
    pub fn tuple(&mut self, values: Vec<TermId>) -> TermId {
        self.push(ErlangTerm::Tuple(values))
    }
    pub fn nil(&mut self) -> TermId {
        self.push(ErlangTerm::Nil)
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
    fn stages_share_input_and_snapshots_keep_stable_ids() {
        let mut original = Arena {
            terms: vec![].into(),
        };
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
        let mut a = Arena {
            terms: vec![].into(),
        };
        let key = a.atom("line");
        let malformed = a.tuple(vec![key]);
        let list = a.list(vec![malformed]);
        assert_eq!(a.keyword(list, "line"), None);
    }
}
