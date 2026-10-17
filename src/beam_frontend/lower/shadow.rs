//! OTP v3_core rename_shadowing_clauses, before clause variable normalization.
use super::*;
impl Lower {
    pub(super) fn register_fun_shadowing(&mut self, fun: TermId) -> TermId {
        self.shadowed_funs.insert(fun, self.known_vars.clone());
        fun
    }
    pub(super) fn rename_fun_shadowing(
        &mut self,
        fun: TermId,
        known: HashSet<String>,
    ) -> Result<TermId> {
        if known.is_empty() {
            return Ok(fun);
        }
        let mut fields = self.a.tuple_fields(fun)?.to_vec();
        if self.a.tag(fields[3]) != Some("c_case") {
            return Ok(fun);
        }
        let mut case = self.a.tuple_fields(fields[3])?.to_vec();
        let mut clauses = Vec::new();
        for clause in self.a.list_items(case[3])? {
            let mut f = self.a.tuple_fields(clause)?.to_vec();
            let mut substitutions = std::collections::BTreeMap::new();
            let mut sets = Vec::new();
            let mut pats = Vec::new();
            for pattern in self.a.list_items(f[2])? {
                pats.push(self.shadow_pattern(pattern, &known, &mut substitutions, &mut sets)?);
            }
            f[2] = self.a.list(pats);
            sets.reverse();
            if !sets.is_empty() {
                let no_guard = self.a.tag(f[3]) == Some("c_literal")
                    && self.a.as_atom(self.a.tuple_fields(f[3])?[2]) == Some("true");
                if !no_guard {
                    f[3] = self.wrap(sets.clone(), f[3]);
                }
                f[4] = self.wrap(sets, f[4]);
            }
            let node = self.a.tuple(f);
            if let Some(allocations) = self.guard_allocations.remove(&clause) {
                self.guard_allocations.insert(node, allocations);
            }
            clauses.push(node);
        }
        case[3] = self.a.list(clauses);
        fields[3] = self.a.tuple(case);
        Ok(self.a.tuple(fields))
    }
    fn shadow_pattern(
        &mut self,
        node: TermId,
        known: &HashSet<String>,
        substitutions: &mut std::collections::BTreeMap<String, TermId>,
        sets: &mut Vec<Pre>,
    ) -> Result<TermId> {
        let mut f = self.a.tuple_fields(node)?.to_vec();
        match self.a.tag(node) {
            Some("c_var") => {
                let Some(name) = self.a.as_atom(f[2]).map(str::to_owned) else {
                    return Ok(node);
                };
                if name == "_" || !known.contains(&name) {
                    return Ok(node);
                }
                if let Some(new) = substitutions.get(&name) {
                    return Ok(*new);
                }
                let empty = self.empty();
                let new = self.fresh(empty);
                substitutions.insert(name, new);
                sets.push(Pre::Set(node, new));
                Ok(new)
            }
            Some("c_literal") => Ok(node),
            Some("c_tuple") => {
                let mut es = Vec::new();
                for e in self.a.list_items(f[2])? {
                    es.push(self.shadow_pattern(e, known, substitutions, sets)?);
                }
                self.ann_c_tuple(f[1], es)
            }
            Some("c_cons") => {
                let h = self.shadow_pattern(f[2], known, substitutions, sets)?;
                let t = self.shadow_pattern(f[3], known, substitutions, sets)?;
                self.ann_c_cons(f[1], h, t)
            }
            Some("c_alias") => {
                f[2] = self.shadow_pattern(f[2], known, substitutions, sets)?;
                f[3] = self.shadow_pattern(f[3], known, substitutions, sets)?;
                Ok(self.a.tuple(f))
            }
            Some("c_map") => {
                let mut pairs = Vec::new();
                for pair in self.a.list_items(f[3])? {
                    let mut pf = self.a.tuple_fields(pair)?.to_vec();
                    pf[4] = self.shadow_pattern(pf[4], known, substitutions, sets)?;
                    pairs.push(self.a.tuple(pf));
                }
                f[3] = self.a.list(pairs);
                Ok(self.a.tuple(f))
            }
            #[cfg(otp_domain = "29")]
            Some("c_record") => {
                let mut pairs = Vec::new();
                for pair in self.a.list_items(f[4])? {
                    let mut p = self.a.tuple_fields(pair)?.to_vec();
                    p[3] = self.shadow_pattern(p[3], known, substitutions, sets)?;
                    pairs.push(self.a.tuple(p));
                }
                f[4] = self.a.list(pairs);
                Ok(self.a.tuple(f))
            }
            Some("c_binary") => {
                let mut segments = Vec::new();
                let mut locals = std::collections::BTreeMap::new();
                for segment in self.a.list_items(f[2])? {
                    let mut sf = self.a.tuple_fields(segment)?.to_vec();
                    if self.a.tag(sf[3]) == Some("c_var") {
                        let size = self.a.tuple_fields(sf[3])?;
                        if let Some(name) = self.a.as_atom(size[2]) {
                            if let Some(new) = locals.get(name) {
                                sf[3] = *new;
                            }
                        }
                    }
                    let old = sf[2];
                    sf[2] = self.shadow_pattern(old, known, substitutions, sets)?;
                    if old != sf[2] && self.a.tag(old) == Some("c_var") {
                        if let Some(name) = self.a.as_atom(self.a.tuple_fields(old)?[2]) {
                            locals.insert(name.to_owned(), sf[2]);
                        }
                    }
                    segments.push(self.a.tuple(sf));
                }
                f[2] = self.a.list(segments);
                Ok(self.a.tuple(f))
            }
            _ => Err(self.err("generated function pattern shadowing")),
        }
    }
}
