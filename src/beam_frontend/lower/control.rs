//! OTP 28.3 v3_core named fun and maybe conversion (Apache-2.0; Ericsson AB).
use super::*;
impl Lower {
    pub(super) fn named_fun_expr(&mut self, f: &[TermId], ann: TermId) -> Result<Expr> {
        let known = self.known_vars.clone();
        let old_case = self.case_patterns;
        let name = self
            .a
            .as_atom(f[2])
            .ok_or_else(|| self.err("named fun name"))?
            .to_owned();
        if name == "_" {
            let descriptor = self.a.tagged("clauses", vec![f[3]]);
            let source = self.a.tagged("fun", vec![f[1], descriptor]);
            return self.expr(source);
        }
        let source = self.a.list_items(f[3])?;
        let first = *source.first().ok_or_else(|| self.err("empty named fun"))?;
        let arity = self.a.list_items(self.a.tuple_fields(first)?[2])?.len();
        let mut clauses = Vec::new();
        for clause in source {
            self.known_vars = known.clone();
            self.known_vars.insert(name.clone());
            let cf = self.a.tuple_fields(clause)?;
            for head in self.source_export_patterns(cf[2])? {
                if head != name {
                    self.known_vars.remove(&head);
                }
            }
            self.case_patterns = true;
            clauses.push(self.clause(clause)?);
        }
        self.known_vars = known.clone();
        self.case_patterns = old_case;
        let empty = self.empty();
        let mut params = Vec::new();
        let mut failed = Vec::new();
        for _ in 0..arity {
            params.push(self.fresh(empty));
        }
        params.reverse();
        for _ in 0..arity {
            failed.push(self.fresh(empty));
        }
        failed.reverse();
        clauses.push(self.failure(failed, ann, "function_clause")?);
        let generated_name = format!("-{}-fun-{}-", self.function, self.fun_counter);
        self.fun_counter += 1;
        let zero = self.a.integer(0);
        let generated = self.a.atom(generated_name);
        let id = self.a.tuple(vec![zero, zero, generated]);
        let idkey = self.a.atom("id");
        let id = self.a.tuple(vec![idkey, id]);
        let marker = self.a.atom("native_named_fun");
        let marker = self.a.tuple(vec![marker, f[2]]);
        let mut annotations = vec![id];
        annotations.extend(self.a.list_items(ann)?);
        annotations.push(marker);
        let fun_ann = self.a.list(annotations);
        let argument = if params.len() == 1 {
            let mut variable = self.a.tuple_fields(params[0])?.to_vec();
            variable[1] = ann;
            self.a.tuple(variable)
        } else {
            let values = self.a.list(params.clone());
            self.a.tagged("c_values", vec![ann, values])
        };
        let clauses = self.a.list(clauses);
        let case = self.a.tagged("c_case", vec![ann, argument, clauses]);
        let params = self.a.list(params);
        let fun = self.a.tagged("c_fun", vec![fun_ann, params, case]);
        self.shadowed_funs.insert(fun, known);
        self.source_annotations.insert(fun, ann);
        Ok(Expr {
            value: fun,
            pre: vec![],
        })
    }
    fn control_uses(&self, node: TermId, name: TermId) -> Result<bool> {
        let Some(tag) = self.a.tag(node) else {
            if let ErlangTerm::List { items, .. } = self.a.get(node) {
                for child in items {
                    if self.control_uses(*child, name)? {
                        return Ok(true);
                    }
                }
            }
            return Ok(false);
        };
        let f = self.a.tuple_fields(node)?;
        if tag == "c_literal" {
            return Ok(false);
        }
        if tag == "c_var" {
            return Ok(self.a.get(f[2]) == self.a.get(name));
        }
        if tag == "c_clause" && self.control_pattern_uses(f[2], name)? {
            return Ok(true);
        }
        if matches!(tag, "c_fun" | "c_let" | "c_clause") {
            let bindings = f[2];
            if self.control_binds(bindings, name)? {
                return if tag == "c_let" {
                    self.control_uses(f[3], name)
                } else {
                    Ok(false)
                };
            }
        }
        for child in f.iter().skip(2) {
            if self.control_uses(*child, name)? {
                return Ok(true);
            }
        }
        Ok(false)
    }
    fn control_binds(&self, node: TermId, name: TermId) -> Result<bool> {
        if self.a.tag(node) == Some("c_var") {
            return Ok(self.a.get(self.a.tuple_fields(node)?[2]) == self.a.get(name));
        }
        if self.a.tag(node) == Some("c_literal") {
            return Ok(false);
        }
        if self.a.tag(node) == Some("c_map") {
            let f = self.a.tuple_fields(node)?;
            for pair in self.a.list_items(f[3])? {
                if self.control_binds(self.a.tuple_fields(pair)?[4], name)? {
                    return Ok(true);
                }
            }
            return Ok(false);
        }
        if self.a.tag(node) == Some("c_binary") {
            let f = self.a.tuple_fields(node)?;
            for segment in self.a.list_items(f[2])? {
                if self.control_binds(self.a.tuple_fields(segment)?[2], name)? {
                    return Ok(true);
                }
            }
            return Ok(false);
        }
        match self.a.get(node) {
            ErlangTerm::List { items, .. } => {
                for child in items {
                    if self.control_binds(*child, name)? {
                        return Ok(true);
                    }
                }
            }
            ErlangTerm::Tuple(f) => {
                for child in f.iter().skip(2) {
                    if self.control_binds(*child, name)? {
                        return Ok(true);
                    }
                }
            }
            _ => (),
        };
        Ok(false)
    }
    fn control_pattern_uses(&self, node: TermId, name: TermId) -> Result<bool> {
        if matches!(self.a.tag(node), Some("c_var" | "c_literal")) {
            return Ok(false);
        }
        if self.a.tag(node) == Some("c_map") {
            let f = self.a.tuple_fields(node)?;
            for pair in self.a.list_items(f[3])? {
                let p = self.a.tuple_fields(pair)?;
                if self.control_uses(p[3], name)? || self.control_pattern_uses(p[4], name)? {
                    return Ok(true);
                }
            }
            return Ok(false);
        }
        if self.a.tag(node) == Some("c_binary") {
            let f = self.a.tuple_fields(node)?;
            for segment in self.a.list_items(f[2])? {
                let f = self.a.tuple_fields(segment)?;
                if self.control_uses(f[3], name)? || self.control_pattern_uses(f[2], name)? {
                    return Ok(true);
                }
            }
            return Ok(false);
        }
        match self.a.get(node) {
            ErlangTerm::Tuple(fields) => {
                for child in fields.iter().skip(2) {
                    if self.control_pattern_uses(*child, name)? {
                        return Ok(true);
                    }
                }
            }
            ErlangTerm::List { items, .. } => {
                for child in items {
                    if self.control_pattern_uses(*child, name)? {
                        return Ok(true);
                    }
                }
            }
            _ => (),
        };
        Ok(false)
    }
    pub(super) fn finish_named_funs(&mut self, node: TermId) -> Result<TermId> {
        let mut f = match self.a.get(node).clone() {
            ErlangTerm::Tuple(f) => f,
            ErlangTerm::List { items, tail } => {
                let mut out = Vec::new();
                for child in items {
                    out.push(self.finish_named_funs(child)?);
                }
                let tail = self.finish_named_funs(tail)?;
                return Ok(self.a.push(ErlangTerm::List { items: out, tail }));
            }
            _ => return Ok(node),
        };
        let tag = self.a.tag(node).map(str::to_owned);
        if matches!(tag.as_deref(), Some("c_literal" | "c_var")) {
            let annotations = self
                .a
                .list_items(f[1])?
                .into_iter()
                .filter(|a| self.a.tag(*a) != Some("native_named_fun"))
                .collect();
            f[1] = self.a.list(annotations);
            return Ok(self.a.tuple(f));
        }
        for child in f
            .iter_mut()
            .skip(if tag.as_deref().is_some_and(|t| t.starts_with("c_")) {
                2
            } else {
                0
            })
        {
            *child = self.finish_named_funs(*child)?;
        }
        if tag.as_deref() == Some("c_fun") {
            let mut annotations = Vec::new();
            let mut name = None;
            for ann in self.a.list_items(f[1])? {
                if self.a.tag(ann) == Some("native_named_fun") {
                    name = Some(self.a.tuple_fields(ann)?[1]);
                } else {
                    annotations.push(ann);
                }
            }
            f[1] = self.a.list(annotations);
            if let Some(name) = name {
                if self.control_uses(f[3], name)? {
                    let arity = self.a.list_items(f[2])?.len();
                    let arity = self.a.integer(arity as i64);
                    let fa = self.a.tuple(vec![name, arity]);
                    let empty = self.empty();
                    let rec = self.variable(empty, fa);
                    let binding = self.variable(empty, name);
                    let bindings = self.a.list(vec![binding]);
                    f[3] = self.a.tagged("c_let", vec![empty, bindings, rec, f[3]]);
                    let annotations = self
                        .a
                        .list_items(f[1])?
                        .into_iter()
                        .filter(|a| self.a.tag(*a) != Some("id"))
                        .collect();
                    let ann = self.a.list(annotations);
                    let fun = self.a.tuple(f);
                    let definition = self.a.tuple(vec![rec, fun]);
                    let defs = self.a.list(vec![definition]);
                    return Ok(self.a.tagged("c_letrec", vec![ann, defs, rec]));
                }
            }
        }
        if tag.as_deref().is_some_and(|t| t.starts_with("c_")) {
            let anns = self
                .a
                .list_items(f[1])?
                .into_iter()
                .filter(|a| self.a.tag(*a) != Some("native_named_fun"))
                .collect();
            f[1] = self.a.list(anns);
        }
        Ok(self.a.tuple(f))
    }
    fn control_fail(&mut self, pattern: TermId, ann: TermId, reason: TermId) -> TermId {
        let name = self.atom_literal("match_fail");
        let args = self.a.list(vec![reason]);
        let failure = self.a.tagged("c_primop", vec![ann, name, args]);
        let compiler = self.a.atom("compiler_generated");
        let generated = self.a.list(vec![compiler]);
        let pats = self.a.list(vec![pattern]);
        let truth = self.atom_literal("true");
        self.a
            .tagged("c_clause", vec![generated, pats, truth, failure])
    }
    pub(super) fn maybe_expr(&mut self, f: &[TermId], ann: TermId) -> Result<Expr> {
        let known = self.known_vars.clone();
        let old_patterns = self.case_patterns;
        let clauses = if f.len() == 4 {
            let e = self.a.tuple_fields(f[3])?;
            self.a.list_items(e[2])?
        } else {
            let empty = self.empty();
            let variable = self.fresh(empty);
            let name = self.a.tuple_fields(variable)?[2];
            let var = self.a.tagged("var", vec![f[1], name]);
            let pats = self.a.list(vec![var]);
            let guards = self.empty();
            let body = self.a.list(vec![var]);
            vec![self.a.tagged("clause", vec![f[1], pats, guards, body])]
        };
        let empty = self.empty();
        let failvar = self.fresh(empty);
        let v2 = self.fresh(empty);
        let v1 = self.fresh(empty);
        let name = self.a.atom("maybe_else_fail");
        let one = self.a.integer(1);
        let fa = self.a.tuple(vec![name, one]);
        let fail = self.variable(empty, fa);
        let body = self.maybe_sequence(&self.a.list_items(f[2])?, fail)?;
        self.known_vars = known.clone();
        self.case_patterns = true;
        let mut translated = Vec::new();
        for clause in clauses {
            translated.push(self.clause(clause)?);
        }
        translated.push(self.failure(vec![failvar], ann, "else_clause")?);
        let translated = self.a.list(translated);
        let inner = self.a.tagged("c_case", vec![empty, v2, translated]);
        let pats = self.a.list(vec![v2]);
        let truth = self.atom_literal("true");
        let success = self.a.tagged("c_clause", vec![empty, pats, truth, inner]);
        let reason = self.atom_literal("never_fails");
        let failure = self.control_fail(failvar, ann, reason);
        let cs = self.a.list(vec![success, failure]);
        let outer = self.a.tagged("c_case", vec![empty, v1, cs]);
        let params = self.a.list(vec![v1]);
        let fun = self.a.tagged("c_fun", vec![empty, params, outer]);
        let fun = self.register_fun_shadowing(fun);
        let definition = self.a.tuple(vec![fail, fun]);
        let definitions = self.a.list(vec![definition]);
        let mut annotations = vec![self.a.atom("letrec_goto"), self.a.atom("no_inline")];
        annotations.extend(self.a.list_items(ann)?);
        let ann = self.a.list(annotations);
        let value = self.a.tagged("c_letrec", vec![ann, definitions, body]);
        self.known_vars = known;
        self.case_patterns = old_patterns;
        Ok(Expr { value, pre: vec![] })
    }
    fn maybe_sequence(&mut self, source: &[TermId], fail: TermId) -> Result<TermId> {
        let Some((&first, rest)) = source.split_first() else {
            return Err(self.err("empty maybe body"));
        };
        if self.a.tag(first) == Some("maybe_match") {
            let f = self.a.tuple_fields(first)?.to_vec();
            let ann = self.annotation(f[1])?;
            let known = self.known_vars.clone();
            self.known_vars.extend(self.source_export_patterns(f[2])?);
            let tail = if rest.is_empty() {
                None
            } else {
                Some(self.maybe_sequence(rest, fail)?)
            };
            self.known_vars = known;
            let pattern = if tail.is_none() {
                let empty = self.empty();
                let all = self.fresh(empty);
                let name = self.a.tuple_fields(all)?[2];
                let source_var = self.a.tagged("var", vec![f[1], name]);
                let pattern = self.a.tagged("match", vec![f[1], f[2], source_var]);
                (pattern, source_var)
            } else {
                let zero = self.a.integer(0);
                let nil = self.a.tagged("nil", vec![zero]);
                (f[2], nil)
            };
            let patterns = self.a.list(vec![pattern.0]);
            let guard = self.empty();
            let result = self.a.list(vec![pattern.1]);
            let source_clause = self.a.tagged("clause", vec![f[1], patterns, guard, result]);
            let old = self.case_patterns;
            self.case_patterns = true;
            let clause = self.clause(source_clause);
            self.case_patterns = old;
            let mut clause = clause?;
            if let Some(tail) = tail {
                let mut fields = self.a.tuple_fields(clause)?.to_vec();
                fields[4] = tail;
                clause = self.a.tuple(fields);
            }
            let mut input = self.source_novars(f[3])?;
            if !matches!(
                self.a.tag(input.value),
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
                let annotation = self.expression_annotation(input.value)?;
                let temp = self.fresh(annotation);
                input.pre.push(Pre::Set(temp, input.value));
                input.value = temp;
            }
            let empty = self.empty();
            let failure_pattern = self.fresh(empty);
            let args = self.a.list(vec![failure_pattern]);
            let apply = self.a.tagged("c_apply", vec![empty, fail, args]);
            let mut annotations = vec![
                self.a.atom("dialyzer_ignore"),
                self.a.atom("compiler_generated"),
            ];
            annotations.extend(self.a.list_items(ann)?);
            let generated = self.a.list(annotations);
            let pats = self.a.list(vec![failure_pattern]);
            let truth = self.atom_literal("true");
            let failure = self
                .a
                .tagged("c_clause", vec![generated, pats, truth, apply]);
            let cs = self.a.list(vec![clause, failure]);
            let value = self.a.tagged("c_case", vec![ann, input.value, cs]);
            Ok(self.wrap(input.pre, value))
        } else {
            // Keep a following expression visible to ordinary match lowering;
            // a non-final match must not allocate a return-value temporary.
            let mut expressions = vec![first];
            if !rest.is_empty() {
                let zero = self.a.integer(0);
                let nil = self.a.tagged("nil", vec![zero]);
                expressions.push(nil);
            }
            let singleton = self.a.list(expressions);
            let mut bound = self.known_vars.clone();
            let (mut steps, _) = self.body_steps(singleton, &mut bound)?;
            if !rest.is_empty() {
                steps.pop();
            }
            let tail = if rest.is_empty() {
                None
            } else {
                Some(self.maybe_sequence(rest, fail)?)
            };
            self.assemble_steps(steps, tail)
        }
    }
}
