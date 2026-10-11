//! Native OTP 28.3 erl_expand_records expression preparation.
//! Adapted from Ericsson AB's Apache-2.0 erl_expand_records.erl.
//! The bridge uses the dialyzer option: record annotations are retained and
//! updates use tuple matches rather than setelement chains.
use super::super::{erl_bifs, term::Arena};
use crate::core::{ErlangTerm, ErlangTerms, TermId};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone)]
struct Access {
    key: Vec<u8>,
    anno: TermId,
    value: TermId,
    name: String,
    size: usize,
}
struct Expander {
    a: Arena,
    records: BTreeMap<String, Vec<(String, TermId)>>,
    count: usize,
    guard: bool,
    strict: bool,
    pending: Vec<Access>,
    checked: BTreeSet<Vec<u8>>,
    calls: BTreeSet<(String, usize)>,
}
pub(super) fn expand(forms: &ErlangTerms) -> Result<ErlangTerms, String> {
    let a = Arena::from_terms(forms);
    let input = a.list_items(forms.root)?;
    let mut e = Expander {
        a,
        records: BTreeMap::new(),
        count: 0,
        guard: false,
        strict: true,
        pending: Vec::new(),
        checked: BTreeSet::new(),
        calls: BTreeSet::new(),
    };
    for &id in &input {
        let f = e.a.tuple_fields(id)?.to_vec();
        if e.a.tag(id) == Some("function") {
            e.calls.insert((
                e.atom_name(f[2])?,
                e.a.as_integer(f[3]).ok_or("function arity")? as usize,
            ));
        }
        if e.a.tag(id) == Some("attribute") && e.a.as_atom(f[2]) == Some("import") {
            let p = e.a.tuple_fields(f[3])?.to_vec();
            for fa in e.a.list_items(p[1])? {
                let p = e.a.tuple_fields(fa)?;
                e.calls.insert((
                    e.atom_name(p[0])?,
                    e.a.as_integer(p[1]).ok_or("import arity")? as usize,
                ));
            }
        }
        if e.a.tag(id) == Some("attribute") && e.a.as_atom(f[2]) == Some("compile") {
            e.options(f[3])?;
        }
    }
    let mut out = Vec::new();
    for id in input {
        let mut f = e.a.tuple_fields(id)?.to_vec();
        if e.a.tag(id) == Some("attribute") && e.a.as_atom(f[2]) == Some("record") {
            let p = e.a.tuple_fields(f[3])?.to_vec();
            let name = e.atom_name(p[0])?;
            let mut defs = Vec::new();
            for field in e.a.list_items(p[1])? {
                let mut q = e.a.tuple_fields(field)?.to_vec();
                if e.a.tag(field) == Some("typed_record_field") {
                    q = e.a.tuple_fields(q[1])?.to_vec();
                }
                let n = e.a.tuple_fields(q[2])?.to_vec();
                let key = e.atom_name(n[2])?;
                let value = if q.len() == 4 {
                    q[3]
                } else {
                    e.lit_atom(q[1], "undefined")
                };
                defs.push((key, value));
            }
            e.records.insert(name, defs);
            out.push(id);
        } else if e.a.tag(id) == Some("function") {
            f[4] = e.clauses(f[4])?;
            out.push(e.a.tuple(f));
        } else {
            out.push(id);
        }
    }
    let root = e.a.list(out);
    Ok(e.a.finish(root))
}
impl Expander {
    fn options(&mut self, id: TermId) -> Result<(), String> {
        if let Some(s) = self.a.as_atom(id) {
            if s == "no_strict_record_tests" {
                self.strict = false;
            }
            if s == "strict_record_tests" {
                self.strict = true;
            }
            return Ok(());
        }
        if matches!(self.a.get(id), ErlangTerm::List { .. } | ErlangTerm::Nil) {
            for x in self.a.list_items(id)? {
                self.options(x)?;
            }
        }
        Ok(())
    }
    fn atom_name(&self, id: TermId) -> Result<String, String> {
        self.a
            .as_atom(id)
            .map(str::to_owned)
            .ok_or_else(|| format!("record preparation: expected atom at {}", id.0))
    }
    fn lit_atom(&mut self, an: TermId, s: &str) -> TermId {
        let x = self.a.atom(s);
        self.a.tagged("atom", vec![an, x])
    }
    fn lit_int(&mut self, an: TermId, n: usize) -> TermId {
        let x = self.a.integer(n);
        self.a.tagged("integer", vec![an, x])
    }
    fn var(&mut self, an: TermId) -> TermId {
        let n = self.a.atom(format!("rec{}", self.count));
        self.count += 1;
        self.a.tagged("var", vec![an, n])
    }
    fn wild(&mut self, an: TermId) -> TermId {
        let n = self.a.atom("_");
        self.a.tagged("var", vec![an, n])
    }
    fn anno_flag(&mut self, an: TermId, key: &str) -> Result<TermId, String> {
        let mut xs = if matches!(self.a.get(an), ErlangTerm::List { .. }) {
            self.a.list_items(an)?
        } else {
            let k = self.a.atom("location");
            vec![self.a.tuple(vec![k, an])]
        };
        let k = self.a.atom(key);
        let t = self.a.atom("true");
        let pair = self.a.tuple(vec![k, t]);
        if let Some(i) = xs.iter().position(|x| {
            self.a
                .tuple_fields(*x)
                .ok()
                .and_then(|p| p.first())
                .and_then(|k| self.a.as_atom(*k))
                == Some(key)
        }) {
            xs[i] = pair;
        } else {
            xs.insert(0, pair);
        }
        Ok(self.a.list(xs))
    }
    fn tuple_expr(&mut self, an: TermId, xs: Vec<TermId>) -> TermId {
        let list = self.a.list(xs);
        self.a.tagged("tuple", vec![an, list])
    }
    fn call(&mut self, an: TermId, name: &str, args: Vec<TermId>) -> TermId {
        let m = self.lit_atom(an, "erlang");
        let f = self.lit_atom(an, name);
        let target = self.a.tagged("remote", vec![an, m, f]);
        let args = self.a.list(args);
        self.a.tagged("call", vec![an, target, args])
    }
    fn op(&mut self, an: TermId, name: &str, l: TermId, r: TermId) -> TermId {
        let n = self.a.atom(name);
        self.a.tagged("op", vec![an, n, l, r])
    }
    fn clause(&mut self, an: TermId, heads: Vec<TermId>, body: Vec<TermId>) -> TermId {
        let h = self.a.list(heads);
        let g = self.a.nil();
        let b = self.a.list(body);
        self.a.tagged("clause", vec![an, h, g, b])
    }
    fn field_values(
        &self,
        id: TermId,
    ) -> Result<(BTreeMap<String, TermId>, Option<TermId>), String> {
        let mut fields = BTreeMap::new();
        let mut wild = None;
        for x in self.a.list_items(id)? {
            let f = self.a.tuple_fields(x)?;
            let key = self.a.tuple_fields(f[2])?;
            let name = self.atom_name(key[2])?;
            if self.a.tag(f[2]) == Some("var") && name == "_" {
                wild = Some(f[3]);
            } else {
                fields.entry(name).or_insert(f[3]);
            }
        }
        Ok((fields, wild))
    }
    fn defs(&self, name: &str) -> Result<Vec<(String, TermId)>, String> {
        self.records
            .get(name)
            .cloned()
            .ok_or_else(|| format!("record preparation: undefined record {name}"))
    }
    fn index(&self, name: &str, field: TermId) -> Result<usize, String> {
        let f = self.a.tuple_fields(field)?;
        let key = self.atom_name(f[2])?;
        self.defs(name)?
            .iter()
            .position(|(n, _)| n == &key)
            .map(|i| i + 2)
            .ok_or_else(|| format!("record preparation: {name}.{key} undefined"))
    }
    fn reanno(&mut self, id: TermId, an: TermId) -> Result<TermId, String> {
        match self.a.get(id).clone() {
            ErlangTerm::Tuple(mut f) => {
                let annotated =
                    f.len() > 1 && !matches!(self.a.as_atom(f[0]), Some("function" | "clauses"));
                if annotated {
                    f[1] = an;
                }
                let end = if self.a.as_atom(f[0]) == Some("bin_element") {
                    4
                } else {
                    f.len()
                };
                for x in f.iter_mut().take(end).skip(if annotated { 2 } else { 1 }) {
                    *x = self.reanno(*x, an)?;
                }
                Ok(self.a.tuple(f))
            }
            ErlangTerm::List { items, tail } => {
                let mut xs = Vec::new();
                for x in items {
                    xs.push(self.reanno(x, an)?);
                }
                let tail = self.reanno(tail, an)?;
                Ok(self.a.push(ErlangTerm::List { items: xs, tail }))
            }
            _ => Ok(id),
        }
    }
    fn list(&mut self, id: TermId, pattern: bool) -> Result<TermId, String> {
        let mut out = Vec::new();
        for x in self.a.list_items(id)? {
            out.push(self.expr(x, pattern)?);
        }
        Ok(self.a.list(out))
    }
    fn clauses(&mut self, id: TermId) -> Result<TermId, String> {
        let mut out = Vec::new();
        for c in self.a.list_items(id)? {
            let mut f = self.a.tuple_fields(c)?.to_vec();
            f[2] = self.list(f[2], true)?;
            let mut guards = Vec::new();
            for gs in self.a.list_items(f[3])? {
                let old = self.guard;
                self.guard = true;
                let mut tests = Vec::new();
                for g in self.a.list_items(gs)? {
                    let g = self.guard_alias(g)?;
                    let g = self.expr(g, false)?;
                    tests.push(self.flush(g)?);
                }
                self.checked.clear();
                self.guard = old;
                guards.push(self.a.list(tests));
            }
            f[3] = self.a.list(guards);
            f[4] = self.list(f[4], false)?;
            out.push(self.a.tuple(f));
        }
        Ok(self.a.list(out))
    }
    fn guard_alias(&mut self, id: TermId) -> Result<TermId, String> {
        let mut f = self.a.tuple_fields(id)?.to_vec();
        if self.a.tag(id) == Some("call") {
            let mut t = self.a.tuple_fields(f[2])?.to_vec();
            if self.a.tag(f[2]) == Some("atom") {
                let n = self.atom_name(t[2])?;
                let ar = self.a.list_items(f[3])?.len();
                let replacement = match (n.as_str(), ar) {
                    ("atom", 1) => Some("is_atom"),
                    ("binary", 1) => Some("is_binary"),
                    ("float", 1) => Some("is_float"),
                    ("function", 1) => Some("is_function"),
                    ("integer", 1) => Some("is_integer"),
                    ("list", 1) => Some("is_list"),
                    ("number", 1) => Some("is_number"),
                    ("pid", 1) => Some("is_pid"),
                    ("port", 1) => Some("is_port"),
                    ("record", 2) => Some("is_record"),
                    ("reference", 1) => Some("is_reference"),
                    ("tuple", 1) => Some("is_tuple"),
                    _ => None,
                };
                if let Some(n) = replacement {
                    t[2] = self.a.atom(n);
                    f[2] = self.a.tuple(t);
                    return Ok(self.a.tuple(f));
                }
            }
        }
        Ok(id)
    }
    fn flush(&mut self, e: TermId) -> Result<TermId, String> {
        if self.pending.is_empty() {
            return Ok(e);
        }
        let pending = std::mem::take(&mut self.pending);
        let mut fresh = Vec::new();
        for x in &pending {
            if self.checked.insert(x.key.clone()) {
                fresh.insert(0, x.clone());
            }
        }
        if fresh.is_empty() {
            return Ok(e);
        }
        let joined = self.conjunction(&fresh, Some(e))?;
        self.expr(joined, false)
    }
    fn conjunction(&mut self, xs: &[Access], end: Option<TermId>) -> Result<TermId, String> {
        let x = &xs[0];
        let n = self.anno_flag(x.anno, "generated")?;
        let name = self.lit_atom(n, &x.name);
        let size = self.lit_int(n, x.size);
        let test = self.call(n, "is_record", vec![x.value, name, size]);
        let fail = self.lit_atom(n, "fail");
        let t = self.op(n, "orelse", test, fail);
        let t = if xs.len() > 1 {
            let rest = self.conjunction(&xs[1..], None)?;
            self.op(n, "and", rest, t)
        } else {
            t
        };
        Ok(if let Some(end) = end {
            self.op(n, "and", t, end)
        } else if xs.len() > 1 {
            t
        } else {
            let yes = self.lit_atom(n, "true");
            self.op(n, "and", t, yes)
        })
    }

    fn simple(&self, id: TermId) -> bool {
        self.a.tag(id) == Some("var") || self.literal(id)
    }
    fn literal(&self, id: TermId) -> bool {
        let Ok(f) = self.a.tuple_fields(id) else {
            return false;
        };
        match self.a.tag(id) {
            Some("atom" | "integer" | "char" | "float" | "string" | "nil") => true,
            Some("tuple") => self
                .a
                .list_items(f[2])
                .is_ok_and(|xs| xs.into_iter().all(|x| self.literal(x))),
            Some("cons") => self.literal(f[2]) && self.literal(f[3]),
            Some("map") if f.len() == 3 => self.a.list_items(f[2]).is_ok_and(|xs| {
                xs.into_iter().all(|x| {
                    self.a.tag(x) == Some("map_field_assoc")
                        && self
                            .a
                            .tuple_fields(x)
                            .is_ok_and(|p| self.literal(p[2]) && self.literal(p[3]))
                })
            }),
            Some("bin") => self.a.list_items(f[2]).is_ok_and(|xs| {
                xs.into_iter().all(|x| {
                    self.a.tuple_fields(x).is_ok_and(|p| {
                        self.literal(p[2])
                            && (self.a.as_atom(p[3]) == Some("default") || self.literal(p[3]))
                    })
                })
            }),
            Some("fun") => self.a.tuple_fields(f[2]).is_ok_and(|t| {
                t.len() == 4
                    && self.a.as_atom(t[0]) == Some("function")
                    && self.a.tag(t[1]) == Some("atom")
                    && self.a.tag(t[2]) == Some("atom")
                    && self.a.tag(t[3]) == Some("integer")
            }),
            Some("op") if f.len() == 4 && matches!(self.a.as_atom(f[2]), Some("+" | "-")) => {
                matches!(self.a.tag(f[3]), Some("char" | "integer" | "float"))
            }
            _ => false,
        }
    }

    fn expr(&mut self, id: TermId, pattern: bool) -> Result<TermId, String> {
        let Ok(fields) = self.a.tuple_fields(id) else {
            return Ok(id);
        };
        let mut f = fields.to_vec();
        let tag = self.a.tag(id).unwrap_or("").to_owned();
        let an = *f
            .get(1)
            .ok_or("record preparation: expression annotation")?;
        match tag.as_str() {
            "var" | "atom" | "integer" | "char" | "float" | "string" | "nil" | "debug_line"
            | "executable_line" | "ssa_check_when" => return Ok(id),
            "record_index" => {
                let name = self.atom_name(f[2])?;
                let i = self.index(&name, f[3])?;
                return Ok(self.lit_int(an, i));
            }
            "record" if f.len() == 4 => {
                let name = self.atom_name(f[2])?;
                let defs = self.defs(&name)?;
                let (values, wild) = self.field_values(f[3])?;
                let mut xs = vec![self.lit_atom(an, &name)];
                for (key, default) in defs {
                    let value = if let Some(x) = values.get(&key) {
                        *x
                    } else if let Some(x) = wild {
                        x
                    } else if pattern {
                        self.wild(an)
                    } else {
                        self.reanno(default, an)?
                    };
                    xs.push(self.expr(value, pattern)?);
                }
                let ann = self.anno_flag(an, "record")?;
                return Ok(self.tuple_expr(ann, xs));
            }
            "record" if f.len() == 5 => return self.update(f[2], f[3], f[4], an),
            "record_field" if f.len() == 5 => return self.access(f[2], f[3], f[4]),
            "call" => {
                let args = self.a.list_items(f[3])?;
                let target = self.a.tuple_fields(f[2])?.to_vec();
                let local = self.a.tag(f[2]) == Some("atom");
                let name = if local {
                    self.a.as_atom(target[2])
                } else if self.a.tag(f[2]) == Some("remote")
                    && self
                        .a
                        .tuple_fields(target[2])
                        .ok()
                        .and_then(|p| p.get(2))
                        .and_then(|x| self.a.as_atom(*x))
                        == Some("erlang")
                {
                    self.a
                        .tuple_fields(target[3])
                        .ok()
                        .and_then(|p| p.get(2))
                        .and_then(|x| self.a.as_atom(*x))
                } else {
                    None
                }
                .map(str::to_owned);
                if name.as_deref() == Some("is_record")
                    && args.len() == 2
                    && self.a.tag(args[1]) == Some("atom")
                {
                    let n = self.a.tuple_fields(args[1])?[2];
                    let n = self.atom_name(n)?;
                    return self.record_test(an, args[0], &n);
                }
                if name.as_deref() == Some("is_record")
                    && args.len() == 3
                    && self.a.tag(args[2]) == Some("integer")
                    && self
                        .a
                        .as_integer(self.a.tuple_fields(args[2])?[2])
                        .is_some_and(|n| n <= 0)
                {
                    return Ok(self.lit_atom(an, "false"));
                }
                if local && name.as_deref() == Some("record_info") && args.len() == 2 {
                    let n = self.atom_name(self.a.tuple_fields(args[1])?[2])?;
                    let defs = self.defs(&n)?;
                    let kind = self.atom_name(self.a.tuple_fields(args[0])?[2])?;
                    if kind == "size" {
                        return Ok(self.lit_int(an, defs.len() + 1));
                    }
                    if kind == "fields" {
                        let mut tail = self.a.tagged("nil", vec![an]);
                        for (n, _) in defs.iter().rev() {
                            let h = self.lit_atom(an, n);
                            tail = self.a.tagged("cons", vec![an, h, tail]);
                        }
                        return Ok(tail);
                    }
                    return Err(format!("record_info unsupported kind {kind}"));
                }
                if self.a.tag(f[2]) == Some("remote") {
                    let mut t = target;
                    t[2] = self.expr(t[2], false)?;
                    t[3] = self.expr(t[3], false)?;
                    f[2] = self.a.tuple(t);
                } else {
                    f[2] = self.expr(f[2], false)?;
                }
                f[3] = self.list(f[3], false)?;
            }
            "fun" => {
                let mut t = self.a.tuple_fields(f[2])?.to_vec();
                if self.a.as_atom(t[0]) == Some("clauses") {
                    let old = self.guard;
                    self.guard = false;
                    t[1] = self.clauses(t[1])?;
                    self.guard = old;
                    f[2] = self.a.tuple(t);
                } else if t.len() == 3 && self.a.as_atom(t[0]) == Some("function") {
                    let name = self.atom_name(t[1])?;
                    let arity = self.a.as_integer(t[2]).ok_or("capture arity")? as usize;
                    if !self.calls.contains(&(name.clone(), arity))
                        && !erl_bifs::is_auto_bif(&name, arity)
                    {
                        let mut vs = Vec::new();
                        for _ in 0..arity {
                            vs.insert(0, self.var(an));
                        }
                        let args = self.a.list(vs.clone());
                        let callee = self.lit_atom(an, &name);
                        let call = self.a.tagged("call", vec![an, callee, args]);
                        let c = self.clause(an, vs, vec![call]);
                        let cs = self.a.list(vec![c]);
                        let cs = self.a.tagged("clauses", vec![cs]);
                        let fun = self.a.tagged("fun", vec![an, cs]);
                        return self.expr(fun, false);
                    }
                }
                return Ok(self.a.tuple(f));
            }
            "named_fun" => {
                let old = self.guard;
                self.guard = false;
                f[3] = self.clauses(f[3])?;
                self.guard = old;
            }
            "case" => {
                f[2] = self.expr(f[2], false)?;
                f[3] = self.clauses(f[3])?;
            }
            "if" => f[2] = self.clauses(f[2])?,
            "receive" => {
                if f.len() == 5 {
                    f[3] = self.expr(f[3], false)?;
                    f[4] = self.list(f[4], false)?;
                }
                f[2] = self.clauses(f[2])?;
            }
            "try" => {
                f[2] = self.list(f[2], false)?;
                f[3] = self.clauses(f[3])?;
                f[4] = self.clauses(f[4])?;
                f[5] = self.list(f[5], false)?;
            }
            "match" | "maybe_match" => {
                f[3] = self.expr(f[3], pattern)?;
                f[2] = self.expr(f[2], true)?;
            }
            "tuple" | "block" => f[2] = self.list(f[2], pattern)?,
            "map" => {
                if f.len() == 4 {
                    f[2] = self.expr(f[2], false)?;
                    f[3] = self.list(f[3], false)?;
                } else {
                    f[2] = self.list(f[2], pattern)?;
                }
            }
            "map_field_assoc" | "map_field_exact" => {
                f[2] = self.expr(f[2], false)?;
                f[3] = self.expr(f[3], pattern)?;
            }
            "bin" => {
                let xs = self.a.list_items(f[2])?;
                let mut out = Vec::new();
                for x in xs.into_iter().rev() {
                    let mut q = self.a.tuple_fields(x)?.to_vec();
                    q[2] = self.expr(q[2], pattern)?;
                    if self.a.as_atom(q[3]) != Some("default") {
                        q[3] = self.expr(q[3], false)?;
                    }
                    out.insert(0, self.a.tuple(q));
                }
                f[2] = self.a.list(out);
            }
            "op" => {
                let name = self.atom_name(f[2])?;
                let boolean = matches!(name.as_str(), "not" | "and" | "or" | "andalso" | "orelse")
                    && !pattern;
                f[3] = self.expr(f[3], pattern)?;
                if boolean {
                    f[3] = self.flush(f[3])?;
                }
                let checked = self.checked.clone();
                if f.len() == 5 {
                    f[4] = self.expr(f[4], pattern)?;
                    if boolean {
                        f[4] = self.flush(f[4])?;
                    }
                    if matches!(name.as_str(), "andalso" | "orelse") {
                        self.checked = checked;
                    }
                }
            }
            "cons" => {
                f[2] = self.expr(f[2], pattern)?;
                f[3] = self.expr(f[3], pattern)?;
            }
            "catch" => f[2] = self.expr(f[2], false)?,
            "lc" | "bc" | "mc" => {
                f[3] = self.qualifiers(f[3])?;
                f[2] = self.expr(f[2], false)?;
            }
            "maybe" => {
                f[2] = self.list(f[2], false)?;
                if f.len() == 4 {
                    let mut t = self.a.tuple_fields(f[3])?.to_vec();
                    t[2] = self.clauses(t[2])?;
                    f[3] = self.a.tuple(t);
                }
            }
            "remote" => return Ok(id),
            _ => {
                return Err(format!(
                    "record preparation: unsupported {} expression",
                    tag
                ));
            }
        }
        Ok(self.a.tuple(f))
    }
    fn qualifiers(&mut self, id: TermId) -> Result<TermId, String> {
        let mut out = Vec::new();
        for q in self.a.list_items(id)? {
            let mut f = self.a.tuple_fields(q)?.to_vec();
            let tag = self.a.tag(q).unwrap_or("");
            if matches!(
                tag,
                "generate"
                    | "generate_strict"
                    | "b_generate"
                    | "b_generate_strict"
                    | "m_generate"
                    | "m_generate_strict"
            ) {
                f[3] = self.expr(f[3], false)?;
                f[2] = self.expr(f[2], true)?;
                out.push(self.a.tuple(f));
            } else if tag == "zip" {
                f[2] = self.qualifiers(f[2])?;
                out.push(self.a.tuple(f));
            } else {
                let guard = self.guard_candidate(q);
                let old = self.guard;
                self.guard = guard;
                let q = if guard { self.guard_alias(q)? } else { q };
                let q = self.expr(q, false)?;
                let q = if guard { self.flush(q)? } else { q };
                self.guard = old;
                out.push(q);
            }
        }
        self.checked.clear();
        Ok(self.a.list(out))
    }
    fn guard_candidate(&self, id: TermId) -> bool {
        let Ok(f) = self.a.tuple_fields(id) else {
            return false;
        };
        match self.a.tag(id) {
            Some(
                "atom" | "integer" | "char" | "float" | "string" | "nil" | "var" | "record_index",
            ) => true,
            Some("record_field") => self.guard_candidate(f[2]),
            Some("op") => f.iter().skip(3).all(|x| self.guard_candidate(*x)),
            Some("call") => {
                let Ok(t) = self.a.tuple_fields(f[2]) else {
                    return false;
                };
                let args = self.a.list_items(f[3]).unwrap_or_default();
                let name = if self.a.tag(f[2]) == Some("atom") {
                    self.a.as_atom(t[2])
                } else if self.a.tag(f[2]) == Some("remote")
                    && self
                        .a
                        .tuple_fields(t[2])
                        .ok()
                        .and_then(|p| p.get(2))
                        .and_then(|x| self.a.as_atom(*x))
                        == Some("erlang")
                {
                    self.a
                        .tuple_fields(t[3])
                        .ok()
                        .and_then(|p| p.get(2))
                        .and_then(|x| self.a.as_atom(*x))
                } else {
                    None
                };
                name.is_some_and(|n| {
                    !self.calls.contains(&(n.into(), args.len())) && is_guard_bif(n, args.len())
                }) && args.into_iter().all(|x| self.guard_candidate(x))
            }
            Some("tuple" | "map") => self
                .a
                .list_items(f[2])
                .is_ok_and(|xs| xs.into_iter().all(|x| self.guard_candidate(x))),
            Some("cons" | "map_field_assoc" | "map_field_exact") => {
                self.guard_candidate(f[2]) && self.guard_candidate(f[3])
            }
            Some("bin") => self.a.list_items(f[2]).is_ok_and(|xs| {
                xs.into_iter().all(|x| {
                    let f = self.a.tuple_fields(x).unwrap();
                    self.guard_candidate(f[2])
                        && (self.a.as_atom(f[3]) == Some("default") || self.guard_candidate(f[3]))
                })
            }),
            _ => false,
        }
    }
    fn record_test(&mut self, an: TermId, value: TermId, name: &str) -> Result<TermId, String> {
        let size = self.defs(name)?.len() + 1;
        if self.guard
            && matches!(
                self.a.tag(value),
                Some(
                    "atom"
                        | "integer"
                        | "float"
                        | "nil"
                        | "cons"
                        | "char"
                        | "string"
                        | "record_index"
                        | "bin"
                        | "op"
                )
            )
        {
            return Ok(self.lit_atom(an, "false"));
        }
        let n = self.anno_flag(an, "generated")?;
        let name = self.lit_atom(an, name);
        let size = self.lit_int(an, size);
        if self.guard {
            let c = self.call(n, "is_record", vec![value, name, size]);
            self.expr(c, false)
        } else {
            let v = self.var(an);
            let m = self.a.tagged("match", vec![an, v, value]);
            let c = self.call(n, "is_record", vec![v, name, size]);
            let list = self.a.list(vec![m, c]);
            let b = self.a.tagged("block", vec![an, list]);
            self.expr(b, false)
        }
    }
    fn access(&mut self, value: TermId, name: TermId, field: TermId) -> Result<TermId, String> {
        let name = self.atom_name(name)?;
        let an = self.first_anno(value)?;
        let defs = self.defs(&name)?;
        let index = self.index(&name, field)?;
        if !self.strict {
            let i = self.lit_int(an, index);
            let c = self.call(an, "element", vec![i, value]);
            return self.expr(c, false);
        }
        if self.guard {
            let value = self.expr(value, false)?;
            let zero = self.a.integer(0);
            let normalized = self.reanno(value, zero)?;
            let normalized = self.a.snapshot(normalized);
            let mut key = crate::external_term::encode(
                &normalized,
                crate::external_term::Limits::default(),
                false,
            )
            .map_err(|e| e.to_string())?;
            key.extend(name.as_bytes());
            self.pending.insert(
                0,
                Access {
                    key,
                    anno: an,
                    value,
                    name,
                    size: defs.len() + 1,
                },
            );
            let i = self.lit_int(an, index);
            return Ok(self.call(an, "element", vec![i, value]));
        }
        let v = self.var(an);
        let mut xs = vec![self.lit_atom(an, &name)];
        for i in 2..=defs.len() + 1 {
            xs.push(if i == index { v } else { self.wild(an) });
        }
        let n = self.anno_flag(an, "generated")?;
        let r = self.anno_flag(n, "record")?;
        let p = self.tuple_expr(r, xs);
        let c1 = self.clause(n, vec![p], vec![v]);
        let bad = self.lit_atom(n, "badrecord");
        let bad = self.tuple_expr(n, vec![bad, v]);
        let error = self.call(n, "error", vec![bad]);
        let c2 = self.clause(n, vec![v], vec![error]);
        let cs = self.a.list(vec![c1, c2]);
        let c = self.a.tagged("case", vec![an, value, cs]);
        self.expr(c, false)
    }
    fn first_anno(&self, id: TermId) -> Result<TermId, String> {
        let f = self.a.tuple_fields(id)?;
        match self.a.tag(id) {
            Some("record_field") => self.first_anno(f[2]),
            Some("op") if f.len() == 5 => self.first_anno(f[3]),
            Some("match") => self.first_anno(f[2]),
            _ => f
                .get(1)
                .copied()
                .ok_or("expression has no annotation".into()),
        }
    }
    fn update(
        &mut self,
        value: TermId,
        name: TermId,
        updates: TermId,
        outer: TermId,
    ) -> Result<TermId, String> {
        let name = self.atom_name(name)?;
        let defs = self.defs(&name)?;
        let an = self.a.tuple_fields(value)?[1];
        let mut pre = Vec::new();
        let mut values = BTreeMap::new();
        for u in self.a.list_items(updates)? {
            let q = self.a.tuple_fields(u)?.to_vec();
            let field = self.atom_name(self.a.tuple_fields(q[2])?[2])?;
            let val = if self.simple(q[3]) {
                q[3]
            } else {
                let v = self.var(q[1]);
                pre.push(self.a.tagged("match", vec![q[1], v, q[3]]));
                v
            };
            values.insert(field, val);
        }
        let v = self.var(an);
        let mut ps = vec![self.lit_atom(an, &name)];
        let mut news = vec![self.lit_atom(an, &name)];
        for (field, _) in defs {
            let p = self.var(outer);
            ps.push(p);
            news.push(values.get(&field).copied().unwrap_or(p));
        }
        let r = self.anno_flag(an, "record")?;
        let p = self.tuple_expr(r, ps);
        let result = self.tuple_expr(r, news);
        let c1 = self.clause(an, vec![p], vec![result]);
        let n = self.anno_flag(an, "generated")?;
        let w = self.wild(n);
        let bad = self.lit_atom(n, "badrecord");
        let bad = self.tuple_expr(n, vec![bad, v]);
        let error = self.call(n, "error", vec![bad]);
        let c2 = self.clause(n, vec![w], vec![error]);
        let cs = self.a.list(vec![c1, c2]);
        let case = self.a.tagged("case", vec![an, v, cs]);
        pre.push(self.a.tagged("match", vec![an, v, value]));
        pre.push(case);
        let list = self.a.list(pre);
        let b = self.a.tagged("block", vec![an, list]);
        self.expr(b, false)
    }
}
fn is_guard_bif(n: &str, a: usize) -> bool {
    matches!(
        (n, a),
        ("abs", 1)
            | ("ceil", 1)
            | ("floor", 1)
            | ("round", 1)
            | ("trunc", 1)
            | ("float", 1)
            | ("hd", 1)
            | ("tl", 1)
            | ("length", 1)
            | ("size", 1)
            | ("tuple_size", 1)
            | ("map_size", 1)
            | ("map_get", 2)
            | ("is_map_key", 2)
            | ("element", 2)
            | ("binary_part", 2)
            | ("binary_part", 3)
            | ("bit_size", 1)
            | ("byte_size", 1)
            | ("self", 0)
            | ("node", 0)
            | ("node", 1)
            | ("is_record", 2)
            | ("is_record", 3)
            | ("record", 2)
            | ("is_function", 2)
    ) || (a == 1
        && matches!(
            n,
            "is_atom"
                | "is_binary"
                | "is_bitstring"
                | "is_boolean"
                | "is_float"
                | "is_function"
                | "is_integer"
                | "is_list"
                | "is_map"
                | "is_number"
                | "is_pid"
                | "is_port"
                | "is_reference"
                | "is_tuple"
                | "atom"
                | "binary"
                | "float"
                | "function"
                | "integer"
                | "list"
                | "number"
                | "pid"
                | "port"
                | "reference"
                | "tuple"
        ))
}
