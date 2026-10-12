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
pub struct NodeShape {
    pub kind: &'static str,
    pub annotations: TermId,
    pub value: Option<TermId>,
    pub groups: Vec<Vec<ViewRef>>,
    pub map_arg: Option<ViewRef>,
    pub pattern: Option<bool>,
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
fn list(terms: &SharedVec<T>, mut id: TermId) -> Result<Vec<TermId>> {
    let mut result = Vec::new();
    loop {
        match terms.get(id.0) {
            Some(T::Nil) => return Ok(result),
            Some(T::List { items, tail }) => {
                result.extend_from_slice(items);
                id = *tail;
            }
            _ => return Err(format!("clean view: expected proper list at {}", id.0)),
        }
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
    Ok(list(terms, ann)?
        .into_iter()
        .any(|id| atom(terms, id) == Some("dialyzer_ignore")))
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
fn shape(
    terms: &SharedVec<T>,
    reference: ViewRef,
    replacements: &FastHashMap<TermId, TermId>,
    raw_len: usize,
) -> Result<NodeShape> {
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
    let child = |id, clean| {
        resolve(
            ViewRef {
                id,
                clean: reference.clean && clean,
            },
            replacements,
        )
    };
    let one = |i, clean| vec![child(f[i], clean)];
    let many = |i, clean| -> Result<Vec<ViewRef>> {
        Ok(list(terms, f[i])?
            .into_iter()
            .map(|id| child(id, clean))
            .collect())
    };
    let pairs = |i, clean| -> Result<Vec<ViewRef>> {
        let mut result = Vec::new();
        for pair in list(terms, f[i])? {
            let p = tuple(terms, pair)?;
            if p.len() != 2 {
                return Err("clean view: malformed definition pair".into());
            }
            result.extend([child(p[0], false), child(p[1], clean)]);
        }
        Ok(result)
    };
    let mut value = None;
    let mut map_arg = None;
    let mut pattern = None;
    let groups = match kind {
        "literal" | "var" | "opaque" => {
            value = Some(f[2]);
            vec![]
        }
        "binary" | "tuple" | "values" => vec![many(2, true)?],
        "alias" | "cons" | "seq" => vec![one(2, true), one(3, true)],
        "apply" => vec![one(2, true), many(3, true)?],
        "primop" => vec![one(2, false), many(3, true)?],
        "case" => {
            let clauses = list(terms, f[3])?
                .into_iter()
                .filter_map(|id| {
                    if !reference.clean || reference.id.0 >= raw_len {
                        Some(Ok(child(id, reference.clean)))
                    } else {
                        match drop_clause(terms, id) {
                            Ok(true) => None,
                            Ok(false) => Some(Ok(child(id, true))),
                            Err(e) => Some(Err(e)),
                        }
                    }
                })
                .collect::<Result<Vec<_>>>()?;
            vec![one(2, true), clauses]
        }
        "bitstr" => vec![
            one(2, true),
            one(3, true),
            one(4, false),
            one(5, false),
            one(6, false),
        ],
        "call" => vec![one(2, true), one(3, true), many(4, true)?],
        "clause" => vec![many(2, false)?, one(3, true), one(4, true)],
        "let" => vec![many(2, false)?, one(3, true), one(4, true)],
        "receive" => vec![many(2, false)?, one(3, false), one(4, false)],
        "fun" => vec![many(2, false)?, one(3, true)],
        "catch" => vec![one(2, true)],
        "try" => vec![
            one(2, true),
            many(3, false)?,
            one(4, true),
            many(5, false)?,
            one(6, true),
        ],
        "letrec" => vec![pairs(2, true)?, one(3, true)],
        "module" => vec![
            one(2, false),
            many(3, false)?,
            pairs(4, false)?,
            pairs(5, true)?,
        ],
        "map_pair" => vec![one(2, false), one(3, true), one(4, true)],
        "map" => {
            map_arg = Some(child(f[2], true));
            pattern = Some(match atom(terms, f[4]) {
                Some("true") => true,
                Some("false") => false,
                _ => return Err("clean view: invalid map pattern flag".into()),
            });
            vec![many(3, true)?]
        }
        _ => unreachable!(),
    };
    Ok(NodeShape {
        kind,
        annotations: f[1],
        value,
        groups,
        map_arg,
        pattern,
    })
}

struct Prepare {
    arena: Arena,
    raw_len: usize,
    replacements: FastHashMap<TermId, TermId>,
    visited: FastHashSet<TermId>,
}
impl Prepare {
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
        let f = tuple(&self.arena.terms, reference.id)?.to_vec();
        if tag(&self.arena.terms, reference.id) == Some("c_letrec") && f.len() != 4 {
            return Err("clean view: malformed letrec".into());
        }
        if tag(&self.arena.terms, reference.id) == Some("c_letrec")
            && list(&self.arena.terms, f[1])?
                .into_iter()
                .any(|id| atom(&self.arena.terms, id) == Some("letrec_goto"))
        {
            let defs = list(&self.arena.terms, f[2])?;
            if defs.len() != 1 {
                return Err("clean view: goto letrec must have one definition".into());
            }
            let pair = tuple(&self.arena.terms, defs[0])?.to_vec();
            if pair.len() != 2 {
                return Err("clean view: malformed goto definition".into());
            }
            let fun = tuple(&self.arena.terms, pair[1])?.to_vec();
            if tag(&self.arena.terms, pair[1]) != Some("c_fun") || fun.len() != 4 {
                return Err("clean view: goto target must be fun".into());
            }
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
        let description = shape(
            &self.arena.terms,
            reference,
            &self.replacements,
            self.raw_len,
        )?;
        for child in description
            .groups
            .into_iter()
            .flatten()
            .chain(description.map_arg)
        {
            self.node(child, depth + 1)?;
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
        elements.extend(list(&self.arena.terms, fun[2])?);
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
    pub fn shape(&self, reference: ViewRef) -> Result<NodeShape> {
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
            nodes.extend(shape.groups.into_iter().flatten());
            nodes.extend(shape.map_arg);
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
                T::ExternalFun {
                    module, function, ..
                } => {
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
