//! OTP v3_core lbody receive lowering. This pass runs after variable
//! normalization so generated message-loop variables follow all source vars.
use super::*;
impl Lower {
    pub(super) fn receive_expr(&mut self, f: &[TermId], ann: TermId) -> Result<Expr> {
        let mut pre = Vec::new();
        let (timeout, action) = if f.len() == 5 {
            let mut timeout = self.expr(f[3])?;
            if !matches!(
                self.a.tag(timeout.value),
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
                let ann = self.a.tuple_fields(timeout.value)?[1];
                let var = self.fresh(ann);
                timeout.pre.push(Pre::Set(var, timeout.value));
                timeout.value = var;
            }
            pre = timeout.pre;
            let known = self.known_vars.clone();
            let mut bound = known.clone();
            let action = self.body(f[4], &mut bound)?;
            self.known_vars = known;
            (timeout.value, action)
        } else {
            (self.atom_literal("infinity"), self.atom_literal("true"))
        };
        let before = self.case_patterns;
        self.case_patterns = true;
        let mut clauses = Vec::new();
        for clause in self.a.list_items(f[2])? {
            clauses.push(self.clause(clause)?);
        }
        self.case_patterns = before;
        let clauses = self.a.list(clauses);
        Ok(Expr {
            value: self
                .a
                .tagged("c_receive", vec![ann, clauses, timeout, action]),
            pre,
        })
    }
    pub(super) fn lower_final(&mut self, node: TermId) -> Result<TermId> {
        if matches!(
            self.a.tag(node),
            Some(
                "c_binary"
                    | "c_call"
                    | "c_cons"
                    | "c_literal"
                    | "c_map"
                    | "c_opaque"
                    | "c_primop"
                    | "c_tuple"
                    | "c_var"
            )
        ) {
            return Ok(node);
        }
        match self.a.get(node).clone() {
            ErlangTerm::Tuple(mut fields) => {
                if fields
                    .first()
                    .and_then(|id| self.a.as_atom(*id))
                    .is_some_and(|tag| tag.starts_with("c_"))
                {
                    for child in fields.iter_mut().skip(2) {
                        *child = self.lower_final(*child)?;
                    }
                } else {
                    for child in &mut fields {
                        *child = self.lower_final(*child)?;
                    }
                }
                let tree = self.a.tuple(fields);
                if self.a.tag(tree) == Some("c_receive") {
                    self.lower_receive(tree)
                } else if self.a.tag(tree) == Some("c_case") {
                    self.split_case(tree)
                } else {
                    Ok(tree)
                }
            }
            ErlangTerm::List { items, tail } => {
                let mut out = Vec::new();
                for child in items {
                    out.push(self.lower_final(child)?);
                }
                let tail = self.lower_final(tail)?;
                Ok(self.a.push(ErlangTerm::List { items: out, tail }))
            }
            _ => Ok(node),
        }
    }
    fn marker_annotation(&mut self, names: &[&str]) -> TermId {
        let atoms = names.iter().map(|n| self.a.atom(*n)).collect();
        self.a.list(atoms)
    }
    fn generated_clause(&mut self, ann: TermId, pattern: TermId, body: TermId) -> TermId {
        let pats = self.a.list(vec![pattern]);
        let truth = self.atom_literal("true");
        self.a.tagged("c_clause", vec![ann, pats, truth, body])
    }
    fn core_case(&mut self, ann: TermId, arg: TermId, clauses: Vec<TermId>) -> TermId {
        let clauses = self.a.list(clauses);
        self.a.tagged("c_case", vec![ann, arg, clauses])
    }
    fn core_let(&mut self, vars: Vec<TermId>, arg: TermId, body: TermId) -> TermId {
        let ann = self.empty();
        let vars = self.a.list(vars);
        self.a.tagged("c_let", vec![ann, vars, arg, body])
    }
    fn lower_receive(&mut self, node: TermId) -> Result<TermId> {
        let f = self.a.tuple_fields(node)?.to_vec();
        let empty = self.empty();
        let mut clauses = self.a.list_items(f[2])?;
        let mut timeout = f[3];
        let mut outer = None;
        let safe = match self.a.tag(timeout) {
            Some("c_literal" | "c_cons" | "c_tuple") => true,
            Some("c_var") => !matches!(
                self.a.get(self.a.tuple_fields(timeout)?[2]),
                ErlangTerm::Tuple(_)
            ),
            _ => false,
        };
        if !safe {
            let var = self.fresh(empty);
            outer = Some((var, timeout));
            timeout = var;
        }
        let infinity = self.a.tag(timeout) == Some("c_literal")
            && self.a.as_atom(self.a.tuple_fields(timeout)?[2]) == Some("infinity");
        let ignore = if infinity {
            self.marker_annotation(&["dialyzer_ignore"])
        } else {
            empty
        };
        let name = self.a.atom(format!("recv$^{}", self.fun_counter));
        self.fun_counter += 1;
        let zero = self.a.integer(0);
        let fa = self.a.tuple(vec![name, zero]);
        let loop_fun = self.variable(empty, fa);
        let apply_ann = self.marker_annotation(&["dialyzer_ignore"]);
        let args = self.empty();
        let apply = self.a.tagged("c_apply", vec![apply_ann, loop_fun, args]);
        let truth = self.atom_literal("true");
        let falsity = self.atom_literal("false");
        let body = if clauses.is_empty() {
            let timeout_bool = self.fresh(empty);
            let first = self.generated_clause(ignore, truth, f[4]);
            let ann = self.marker_annotation(&["compiler_generated", "dialyzer_ignore"]);
            let second = self.generated_clause(ann, falsity, apply);
            let ann = self.marker_annotation(&["receive_timeout"]);
            let case = self.core_case(ann, timeout_bool, vec![first, second]);
            let wait = self.primop("recv_wait_timeout", vec![timeout]);
            self.core_let(vec![timeout_bool], wait, case)
        } else {
            for clause in &mut clauses {
                let mut cf = self.a.tuple_fields(*clause)?.to_vec();
                let remove = self.primop("remove_message", vec![]);
                cf[4] = self.a.tagged("c_seq", vec![empty, remove, cf[4]]);
                *clause = self.a.tuple(cf);
            }
            let next = self.primop("recv_next", vec![]);
            let next = self.a.tagged("c_seq", vec![empty, next, apply]);
            let other = self.a.atom("Other");
            let other = self.variable(empty, other);
            let ann = self.marker_annotation(&["compiler_generated", "dialyzer_ignore"]);
            clauses.push(self.generated_clause(ann, other, next));
            let message = self.fresh(empty);
            let message_case = self.core_case(f[1], message, clauses);
            let message_case = self.split_case(message_case)?;
            let timeout_bool = self.fresh(empty);
            let first = self.generated_clause(empty, truth, f[4]);
            let ann = self.marker_annotation(&["dialyzer_ignore"]);
            let second = self.generated_clause(ann, falsity, apply);
            let case = self.core_case(empty, timeout_bool, vec![first, second]);
            let wait = self.primop("recv_wait_timeout", vec![timeout]);
            let wait = self.core_let(vec![timeout_bool], wait, case);
            let peek = self.fresh(empty);
            let first = self.generated_clause(empty, truth, message_case);
            let second = self.generated_clause(ignore, falsity, wait);
            let case = self.core_case(empty, peek, vec![first, second]);
            let primop = self.primop("recv_peek_message", vec![]);
            let mut pf = self.a.tuple_fields(primop)?.to_vec();
            pf[1] = f[1];
            let primop = self.a.tuple(pf);
            self.core_let(vec![peek, message], primop, case)
        };
        let vars = self.empty();
        let fun = self.a.tagged("c_fun", vec![empty, vars, body]);
        let pair = self.a.tuple(vec![loop_fun, fun]);
        let defs = self.a.list(vec![pair]);
        let ann = self.marker_annotation(&["letrec_goto", "no_inline"]);
        let value = self.a.tagged("c_letrec", vec![ann, defs, apply]);
        Ok(if let Some((var, arg)) = outer {
            self.core_let(vec![var], arg, value)
        } else {
            value
        })
    }
}
