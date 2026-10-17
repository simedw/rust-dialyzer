//! Native records in OTP29 v3_core.erl. Field expressions retain source order;
//! record identity and accessibility checks precede an update's result.
use super::*;
impl Lower {
    pub(super) fn raw_record_pattern(&mut self, f: &[TermId], ann: TermId) -> Result<TermId> {
        let mut pairs = Vec::new();
        for pair in self.a.list_items(f[3])? {
            let p = self.a.tuple_fields(pair)?.to_vec();
            if self.a.tag(pair) != Some("record_field") || p.len() != 4 { return Err(self.err("native record pattern field")); }
            let key = self.raw_pattern(p[2])?;
            let value = self.raw_pattern(p[3])?;
            let ann = self.annotation(p[1])?;
            pairs.push(self.a.tagged("c_record_pair", vec![ann, key, value]));
        }
        let empty = self.empty();
        let argument = self.atom_literal("ok");
        let id = self.literal(empty, f[2]);
        let pairs = self.a.list(pairs);
        Ok(self.a.tagged("c_record", vec![ann, argument, id, pairs]))
    }
    pub(super) fn build_record_pairs(&mut self, source: TermId, argument: TermId, id: TermId, ann: TermId) -> Result<Expr> {
        let mut pairs = Vec::new();
        let mut pre = Vec::new();
        for pair in self.a.list_items(source)? {
            let p = self.a.tuple_fields(pair)?.to_vec();
            if self.a.tag(pair) != Some("record_field") || p.len() != 4 { return Err(self.err("native record expression field")); }
            let key = self.source_safe(p[2])?;
            let value = self.source_safe(p[3])?;
            pre.extend(key.pre);
            pre.extend(value.pre);
            let ann = self.annotation(p[1])?;
            pairs.push(self.a.tagged("c_record_pair", vec![ann, key.value, value.value]));
        }
        let pairs = self.a.list(pairs);
        Ok(Expr { value: self.a.tagged("c_record", vec![ann, argument, id, pairs]), pre })
    }
    fn record_bad_case(&mut self, ann: TermId, record: TermId, guard: TermId, body: TermId) -> Result<TermId> {
        let generated = self.a.atom("compiler_generated");
        let mut annotations = vec![generated];
        annotations.extend(self.a.list_items(ann)?);
        let generated_ann = self.a.list(annotations);
        let empty = self.empty();
        let clause = self.a.tagged("c_clause", vec![generated_ann, empty, guard, body]);
        let badrecord = self.atom_literal("badrecord");
        let es = self.a.list(vec![badrecord, record]);
        let reason = self.a.tagged("c_tuple", vec![empty, es]);
        let eval = self.a.atom("eval_failure");
        let bad = self.a.atom("badrecord");
        let eval = self.a.tuple(vec![eval, bad]);
        let mut annotations = vec![eval];
        annotations.extend(self.a.list_items(ann)?);
        let failure_ann = self.a.list(annotations);
        let name = self.atom_literal("match_fail");
        let args = self.a.list(vec![reason]);
        let fail = self.a.tagged("c_primop", vec![failure_ann, name, args]);
        let generated_ann = self.a.list(vec![generated]);
        let truth = self.atom_literal("true");
        let fail = self.a.tagged("c_clause", vec![generated_ann, empty, truth, fail]);
        let clauses = self.a.list(vec![clause, fail]);
        let values = self.a.tagged("c_values", vec![empty, empty]);
        Ok(self.a.tagged("c_case", vec![ann, values, clauses]))
    }
    fn record_check(&mut self, ann: TermId, name: &str, args: Vec<TermId>) -> TermId {
        let empty = self.empty();
        let native = name == "is_native_record";
        let name = self.a.atom(name);
        let name = self.literal(ann, name);
        let args = self.a.list(args);
        self.a.tagged("c_primop", vec![if native { empty } else { ann }, name, args])
    }
    pub(super) fn record_update(&mut self, fields: &[TermId], ann: TermId) -> Result<Expr> {
        if self.in_guard { return Err(self.err("native record update in guard")); }
        let mut base = self.source_safe(fields[2])?;
        if self.a.tag(base.value) != Some("c_var") {
            let empty = self.empty();
            let variable = self.fresh(empty);
            let annotation = self.expression_annotation(base.value)?;
            self.let_annotations.insert(variable, annotation);
            base.pre.push(Pre::Set(variable, base.value));
            base.value = variable;
        }
        let empty = self.empty();
        let id = self.literal(empty, fields[3]);
        let full = self.full_annotation(ann)?;
        let updates = self.build_record_pairs(fields[4], base.value, id, full)?;
        base.pre.extend(updates.pre);
        let mut body = if self.a.list_items(fields[4])?.is_empty() { base.value } else { updates.value };
        let identity = self.a.get(fields[3]).clone();
        match identity {
            ErlangTerm::Tuple(names) if names.len() == 2 => {
                let scope = self.atom_literal("external");
                let accessible = self.record_check(ann, "is_record_accessible", vec![base.value, scope]);
                body = self.record_bad_case(ann, base.value, accessible, body)?;
                let module = self.literal(empty, names[0]);
                let name = self.literal(empty, names[1]);
                let check = self.record_check(ann, "is_native_record", vec![base.value, module, name]);
                body = self.record_bad_case(ann, base.value, check, body)?;
            }
            ErlangTerm::Nil => {
                let scope = self.atom_literal("auto_local");
                let accessible = self.record_check(ann, "is_record_accessible", vec![base.value, scope]);
                body = self.record_bad_case(ann, base.value, accessible, body)?;
            }
            ErlangTerm::Atom(name) => {
                let module = self.atom_literal(&self.module.clone());
                let name = self.atom_literal(&name);
                let check = self.record_check(ann, "is_native_record", vec![base.value, module, name]);
                body = self.record_bad_case(ann, base.value, check, body)?;
            }
            _ => return Err(self.err("native record update identifier")),
        }
        let guard = self.guard_call(ann, "is_record", vec![base.value]);
        base.value = self.record_bad_case(ann, base.value, guard, body)?;
        Ok(base)
    }
}
