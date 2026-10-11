//! OTP 28.3 v3_core comprehension translation (Apache-2.0; Ericsson AB).
//! Qualifiers are prepared before recursive loop emission to retain OTP's
//! variable/function numbering, evaluation order, and strict-match failures.
use super::*;
#[derive(Clone)]
struct Generator {
    known_before: HashSet<String>,
    scope_names: HashSet<String>,
    ann: TermId,
    accept: TermId,
    guard: TermId,
    has_guard: bool,
    allocations: Vec<TermId>,
    skip: TermId,
    mismatch: Option<TermId>,
    tail: TermId,
    end: TermId,
    refill: Option<(TermId, TermId)>,
    input: Expr,
    binary: bool,
}
#[derive(Clone)]
enum Qualifier {
    Gen(Generator),
    Zip { ann: TermId, gens: Vec<Generator> },
    Guard { ann: TermId, test: TermId },
    Filter { ann: TermId, arg: Expr },
}
impl Lower {
    pub(super) fn comprehension_append(&mut self, node: TermId, tail: TermId) -> Result<Expr> {
        let (values, mut pre) = self.safe_list(vec![tail])?;
        let f = self.a.tuple_fields(node)?.to_vec();
        let known = self.known_vars.clone();
        let qs = self.comp_qualifiers(f[1], f[3], &BTreeSet::new())?;
        let mut result = self.comp_emit(f[1], f[2], &qs, values[0], false)?;
        pre.append(&mut result.pre);
        result.pre = pre;
        self.known_vars = known;
        Ok(result)
    }

    pub(super) fn comprehension(&mut self, node: TermId) -> Result<Expr> {
        let f = self.a.tuple_fields(node)?.to_vec();
        let kind = self.a.tag(node).unwrap().to_owned();
        let ann = self.annotation(f[1])?;
        let empty = self.empty();
        let accumulator = if kind == "bc" {
            Some(self.fresh(empty))
        } else {
            None
        };
        let known = self.known_vars.clone();
        let mut qs = self.comp_qualifiers(f[1], f[3], &BTreeSet::new())?;
        let mut prefix = Vec::new();
        if let Some(bin) = accumulator {
            if let Some(Qualifier::Gen(g)) = qs.first_mut() {
                prefix.append(&mut g.input.pre);
            }
            let n = self.a.integer(256);
            let n = self.literal(empty, n);
            let init = self.comp_primop(ann, "bs_init_writable", vec![n]);
            prefix.push(Pre::Set(bin, init));
        }
        let nil = self.empty();
        let more = if let Some(bin) = accumulator {
            bin
        } else {
            self.literal(ann, nil)
        };
        let mut value = f[2];
        if kind == "mc" {
            let p = self.a.tuple_fields(value)?.to_vec();
            let es = self.a.list(vec![p[2], p[3]]);
            value = self.a.tagged("tuple", vec![p[1], es]);
        }
        let mut result = self.comp_emit(f[1], value, &qs, more, kind == "bc")?;
        prefix.append(&mut result.pre);
        result.pre = prefix;
        if kind == "mc" {
            let empty = self.empty();
            let var = self.fresh(empty);
            result.pre.push(Pre::Set(var, result.value));
            result.value = self.comp_call(empty, "maps", "from_list", vec![var], false);
        }
        self.known_vars = known;
        Ok(result)
    }
    fn comp_mark(&mut self, ann: TermId, markers: &[&str]) -> Result<TermId> {
        let mut xs = Vec::new();
        for m in markers {
            xs.push(self.a.atom(*m));
        }
        xs.extend(self.a.list_items(ann)?);
        Ok(self.a.list(xs))
    }
    fn comp_reann(&mut self, id: TermId, ann: TermId) -> Result<TermId> {
        let mut f = self.a.tuple_fields(id)?.to_vec();
        f[1] = ann;
        Ok(self.a.tuple(f))
    }
    fn comp_vars(&mut self, n: usize, ann: TermId) -> Vec<TermId> {
        let mut out = Vec::new();
        for _ in 0..n {
            out.insert(0, self.fresh(ann));
        }
        out
    }
    fn comp_apply(&mut self, ann: TermId, op: TermId, args: Vec<TermId>) -> TermId {
        let args = self.a.list(args);
        self.a.tagged("c_apply", vec![ann, op, args])
    }
    fn comp_call(
        &mut self,
        ann: TermId,
        module: &str,
        name: &str,
        args: Vec<TermId>,
        annotate: bool,
    ) -> TermId {
        let empty = self.empty();
        let ma = self.a.atom(module);
        let fa = self.a.atom(name);
        let la = if annotate { ann } else { empty };
        let ma = self.literal(la, ma);
        let fa = self.literal(la, fa);
        let args = self.a.list(args);
        self.a.tagged("c_call", vec![ann, ma, fa, args])
    }
    fn comp_primop(&mut self, ann: TermId, name: &str, args: Vec<TermId>) -> TermId {
        let name = self.atom_literal(name);
        let args = self.a.list(args);
        self.a.tagged("c_primop", vec![ann, name, args])
    }
    fn comp_tuple(&mut self, ann: TermId, xs: Vec<TermId>) -> TermId {
        let xs = self.a.list(xs);
        self.a.tagged("c_tuple", vec![ann, xs])
    }
    fn comp_clone_wildcards(&mut self, node: TermId) -> Result<TermId> {
        if self.a.tag(node) == Some("c_var") {
            let f = self.a.tuple_fields(node)?.to_vec();
            if self.a.as_atom(f[2]) == Some("_") {
                return Ok(self.a.tuple(f));
            }
            return Ok(node);
        }
        if self.a.tag(node) == Some("c_literal") {
            return Ok(node);
        }
        match self.a.get(node).clone() {
            ErlangTerm::Tuple(mut fields) => {
                for child in fields.iter_mut().skip(
                    if self.a.tag(node).is_some_and(|tag| tag.starts_with("c_")) {
                        2
                    } else {
                        0
                    },
                ) {
                    *child = self.comp_clone_wildcards(*child)?;
                }
                Ok(self.a.tuple(fields))
            }
            ErlangTerm::List { items, tail } => {
                let mut out = Vec::new();
                for child in items {
                    out.push(self.comp_clone_wildcards(child)?);
                }
                let tail = self.comp_clone_wildcards(tail)?;
                Ok(self.a.push(ErlangTerm::List { items: out, tail }))
            }
            _ => Ok(node),
        }
    }
    fn comp_clause(
        &mut self,
        ann: TermId,
        pats: Vec<TermId>,
        guard: TermId,
        body: TermId,
    ) -> Result<TermId> {
        let outer = std::mem::take(&mut self.pin_tests);
        let old = self.case_patterns;
        self.case_patterns = false;
        let mut bound = HashSet::new();
        let mut normalized = Vec::new();
        for pat in pats {
            let pat = self.comp_clone_wildcards(pat)?;
            normalized.push(self.normalize_pattern(pat, &mut bound)?);
        }
        self.case_patterns = old;
        let pins = std::mem::take(&mut self.pin_tests);
        self.pin_tests = outer;
        let empty_guard = self.a.tag(guard) == Some("c_literal")
            && self.a.as_atom(self.a.tuple_fields(guard)?[2]) == Some("true");
        let (guard, allocations) = self.compose_pattern_guard(pins, guard, empty_guard)?;
        let pats = self.a.list(normalized);
        let clause = self.a.tagged("c_clause", vec![ann, pats, guard, body]);
        if !allocations.is_empty() {
            self.guard_allocations.insert(clause, allocations);
        }
        Ok(clause)
    }
    fn comp_bad_generator(
        &mut self,
        pats: Vec<TermId>,
        bad: TermId,
        input: TermId,
        plural: bool,
    ) -> Result<TermId> {
        let ann = self.a.tuple_fields(input)?[1];
        let reason = self.atom_literal(if plural {
            "bad_generators"
        } else {
            "bad_generator"
        });
        let reason = self.comp_tuple(ann, vec![reason, bad]);
        let call = self.comp_call(ann, "erlang", "error", vec![reason], true);
        let empty = self.empty();
        let an = self.comp_mark(empty, &["compiler_generated"])?;
        let truth = self.atom_literal("true");
        Ok(self.comp_clause(an, pats, truth, call)?)
    }
    fn comp_is_guard(&self, node: TermId) -> bool {
        let Ok(f) = self.a.tuple_fields(node) else {
            return false;
        };
        match self.a.tag(node) {
            Some("var" | "atom" | "char" | "integer" | "float" | "string" | "nil") => true,
            Some("tuple") => self
                .a
                .list_items(f[2])
                .is_ok_and(|xs| xs.into_iter().all(|x| self.comp_is_guard(x))),
            Some("cons" | "map_field_assoc" | "map_field_exact") => {
                self.comp_is_guard(f[2]) && self.comp_is_guard(f[3])
            }
            Some("map") => {
                self.a
                    .list_items(*f.last().unwrap())
                    .is_ok_and(|xs| xs.into_iter().all(|x| self.comp_is_guard(x)))
                    && (f.len() == 3 || self.comp_is_guard(f[2]))
            }
            Some("op") => {
                self.a
                    .as_atom(f[2])
                    .is_some_and(|n| guard_op(n, f.len() - 3))
                    && f[3..].iter().all(|x| self.comp_is_guard(*x))
            }
            Some("call") => {
                let Ok(t) = self.a.tuple_fields(f[2]) else {
                    return false;
                };
                if self.a.tag(f[2]) != Some("remote") {
                    return false;
                }
                let m = self
                    .a
                    .tuple_fields(t[2])
                    .ok()
                    .and_then(|p| p.get(2))
                    .and_then(|x| self.a.as_atom(*x));
                let n = self
                    .a
                    .tuple_fields(t[3])
                    .ok()
                    .and_then(|p| p.get(2))
                    .and_then(|x| self.a.as_atom(*x));
                let args = self.a.list_items(f[3]).unwrap_or_default();
                m == Some("erlang")
                    && n.is_some_and(|n| guard_bif(n, args.len()) || guard_op(n, args.len()))
                    && args.iter().all(|x| self.comp_is_guard(*x))
            }
            Some("bin") => self.a.list_items(f[2]).is_ok_and(|xs| {
                xs.into_iter().all(|x| {
                    let f = self.a.tuple_fields(x).unwrap();
                    self.comp_is_guard(f[2])
                        && (self.a.as_atom(f[3]) == Some("default") || self.comp_is_guard(f[3]))
                })
            }),
            _ => false,
        }
    }
    fn comp_guards(&mut self, nodes: &[TermId]) -> Result<TermId> {
        if nodes.is_empty() {
            return Ok(self.atom_literal("true"));
        }
        let seq = self.a.list(nodes.to_vec());
        let alternatives = self.a.list(vec![seq]);
        self.guard(alternatives)
    }
    fn comp_qualifiers(
        &mut self,
        line: TermId,
        list: TermId,
        strict: &BTreeSet<String>,
    ) -> Result<Vec<Qualifier>> {
        let qs = self.a.list_items(list)?;
        let mut out = Vec::new();
        let mut i = 0;
        while i < qs.len() {
            let q = qs[i];
            let f = self.a.tuple_fields(q)?.to_vec();
            let tag = self.a.tag(q).unwrap_or("").to_owned();
            if tag == "zip" {
                let gs = self.a.list_items(f[2])?;
                let mut strict_names = BTreeSet::new();
                for &g in &gs {
                    let p = self.a.tuple_fields(g)?;
                    if self.a.tag(g).is_some_and(|s| s.ends_with("_strict")) {
                        self.comp_source_vars(p[2], &mut strict_names);
                    }
                }
                let mut gens = Vec::new();
                for g in gs {
                    gens.push(self.comp_generator(line, g, &[], &strict_names)?);
                }
                for g in &gens {
                    self.known_vars.extend(g.scope_names.iter().cloned());
                }
                let ann = self.annotation(f[1])?;
                out.push(Qualifier::Zip { ann, gens });
                i += 1;
                continue;
            }
            if is_generator(&tag) {
                let mut end = i + 1;
                while end < qs.len() && self.comp_is_guard(qs[end]) {
                    end += 1;
                }
                let g = self.comp_generator(line, q, &qs[i + 1..end], strict)?;
                self.known_vars.extend(g.scope_names.iter().cloned());
                out.push(Qualifier::Gen(g));
                i = end;
                continue;
            }
            let ann = self.annotation(f[1])?;
            if self.comp_is_guard(q) {
                let mut end = i + 1;
                while end < qs.len() && self.comp_is_guard(qs[end]) {
                    end += 1;
                }
                let test = self.comp_guards(&qs[i..end])?;
                out.push(Qualifier::Guard { ann, test });
                i = end;
            } else {
                let mut arg = self.source_novars(q)?;
                if !matches!(
                    self.a.tag(arg.value),
                    Some(
                        "c_apply"
                            | "c_call"
                            | "c_fun"
                            | "c_binary"
                            | "c_map"
                            | "c_literal"
                            | "c_var"
                            | "c_cons"
                            | "c_tuple"
                    )
                ) {
                    let a = self.a.tuple_fields(arg.value)?[1];
                    let v = self.fresh(a);
                    arg.pre.push(Pre::Set(v, arg.value));
                    arg.value = v;
                }
                out.push(Qualifier::Filter { ann, arg });
                i += 1;
            }
        }
        Ok(out)
    }
    fn comp_source_vars(&self, id: TermId, names: &mut BTreeSet<String>) {
        match self.a.get(id) {
            ErlangTerm::Tuple(f) => {
                if self.a.tag(id) == Some("var") {
                    if let Some(n) = self.a.as_atom(f[2]) {
                        if n != "_" {
                            names.insert(n.into());
                        }
                    }
                } else {
                    for x in f.iter().skip(2) {
                        self.comp_source_vars(*x, names);
                    }
                }
            }
            ErlangTerm::List { items, tail } => {
                for x in items {
                    self.comp_source_vars(*x, names);
                }
                self.comp_source_vars(*tail, names);
            }
            _ => (),
        }
    }
    fn comp_replace_vars(&mut self, id: TermId, strict: &BTreeSet<String>) -> Result<TermId> {
        let mut f = self.a.tuple_fields(id)?.to_vec();
        if self.a.tag(id) == Some("c_literal") {
            return Ok(id);
        }
        if self.a.tag(id) == Some("c_var") {
            if self.a.as_atom(f[2]).is_some_and(|n| strict.contains(n)) {
                return Ok(id);
            }
            f[2] = self.a.atom("_");
            return Ok(self.a.tuple(f));
        }
        match self.a.tag(id) {
            Some("c_cons") => {
                f[1] = self.empty();
                f[2] = self.comp_replace_vars(f[2], strict)?;
                f[3] = self.comp_replace_vars(f[3], strict)?;
            }
            Some("c_tuple") => {
                f[1] = self.empty();
                let mut xs = Vec::new();
                for x in self.a.list_items(f[2])? {
                    xs.push(self.comp_replace_vars(x, strict)?);
                }
                f[2] = self.a.list(xs);
            }
            Some("c_map") => {
                let mut pairs = Vec::new();
                for pair in self.a.list_items(f[3])? {
                    let mut pf = self.a.tuple_fields(pair)?.to_vec();
                    pf[4] = self.comp_replace_vars(pf[4], strict)?;
                    let pair = self.a.tuple(pf);
                    if self.comp_contains_named(pair) {
                        pairs.push(pair);
                    }
                }
                f[3] = self.a.list(pairs);
            }
            _ => return Ok(id),
        }
        let out = self.a.tuple(f);
        if !self.comp_contains_named(out) {
            let empty = self.empty();
            let n = self.a.atom("_");
            return Ok(self.variable(empty, n));
        }
        Ok(out)
    }
    fn comp_contains_named(&self, id: TermId) -> bool {
        match self.a.get(id) {
            ErlangTerm::Tuple(f) => match self.a.tag(id) {
                Some("c_var") => {
                    self.a.as_atom(f[2]).is_some_and(|n| n != "_")
                        || self.a.as_integer(f[2]).is_some()
                }
                Some("c_cons" | "c_tuple" | "c_map" | "c_map_pair") => f
                    .iter()
                    .skip(2)
                    .any(|child| self.comp_contains_named(*child)),
                _ => false,
            },
            ErlangTerm::List { items, tail } => {
                items.iter().any(|child| self.comp_contains_named(*child))
                    || self.comp_contains_named(*tail)
            }
            _ => false,
        }
    }
    fn comp_pattern_guard(
        &mut self,
        tests: Vec<TermId>,
        guard: TermId,
        empty: bool,
    ) -> Result<(TermId, Vec<TermId>)> {
        self.compose_pattern_guard(tests, guard, empty)
    }
    fn comp_generator(
        &mut self,
        line: TermId,
        node: TermId,
        guards: &[TermId],
        strict_names: &BTreeSet<String>,
    ) -> Result<Generator> {
        let f = self.a.tuple_fields(node)?.to_vec();
        let known_before = self.known_vars.clone();
        let mut names = BTreeSet::new();
        self.comp_source_vars(f[2], &mut names);
        let scope_names: HashSet<String> = names.into_iter().collect();
        let kind = self.a.tag(node).unwrap().to_owned();
        let strict = kind.ends_with("_strict");
        let la = self.annotation(line)?;
        let ga = self.annotation(f[1])?;
        let empty = self.empty();
        let mut bound = HashSet::new();
        let old = self.case_patterns;
        self.case_patterns = false;
        let outer_pins = std::mem::take(&mut self.pin_tests);
        let (accept, skip, tail, end, mismatch, refill, input);
        if kind.starts_with("m_generate") {
            let p = self.a.tuple_fields(f[2])?.to_vec();
            let source = self.a.tagged("cons", vec![f[1], p[2], p[3]]);
            let pat = self.raw_pattern(source)?;
            let vars = self.comp_vars(5, empty);
            let nk = vars[0];
            let nv = vars[1];
            tail = vars[2];
            let outer = vars[3];
            let p = self.a.tuple_fields(pat)?.to_vec();
            accept = self.comp_tuple(empty, vec![p[2], p[3], tail]);
            skip = if strict && !strict_names.is_empty() {
                accept
            } else if strict_names.is_empty() {
                self.comp_tuple(empty, vec![nk, nv, tail])
            } else {
                let replaced = self.comp_replace_vars(pat, strict_names)?;
                let p = self.a.tuple_fields(replaced)?.to_vec();
                if self.a.tag(replaced) == Some("c_var") {
                    self.comp_tuple(empty, vec![nk, nv, tail])
                } else {
                    self.comp_tuple(empty, vec![p[2], p[3], tail])
                }
            };
            mismatch = if strict {
                Some(self.comp_tuple(empty, vec![nk, nv]))
            } else {
                None
            };
            let none = self.a.atom("none");
            end = self.literal(la, none);
            let cons = self.a.tagged("c_cons", vec![ga, nk, nv]);
            let call = self.comp_call(ga, "erts_internal", "mc_refill", vec![cons], false);
            let action = self.wrap(vec![Pre::Set(tail, call)], tail);
            refill = Some((cons, action));
            let pin_tests = std::mem::take(&mut self.pin_tests);
            let guard = self.comp_guards(guards)?;
            let (guard, allocations) =
                self.comp_pattern_guard(pin_tests, guard, guards.is_empty())?;
            let (vs, pre) = self.safe_list(vec![f[3]])?;
            let ce = vs[0];
            let init = self.comp_call(ga, "erts_internal", "mc_iterator", vec![ce], false);
            let nil = self.empty();
            let nil = self.literal(empty, nil);
            let nil_input = self.comp_reann(nil, ga)?;
            let bad = self.comp_bad_generator(vec![nil], ce, nil_input, false)?;
            let truth = self.atom_literal("true");
            let pass = self.comp_clause(ga, vec![tail], truth, tail)?;
            let cs = self.a.list(vec![bad, pass]);
            let before = self.a.tagged("c_case", vec![empty, init, cs]);
            let mut pre = pre;
            pre.push(Pre::Set(outer, before));
            input = Expr { value: outer, pre };
            self.case_patterns = old;
            self.pin_tests = outer_pins;
            self.known_vars = known_before.clone();
            return Ok(Generator {
                known_before,
                scope_names,
                ann: ga,
                accept,
                guard,
                has_guard: !guards.is_empty(),
                allocations,
                skip,
                mismatch,
                tail,
                end,
                refill,
                input,
                binary: false,
            });
        } else if kind.starts_with("b_generate") {
            let pat = self.pattern(f[2], &mut bound)?;
            let p = self.a.tuple_fields(pat)?.to_vec();
            if self.a.tag(pat) != Some("c_binary") {
                return Err(self.err("binary generator pattern is not a binary"));
            }
            let mut segments = self.a.list_items(p[2])?;
            tail = self.fresh(empty);
            let all = self.atom_literal("all");
            let one = self.a.integer(1);
            let one = self.literal(empty, one);
            let binary = self.atom_literal("binary");
            let unsigned = self.a.atom("unsigned");
            let big = self.a.atom("big");
            let flags = self.a.list(vec![unsigned, big]);
            let flags = self.literal(empty, flags);
            let tailseg = self
                .a
                .tagged("c_bitstr", vec![empty, tail, all, one, binary, flags]);
            segments.push(tailseg);
            let list = self.a.list(segments.clone());
            accept = self.a.tagged("c_binary", vec![p[1], list]);
            let pin_tests = std::mem::take(&mut self.pin_tests);
            let guard = self.comp_guards(guards)?;
            let (guard, allocations) =
                self.comp_pattern_guard(pin_tests, guard, guards.is_empty())?;
            if strict {
                let segs = self.a.list(vec![tailseg]);
                skip = self.a.tagged("c_binary", vec![la, segs]);
                mismatch = Some(tail);
                let segs = self.a.nil();
                end = self.a.tagged("c_binary", vec![la, segs]);
            } else {
                let mut skips = Vec::new();
                for s in segments {
                    let mut p = self.a.tuple_fields(s)?.to_vec();
                    let is_float = self.a.as_atom(self.a.tuple_fields(p[5])?[2]) == Some("float");
                    if self.a.tag(p[2]) != Some("c_var") {
                        p[2] = self.fresh(empty);
                    } else if self.deferred_vars.contains(&p[2]) {
                        let ann = self.a.tuple_fields(p[2])?[1];
                        p[2] = self.deferred_fresh(ann);
                    }
                    if is_float {
                        p[5] = self.atom_literal("integer");
                    }
                    skips.push(self.a.tuple(p));
                }
                let list = self.a.list(skips);
                skip = self.a.tagged("c_binary", vec![p[1], list]);
                mismatch = None;
                let list = self.a.list(vec![tailseg]);
                end = self.a.tagged("c_binary", vec![la, list]);
            }
            refill = None;
            let (vs, pre) = self.safe_list(vec![f[3]])?;
            input = Expr { value: vs[0], pre };
            self.case_patterns = old;
            self.pin_tests = outer_pins;
            self.known_vars = known_before.clone();
            return Ok(Generator {
                known_before,
                scope_names,
                ann: ga,
                accept,
                guard,
                has_guard: !guards.is_empty(),
                allocations,
                skip,
                mismatch,
                tail,
                end,
                refill,
                input,
                binary: true,
            });
        } else {
            let head = self.raw_pattern(f[2])?;
            let vars = self.comp_vars(2, empty);
            tail = vars[0];
            let nomatch = vars[1];
            accept = self.a.tagged("c_cons", vec![la, head, tail]);
            skip = if strict && !strict_names.is_empty() {
                accept
            } else if strict_names.is_empty() {
                self.a.tagged("c_cons", vec![la, nomatch, tail])
            } else {
                let h = self.comp_replace_vars(head, strict_names)?;
                self.a.tagged("c_cons", vec![la, h, tail])
            };
            mismatch = if strict { Some(nomatch) } else { None };
            let nil = self.empty();
            end = self.literal(la, nil);
            refill = None;
        }
        let pin_tests = std::mem::take(&mut self.pin_tests);
        let guard = self.comp_guards(guards)?;
        let (guard, allocations) = self.comp_pattern_guard(pin_tests, guard, guards.is_empty())?;
        let (vs, pre) = self.safe_list(vec![f[3]])?;
        input = Expr { value: vs[0], pre };
        self.case_patterns = old;
        self.pin_tests = outer_pins;
        self.known_vars = known_before.clone();
        Ok(Generator {
            known_before,
            scope_names,
            ann: ga,
            accept,
            guard,
            has_guard: !guards.is_empty(),
            allocations,
            skip,
            mismatch,
            tail,
            end,
            refill,
            input,
            binary: false,
        })
    }
    fn comp_emit(
        &mut self,
        line: TermId,
        value: TermId,
        qs: &[Qualifier],
        more: TermId,
        binary: bool,
    ) -> Result<Expr> {
        let la = self.annotation(line)?;
        let Some(first) = qs.first() else {
            if binary {
                return self.comp_binary_build(line, value, more);
            }
            let (vs, mut pre) = self.safe_list(vec![value])?;
            let mut tail = more;
            if !matches!(
                self.a.tag(tail),
                Some("c_var" | "c_literal" | "c_cons" | "c_tuple")
            ) {
                let a = self.a.tuple_fields(tail)?[1];
                let v = self.fresh(a);
                pre.push(Pre::Set(v, tail));
                tail = v;
            }
            let an = self.comp_mark(la, &["compiler_generated"])?;
            let value = if self.a.tag(vs[0]) == Some("c_literal")
                && self.a.tag(tail) == Some("c_literal")
            {
                let head = self.a.tuple_fields(vs[0])?[2];
                let tail = self.a.tuple_fields(tail)?[2];
                let literal = self.a.push(ErlangTerm::List {
                    items: vec![head],
                    tail,
                });
                self.literal(an, literal)
            } else {
                self.a.tagged("c_cons", vec![an, vs[0], tail])
            };
            return Ok(Expr { value, pre });
        };
        match first {
            Qualifier::Guard { ann, test } => {
                let body = self.comp_emit(line, value, &qs[1..], more, binary)?;
                let body = self.wrap(body.pre, body.value);
                let c = self.comp_clause(*ann, vec![], *test, body)?;
                let generated = self.comp_mark(*ann, &["compiler_generated"])?;
                let t = self.atom_literal("true");
                let fail = self.comp_clause(generated, vec![], t, more)?;
                let empty = self.empty();
                let vals = self.a.nil();
                let args = self.a.tagged("c_values", vec![empty, vals]);
                let cs = self.a.list(vec![c, fail]);
                let ann = self.comp_mark(*ann, &["list_comprehension"])?;
                Ok(Expr {
                    value: self.a.tagged("c_case", vec![ann, args, cs]),
                    pre: vec![],
                })
            }
            Qualifier::Filter { ann, arg } => {
                let body = self.comp_emit(line, value, &qs[1..], more, binary)?;
                let body = self.wrap(body.pre, body.value);
                let t = self.atom_literal("true");
                let truth = self.atom_literal("true");
                let c = self.comp_clause(*ann, vec![t], truth, body)?;
                let generated = self.comp_mark(*ann, &["compiler_generated"])?;
                let f = self.atom_literal("false");
                let c2 = self.comp_clause(generated, vec![f], truth, more)?;
                let empty = self.empty();
                let fail = self.fresh(empty);
                let c3 = self.failure(vec![fail], *ann, "bad_filter")?;
                let cs = self.a.list(vec![c, c2, c3]);
                let an = self.comp_mark(*ann, &["list_comprehension"])?;
                Ok(Expr {
                    value: self.a.tagged("c_case", vec![an, arg.value, cs]),
                    pre: arg.pre.clone(),
                })
            }
            Qualifier::Gen(g) => self.comp_loop(
                line,
                value,
                &qs[1..],
                more,
                binary,
                &[g.clone()],
                g.ann,
                false,
            ),
            Qualifier::Zip { ann, gens } => {
                self.comp_loop(line, value, &qs[1..], more, binary, gens, *ann, true)
            }
        }
    }
    fn comp_loop(
        &mut self,
        line: TermId,
        value: TermId,
        qs: &[Qualifier],
        more: TermId,
        binary: bool,
        gens: &[Generator],
        ga: TermId,
        zip: bool,
    ) -> Result<Expr> {
        let la = self.annotation(line)?;
        let empty = self.empty();
        let arity = gens.len() + usize::from(binary);
        let prefix = match (binary, zip) {
            (false, false) => "lc",
            (true, false) => "lbc",
            (false, true) => "zlc",
            (true, true) => "bzip",
        };
        let name = self.a.atom(format!("{prefix}$^{}", self.fun_counter));
        self.fun_counter += 1;
        let ar = self.a.integer(arity);
        let fa = self.a.tuple(vec![name, ar]);
        let op = self.variable(la, fa);
        let (params, failparams, ignore, acc) = if !zip && !binary {
            let vs = self.comp_vars(2, empty);
            (vec![vs[1]], vec![vs[0]], None, None)
        } else {
            let an = if binary { la } else { empty };
            let ps = self.comp_vars(arity, an);
            let fs = self.comp_vars(arity, an);
            let ig = if binary && !zip {
                Some(self.fresh(la))
            } else {
                None
            };
            let acc = if binary {
                Some(*ps.last().unwrap())
            } else {
                None
            };
            (ps, fs, ig, acc)
        };
        let mut tailargs: Vec<_> = gens.iter().map(|g| g.tail).collect();
        if let Some(acc) = acc {
            tailargs.push(acc);
        }
        let sc = self.comp_apply(ga, op, tailargs);
        let prior = self.known_vars.clone();
        self.known_vars = gens[0].known_before.clone();
        for g in gens {
            self.known_vars.extend(g.scope_names.iter().cloned());
        }
        let nested = self.comp_emit(line, value, qs, acc.unwrap_or(sc), binary);
        self.known_vars = prior;
        let mut nested = nested?;
        let body = if let Some(acc) = acc {
            nested.pre.push(Pre::Set(acc, nested.value));
            self.wrap(nested.pre, sc)
        } else {
            self.wrap(nested.pre, nested.value)
        };
        let mut accepts: Vec<_> = gens.iter().map(|g| g.accept).collect();
        if binary {
            accepts.push(ignore.unwrap_or(more));
        }
        let guard = gens[0].guard;
        let clause = self.comp_clause(la, accepts.clone(), guard, body)?;
        if !gens[0].allocations.is_empty() {
            self.guard_allocations
                .entry(clause)
                .or_default()
                .extend(gens[0].allocations.iter().copied());
        }
        let mut clauses = vec![clause];
        let truth = self.atom_literal("true");
        let allstrict = gens.iter().all(|g| g.mismatch.is_some());
        let anystrict = gens.iter().any(|g| g.mismatch.is_some());
        if !zip && anystrict && gens[0].has_guard {
            let an = self.comp_mark(la, &["compiler_generated"])?;
            clauses.push(self.comp_clause(an, accepts, truth, sc)?);
        }
        let skipann = self.comp_mark(
            la,
            &if zip {
                vec!["skip_clause", "compiler_generated"]
            } else {
                vec!["compiler_generated"]
            },
        )?;
        let mut skips: Vec<_> = gens
            .iter()
            .map(|g| {
                if zip && g.mismatch.is_some() {
                    g.accept
                } else {
                    g.skip
                }
            })
            .collect();
        if binary {
            skips.push(ignore.unwrap_or(more));
        }
        let skipbody = if !zip {
            if let Some(bm) = gens[0].mismatch {
                let bad = self.atom_literal("badmatch");
                let tuple = self.comp_tuple(ga, vec![bad, bm]);
                self.comp_call(ga, "erlang", "error", vec![tuple], true)
            } else {
                sc
            }
        } else {
            sc
        };
        let skip = if !zip || !allstrict {
            Some(self.comp_clause(skipann, skips, truth, skipbody)?)
        } else {
            None
        };
        let mut ends: Vec<_> = gens
            .iter()
            .map(|g| {
                if zip && g.mismatch.is_some() && g.binary {
                    let nil = self.a.nil();
                    self.a.tagged(
                        "c_binary",
                        vec![self.a.tuple_fields(g.end).unwrap()[1], nil],
                    )
                } else {
                    g.end
                }
            })
            .collect();
        if binary {
            ends.push(ignore.unwrap_or(more));
        }
        let end = self.comp_clause(la, ends, truth, acc.unwrap_or(more))?;
        if !zip && gens[0].binary && gens[0].mismatch.is_some() {
            clauses.push(end);
            if let Some(skip) = skip {
                clauses.push(skip);
            }
        } else {
            if let Some(skip) = skip {
                clauses.push(skip);
            }
            clauses.push(end);
        }
        for (i, g) in gens.iter().enumerate() {
            if let Some((pat, action)) = g.refill {
                let mut pats: Vec<_> = gens.iter().map(|g| g.tail).collect();
                pats[i] = pat;
                if binary {
                    pats.push(acc.unwrap());
                }
                let b = self.a.tuple_fields(action)?.to_vec();
                let body = if self.a.tag(action) == Some("c_let") {
                    let vars = self.a.list_items(b[2])?;
                    self.wrap(vec![Pre::Set(vars[0], b[3])], sc)
                } else {
                    let empty = self.empty();
                    self.a.tagged("c_seq", vec![empty, action, sc])
                };
                clauses.push(self.comp_clause(la, pats, truth, body)?);
            }
        }
        let bad = if zip {
            self.comp_tuple(
                empty,
                if binary {
                    failparams[..failparams.len() - 1].to_vec()
                } else {
                    failparams.clone()
                },
            )
        } else {
            failparams[0]
        };
        clauses.push(self.comp_bad_generator(failparams, bad, gens[0].input.value, zip)?);
        let arg = if params.len() == 1 {
            self.comp_reann(params[0], ga)?
        } else {
            let xs = self.a.list(params.clone());
            self.a.tagged("c_values", vec![ga, xs])
        };
        let cs = self.a.list(clauses);
        let case = self.a.tagged("c_case", vec![ga, arg, cs]);
        let ps = self.a.list(params);
        let fun = self.a.tagged("c_fun", vec![ga, ps, case]);
        self.shadowed_funs.insert(fun, gens[0].known_before.clone());
        let binder = self.variable(empty, fa);
        let def = self.a.tuple(vec![binder, fun]);
        let defs = self.a.list(vec![def]);
        let mut inputs = Vec::new();
        let mut pre = Vec::new();
        for g in gens {
            inputs.push(g.input.value);
            pre.extend(g.input.pre.clone());
        }
        if binary {
            inputs.push(more);
        }
        let callann = if binary { la } else { ga };
        let call = self.comp_apply(callann, op, inputs);
        let body = self.wrap(pre, call);
        let lrann = self.comp_mark(
            if binary { la } else { ga },
            if binary {
                &["list_comprehension", "no_inline"]
            } else {
                &["list_comprehension"]
            },
        )?;
        Ok(Expr {
            value: self.a.tagged("c_letrec", vec![lrann, defs, body]),
            pre: vec![],
        })
    }
    fn comp_binary_build(&mut self, line: TermId, value: TermId, acc: TermId) -> Result<Expr> {
        let (line, mut segments, mut pre) = if self.a.tag(value) == Some("bin") {
            let f = self.a.tuple_fields(value)?.to_vec();
            (f[1], self.a.list_items(f[2])?, vec![])
        } else {
            let (vs, pre) = self.safe_list(vec![value])?;
            let name = self.a.tuple_fields(vs[0])?.to_vec();
            let node = if self.a.tag(vs[0]) == Some("c_var") {
                self.a.tagged("var", vec![line, name[2]])
            } else {
                let atom = self.a.atom("bad_value");
                self.a.tagged("atom", vec![line, atom])
            };
            let sz = self.comp_all_size(line)?;
            let flags = self.comp_binary_flags();
            let seg = self.a.tagged("bin_element", vec![line, node, sz, flags]);
            (line, vec![seg], pre)
        };
        let name = self.a.tuple_fields(acc)?[2];
        let v = self.a.tagged("var", vec![line, name]);
        let size = self.comp_all_size(line)?;
        let flags = self.comp_binary_flags();
        let seg = self.a.tagged("bin_element", vec![line, v, size, flags]);
        segments.insert(0, seg);
        let segments = self.a.list(segments);
        let source = self.a.tagged("bin", vec![line, segments]);
        let mut out = self.expr(source)?;
        let an = self.a.tuple_fields(out.value)?[1];
        let an = self.comp_mark(an, &["compiler_generated", "single_use"])?;
        out.value = self.comp_reann(out.value, an)?;
        pre.append(&mut out.pre);
        out.pre = pre;
        Ok(out)
    }
    fn comp_binary_flags(&mut self) -> TermId {
        let b = self.a.atom("binary");
        let u = self.a.atom("unit");
        let one = self.a.integer(1);
        let unit = self.a.tuple(vec![u, one]);
        self.a.list(vec![b, unit])
    }
    fn comp_all_size(&mut self, line: TermId) -> Result<TermId> {
        let mut xs = if matches!(self.a.get(line), ErlangTerm::List { .. }) {
            self.a.list_items(line)?
        } else {
            let k = self.a.atom("location");
            vec![self.a.tuple(vec![k, line])]
        };
        let generated = self.a.atom("generated");
        let yes = self.a.atom("true");
        xs.insert(0, self.a.tuple(vec![generated, yes]));
        let ann = self.a.list(xs);
        let all = self.a.atom("all");
        Ok(self.a.tagged("atom", vec![ann, all]))
    }
}
fn is_generator(tag: &str) -> bool {
    matches!(
        tag,
        "generate"
            | "generate_strict"
            | "b_generate"
            | "b_generate_strict"
            | "m_generate"
            | "m_generate_strict"
    )
}
fn guard_op(n: &str, a: usize) -> bool {
    match a {
        1 => matches!(n, "+" | "-" | "bnot" | "not"),
        2 => matches!(
            n,
            "+" | "-"
                | "*"
                | "/"
                | "div"
                | "rem"
                | "band"
                | "bor"
                | "bxor"
                | "bsl"
                | "bsr"
                | "and"
                | "or"
                | "xor"
                | "andalso"
                | "orelse"
                | "=="
                | "/="
                | "=:="
                | "=/="
                | "<"
                | ">"
                | "=<"
                | ">="
        ),
        _ => false,
    }
}
fn guard_bif(n: &str, a: usize) -> bool {
    matches!(
        (n, a),
        ("abs", 1)
            | ("binary_part", 2)
            | ("binary_part", 3)
            | ("bit_size", 1)
            | ("byte_size", 1)
            | ("ceil", 1)
            | ("element", 2)
            | ("float", 1)
            | ("floor", 1)
            | ("hd", 1)
            | ("is_map_key", 2)
            | ("length", 1)
            | ("map_size", 1)
            | ("map_get", 2)
            | ("max", 2)
            | ("min", 2)
            | ("node", 0)
            | ("node", 1)
            | ("round", 1)
            | ("self", 0)
            | ("size", 1)
            | ("tl", 1)
            | ("trunc", 1)
            | ("tuple_size", 1)
            | ("is_function", 2)
            | ("is_record", 2)
            | ("is_record", 3)
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
        ))
}
