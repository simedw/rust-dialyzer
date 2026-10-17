//! OTP source matching and force_safe, before Core export conversion.
use super::*;
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum MatchMode {
    Last,
    Discard,
    Safe,
}
impl Lower {
    pub(super) fn source_safe(&mut self, node: TermId) -> Result<Expr> {
        if self.a.tag(node) == Some("match") {
            let mut bound = self.known_vars.clone();
            return self.matching(node, &mut bound, MatchMode::Safe);
        }
        let expr = self.expr(node)?;
        self.force_safe_expression(expr)
    }
    pub(super) fn force_safe_expression(&mut self, mut expr: Expr) -> Result<Expr> {
        let safe = match self.a.tag(expr.value) {
            Some("c_literal" | "c_tuple" | "c_cons") => true,
            Some("c_var") => !matches!(
                self.a.get(self.a.tuple_fields(expr.value)?[2]),
                ErlangTerm::Tuple(_)
            ),
            _ => false,
        };
        if !safe {
            let mut annotations = self.a.list_items(self.expression_annotation(expr.value)?)?;
            if self.a.tag(expr.value) == Some("c_fun") {
                annotations.retain(|a| self.a.tag(*a) != Some("id"));
            }
            let ann = self.a.list(annotations);
            let var = self.fresh(ann);
            expr.pre.push(Pre::Set(var, expr.value));
            expr.value = var;
        }
        Ok(expr)
    }
    pub(super) fn source_novars(&mut self, node: TermId) -> Result<Expr> {
        if self.a.tag(node) == Some("match") {
            return self.source_safe(node);
        }
        let expr = self.expr(node)?;
        #[cfg(otp_domain = "29")]
        if self.a.tag(expr.value) == Some("c_record") { return Ok(expr); }
        if matches!(
            self.a.tag(expr.value),
            Some("c_apply" | "c_call" | "c_fun" | "c_binary" | "c_map")
        ) {
            Ok(expr)
        } else {
            self.force_safe_expression(expr)
        }
    }
    pub(super) fn matching(
        &mut self,
        node: TermId,
        bound: &mut HashSet<String>,
        mode: MatchMode,
    ) -> Result<Expr> {
        let root = self.a.tuple_fields(node)?.to_vec();
        let old_wanted = self.wanted;
        let p = self.a.tuple_fields(root[2])?;
        if self.a.tag(root[2]) == Some("var")
            && self.a.as_atom(p[2]).is_some_and(|n| n.starts_with('_'))
        {
            self.wanted = false;
        }
        let mut source = node;
        let mut patterns = Vec::new();
        loop {
            let f = self.a.tuple_fields(source)?.to_vec();
            patterns.push((f[1], f[2]));
            source = f[3];
            if self.a.tag(source) != Some("match") {
                break;
            }
        }
        let nested = patterns.len() > 1;
        let right = if nested {
            self.source_safe(source)
        } else {
            self.source_novars(source)
        };
        self.wanted = old_wanted;
        let mut right = right?;
        let exports = self.source_export_names(source)?;
        bound.extend(exports.iter().cloned());
        self.known_vars.extend(exports);
        if nested {
            if self.a.tag(right.value) != Some("c_var") {
                let empty = self.empty();
                let variable = self.fresh(empty);
                right.pre.push(Pre::Set(variable, right.value));
                right.value = variable;
            }
            let shared = right.value;
            patterns.reverse();
            let count = patterns.len();
            for (i, (_location, pattern)) in patterns.into_iter().enumerate() {
                let step_mode = if i + 1 == count {
                    mode
                } else {
                    MatchMode::Discard
                };
                let step = self.single_matching(
                    root[1],
                    pattern,
                    Expr {
                        value: shared,
                        pre: vec![],
                    },
                    bound,
                    step_mode,
                )?;
                right.pre.extend(step.pre);
                right.value = step.value;
            }
            Ok(right)
        } else {
            self.single_matching(root[1], root[2], right, bound, mode)
        }
    }
    fn single_matching(
        &mut self,
        location: TermId,
        source: TermId,
        mut right: Expr,
        bound: &mut HashSet<String>,
        mode: MatchMode,
    ) -> Result<Expr> {
        // Source single_match allocates the failure pattern even if uexprs subsequently elides it.
        let empty = self.empty();
        let failed = self.fresh(empty);
        let ann = self.annotation(location)?;
        let failure = self.failure(vec![failed], ann, "badmatch")?;
        let counters = (self.counter, self.fun_counter, self.goto_counter);
        let mut pattern = match self.raw_pattern(source) {
            Ok(pattern) => pattern,
            Err(Error::NoMatch) => {
                (self.counter, self.fun_counter, self.goto_counter) = counters;
                right = self.force_safe_expression(right)?;
                let sanitized = self.sanitize_pattern(source)?;
                let pattern = self.raw_pattern(sanitized)?;
                let tuple_ann = self.empty();
                let reason = if self.a.tag(right.value) == Some("c_literal") {
                    let badmatch = self.a.atom("badmatch");
                    let value = self.a.tuple_fields(right.value)?[2];
                    let value = self.a.tuple(vec![badmatch, value]);
                    self.literal(tuple_ann, value)
                } else {
                    let badmatch = self.atom_literal("badmatch");
                    let values = self.a.list(vec![badmatch, right.value]);
                    self.a.tagged("c_tuple", vec![tuple_ann, values])
                };
                let name = self.atom_literal("match_fail");
                let arguments = self.a.list(vec![reason]);
                let failure = self.a.tagged("c_primop", vec![ann, name, arguments]);
                right.pre.push(Pre::Effect(failure));
                pattern
            }
            Err(error) => return Err(error),
        };
        let mut result = right.value;
        if mode == MatchMode::Safe {
            right = self.force_safe_expression(right)?;
            result = right.value;
            if self.a.tag(result) != Some("c_var") {
                let empty = self.empty();
                let variable = self.fresh(empty);
                let aliasann = self.empty();
                pattern = self.a.tagged("c_alias", vec![aliasann, variable, pattern]);
                result = variable;
            }
        }
        if self.a.tag(pattern) == Some("c_var") {
            let p = self.a.tuple_fields(pattern)?.to_vec();
            let name = self
                .a
                .as_atom(p[2])
                .map(str::to_owned)
                .or_else(|| self.a.as_integer(p[2]).map(|n| format!("#native:{n}")))
                .ok_or("lower: match variable")?;
            if name == "_" || !bound.contains(&name) {
                if name != "_" {
                    bound.insert(name.clone());
                    self.known_vars.insert(name);
                } else {
                    let mut p = p;
                    p[1] = self.empty();
                    pattern = self.a.tuple(p);
                }
                right.pre.push(Pre::Set(pattern, right.value));
                right.value = if mode == MatchMode::Safe {
                    result
                } else {
                    pattern
                };
                return Ok(right);
            }
        }
        let old_case = self.case_patterns;
        self.case_patterns = true;
        let old_pins = std::mem::take(&mut self.pin_tests);
        let raw_pattern = pattern;
        let normalized = self.normalize_pattern(pattern, bound);
        self.case_patterns = old_case;
        let pattern = normalized?;
        let pins = std::mem::take(&mut self.pin_tests);
        let truth = self.atom_literal("true");
        let (guard, allocations) = self.compose_pattern_guard(pins, truth, true)?;
        self.pin_tests = old_pins;
        self.known_vars.extend(bound.iter().cloned());
        if mode == MatchMode::Last {
            result = self.match_return_value(&mut right)?;
        }
        right.pre.push(Pre::Match {
            ann,
            arg: right.value,
            pattern,
            raw_pattern,
            guard,
            failure,
            allocations,
        });
        right.value = result;
        Ok(right)
    }
}
