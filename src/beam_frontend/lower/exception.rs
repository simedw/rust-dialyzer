//! OTP v3_core try/catch lowering. Exception triples retain the raw stacktrace
//! until the matching handler asks for its materialized representation.
use super::*;
impl Lower {
    pub(super) fn primop(&mut self, name: &str, args: Vec<TermId>) -> TermId {
        let ann = self.empty();
        let name = self.atom_literal(name);
        let args = self.a.list(args);
        self.a.tagged("c_primop", vec![ann, name, args])
    }
    fn exception_handler(&mut self, input: TermId) -> Result<(Vec<TermId>, TermId)> {
        let empty = self.empty();
        let mut evars = Vec::new();
        for _ in 0..3 {
            evars.push(self.fresh(empty));
        }
        evars.reverse();
        let mut clauses = Vec::new();
        for clause in self.a.list_items(input)? {
            let old_case = self.case_patterns;
            self.case_patterns = true;
            let lowered = self.clause(clause);
            self.case_patterns = old_case;
            let node = lowered?;
            let mut fields = self.a.tuple_fields(node)?.to_vec();
            let pats = self.a.list_items(fields[2])?;
            if pats.len() != 1 || self.a.tag(pats[0]) != Some("c_tuple") {
                return Err(self.err("exception pattern is not a triple"));
            }
            let mut tuple = self.a.tuple_fields(pats[0])?.to_vec();
            let mut es = self.a.list_items(tuple[2])?;
            if es.len() != 3 {
                return Err(self.err("exception pattern arity"));
            }
            let stk = self.a.tuple_fields(es[2])?.to_vec();
            if self.a.tag(es[2]) != Some("c_var") || self.a.as_atom(stk[2]) != Some("_") {
                let stack = es[2];
                es[2] = evars[0];
                tuple[2] = self.a.list(es);
                let tuple = self.a.tuple(tuple);
                fields[2] = self.a.list(vec![tuple]);
                let build = self.primop("build_stacktrace", vec![evars[0]]);
                fields[4] = self.wrap(vec![Pre::Set(stack, build)], fields[4]);
            }
            let rewritten = self.a.tuple(fields);
            if let Some(allocations) = self.guard_allocations.remove(&node) {
                self.guard_allocations.insert(rewritten, allocations);
            }
            clauses.push(rewritten);
        }
        let ann = if let Some(first) = clauses.first() {
            self.a.tuple_fields(*first)?[1]
        } else {
            empty
        };
        let es = self.a.list(evars.clone());
        let tuple = self.a.tagged("c_tuple", vec![empty, es]);
        let generated = self.a.atom("compiler_generated");
        let mut fail_ann = vec![generated];
        fail_ann.extend(self.a.list_items(ann)?);
        let fail_ann = self.a.list(fail_ann);
        let raise = self.primop("raise", vec![evars[2], evars[1]]);
        let pats = self.a.list(vec![tuple]);
        let truth = self.atom_literal("true");
        clauses.push(
            self.a
                .tagged("c_clause", vec![fail_ann, pats, truth, raise]),
        );
        let clauses = self.a.list(clauses);
        let handler = self.a.tagged("c_case", vec![ann, tuple, clauses]);
        Ok((evars, handler))
    }
    pub(super) fn try_expr(&mut self, f: &[TermId], ann: TermId) -> Result<Expr> {
        if !self.a.list_items(f[5])?.is_empty() {
            if self.a.list_items(f[3])?.is_empty() && self.a.list_items(f[4])?.is_empty() {
                return self.try_after(f, ann);
            }
            let empty = self.empty();
            let inner = self.a.tagged("try", vec![f[1], f[2], f[3], f[4], empty]);
            let expressions = self.a.list(vec![inner]);
            let outer = vec![f[0], f[1], expressions, empty, empty, f[5]];
            return self.try_after(&outer, ann);
        }
        let known = self.known_vars.clone();
        let mut bound = known.clone();
        let arg = self.body(f[2], &mut bound)?;
        let empty = self.empty();
        let result = self.fresh(empty);
        let body = if self.a.list_items(f[3])?.is_empty() {
            result
        } else {
            let mut clauses = Vec::new();
            let before = self.case_patterns;
            self.case_patterns = true;
            self.known_vars = bound;
            for clause in self.a.list_items(f[3])? {
                clauses.push(self.clause(clause)?);
            }
            self.case_patterns = before;
            let failure = self.fresh(empty);
            clauses.push(self.failure(vec![failure], ann, "try_clause")?);
            let clauses = self.a.list(clauses);
            self.a.tagged("c_case", vec![ann, result, clauses])
        };
        self.known_vars = known.clone();
        let (evars, handler) = self.exception_handler(f[4])?;
        self.known_vars = known;
        let vars = self.a.list(vec![result]);
        let evars = self.a.list(evars);
        Ok(Expr {
            value: self
                .a
                .tagged("c_try", vec![ann, arg, vars, body, evars, handler]),
            pre: vec![],
        })
    }
    fn try_after(&mut self, f: &[TermId], ann: TermId) -> Result<Expr> {
        let known = self.known_vars.clone();
        let mut bound = known.clone();
        let arg = self.body(f[2], &mut bound)?;
        self.known_vars = known.clone();
        let mut expressions = self.a.list_items(f[5])?;
        let last = expressions.pop().ok_or_else(|| self.err("empty after"))?;
        let underscore = self.a.atom("_");
        let underscore = self.a.tagged("var", vec![f[1], underscore]);
        let matched = self.a.tagged("match", vec![f[1], underscore, last]);
        expressions.push(matched);
        let expressions = self.a.list(expressions);
        let mut bound = known.clone();
        let (steps, size) = self.body_steps(expressions, &mut bound)?;
        self.known_vars = known.clone();
        let empty = self.empty();
        let result = self.fresh(empty);
        let (body, handler, defs) = if size < 20 {
            let mut duplicate = Vec::new();
            let mut memo = std::collections::BTreeMap::new();
            for (expression, assigned) in &steps {
                duplicate.push((self.duplicate_expr(expression, &mut memo)?, *assigned));
            }
            let body = self.assemble_steps(steps, Some(result))?;
            (body, Some(duplicate), None)
        } else {
            let name = self.a.atom(format!("after$^{}", self.fun_counter));
            self.fun_counter += 1;
            let zero = self.a.integer(0);
            let fa = self.a.tuple(vec![name, zero]);
            let sourcebody = self.assemble_steps(steps, None)?;
            let truth = self.atom_literal("true");
            let pats = self.empty();
            let clause = self
                .a
                .tagged("c_clause", vec![ann, pats, truth, sourcebody]);
            let failure = self.failure(vec![], ann, "function_clause")?;
            let clauses = self.a.list(vec![clause, failure]);
            let es = self.empty();
            let values = self.a.tagged("c_values", vec![ann, es]);
            let case = self.a.tagged("c_case", vec![ann, values, clauses]);
            let vars = self.empty();
            let fun = self.a.tagged("c_fun", vec![ann, vars, case]);
            let binder = self.variable(empty, fa);
            let definition = self.a.tuple(vec![binder, fun]);
            let marker = self.a.atom("compiler_generated");
            let mut annotations = vec![marker];
            annotations.extend(self.a.list_items(ann)?);
            let applyann = self.a.list(annotations);
            let operator = self.variable(ann, fa);
            let args = self.empty();
            let apply = self.a.tagged("c_apply", vec![applyann, operator, args]);
            let body = self.a.tagged("c_seq", vec![empty, apply, result]);
            (
                body,
                Some(vec![(
                    Expr {
                        value: apply,
                        pre: vec![],
                    },
                    false,
                )]),
                Some(definition),
            )
        };
        let mut evars = Vec::new();
        for _ in 0..3 {
            evars.push(self.fresh(empty));
        }
        evars.reverse();
        let raise = self.primop("raise", vec![evars[2], evars[1]]);
        let handlerbody = self.assemble_steps(handler.unwrap(), Some(raise))?;
        let triple = self.a.list(evars.clone());
        let triple = self.a.tagged("c_tuple", vec![empty, triple]);
        let pats = self.a.list(vec![triple]);
        let truth = self.atom_literal("true");
        let marker = self.a.atom("compiler_generated");
        let generated = self.a.list(vec![marker]);
        let clause = self
            .a
            .tagged("c_clause", vec![generated, pats, truth, handlerbody]);
        let clauses = self.a.list(vec![clause]);
        let handler = self.a.tagged("c_case", vec![empty, triple, clauses]);
        let vars = self.a.list(vec![result]);
        let evars = self.a.list(evars);
        let mut value = self
            .a
            .tagged("c_try", vec![ann, arg, vars, body, evars, handler]);
        if let Some(definition) = defs {
            let defs = self.a.list(vec![definition]);
            value = self.a.tagged("c_letrec", vec![ann, defs, value]);
        }
        self.known_vars = known;
        Ok(Expr { value, pre: vec![] })
    }
    fn duplicate_expr(
        &mut self,
        e: &Expr,
        memo: &mut std::collections::BTreeMap<TermId, TermId>,
    ) -> Result<Expr> {
        let value = self.duplicate_term(e.value, memo)?;
        let mut pre = Vec::new();
        for instruction in &e.pre {
            pre.push(match instruction {
                Pre::Effect(arg) => Pre::Effect(self.duplicate_term(*arg, memo)?),
                Pre::Set(var, arg) => Pre::Set(
                    self.duplicate_term(*var, memo)?,
                    self.duplicate_term(*arg, memo)?,
                ),
                Pre::Match {
                    ann,
                    arg,
                    pattern,
                    raw_pattern,
                    guard,
                    failure,
                    allocations,
                } => Pre::Match {
                    ann: *ann,
                    raw_pattern: self.duplicate_term(*raw_pattern, memo)?,
                    arg: self.duplicate_term(*arg, memo)?,
                    pattern: self.duplicate_term(*pattern, memo)?,
                    guard: self.duplicate_term(*guard, memo)?,
                    failure: self.duplicate_term(*failure, memo)?,
                    allocations: allocations
                        .iter()
                        .map(|v| self.duplicate_term(*v, memo))
                        .collect::<Result<_>>()?,
                },
            });
        }
        Ok(Expr { value, pre })
    }
    fn duplicate_term(
        &mut self,
        node: TermId,
        memo: &mut std::collections::BTreeMap<TermId, TermId>,
    ) -> Result<TermId> {
        if let Some(copy) = memo.get(&node) {
            return Ok(*copy);
        }
        let copy = match self.a.get(node).clone() {
            ErlangTerm::Tuple(mut f) => {
                for child in &mut f {
                    *child = self.duplicate_term(*child, memo)?;
                }
                self.a.tuple(f)
            }
            ErlangTerm::List { items, tail } => {
                let mut copies = Vec::new();
                for child in items {
                    copies.push(self.duplicate_term(child, memo)?);
                }
                let tail = self.duplicate_term(tail, memo)?;
                self.a.push(ErlangTerm::List {
                    items: copies,
                    tail,
                })
            }
            _ => node,
        };
        memo.insert(node, copy);
        if self.deferred_vars.contains(&node) {
            self.deferred_vars.insert(copy);
        }
        if let Some(original) = self.deferred_aliases.get(&node).copied() {
            let original = self.duplicate_term(original, memo)?;
            self.deferred_aliases.insert(copy, original);
        }
        if let Some(variables) = self.guard_allocations.get(&node).cloned() {
            let mut copies = Vec::new();
            for variable in variables {
                copies.push(self.duplicate_term(variable, memo)?);
            }
            self.guard_allocations.insert(copy, copies);
        }
        Ok(copy)
    }
    pub(super) fn catch_expr(&mut self, f: &[TermId]) -> Result<Expr> {
        let known = self.known_vars.clone();
        let expression = self.expr(f[2])?;
        self.known_vars = known;
        let body = self.wrap(expression.pre, expression.value);
        let ann = self.empty();
        let value = self.a.tagged("c_catch", vec![ann, body]);
        let source = self.annotation(f[1])?;
        self.source_annotations.insert(value, source);
        Ok(Expr { value, pre: vec![] })
    }
}
