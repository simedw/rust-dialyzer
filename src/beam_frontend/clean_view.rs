//! Selected Dialyzer cleanup as a borrowed Core view. Only goto replacements
//! allocate Erlang records; ordinary nodes and child lists remain in raw Core.
use super::term::Arena;
use crate::{
    core::{ErlangTerm as T, ErlangTerms, TermId},
    fast_hash::{FastHashMap, FastHashSet},
    shared_vec::SharedVec,
};
use std::sync::Arc;
type Result<V> = std::result::Result<V, String>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ViewRef {
    pub id: TermId,
    pub clean: bool,
}
impl ViewRef {
    fn cleaned(id: TermId) -> Self {
        Self { id, clean: true }
    }
    fn preserved(id: TermId) -> Self {
        Self { id, clean: false }
    }
}
#[derive(Clone, Copy)]
enum Group {
    Empty,
    One(ViewRef),
    List {
        root: TermId,
        clean: bool,
        filter: bool,
    },
    Pairs {
        root: TermId,
        clean: bool,
    },
}
pub struct NodeShape<'a> {
    pub kind: &'static str,
    pub annotations: TermId,
    pub value: Option<TermId>,
    pub map_arg: Option<ViewRef>,
    pub pattern: Option<bool>,
    terms: &'a SharedVec<T>,
    groups: [Group; 5],
    count: usize,
}
impl<'a> NodeShape<'a> {
    pub fn group_count(&self) -> usize {
        self.count
    }
    pub fn group_children(&self, index: usize) -> GroupChildren<'a> {
        assert!(index < self.count);
        GroupChildren::new(self.terms, self.groups[index])
    }
    pub fn children(&self) -> Children<'a> {
        Children {
            terms: self.terms,
            groups: self.groups,
            count: self.count,
            next: 0,
            current: GroupChildren::new(self.terms, Group::Empty),
            map_arg: self.map_arg,
        }
    }
}
/// Cursor over existing list chunks; neither chunks nor definitions are copied.
struct ListCursor<'a> {
    terms: &'a SharedVec<T>,
    next: Option<TermId>,
    items: std::slice::Iter<'a, TermId>,
}
impl<'a> ListCursor<'a> {
    fn new(terms: &'a SharedVec<T>, root: TermId) -> Self {
        Self {
            terms,
            next: Some(root),
            items: [].iter(),
        }
    }
}
impl Iterator for ListCursor<'_> {
    type Item = Result<TermId>;
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(&id) = self.items.next() {
                return Some(Ok(id));
            }
            let id = self.next.take()?;
            match self.terms.get(id.0) {
                Some(T::Nil) => return None,
                Some(T::List { items, tail }) => {
                    self.items = items.iter();
                    self.next = Some(*tail);
                }
                _ => return Some(Err(format!("clean view: expected proper list at {}", id.0))),
            }
        }
    }
}
pub struct GroupChildren<'a> {
    terms: &'a SharedVec<T>,
    group: Group,
    list: Option<ListCursor<'a>>,
    pending: Option<ViewRef>,
    failed: bool,
}
impl<'a> GroupChildren<'a> {
    fn new(terms: &'a SharedVec<T>, group: Group) -> Self {
        let list = match group {
            Group::List { root, .. } | Group::Pairs { root, .. } => {
                Some(ListCursor::new(terms, root))
            }
            _ => None,
        };
        let pending = if let Group::One(reference) = group {
            Some(reference)
        } else {
            None
        };
        Self {
            terms,
            group,
            list,
            pending,
            failed: false,
        }
    }
    fn step(&mut self) -> Result<Option<ViewRef>> {
        if let Some(reference) = self.pending.take() {
            return Ok(Some(reference));
        }
        loop {
            let Some(cursor) = self.list.as_mut() else {
                return Ok(None);
            };
            let Some(id) = cursor.next() else {
                return Ok(None);
            };
            let id = id?;
            match self.group {
                Group::List { clean, filter, .. } => {
                    if filter && drop_clause(self.terms, id)? {
                        continue;
                    }
                    return Ok(Some(ViewRef { id, clean }));
                }
                Group::Pairs { clean, .. } => {
                    let pair = tuple(self.terms, id)?;
                    if pair.len() != 2 {
                        return Err("clean view: malformed definition pair".into());
                    }
                    self.pending = Some(ViewRef { id: pair[1], clean });
                    return Ok(Some(ViewRef::preserved(pair[0])));
                }
                _ => unreachable!(),
            }
        }
    }
}
impl Iterator for GroupChildren<'_> {
    type Item = Result<ViewRef>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.failed {
            return None;
        }
        match self.step() {
            Ok(Some(reference)) => Some(Ok(reference)),
            Ok(None) => None,
            Err(e) => {
                self.failed = true;
                Some(Err(e))
            }
        }
    }
}
pub struct Children<'a> {
    terms: &'a SharedVec<T>,
    groups: [Group; 5],
    count: usize,
    next: usize,
    current: GroupChildren<'a>,
    map_arg: Option<ViewRef>,
}
impl Iterator for Children<'_> {
    type Item = Result<ViewRef>;
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(child) = self.current.next() {
                return Some(child);
            }
            if self.next < self.count {
                self.current = GroupChildren::new(self.terms, self.groups[self.next]);
                self.next += 1;
            } else {
                return self.map_arg.take().map(Ok);
            }
        }
    }
}
pub struct CleanView {
    arena: Arc<ErlangTerms>,
    root: ViewRef,
    raw_len: usize,
    replacements: FastHashMap<TermId, TermId>,
}
fn tuple(terms: &SharedVec<T>, id: TermId) -> Result<&[TermId]> {
    match terms.get(id.0) {
        Some(T::Tuple(f)) => Ok(f),
        _ => Err(format!("clean view: expected tuple at {}", id.0)),
    }
}
fn atom(terms: &SharedVec<T>, id: TermId) -> Option<&str> {
    match terms.get(id.0) {
        Some(T::Atom(a)) => Some(a),
        _ => None,
    }
}
fn tag(terms: &SharedVec<T>, id: TermId) -> Option<&str> {
    tuple(terms, id)
        .ok()?
        .first()
        .and_then(|id| atom(terms, *id))
}
fn resolve(mut reference: ViewRef, replacements: &FastHashMap<TermId, TermId>) -> ViewRef {
    if reference.clean {
        while let Some(&id) = replacements.get(&reference.id) {
            reference.id = id;
        }
    }
    reference
}
fn ignored(terms: &SharedVec<T>, id: TermId) -> Result<bool> {
    let f = tuple(terms, id)?;
    let ann = *f.get(1).ok_or("clean view: missing annotations")?;
    let mut found = false;
    for id in ListCursor::new(terms, ann) {
        if atom(terms, id?) == Some("dialyzer_ignore") {
            found = true;
        }
    }
    Ok(found)
}
fn drop_clause(terms: &SharedVec<T>, id: TermId) -> Result<bool> {
    let f = tuple(terms, id)?;
    if tag(terms, id) != Some("c_clause") || f.len() != 5 {
        return Err("clean view: malformed clause".into());
    }
    if ignored(terms, id)? {
        return Ok(true);
    }
    if tag(terms, f[4]) == Some("c_primop") {
        let body = tuple(terms, f[4])?;
        let name = tuple(terms, *body.get(2).ok_or("clean view: malformed primop")?)?;
        return Ok(matches!(
            name.get(2).and_then(|id| atom(terms, *id)),
            Some("match_fail" | "raise")
        ));
    }
    Ok(false)
}
fn shape<'a>(
    terms: &'a SharedVec<T>,
    reference: ViewRef,
    replacements: &FastHashMap<TermId, TermId>,
    raw_len: usize,
) -> Result<NodeShape<'a>> {
    let reference = resolve(reference, replacements);
    let f = tuple(terms, reference.id)?;
    let (kind, arity) = match tag(terms, reference.id) {
        Some("c_literal") => ("literal", 3),
        Some("c_var") => ("var", 3),
        Some("c_opaque") => ("opaque", 3),
        Some("c_binary") => ("binary", 3),
        Some("c_tuple") => ("tuple", 3),
        Some("c_values") => ("values", 3),
        Some("c_catch") => ("catch", 3),
        Some("c_alias") => ("alias", 4),
        Some("c_apply") => ("apply", 4),
        Some("c_case") => ("case", 4),
        Some("c_cons") => ("cons", 4),
        Some("c_fun") => ("fun", 4),
        Some("c_letrec") => ("letrec", 4),
        Some("c_primop") => ("primop", 4),
        Some("c_seq") => ("seq", 4),
        Some("c_call") => ("call", 5),
        Some("c_clause") => ("clause", 5),
        Some("c_let") => ("let", 5),
        Some("c_map") => ("map", 5),
        Some("c_map_pair") => ("map_pair", 5),
        Some("c_receive") => ("receive", 5),
        Some("c_module") => ("module", 6),
        Some("c_bitstr") => ("bitstr", 7),
        Some("c_try") => ("try", 7),
        other => return Err(format!("clean view: unsupported Core record {other:?}")),
    };
    if f.len() != arity {
        return Err(format!("clean view: wrong record arity for {kind}"));
    }
    if reference.clean && matches!(kind, "alias" | "opaque" | "receive") {
        return Err(format!("clean view: unsupported cleaned record {kind}"));
    }
    // Child resolution is deferred to shape(child), so cursors only borrow the
    // immutable source arena while sparse replacement preparation can mutate.
    let one = |i, clean| {
        Group::One(ViewRef {
            id: f[i],
            clean: reference.clean && clean,
        })
    };
    let many = |i, clean| Group::List {
        root: f[i],
        clean: reference.clean && clean,
        filter: false,
    };
    let pairs = |i, clean| Group::Pairs {
        root: f[i],
        clean: reference.clean && clean,
    };
    let mut s = NodeShape {
        kind,
        annotations: f[1],
        value: None,
        map_arg: None,
        pattern: None,
        terms,
        groups: [Group::Empty; 5],
        count: 0,
    };
    let mut set = |groups: &[Group]| {
        s.count = groups.len();
        s.groups[..groups.len()].copy_from_slice(groups);
    };
    match kind {
        "literal" | "var" | "opaque" => s.value = Some(f[2]),
        "binary" | "tuple" | "values" => set(&[many(2, true)]),
        "alias" | "cons" | "seq" => set(&[one(2, true), one(3, true)]),
        "apply" => set(&[one(2, true), many(3, true)]),
        "primop" => set(&[one(2, false), many(3, true)]),
        "case" => set(&[
            one(2, true),
            Group::List {
                root: f[3],
                clean: reference.clean,
                filter: reference.clean && reference.id.0 < raw_len,
            },
        ]),
        "bitstr" => set(&[
            one(2, true),
            one(3, true),
            one(4, false),
            one(5, false),
            one(6, false),
        ]),
        "call" => set(&[one(2, true), one(3, true), many(4, true)]),
        "clause" | "let" => set(&[many(2, false), one(3, true), one(4, true)]),
        "receive" => set(&[many(2, false), one(3, false), one(4, false)]),
        "fun" => set(&[many(2, false), one(3, true)]),
        "catch" => set(&[one(2, true)]),
        "try" => set(&[
            one(2, true),
            many(3, false),
            one(4, true),
            many(5, false),
            one(6, true),
        ]),
        "letrec" => set(&[pairs(2, true), one(3, true)]),
        "module" => set(&[
            one(2, false),
            many(3, false),
            pairs(4, false),
            pairs(5, true),
        ]),
        "map_pair" => set(&[one(2, false), one(3, true), one(4, true)]),
        "map" => {
            set(&[many(3, true)]);
            s.map_arg = Some(ViewRef {
                id: f[2],
                clean: reference.clean,
            });
            s.pattern = Some(match atom(terms, f[4]) {
                Some("true") => true,
                Some("false") => false,
                _ => return Err("clean view: invalid map pattern flag".into()),
            });
        }
        _ => unreachable!(),
    }
    Ok(s)
}
struct Prepare<'a> {
    raw: &'a ErlangTerms,
    arena: Arena,
    raw_len: usize,
    replacements: FastHashMap<TermId, TermId>,
    visited: FastHashSet<TermId>,
}
impl Prepare<'_> {
    fn node(&mut self, reference: ViewRef, depth: usize) -> Result<()> {
        if !reference.clean {
            return Ok(());
        }
        if depth >= 4096 {
            return Err("clean view: Core nesting exceeds 4096".into());
        }
        if !self.visited.insert(reference.id) {
            return Ok(());
        }
        let raw = self.raw;
        let fields = tuple(&raw.terms, reference.id)?;
        if tag(&raw.terms, reference.id) == Some("c_letrec") && fields.len() != 4 {
            return Err("clean view: malformed letrec".into());
        }
        let is_goto = if tag(&raw.terms, reference.id) == Some("c_letrec") {
            let mut found = false;
            for id in ListCursor::new(&raw.terms, fields[1]) {
                if atom(&raw.terms, id?) == Some("letrec_goto") {
                    found = true;
                }
            }
            found
        } else {
            false
        };
        if is_goto {
            let f = [fields[0], fields[1], fields[2], fields[3]];
            let mut defs = ListCursor::new(&raw.terms, f[2]);
            let definition = defs
                .next()
                .transpose()?
                .ok_or("clean view: goto letrec must have one definition")?;
            if defs.next().transpose()?.is_some() {
                return Err("clean view: goto letrec must have one definition".into());
            }
            let pair = tuple(&raw.terms, definition)?;
            if pair.len() != 2 {
                return Err("clean view: malformed goto definition".into());
            }
            let fields = tuple(&raw.terms, pair[1])?;
            if tag(&raw.terms, pair[1]) != Some("c_fun") || fields.len() != 4 {
                return Err("clean view: goto target must be fun".into());
            }
            let fun = [fields[0], fields[1], fields[2], fields[3]];
            self.node(ViewRef::cleaned(fun[3]), depth + 1)?;
            self.node(ViewRef::cleaned(f[3]), depth + 1)?;
            let fun_body = resolve(ViewRef::cleaned(fun[3]), &self.replacements).id;
            let body = resolve(ViewRef::cleaned(f[3]), &self.replacements).id;
            let replacement = if ignored(&self.arena.terms, body)? {
                fun_body
            } else {
                self.goto_case(&fun, fun_body, body)?
            };
            self.replacements.insert(reference.id, replacement);
            return Ok(());
        }
        let description = shape(&raw.terms, reference, &self.replacements, self.raw_len)?;
        for child in description.children() {
            self.node(child?, depth + 1)?;
        }
        Ok(())
    }
    fn literal(&mut self, name: &str) -> TermId {
        let ann = self.arena.nil();
        let value = self.arena.atom(name);
        self.arena.tagged("c_literal", vec![ann, value])
    }
    fn goto_case(&mut self, fun: &[TermId], fun_body: TermId, body: TermId) -> Result<TermId> {
        let ann = self.arena.nil();
        let name = self.literal("dialyzer_unknown");
        let args = self.arena.nil();
        let unknown = self.arena.tagged("c_primop", vec![ann, name, args]);
        let a = self.literal("a");
        let b = self.literal("b");
        let mut elements = vec![b];
        for id in ListCursor::new(&self.arena.terms, fun[2]) {
            elements.push(id?);
        }
        let ann = self.arena.nil();
        let pattern_b = if elements.len() == 1 {
            let atom = tuple(&self.arena.terms, b)?[2];
            let value = self.arena.tuple(vec![atom]);
            self.arena.tagged("c_literal", vec![ann, value])
        } else {
            let elements = self.arena.list(elements);
            self.arena.tagged("c_tuple", vec![ann, elements])
        };
        let mut clauses = Vec::new();
        for (pattern, result) in [(a, body), (pattern_b, fun_body)] {
            let ann = self.arena.nil();
            let patterns = self.arena.list(vec![pattern]);
            let guard = self.literal("true");
            clauses.push(
                self.arena
                    .tagged("c_clause", vec![ann, patterns, guard, result]),
            );
        }
        let ann = self.arena.nil();
        let clauses = self.arena.list(clauses);
        Ok(self.arena.tagged("c_case", vec![ann, unknown, clauses]))
    }
}
impl CleanView {
    pub fn new(raw: ErlangTerms) -> Result<Self> {
        let mut prepare = Prepare {
            raw: &raw,
            arena: Arena::from_terms(&raw),
            raw_len: raw.terms.len(),
            replacements: FastHashMap::default(),
            visited: FastHashSet::default(),
        };
        prepare.node(ViewRef::cleaned(raw.root), 0)?;
        let root = resolve(ViewRef::cleaned(raw.root), &prepare.replacements);
        Ok(Self {
            arena: Arc::new(prepare.arena.finish(root.id)),
            root,
            raw_len: prepare.raw_len,
            replacements: prepare.replacements,
        })
    }
    pub fn arena(&self) -> &Arc<ErlangTerms> {
        &self.arena
    }
    pub fn root(&self) -> ViewRef {
        self.root
    }
    pub fn shape(&self, reference: ViewRef) -> Result<NodeShape<'_>> {
        shape(
            &self.arena.terms,
            reference,
            &self.replacements,
            self.raw_len,
        )
    }
    /// Atoms in the exact virtual cleaned Erlang representation, including tags.
    pub fn atoms(&self) -> Result<FastHashSet<String>> {
        let mut names = FastHashSet::default();
        let mut values = Vec::new();
        let mut nodes = vec![self.root];
        let mut visited = FastHashSet::default();
        while let Some(reference) = nodes.pop() {
            let reference = resolve(reference, &self.replacements);
            if !visited.insert(reference) {
                continue;
            }
            let shape = self.shape(reference)?;
            let tag = match shape.kind {
                "literal" => "c_literal",
                "var" => "c_var",
                "opaque" => "c_opaque",
                "binary" => "c_binary",
                "tuple" => "c_tuple",
                "values" => "c_values",
                "catch" => "c_catch",
                "alias" => "c_alias",
                "apply" => "c_apply",
                "case" => "c_case",
                "cons" => "c_cons",
                "fun" => "c_fun",
                "letrec" => "c_letrec",
                "primop" => "c_primop",
                "seq" => "c_seq",
                "call" => "c_call",
                "clause" => "c_clause",
                "let" => "c_let",
                "map" => "c_map",
                "map_pair" => "c_map_pair",
                "receive" => "c_receive",
                "module" => "c_module",
                "bitstr" => "c_bitstr",
                "try" => "c_try",
                _ => unreachable!(),
            };
            if !names.contains(tag) {
                names.insert(tag.to_owned());
            }
            values.push(shape.annotations);
            if let Some(value) = shape.value {
                values.push(value);
            }
            if let Some(pattern) = shape.pattern {
                names.insert(if pattern { "true" } else { "false" }.into());
            }
            for child in shape.children() {
                nodes.push(child?);
            }
        }
        let mut visited = FastHashSet::default();
        while let Some(id) = values.pop() {
            if !visited.insert(id) {
                continue;
            }
            match &self.arena.terms[id.0] {
                T::Atom(a) => {
                    if !names.contains(a) {
                        names.insert(a.clone());
                    }
                }
                T::ExternalFun(external_fun_payload) => {
                    let crate::core::ExternalFun {
                        module, function, ..
                    } = external_fun_payload.as_ref();

                    names.insert(module.clone());
                    names.insert(function.clone());
                }
                T::Tuple(items) => values.extend(items.iter().copied()),
                T::List { items, tail } => {
                    values.extend(items.iter().copied());
                    values.push(*tail);
                }
                T::Map(entries) => {
                    for &(key, value) in entries {
                        values.extend([key, value]);
                    }
                }
                _ => {}
            }
        }
        Ok(names)
    }
}
