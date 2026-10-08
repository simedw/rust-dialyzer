// SPDX-License-Identifier: Apache-2.0
use super::*;
impl Lower {
    pub(super) fn or_guards(&self, id: Id) -> Result<Vec<Id>> {
        if let T::Tuple(p) = self.get(id) {
            if p.len() == 3 && self.name(p[0]).ok().as_deref() == Some("when") {
                let a = self.items(p[2])?;
                let mut out = vec![a[0]];
                out.extend(self.or_guards(a[1])?);
                return Ok(out);
            }
        }
        Ok(vec![id])
    }
    pub(super) fn with_expr(&mut self, xs: &[Id], ann: Id, s: &mut Scope) -> Result<Id> {
        let (opts, steps) = xs.split_last().ok_or("invalid with")?;
        let body = self.prop(*opts, "do")?.ok_or("with lacks do")?;
        let generated = self.generated(ann);
        let mut assignment = None;
        let else_clause = if let Some(else_arrows) = self.prop(*opts, "else")? {
            let arrows = self.items(else_arrows)?;
            let single_var = if arrows.len() == 1 {
                let p = self.tup(arrows[0])?;
                let parts = self.items(p[2])?;
                let args = self.items(parts[0])?;
                args.len() == 1
                    && matches!(self.get(args[0]),T::Tuple(v)if v.len()==3&&matches!(self.get(v[0]),T::Atom(_))&&matches!(self.get(v[2]),T::Atom(_)))
            } else {
                false
            };
            let effective_ann = if single_var { ann } else { generated };
            let clauses = if single_var {
                let p = self.tup(arrows[0])?;
                let parts = self.items(p[2])?;
                let rawargs = self.items(parts[0])?;
                let arg = self.expr(rawargs[0], ann, s, Context::Match, 0)?;
                let body = self.expr(parts[1], ann, s, Context::Expr, 0)?;
                vec![self.make_clause(ann, vec![arg], vec![], vec![body])]
            } else {
                let raise_var = self.fresh(generated, s);
                let reason_tag = self.atom("else_clause");
                let reason = self.ast("atom", generated, vec![reason_tag]);
                let tuple = self.list(vec![reason, raise_var]);
                let reason = self.ast("tuple", generated, vec![tuple]);
                let error = self.remote(generated, "erlang", "error", vec![reason]);
                let catchall = self.make_clause(generated, vec![raise_var], vec![], vec![error]);
                let mut clauses = self.arrows(else_arrows, s)?;
                clauses.push(catchall);
                clauses
            };
            let fun_var = self.fresh(effective_ann, s);
            let arg_var = self.fresh(effective_ann, s);
            let tag = self.atom("clauses");
            let clauses = self.list(clauses);
            let inner = self.tuple(vec![tag, clauses]);
            let fun = self.ast("fun", generated, vec![inner]);
            assignment = Some(self.ast("match", effective_ann, vec![fun_var, fun]));
            let args = self.list(vec![arg_var]);
            let call = self.ast("call", effective_ann, vec![fun_var, args]);
            self.make_clause(generated, vec![arg_var], vec![], vec![call])
        } else {
            let var = self.fresh(ann, s);
            self.make_clause(generated, vec![var], vec![], vec![var])
        };
        let result = self.with_steps(steps, ann, body, else_clause, s)?;
        if let Some(assign) = assignment {
            let body = self.unblock(result)?;
            let mut v = vec![assign];
            v.extend(body);
            let v = self.list(v);
            Ok(self.ast("block", ann, vec![v]))
        } else {
            Ok(result)
        }
    }
    pub(super) fn unblock(&self, id: Id) -> Result<Vec<Id>> {
        match self.get(id) {
            T::Tuple(v) if v.len() == 3 && self.name(v[0])? == "block" => self.items(v[2]),
            _ => Ok(vec![id]),
        }
    }
    pub(super) fn with_steps(
        &mut self,
        steps: &[Id],
        ann: Id,
        body: Id,
        otherwise: Id,
        s: &mut Scope,
    ) -> Result<Id> {
        let Some((head, tail)) = steps.split_first() else {
            return self.expr(body, ann, s, Context::Expr, 0);
        };
        if let T::Tuple(p) = self.get(*head).clone() {
            if p.len() == 3 && self.name(p[0]).ok().as_deref() == Some("<-") {
                let parts = self.items(p[2])?;
                let stepann = self.ann(p[1])?;
                let rawleft = self.get(parts[0]).clone();
                if matches!(rawleft,T::Tuple(ref v)if v.len()==3&&matches!(self.get(v[0]),T::Atom(_))&&matches!(self.get(v[2]),T::Atom(_)))
                {
                    let right = self.expr(parts[1], stepann, s, Context::Expr, 0)?;
                    let left = self.expr(parts[0], stepann, s, Context::Match, 0)?;
                    let assign = self.ast("match", stepann, vec![left, right]);
                    let rest = self.with_steps(tail, stepann, body, otherwise, s)?;
                    let mut body = vec![assign];
                    body.extend(self.unblock(rest)?);
                    let body = self.list(body);
                    return Ok(self.ast("block", stepann, vec![body]));
                }
                let right = self.expr(parts[1], stepann, s, Context::Expr, 0)?;
                let (left, guards) = if let T::Tuple(w) = rawleft {
                    if w.len() == 3 && self.name(w[0]).ok().as_deref() == Some("when") {
                        let args = self.items(w[2])?;
                        (args[0], self.or_guards(args[1])?)
                    } else {
                        (parts[0], vec![])
                    }
                } else {
                    (parts[0], vec![])
                };
                let left = self.expr(left, stepann, s, Context::Match, 0)?;
                let gs = self.take_pattern_guards(s, guards, stepann)?;
                let rest = self.with_steps(tail, stepann, body, otherwise, s)?;
                let rest = self.unblock(rest)?;
                let clause = self.make_clause(stepann, vec![left], gs, rest);
                let clauses = self.list(vec![clause, otherwise]);
                return Ok(self.ast("case", stepann, vec![right, clauses]));
            }
        }
        let first = self.expr(*head, ann, s, Context::Expr, 0)?;
        let rest = self.with_steps(tail, ann, body, otherwise, s)?;
        let mut xs = vec![first];
        xs.extend(self.unblock(rest)?);
        let xs = self.list(xs);
        Ok(self.ast("block", ann, vec![xs]))
    }
    pub(super) fn boolean(&self, id: Id) -> bool {
        if matches!(self.get(id),T::Atom(s) if s=="true"||s=="false") {
            return true;
        }
        let T::Tuple(p) = self.get(id) else {
            return false;
        };
        if p.len() != 3 {
            return false;
        }
        let T::Tuple(dot) = self.get(p[0]) else {
            return false;
        };
        if dot.len() != 3 {
            return false;
        }
        let Ok(parts) = self.items(dot[2]) else {
            return false;
        };
        if parts.len() != 2 || self.name(parts[0]).ok().as_deref() != Some("erlang") {
            return false;
        }
        let Ok(name) = self.name(parts[1]) else {
            return false;
        };
        matches!(
            name.as_str(),
            "==" | "/="
                | "=:="
                | "=/="
                | "<"
                | "=<"
                | ">"
                | ">="
                | "and"
                | "or"
                | "xor"
                | "not"
                | "andalso"
                | "orelse"
        ) || name.starts_with("is_")
    }
    pub(super) fn cond_expr(&mut self, xs: &[Id], ann: Id, s: &mut Scope) -> Result<Id> {
        if xs.len() != 1 {
            return Err("invalid cond".into());
        }
        let clauses = self.prop(xs[0], "do")?.ok_or("cond lacks do")?;
        let clauses = self.items(clauses)?;
        let last = self.tup(*clauses.last().ok_or("empty cond")?)?;
        let lastann = self.ann(last[1])?;
        let parts = self.items(last[2])?;
        let conditions = self.items(parts[0])?;
        let mut oldann = if matches!(self.get(conditions[0]),T::Atom(name) if name!="false"&&name!="nil")
        {
            self.generated(lastann)
        } else {
            lastann
        };
        let reason = self.atom("cond_clause");
        let reason = self.ast("atom", lastann, vec![reason]);
        let mut result = self.remote(lastann, "erlang", "error", vec![reason]);
        for (index, clause) in clauses.into_iter().enumerate().rev() {
            let p = self.tup(clause)?;
            let clauseann = self.ann(p[1])?;
            let pair = self.items(p[2])?;
            let conditions = self.items(pair[0])?;
            let condition = conditions[0];
            let boolean = self.boolean(condition);
            let val = self.expr(
                condition,
                if index == 0 { ann } else { clauseann },
                s,
                Context::Expr,
                0,
            )?;
            let body = self.body(pair[1], clauseann, s)?;
            let body = self.items(body)?;
            let (arg, gs, fallback) = if boolean {
                let yes = self.atom("true");
                let yes = self.ast("atom", clauseann, vec![yes]);
                let no = self.atom("false");
                let no = self.ast("atom", oldann, vec![no]);
                (yes, vec![], no)
            } else {
                let var = self.fresh(clauseann, s);
                let zero = self.int(0);
                let nil = self.atom("nil");
                let nil = self.ast("atom", zero, vec![nil]);
                let no = self.atom("false");
                let no = self.ast("atom", zero, vec![no]);
                let neq = self.atom("/=");
                let left = self.ast("op", zero, vec![neq, var, nil]);
                let right = self.ast("op", zero, vec![neq, var, no]);
                let and = self.atom("andalso");
                let guard = self.ast("op", zero, vec![and, left, right]);
                let guard = self.list(vec![guard]);
                let wild = self.atom("_");
                let wild = self.ast("var", zero, vec![wild]);
                (var, vec![guard], wild)
            };
            let first = self.make_clause(clauseann, vec![arg], gs, body);
            let second = self.make_clause(oldann, vec![fallback], vec![], vec![result]);
            let cs = self.list(vec![first, second]);
            result = self.ast("case", clauseann, vec![val, cs]);
            oldann = clauseann
        }
        let mut p = self.tup(result)?;
        p[1] = ann;
        Ok(self.tuple(p))
    }
}
