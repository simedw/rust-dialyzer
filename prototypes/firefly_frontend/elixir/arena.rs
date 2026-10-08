// SPDX-License-Identifier: Apache-2.0
use super::*;
impl Lower {
    pub(super) fn add(&mut self, t: T) -> Id {
        let id = Id(self.a.terms.len());
        self.a.terms.push(t);
        id
    }
    pub(super) fn atom(&mut self, s: &str) -> Id {
        self.add(T::Atom(s.into()))
    }
    pub(super) fn int(&mut self, n: i64) -> Id {
        self.add(T::Integer(n.to_string()))
    }
    pub(super) fn tuple(&mut self, v: Vec<Id>) -> Id {
        self.add(T::Tuple(v))
    }
    pub(super) fn list(&mut self, v: Vec<Id>) -> Id {
        let tail = self.add(T::Nil);
        self.add(T::List { items: v, tail })
    }
    pub(super) fn ast(&mut self, tag: &str, ann: Id, more: Vec<Id>) -> Id {
        let mut v = vec![self.atom(tag), ann];
        v.extend(more);
        self.tuple(v)
    }
    pub(super) fn get(&self, id: Id) -> &T {
        &self.a.terms[id.0]
    }
    pub(super) fn tup(&self, id: Id) -> Result<Vec<Id>> {
        match self.get(id) {
            T::Tuple(v) => Ok(v.clone()),
            _ => Err("expected tuple".into()),
        }
    }
    pub(super) fn items(&self, id: Id) -> Result<Vec<Id>> {
        match self.get(id) {
            T::Nil => Ok(vec![]),
            T::List { items, tail } => {
                let mut v = items.clone();
                v.extend(self.items(*tail)?);
                Ok(v)
            }
            _ => Err("expected proper list".into()),
        }
    }
    pub(super) fn name(&self, id: Id) -> Result<String> {
        match self.get(id) {
            T::Atom(s) => Ok(s.clone()),
            _ => Err("expected atom".into()),
        }
    }
    pub(super) fn integer(&self, id: Id) -> Result<i64> {
        match self.get(id) {
            T::Integer(s) => s.parse().map_err(|_| "invalid integer".into()),
            _ => Err("expected integer".into()),
        }
    }
    pub(super) fn prop(&self, list: Id, key: &str) -> Result<Option<Id>> {
        for id in self.items(list)? {
            let p = self.tup(id)?;
            if p.len() == 2 && self.name(p[0])? == key {
                return Ok(Some(p[1]));
            }
        }
        Ok(None)
    }
    pub(super) fn map(&self, map: Id, key: &str) -> Result<Id> {
        match self.get(map) {
            T::Map(v) => v
                .iter()
                .find_map(|(k, v)| matches!(self.get(*k),T::Atom(s) if s==key).then_some(*v))
                .ok_or_else(|| format!("missing metadata key {key}")),
            _ => Err("metadata must be a map".into()),
        }
    }
    pub(super) fn optional(&self, map: Id, key: &str) -> Option<Id> {
        self.map(map, key).ok()
    }
    pub(super) fn ann(&mut self, meta: Id) -> Result<Id> {
        let line = match self.prop(meta, "line")? {
            Some(v) => v,
            None => self.int(0),
        };
        let loc = if let Some(c) = self.prop(meta, "column")? {
            self.tuple(vec![line, c])
        } else {
            line
        };
        if self
            .prop(meta, "generated")?
            .is_some_and(|v| matches!(self.get(v),T::Atom(s) if s=="true"))
        {
            let yes = self.atom("true");
            let generated = self.atom("generated");
            let loc_key = self.atom("location");
            let g = self.tuple(vec![generated, yes]);
            let l = self.tuple(vec![loc_key, loc]);
            Ok(self.list(vec![g, l]))
        } else {
            Ok(loc)
        }
    }
    pub(super) fn generated(&mut self, ann: Id) -> Id {
        if matches!(self.get(ann), T::List { .. }) {
            return ann;
        }
        let generated = self.atom("generated");
        let yes = self.atom("true");
        let location = self.atom("location");
        let g = self.tuple(vec![generated, yes]);
        let l = self.tuple(vec![location, ann]);
        self.list(vec![g, l])
    }
    pub(super) fn generated_arrows(&mut self, id: Id) -> Result<Id> {
        let mut out = vec![];
        for x in self.items(id)? {
            let mut p = self.tup(x)?;
            let mut meta = self.items(p[1])?;
            let key = self.atom("generated");
            let yes = self.atom("true");
            meta.insert(0, self.tuple(vec![key, yes]));
            p[1] = self.list(meta);
            out.push(self.tuple(p))
        }
        Ok(self.list(out))
    }
    pub(super) fn fresh(&mut self, ann: Id, s: &mut Scope) -> Id {
        let counter = s.counters.entry("_".into()).or_default();
        *counter += 1;
        let name = self.atom(&self.compiler.variable_name(*counter));
        self.ast("var", ann, vec![name])
    }
    pub(super) fn make_clause(
        &mut self,
        ann: Id,
        args: Vec<Id>,
        guards: Vec<Id>,
        body: Vec<Id>,
    ) -> Id {
        let args = self.list(args);
        let guards = self.list(guards);
        let body = self.list(body);
        self.ast("clause", ann, vec![args, guards, body])
    }
    pub(super) fn meta_from_ann(&mut self, ann: Id) -> Result<Id> {
        let (loc, generated) = match self.get(ann) {
            T::List { .. } => (
                self.prop(ann, "location")?
                    .ok_or("annotation lacks location")?,
                true,
            ),
            _ => (ann, false),
        };
        let (line, column) = match self.get(loc) {
            T::Tuple(v) => (v[0], Some(v[1])),
            _ => (loc, None),
        };
        let key = self.atom("line");
        let mut meta = vec![self.tuple(vec![key, line])];
        if let Some(c) = column {
            let key = self.atom("column");
            meta.push(self.tuple(vec![key, c]))
        }
        if generated {
            let key = self.atom("generated");
            let yes = self.atom("true");
            meta.push(self.tuple(vec![key, yes]))
        }
        Ok(self.list(meta))
    }
    pub(super) fn characters(&mut self, id: Id) -> Result<Id> {
        match self.get(id).clone() {
            T::Binary(bytes) => {
                let text = String::from_utf8(bytes).map_err(|_| "invalid file UTF-8")?;
                let xs = text.chars().map(|c| self.int(c as i64)).collect();
                Ok(self.list(xs))
            }
            T::Nil | T::List { .. } => Ok(id),
            _ => Err("invalid file characters".into()),
        }
    }
}
pub(super) fn validate_input(input: &ErlangTerms, metadata: Id) -> Result<()> {
    if metadata.0 >= input.terms.len() || input.terms.len() > 2_000_000 {
        return Err("invalid metadata arena size or root".into());
    }
    let mut depths = Vec::with_capacity(input.terms.len());
    for (index, term) in input.terms.iter().enumerate() {
        let children: Vec<_> = match term {
            T::Tuple(v) => v.clone(),
            T::List { items, tail } => {
                let mut xs = items.clone();
                xs.push(*tail);
                xs
            }
            T::Map(v) => v.iter().flat_map(|(k, v)| [*k, *v]).collect(),
            _ => vec![],
        };
        let mut depth = 0;
        for child in children {
            if child.0 >= index {
                return Err("metadata arena must be acyclic and in postorder".into());
            }
            depth = depth.max(depths[child.0] + 1)
        }
        if depth > 256 {
            return Err("Elixir metadata nesting exceeds 256".into());
        }
        depths.push(depth)
    }
    Ok(())
}
