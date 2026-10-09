use crate::bifs::boundary::NativeTerm;
use crate::core::{ErlangTerm, ErlangTerms, TermId};
use crate::raw_literal::{RawLiteral, RawLiteralArena, normal_usort};
use std::{collections::{BTreeSet, BTreeMap}, sync::Arc};
use serde_json::{Value, json};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportError {
    FunctionClause,
    BadMatchModules(Vec<RawLiteral>),
}
type Result<T> = std::result::Result<T, ExportError>;

struct Arena<'a>(&'a crate::shared_vec::SharedVec<ErlangTerm>);
enum Walk {
    Visit(TermId, i64),
    Reduce,
    List(std::sync::Arc<[TermId]>, usize, i64),
    Map(std::sync::Arc<[TermId]>, usize, i64),
}
impl Arena<'_> {
    fn tuple(&self, id: TermId) -> Result<&[TermId]> {
        match &self.0[id.0] {
            ErlangTerm::Tuple(v) => Ok(v),
            _ => Err(ExportError::FunctionClause),
        }
    }
    fn atom(&self, id: TermId) -> Option<&str> {
        match &self.0[id.0] {
            ErlangTerm::Atom(v) => Some(v),
            _ => None,
        }
    }
    fn list(&self, id: TermId) -> Result<Vec<TermId>> {
        let mut result = Vec::new();
        let mut cursor = id;
        loop {
            match &self.0[cursor.0] {
                ErlangTerm::Nil => return Ok(result),
                ErlangTerm::List { items, tail } => {
                    result.extend(items);
                    cursor = *tail;
                }
                _ => return Err(ExportError::FunctionClause),
            }
        }
    }
    fn visit_list(
        &self,
        ids: &[TermId],
        d: i64,
        l: i64,
        acc: &mut Vec<RawLiteral>,
        literals: &RawLiteralArena,
    ) -> Result<i64> {
        self.walk(vec![Walk::List(ids.into(), 0, d)], l, acc, literals)
    }
    fn visit(
        &self,
        id: TermId,
        d: i64,
        l: i64,
        acc: &mut Vec<RawLiteral>,
        literals: &RawLiteralArena,
    ) -> Result<i64> {
        self.walk(vec![Walk::Visit(id, d)], l, acc, literals)
    }
    fn walk(
        &self,
        mut work: Vec<Walk>,
        mut l: i64,
        acc: &mut Vec<RawLiteral>,
        literals: &RawLiteralArena,
    ) -> Result<i64> {
        while let Some(task) = work.pop() {
            match task {
                Walk::Reduce => l -= 1,
                Walk::List(ids, i, d) => {
                    if let Some(id) = ids.get(i) {
                        let id = *id;
                        work.push(Walk::List(ids, i + 1, d));
                        work.push(Walk::Visit(id, d));
                        work.push(Walk::Reduce);
                    }
                }
                Walk::Map(ids, i, d) => {
                    if l <= 0 {
                        return Err(ExportError::BadMatchModules(acc.clone()));
                    }
                    if let Some(id) = ids.get(i) {
                        let p = self.tuple(*id)?;
                        if p.len() != 4 || self.atom(p[0]) != Some("type") {
                            return Err(ExportError::FunctionClause);
                        }
                        let kv = self.list(p[3])?;
                        if kv.len() != 2 {
                            return Err(ExportError::FunctionClause);
                        }
                        work.push(Walk::Map(ids, i + 1, d));
                        work.push(Walk::Reduce);
                        work.push(Walk::Visit(kv[1], d - 1));
                        work.push(Walk::Visit(kv[0], d - 1));
                    }
                }
                Walk::Visit(id, d) => {
                    if d <= 0 || l <= 0 {
                        continue;
                    }
                    let t = self.tuple(id)?;
                    let tag = t
                        .first()
                        .and_then(|id| self.atom(*id))
                        .ok_or(ExportError::FunctionClause)?;
                    match (tag, t.len()) {
                        ("var" | "atom" | "integer" | "char", 3) | ("op", 4 | 5) => {}
                        ("ann_type", 3) => {
                            let a = self.list(t[2])?;
                            if a.len() != 2 {
                                return Err(ExportError::FunctionClause);
                            }
                            work.push(Walk::Visit(a[1], d));
                        }
                        ("paren_type", 3) => {
                            let a = self.list(t[2])?;
                            if a.len() != 1 {
                                return Err(ExportError::FunctionClause);
                            }
                            work.push(Walk::Visit(a[0], d));
                        }
                        ("remote_type", 3) => {
                            let a = self.list(t[2])?;
                            if a.len() != 3 {
                                return Err(ExportError::FunctionClause);
                            }
                            let module = self.tuple(a[0])?;
                            let name = self.tuple(a[1])?;
                            if module.len() != 3
                                || name.len() != 3
                                || self.atom(module[0]) != Some("atom")
                                || self.atom(name[0]) != Some("atom")
                            {
                                return Err(ExportError::FunctionClause);
                            }
                            acc.insert(
                                0,
                                literals
                                    .at(module[2])
                                    .map_err(|_| ExportError::FunctionClause)?,
                            );
                            work.push(Walk::List(self.list(a[2])?.into(), 0, d));
                        }
                        ("user_type", 4) => work.push(Walk::List(self.list(t[3])?.into(), 0, d)),
                        ("type", 4) => {
                            let name = self.atom(t[2]).ok_or(ExportError::FunctionClause)?;
                            if matches!(name, "map" | "tuple") && self.atom(t[3]) == Some("any") {
                                continue;
                            }
                            let a = self.list(t[3])?;
                            match (name, a.as_slice()) {
                                ("fun", [domain, range]) => {
                                    let domain = self.tuple(*domain)?;
                                    if domain.len() == 3
                                        && self.atom(domain[0]) == Some("type")
                                        && self.atom(domain[2]) == Some("any")
                                    {
                                        work.push(Walk::Visit(*range, d - 1));
                                        work.push(Walk::Reduce);
                                    } else if domain.len() == 4
                                        && self.atom(domain[0]) == Some("type")
                                        && self.atom(domain[2]) == Some("product")
                                    {
                                        work.push(Walk::Visit(*range, d));
                                        work.push(Walk::List(self.list(domain[3])?.into(), 0, d));
                                    } else {
                                        work.push(Walk::List(a.into(), 0, d))
                                    }
                                }
                                ("list", [body]) => {
                                    work.push(Walk::Visit(*body, d - 1));
                                    work.push(Walk::Reduce);
                                }
                                ("nonempty_list", [body]) => {
                                    work.push(Walk::Visit(*body, d));
                                    work.push(Walk::Reduce);
                                }
                                (
                                    "nonempty_improper_list"
                                    | "nonempty_maybe_improper_list"
                                    | "maybe_improper_list",
                                    [content, tail],
                                ) => {
                                    work.push(Walk::Visit(*tail, d));
                                    work.push(Walk::Visit(*content, d));
                                    work.push(Walk::Reduce);
                                }
                                ("range", [_, _]) => {}
                                ("tuple" | "product", _) => {
                                    work.push(Walk::List(a.into(), 0, d - 1))
                                }
                                ("map", _) => work.push(Walk::Map(a.into(), 0, d)),
                                _ => work.push(Walk::List(a.into(), 0, d)),
                            }
                        }
                        _ => return Err(ExportError::FunctionClause),
                    }
                }
            }
        }
        Ok(l)
    }
}

/// Literal arenas by source arena, so that the term order of an arena is
/// built once for all the forms read from it (each call built it again).
/// Keyed by the arena's address; each entry holds the arena alive, so an
/// address is never reused while cached. Used for identity only.
#[derive(Default)]
pub struct LiteralArenas {
    arenas: std::collections::HashMap<
        usize,
        (Arc<ErlangTerms>, std::result::Result<RawLiteralArena, String>),
    >,
    orders: std::collections::HashMap<
        usize,
        (Arc<ErlangTerms>, std::result::Result<crate::term_order::TermOrderArena, String>),
    >,
}
impl LiteralArenas {
    /// `RawLiteralArena::new(source.clone())`, built once per arena.
    pub fn arena(
        &mut self,
        source: &Arc<ErlangTerms>,
    ) -> std::result::Result<&RawLiteralArena, String> {
        let (_, literals) = self
            .arenas
            .entry(Arc::as_ptr(source) as usize)
            .or_insert_with(|| (source.clone(), RawLiteralArena::new(source.clone())));
        literals.as_ref().map_err(Clone::clone)
    }
    /// `TermOrderArena::new(source)`, built once per arena.
    pub fn order(
        &mut self,
        source: &Arc<ErlangTerms>,
    ) -> std::result::Result<&crate::term_order::TermOrderArena, String> {
        let (_, order) = self
            .orders
            .entry(Arc::as_ptr(source) as usize)
            .or_insert_with(|| (source.clone(), crate::term_order::TermOrderArena::new(source)));
        order.as_ref().map_err(Clone::clone)
    }
    fn get(&mut self, source: &Arc<ErlangTerms>) -> Result<&RawLiteralArena> {
        self.arena(source).map_err(|_| ExportError::FunctionClause)
    }
}

pub fn type_forms_to_remote_modules(
    source: &Arc<ErlangTerms>,
    forms: &[TermId],
) -> Result<Vec<RawLiteral>> {
    type_forms_to_remote_modules_in(&mut LiteralArenas::default(), source, forms)
}
pub fn type_forms_to_remote_modules_in(
    arenas: &mut LiteralArenas,
    source: &Arc<ErlangTerms>,
    forms: &[TermId],
) -> Result<Vec<RawLiteral>> {
    let terms = &source.terms;
    let literals = arenas.get(source)?;
    let mut modules = Vec::new();
    Arena(terms).visit_list(forms, 16, 10_000, &mut modules, literals)?;
    Ok(normal_usort(modules))
}

/// Original dialyzer_contracts constraint-form dependency traversal.
pub fn constraint_form_to_remote_modules(
    source: &Arc<ErlangTerms>,
    constraints: TermId,
) -> Result<Vec<RawLiteral>> {
    constraint_form_to_remote_modules_in(&mut LiteralArenas::default(), source, constraints)
}
pub fn constraint_form_to_remote_modules_in(
    arenas: &mut LiteralArenas,
    source: &Arc<ErlangTerms>,
    constraints: TermId,
) -> Result<Vec<RawLiteral>> {
    let terms = &source.terms;
    let arena = Arena(terms);
    let mut modules = Vec::new();
    let literals = arenas.get(source)?;
    let mut cursor = constraints;
    loop {
        let (items, tail) = match &terms[cursor.0] {
            ErlangTerm::Nil => break,
            ErlangTerm::List { items, tail } => (items, *tail),
            _ => return Err(ExportError::FunctionClause),
        };
        for &constraint in items {
            let xs = arena.tuple(constraint)?;
            if xs.len() != 4
                || arena.atom(xs[0]) != Some("type")
                || arena.atom(xs[2]) != Some("constraint")
            {
                return Err(ExportError::FunctionClause);
            }
            let args = arena.list(xs[3])?;
            if args.len() != 2 {
                return Err(ExportError::FunctionClause);
            }
            let operator = arena.tuple(args[0])?;
            if operator.len() != 3 || arena.atom(operator[0]) != Some("atom") {
                return Err(ExportError::FunctionClause);
            }
            modules.extend(type_form_with_literals(source, args[1], literals)?);
        }
        cursor = tail;
    }
    Ok(normal_usort(modules))
}

pub fn type_form_to_remote_modules(
    source: &Arc<ErlangTerms>,
    form: TermId,
) -> Result<Vec<RawLiteral>> {
    let literals = RawLiteralArena::new(source.clone()).map_err(|_| ExportError::FunctionClause)?;
    type_form_with_literals(source, form, &literals)
}
fn type_form_with_literals(
    source: &Arc<ErlangTerms>,
    form: TermId,
    literals: &RawLiteralArena,
) -> Result<Vec<RawLiteral>> {
    let terms = &source.terms;
    let arena = Arena(terms);
    let mut modules = Vec::new();
    match &terms[form.0] {
        ErlangTerm::Nil => {}
        ErlangTerm::List { .. } => {
            arena.visit_list(&arena.list(form)?, 16, 10_000, &mut modules, literals)?;
        }
        _ => {
            arena.visit(form, 16, 10_000, &mut modules, literals)?;
        }
    }
    Ok(normal_usort(modules))
}

/// Returns the original arena identities, including integer variable names.
pub fn t_var_names(terms: &crate::shared_vec::SharedVec<ErlangTerm>, forms: TermId) -> Result<Vec<TermId>> {
    let arena = Arena(terms);
    arena
        .list(forms)?
        .iter()
        .map(|id| {
            let t = arena.tuple(*id)?;
            if t.len() != 3 || arena.atom(t[0]) != Some("var") || arena.atom(t[2]) == Some("_") {
                return Err(ExportError::FunctionClause);
            }
            Ok(t[2])
        })
        .collect()
}

impl Arena<'_> {
    fn equal(&self, left: TermId, right: TermId) -> bool {
        let mut work = vec![(left, right)];
        while let Some((a, b)) = work.pop() {
            match (&self.0[a.0], &self.0[b.0]) {
                (ErlangTerm::Tuple(a), ErlangTerm::Tuple(b)) if a.len() == b.len() => {
                    work.extend(a.iter().copied().zip(b.iter().copied()))
                }
                (ErlangTerm::Integer(a), ErlangTerm::Integer(b)) => {
                    if a.parse::<num_bigint::BigInt>().ok() != b.parse::<num_bigint::BigInt>().ok()
                    {
                        return false;
                    }
                }
                (
                    ErlangTerm::List { .. } | ErlangTerm::Nil,
                    ErlangTerm::List { .. } | ErlangTerm::Nil,
                ) => match (self.list(a), self.list(b)) {
                    (Ok(a), Ok(b)) if a.len() == b.len() => work.extend(a.into_iter().zip(b)),
                    _ => return false,
                },
                (a, b) if a == b => {}
                _ => return false,
            }
        }
        true
    }
}

/// Type-table values retain original raw form identities; resolution remains in Resolver.
pub fn lookup_record(
    terms: &crate::shared_vec::SharedVec<ErlangTerm>,
    tag: &str,
    arity: TermId,
    table: TermId,
) -> Result<Option<TermId>> {
    let arena = Arena(terms);
    let ErlangTerm::Map(entries) = &terms[table.0] else {
        return Err(ExportError::FunctionClause);
    };
    for (key, value) in entries {
        let Ok(key) = arena.tuple(*key) else { continue };
        if key.len() != 2 || arena.atom(key[0]) != Some("record") || arena.atom(key[1]) != Some(tag)
        {
            continue;
        }
        let value = arena.tuple(*value)?;
        if value.len() != 2 {
            return Err(ExportError::FunctionClause);
        }
        // Native orddict uses numeric ordering; exact integer lookup suffices for compiler metadata.
        for pair in arena.list(value[1])? {
            let pair = arena.tuple(pair)?;
            if pair.len() != 2 {
                return Err(ExportError::FunctionClause);
            }
            let numeric_order = match (&terms[pair[0].0], &terms[arity.0]) {
                (ErlangTerm::Integer(a), ErlangTerm::Integer(b)) => Some(
                    a.parse::<num_bigint::BigInt>()
                        .map_err(|_| ExportError::FunctionClause)?
                        .cmp(
                            &b.parse::<num_bigint::BigInt>()
                                .map_err(|_| ExportError::FunctionClause)?,
                        ),
                ),
                (ErlangTerm::Integer(a), ErlangTerm::Float(bits)) => {
                    use num_traits::FromPrimitive;
                    let f = f64::from_bits(*bits);
                    let trunc =
                        num_bigint::BigInt::from_f64(f).ok_or(ExportError::FunctionClause)?;
                    let n = a
                        .parse::<num_bigint::BigInt>()
                        .map_err(|_| ExportError::FunctionClause)?;
                    Some(match n.cmp(&trunc) {
                        std::cmp::Ordering::Equal => {
                            if f.fract() > 0.0 {
                                std::cmp::Ordering::Less
                            } else if f.fract() < 0.0 {
                                std::cmp::Ordering::Greater
                            } else {
                                std::cmp::Ordering::Equal
                            }
                        }
                        other => other,
                    })
                }
                _ => None,
            };
            if numeric_order == Some(std::cmp::Ordering::Greater) {
                return Ok(None);
            }
            if numeric_order == Some(std::cmp::Ordering::Equal) || arena.equal(pair[0], arity) {
                return Ok(Some(pair[1]));
            }
        }
        return Ok(None);
    }
    Ok(None)
}

pub fn type_is_defined(
    terms: &crate::shared_vec::SharedVec<ErlangTerm>,
    kind: TermId,
    name: TermId,
    arity: TermId,
    table: TermId,
) -> Result<bool> {
    let arena = Arena(terms);
    let ErlangTerm::Map(entries) = &terms[table.0] else {
        return Err(ExportError::FunctionClause);
    };
    Ok(entries.iter().any(|(key, _)| {
        arena.tuple(*key).is_ok_and(|v| {
            v.len() == 3
                && arena.equal(v[0], kind)
                && arena.equal(v[1], name)
                && arena.equal(v[2], arity)
        })
    }))
}

pub fn module_type_deps_of_type_defs(
    source: &Arc<ErlangTerms>,
    table: TermId,
) -> Result<Vec<RawLiteral>> {
    module_type_deps_of_type_defs_in(&mut LiteralArenas::default(), source, table)
}
pub fn module_type_deps_of_type_defs_in(
    arenas: &mut LiteralArenas,
    source: &Arc<ErlangTerms>,
    table: TermId,
) -> Result<Vec<RawLiteral>> {
    let terms = &source.terms;
    let literals = arenas.get(source)?;
    let arena = Arena(terms);
    let mut result = Vec::new();
    let ErlangTerm::Map(entries) = &terms[table.0] else {
        return Err(ExportError::FunctionClause);
    };
    // Arena maps retain native maps:to_list order at the interop boundary.
    for (key, value) in entries {
        let key = arena.tuple(*key)?;
        let value = arena.tuple(*value)?;
        let forms = match (
            key.first().and_then(|id| arena.atom(*id)),
            key.len(),
            value.len(),
        ) {
            (Some("type" | "opaque" | "nominal"), 3, 2) => {
                let definition = arena.tuple(value[0])?;
                if definition.len() != 4 {
                    return Err(ExportError::FunctionClause);
                }
                vec![definition[2]]
            }
            (Some("record"), 2, 2) => {
                let mut fields = Vec::new();
                for size in arena.list(value[1])? {
                    let size = arena.tuple(size)?;
                    if size.len() != 2 {
                        return Err(ExportError::FunctionClause);
                    }
                    for field in arena.list(size[1])? {
                        let field = arena.tuple(field)?;
                        if field.len() != 3 {
                            return Err(ExportError::FunctionClause);
                        }
                        fields.push(field[1]);
                    }
                }
                fields
            }
            _ => return Err(ExportError::FunctionClause),
        };
        let mut modules = Vec::new();
        if key.len() == 3 {
            arena.visit(forms[0], 16, 10_000, &mut modules, literals)?;
        } else {
            arena.visit_list(&forms, 16, 10_000, &mut modules, literals)?;
        }
        result.extend(normal_usort(modules));
    }
    Ok(result)
}

/// Lossless compact CoreExport arena view, used by context/cache interop.
/// This is serialization of validated terms, not a second term parser.
pub fn reachable_term_ids(terms: &crate::shared_vec::SharedVec<ErlangTerm>, root: TermId) -> BTreeSet<usize> {
    let mut reachable = BTreeSet::new();
    let mut work = vec![root];
    while let Some(id) = work.pop() {
        if !reachable.insert(id.0) {
            continue;
        }
        match &terms[id.0] {
            ErlangTerm::Tuple(ids) => work.extend(ids),
            ErlangTerm::List { items, tail } => {
                work.extend(items);
                work.push(*tail)
            }
            ErlangTerm::Map(entries) => {
                for (k, v) in entries {
                    work.push(*k);
                    work.push(*v)
                }
            }
            _ => {}
        }
    }
    reachable
}
pub fn compact_arena(arena: &ErlangTerms) -> ErlangTerms {
    let reachable = reachable_term_ids(&arena.terms, arena.root);
    let ids = reachable
        .iter()
        .enumerate()
        .map(|(new, old)| (*old, TermId(new)))
        .collect::<BTreeMap<_, _>>();
    let at = |id: TermId| ids[&id.0];
    let terms = reachable
        .iter()
        .map(|id| match &arena.terms[*id] {
            ErlangTerm::Tuple(v) => ErlangTerm::Tuple(v.iter().map(|v| at(*v)).collect()),
            ErlangTerm::List { items, tail } => ErlangTerm::List {
                items: items.iter().map(|v| at(*v)).collect(),
                tail: at(*tail),
            },
            ErlangTerm::Map(v) => {
                ErlangTerm::Map(v.iter().map(|(k, v)| (at(*k), at(*v))).collect())
            }
            other => other.clone(),
        })
        .collect();
    ErlangTerms {
        root: at(arena.root),
        terms,
    }
}
pub fn term_to_value(term: &ErlangTerm, at: impl Fn(TermId) -> usize) -> Value {
    match term {
        ErlangTerm::ExternalFun {
            module,
            function,
            arity,
        } => json!({"kind":"external_fun","module":module,"function":function,"arity":arity}),
        ErlangTerm::Atom(v) => json!({"kind":"atom","value":v}),
        ErlangTerm::Integer(v) => json!({"kind":"integer","value":v}),
        ErlangTerm::Float(v) => json!({"kind":"float","value":format!("{v:016X}")}),
        ErlangTerm::Binary(v) => json!({"kind":"binary","value":encode_base64(v)}),
        ErlangTerm::Bitstring { bytes, bits } => {
            json!({"kind":"bitstring","bytes":encode_base64(bytes),"bits":bits})
        }
        ErlangTerm::Nil => json!({"kind":"nil"}),
        ErlangTerm::Tuple(v) => {
            json!({"kind":"tuple","items":v.iter().map(|v|at(*v)).collect::<Vec<_>>()})
        }
        ErlangTerm::List { items, tail } => {
            json!({"kind":"list","items":items.iter().map(|v|at(*v)).collect::<Vec<_>>(),"tail":at(*tail)})
        }
        ErlangTerm::Map(v) => {
            json!({"kind":"map","entries":v.iter().map(|(k,v)|[at(*k),at(*v)]).collect::<Vec<_>>()})
        }
    }
}
pub fn arena_to_value(terms: &crate::shared_vec::SharedVec<ErlangTerm>, root: TermId) -> serde_json::Value {
    use serde_json::json;
    let reachable = reachable_term_ids(terms, root);
    let ids = reachable
        .iter()
        .enumerate()
        .map(|(new, old)| (*old, new))
        .collect::<std::collections::BTreeMap<_, _>>();
    let at = |id: TermId| ids[&id.0];
    let values = reachable.iter().map(|id| term_to_value(&terms[*id], at)).collect::<Vec<_>>();
    json!({"root":at(root),"terms":values})
}
pub(crate) fn encode_base64(bytes: &[u8]) -> String {
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for part in bytes.chunks(3) {
        let a = part[0] as usize;
        let b = part.get(1).copied().unwrap_or(0) as usize;
        let c = part.get(2).copied().unwrap_or(0) as usize;
        out.push(alphabet[a >> 2] as char);
        out.push(alphabet[((a & 3) << 4) | (b >> 4)] as char);
        out.push(if part.len() > 1 {
            alphabet[((b & 15) << 2) | (c >> 6)] as char
        } else {
            '='
        });
        out.push(if part.len() > 2 {
            alphabet[c & 63] as char
        } else {
            '='
        });
    }
    out
}

pub fn native_type_term(
    arena: &ErlangTerms,
    id: TermId,
) -> std::result::Result<NativeTerm, String> {
    Ok(match &arena.terms[id.0] {
        ErlangTerm::Atom(value) => NativeTerm::Atom {
            value: value.clone(),
        },
        ErlangTerm::Integer(value) => NativeTerm::Integer {
            value: value.clone(),
        },
        ErlangTerm::Nil => NativeTerm::Nil,
        ErlangTerm::Tuple(ids) => NativeTerm::Tuple {
            items: ids
                .iter()
                .map(|id| native_type_term(arena, *id))
                .collect::<std::result::Result<_, _>>()?,
        },
        ErlangTerm::List { items, tail } => NativeTerm::List {
            items: items
                .iter()
                .map(|id| native_type_term(arena, *id))
                .collect::<std::result::Result<_, _>>()?,
            tail: Box::new(native_type_term(arena, *tail)?),
        },
        other => return Err(format!("invalid native type scalar {other:?}")),
    })
}

pub fn type_from_arena(
    arena: &ErlangTerms,
    id: TermId,
) -> std::result::Result<crate::lattice::Type, String> {
    native_type_term(arena, id)?.type_value()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::ErlangTerms;
    use serde_json::json;
    #[test]
    fn typed_compaction_keeps_sorted_reachable_ids_and_all_term_payloads() {
        let arena = ErlangTerms {
            root: TermId(10),
            terms: vec![
                ErlangTerm::Atom("unreachable".into()),
                ErlangTerm::Nil,
                ErlangTerm::Atom("ok".into()),
                ErlangTerm::Integer("123456789012345678901234567890".into()),
                ErlangTerm::Float(0x8000000000000000),
                ErlangTerm::Binary(vec![255]),
                ErlangTerm::Bitstring {
                    bytes: vec![128],
                    bits: 1,
                },
                ErlangTerm::ExternalFun {
                    module: "m".into(),
                    function: "f".into(),
                    arity: 2,
                },
                ErlangTerm::List {
                    items: vec![TermId(2), TermId(3)],
                    tail: TermId(2),
                },
                ErlangTerm::Map(vec![(TermId(2), TermId(8))]),
                ErlangTerm::Tuple((1..10).map(TermId).collect()),
                ErlangTerm::Atom("unreachable-after".into()),
            ].into(),
        };
        let compact = compact_arena(&arena);
        assert_eq!(compact.root, TermId(9));
        assert_eq!(compact.terms.len(), 10);
        assert_eq!(compact.terms[0], ErlangTerm::Nil);
        let original = arena_to_value(&arena.terms, arena.root);
        let compact_wire = serde_json::json!({"root":compact.root.0,"terms":compact.terms.iter().map(|t|term_to_value(t,|id|id.0)).collect::<Vec<_>>()});
        assert_eq!(original, compact_wire);
        assert_eq!(arena.terms.len(), 12);
    }
    #[test]
    fn compact_context_transport_preserves_all_scalar_bits_and_metadata_order() {
        let arena = ErlangTerms::parse(&json!({"root":10,"terms":[
            {"kind":"atom","value":"unused"},
            {"kind":"integer","value":"1234567890123456789012345678901234567890"},
            {"kind":"float","value":"8000000000000000"},
            {"kind":"binary","value":"AAH+/w=="},
            {"kind":"bitstring","bytes":"oA==","bits":3},
            {"kind":"atom","value":"alpha"},
            {"kind":"nil"},
            {"kind":"list","items":[1,2],"tail":5},
            {"kind":"tuple","items":[3,4,7]},
            {"kind":"map","entries":[[5,8],[1,2]]},
            {"kind":"tuple","items":[9,6]}]}))
        .unwrap();
        let encoded = arena_to_value(&arena.terms, arena.root);
        let decoded = ErlangTerms::parse(&encoded).unwrap();
        assert_eq!(decoded.terms.len(), 10);
        assert_eq!(encoded["terms"][1]["value"], "8000000000000000");
        assert_eq!(encoded["terms"][8]["entries"], json!([[4, 7], [0, 1]]));
        assert_eq!(arena_to_value(&decoded.terms, decoded.root), encoded);
    }
}
