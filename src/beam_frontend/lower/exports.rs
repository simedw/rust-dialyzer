//! OTP 28.3 v3_core use/new and cexpr export conversion (Apache-2.0; Ericsson AB).
use super::*;
use std::collections::BTreeSet;
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Name {
    Integer(i64),
    Atom(String),
}
type Names = BTreeSet<Name>;
impl Lower {
    fn export_name(&self, node: TermId) -> Option<Name> {
        if self.a.tag(node) != Some("c_var") {
            return None;
        }
        let name = self.a.tuple_fields(node).ok()?[2];
        self.a
            .as_integer(name)
            .map(Name::Integer)
            .or_else(|| self.a.as_atom(name).map(|a| Name::Atom(a.into())))
    }
    fn export_vars(&mut self, names: &Names) -> Vec<TermId> {
        names
            .iter()
            .map(|name| {
                let id = match name {
                    Name::Integer(n) => self.a.integer(*n),
                    Name::Atom(a) => self.a.atom(a),
                };
                let ann = self.empty();
                self.variable(ann, id)
            })
            .collect()
    }
    fn export_children(&self, node: TermId) -> Result<Vec<TermId>> {
        if matches!(self.a.tag(node), Some("c_var" | "c_literal")) {
            return Ok(vec![]);
        }
        match self.a.get(node) {
            ErlangTerm::Tuple(fields) => Ok(fields
                .iter()
                .skip(if self.a.tag(node).is_some_and(|t| t.starts_with("c_")) {
                    2
                } else {
                    0
                })
                .copied()
                .collect()),
            ErlangTerm::List { items, .. } => Ok(items.clone()),
            _ => Ok(vec![]),
        }
    }
    fn export_pattern_names(&self, node: TermId) -> Result<Names> {
        if let Some(name) = self.export_name(node) {
            return Ok(BTreeSet::from([name]));
        }
        if self.a.tag(node) == Some("c_map") {
            let f = self.a.tuple_fields(node)?;
            let mut names = Names::new();
            for pair in self.a.list_items(f[3])? {
                names.extend(self.export_pattern_names(self.a.tuple_fields(pair)?[4])?);
            }
            return Ok(names);
        }
        if self.a.tag(node) == Some("c_binary") {
            let f = self.a.tuple_fields(node)?;
            let mut names = Names::new();
            for segment in self.a.list_items(f[2])? {
                names.extend(self.export_pattern_names(self.a.tuple_fields(segment)?[2])?);
            }
            return Ok(names);
        }
        let mut names = Names::new();
        for child in self.export_children(node)? {
            names.extend(self.export_pattern_names(child)?);
        }
        Ok(names)
    }
    fn export_pattern_free(&self, node: TermId) -> Result<Names> {
        if matches!(self.a.tag(node), Some("c_var" | "c_literal")) {
            return Ok(Names::new());
        }
        let mut out = Names::new();
        if self.a.tag(node) == Some("c_map") {
            let f = self.a.tuple_fields(node)?;
            for pair in self.a.list_items(f[3])? {
                let p = self.a.tuple_fields(pair)?;
                out.extend(self.export_free(p[3])?);
                out.extend(self.export_pattern_free(p[4])?);
            }
        } else if self.a.tag(node) == Some("c_binary") {
            let f = self.a.tuple_fields(node)?;
            for segment in self.a.list_items(f[2])? {
                let p = self.a.tuple_fields(segment)?;
                out.extend(self.export_free(p[3])?);
                out.extend(self.export_pattern_free(p[2])?);
            }
        } else {
            for child in self.export_children(node)? {
                out.extend(self.export_pattern_free(child)?);
            }
        }
        for name in self.export_pattern_names(node)? {
            out.remove(&name);
        }
        Ok(out)
    }
    fn export_failure(&self, node: TermId) -> bool {
        let Ok(f) = self.a.tuple_fields(node) else {
            return false;
        };
        if self.a.tag(node) == Some("c_clause") {
            return self.export_failure(f[4]);
        }
        if self.a.tag(node) != Some("c_primop") {
            return false;
        }
        self.a
            .tuple_fields(f[2])
            .ok()
            .and_then(|n| self.a.as_atom(n[2]))
            == Some("match_fail")
    }
    fn export_produced(&self, node: TermId) -> Result<Names> {
        let mut out = Names::new();
        let f = self.a.tuple_fields(node).ok();
        match self.a.tag(node) {
            Some("c_fun" | "c_try" | "c_catch" | "c_var" | "c_literal") => (),
            Some("c_case" | "c_receive") => {
                let f = f.unwrap();
                let receive = self.a.tag(node) == Some("c_receive");
                if self
                    .a
                    .list_items(f[1])?
                    .iter()
                    .any(|id| self.a.as_atom(*id) == Some("list_comprehension"))
                {
                    return Ok(out);
                }
                let mut branches = self.a.list_items(f[if receive { 2 } else { 3 }])?;
                branches.retain(|c| !self.export_failure(*c));
                if receive
                    && !(self.a.tag(f[3]) == Some("c_literal")
                        && self.a.as_atom(self.a.tuple_fields(f[3])?[2]) == Some("infinity"))
                {
                    branches.push(f[4]);
                }
                let mut first = true;
                for branch in branches {
                    let ns = self.export_produced(branch)?;
                    if first {
                        out = ns;
                        first = false;
                    } else {
                        out = out.intersection(&ns).cloned().collect();
                    }
                }
            }
            Some("c_clause") => {
                let f = f.unwrap();
                out = self.export_pattern_names(f[2])?;
                out.extend(self.export_produced(f[4])?);
            }
            Some("c_let") => {
                let f = f.unwrap();
                out = self.export_pattern_names(f[2])?;
                out.extend(self.export_produced(f[3])?);
                out.extend(self.export_produced(f[4])?);
            }
            Some("c_seq") => {
                let f = f.unwrap();
                out = self.export_produced(f[2])?;
                out.extend(self.export_produced(f[3])?);
            }
            Some("c_letrec") => {
                out = self.export_produced(f.unwrap()[3])?;
            }
            _ => (),
        }
        Ok(out)
    }
    fn export_free(&self, node: TermId) -> Result<Names> {
        if let Some(name) = self.export_name(node) {
            return Ok(BTreeSet::from([name]));
        }
        let mut out = Names::new();
        let f = self.a.tuple_fields(node).ok();
        match self.a.tag(node) {
            Some("c_literal") => (),
            Some("c_let") => {
                let f = f.unwrap();
                out = self.export_free(f[4])?;
                for n in self.export_pattern_names(f[2])? {
                    out.remove(&n);
                }
                out.extend(self.export_free(f[3])?);
            }
            Some("c_fun") => {
                let f = f.unwrap();
                out = self.export_free(f[3])?;
                for n in self.export_pattern_names(f[2])? {
                    out.remove(&n);
                }
            }
            Some("c_clause") => {
                let f = f.unwrap();
                out = self.export_free(f[3])?;
                out.extend(self.export_free(f[4])?);
                for n in self.export_pattern_names(f[2])? {
                    out.remove(&n);
                }
                out.extend(self.export_pattern_free(f[2])?);
            }
            Some("c_try") => {
                let f = f.unwrap();
                out = self.export_free(f[4])?;
                for n in self.export_pattern_names(f[3])? {
                    out.remove(&n);
                }
                let mut handler = self.export_free(f[6])?;
                for n in self.export_pattern_names(f[5])? {
                    handler.remove(&n);
                }
                out.extend(handler);
                out.extend(self.export_free(f[2])?);
            }
            _ => {
                for child in self.export_children(node)? {
                    out.extend(self.export_free(child)?);
                }
            }
        }
        Ok(out)
    }
    fn export_values(&mut self, value: TermId, names: &Names) -> TermId {
        if names.is_empty() {
            return value;
        }
        let mut es = vec![value];
        es.extend(self.export_vars(names));
        let ann = self
            .a
            .tuple_fields(value)
            .map(|f| f[1])
            .unwrap_or_else(|_| self.empty());
        let es = self.a.list(es);
        self.a.tagged("c_values", vec![ann, es])
    }
    fn export_sequence(&mut self, node: TermId, wanted: &Names, known: &Names) -> Result<TermId> {
        let mut f = self.a.tuple_fields(node)?.to_vec();
        match self.a.tag(node) {
            Some("c_let") => {
                let mut body_known = known.clone();
                body_known.extend(self.export_pattern_names(f[2])?);
                body_known.extend(self.export_produced(f[3])?);
                f[4] = self.export_sequence(f[4], wanted, &body_known)?;
                let after = self.export_free(f[4])?;
                let (arg, exports) = self.export_expression(f[3], &after, known)?;
                f[3] = arg;
                let mut vars = self.a.list_items(f[2])?;
                vars.extend(self.export_vars(&exports));
                f[2] = self.a.list(vars);
                Ok(self.a.tuple(f))
            }
            Some("c_seq") => {
                let mut body_known = known.clone();
                body_known.extend(self.export_produced(f[2])?);
                f[3] = self.export_sequence(f[3], wanted, &body_known)?;
                let after = self.export_free(f[3])?;
                let (arg, exports) = self.export_expression(f[2], &after, known)?;
                if exports.is_empty() {
                    f[2] = arg;
                    Ok(self.a.tuple(f))
                } else {
                    let ann = self.empty();
                    let r = self.fresh(ann);
                    let mut vars = vec![r];
                    vars.extend(self.export_vars(&exports));
                    let vars = self.a.list(vars);
                    Ok(self.a.tagged("c_let", vec![f[1], vars, arg, f[3]]))
                }
            }
            _ => {
                let (arg, exports) = self.export_expression(node, wanted, known)?;
                if exports.is_empty() {
                    Ok(self.export_values(arg, wanted))
                } else {
                    let ann = self.empty();
                    let r = self.fresh(ann);
                    let mut vars = vec![r];
                    vars.extend(self.export_vars(&exports));
                    let vars = self.a.list(vars);
                    let body = self.export_values(r, wanted);
                    let ann = self.export_line_annotation(arg)?;
                    Ok(self.a.tagged("c_let", vec![ann, vars, arg, body]))
                }
            }
        }
    }
    fn export_line_annotation(&mut self, node: TermId) -> Result<TermId> {
        let f = self.a.tuple_fields(node)?;
        let annotations = self.a.list_items(f[1])?;
        let annotations = annotations
            .into_iter()
            .filter(|a| {
                self.a.as_integer(*a).is_some() || matches!(self.a.get(*a), ErlangTerm::Tuple(_))
            })
            .collect();
        Ok(self.a.list(annotations))
    }
    fn export_expression(
        &mut self,
        node: TermId,
        after: &Names,
        known: &Names,
    ) -> Result<(TermId, Names)> {
        let produced = self.export_produced(node)?;
        let exports: Names = produced
            .intersection(after)
            .filter(|n| !known.contains(*n))
            .cloned()
            .collect();
        let mut f = self.a.tuple_fields(node)?.to_vec();
        match self.a.tag(node) {
            Some("c_case" | "c_receive") => {
                let receive = self.a.tag(node) == Some("c_receive");
                let index = if receive { 2 } else { 3 };
                let clauses = self.a.list_items(f[index])?;
                let mut out = Vec::new();
                for clause in clauses {
                    let mut cf = self.a.tuple_fields(clause)?.to_vec();
                    let mut branch_known = known.clone();
                    branch_known.extend(self.export_pattern_names(cf[2])?);
                    if self.export_failure(clause) {
                        cf[4] = self.export_sequence(cf[4], &Names::new(), &branch_known)?;
                        if !exports.is_empty() {
                            let ann = self.empty();
                            let r = self.fresh(ann);
                            let vars = self.a.list(vec![r]);
                            let nil = self.a.nil();
                            let nil = self.literal(ann, nil);
                            let mut values = vec![r];
                            values.extend(std::iter::repeat_n(nil, exports.len()));
                            let values = self.a.list(values);
                            let values = self.a.tagged("c_values", vec![ann, values]);
                            cf[4] = self.a.tagged("c_let", vec![ann, vars, cf[4], values]);
                        }
                    } else {
                        cf[4] = self.export_sequence(cf[4], &exports, &branch_known)?;
                    }
                    out.push(self.a.tuple(cf));
                }
                f[index] = self.a.list(out);
                if receive {
                    let infinity = self.a.tag(f[3]) == Some("c_literal")
                        && self.a.as_atom(self.a.tuple_fields(f[3])?[2]) == Some("infinity");
                    f[4] = self.export_sequence(f[4], &exports, known)?;
                    if infinity && !exports.is_empty() {
                        let ann = self.empty();
                        let truth = self.atom_literal("true");
                        let vals = self.a.list(vec![truth; 1 + exports.len()]);
                        f[4] = self.a.tagged("c_values", vec![ann, vals]);
                    }
                }
                Ok((self.a.tuple(f), exports))
            }
            Some("c_fun") => {
                let mut scope = known.clone();
                scope.extend(self.export_pattern_names(f[2])?);
                f[3] = self.export_sequence(f[3], &Names::new(), &scope)?;
                Ok((self.a.tuple(f), Names::new()))
            }
            Some("c_let" | "c_seq") => Ok((self.export_sequence(node, &exports, known)?, exports)),
            Some("c_try") => {
                let arg_names = self.export_produced(f[2])?;
                let used = self.export_free(f[4])?;
                let arg_exports = arg_names
                    .intersection(&used)
                    .filter(|n| !known.contains(*n))
                    .cloned()
                    .collect::<Names>();
                f[2] = self.export_sequence(f[2], &arg_exports, known)?;
                let mut scope = known.clone();
                scope.extend(self.export_pattern_names(f[3])?);
                scope.extend(arg_exports.iter().cloned());
                f[4] = self.export_sequence(f[4], &Names::new(), &scope)?;
                let mut handler_scope = known.clone();
                handler_scope.extend(self.export_pattern_names(f[5])?);
                f[6] = self.export_sequence(f[6], &Names::new(), &handler_scope)?;
                let mut vars = self.a.list_items(f[3])?;
                vars.extend(self.export_vars(&arg_exports));
                f[3] = self.a.list(vars);
                Ok((self.a.tuple(f), Names::new()))
            }
            Some("c_catch") => {
                f[2] = self.export_sequence(f[2], &Names::new(), known)?;
                Ok((self.a.tuple(f), Names::new()))
            }
            Some("c_letrec") => {
                let mut defs = Vec::new();
                for def in self.a.list_items(f[2])? {
                    let mut df = self.a.tuple_fields(def)?.to_vec();
                    df[1] = self.export_expression(df[1], &Names::new(), known)?.0;
                    defs.push(self.a.tuple(df));
                }
                f[2] = self.a.list(defs);
                f[3] = self.export_sequence(f[3], &exports, known)?;
                Ok((self.a.tuple(f), exports))
            }
            _ => Ok((node, Names::new())),
        }
    }
    pub(super) fn export_liveness(&mut self, fun: TermId) -> Result<TermId> {
        self.export_expression(fun, &Names::new(), &Names::new())
            .map(|r| r.0)
    }
}
impl Lower {
    pub(super) fn source_export_patterns(&self, node: TermId) -> Result<HashSet<String>> {
        let mut out = HashSet::new();
        if self.a.tag(node) == Some("var") {
            let f = self.a.tuple_fields(node)?;
            if let Some(n) = self.a.as_atom(f[2]) {
                if n != "_" {
                    out.insert(n.into());
                }
            }
            return Ok(out);
        }
        match self.a.get(node) {
            ErlangTerm::List { items, .. } => {
                for c in items {
                    out.extend(self.source_export_patterns(*c)?);
                }
            }
            ErlangTerm::Tuple(f) => match self.a.tag(node) {
                Some("map_field_exact" | "map_field_assoc" | "bin_element") => {
                    out.extend(self.source_export_patterns(
                        f[2 + usize::from(self.a.tag(node) != Some("bin_element"))],
                    )?)
                }
                Some("atom" | "integer" | "float" | "char" | "string") => (),
                _ => {
                    for c in f.iter().skip(2) {
                        out.extend(self.source_export_patterns(*c)?);
                    }
                }
            },
            _ => (),
        }
        Ok(out)
    }
    pub(super) fn source_export_names(&self, node: TermId) -> Result<HashSet<String>> {
        let mut out = HashSet::new();
        if let ErlangTerm::List { items, .. } = self.a.get(node) {
            for child in items {
                out.extend(self.source_export_names(*child)?);
            }
            return Ok(out);
        }
        let Ok(f) = self.a.tuple_fields(node) else {
            return Ok(out);
        };
        match self.a.tag(node) {
            Some("match") => {
                out = self.source_export_patterns(f[2])?;
                out.extend(self.source_export_names(f[3])?);
            }
            Some("clause") => {
                out = self.source_export_patterns(f[2])?;
                out.extend(self.source_export_names(f[4])?);
            }
            Some("block") => out = self.source_export_names(f[2])?,
            Some("case" | "if" | "receive") => {
                let i = if self.a.tag(node) == Some("case") {
                    3
                } else {
                    2
                };
                let mut branches = self.a.list_items(f[i])?;
                if self.a.tag(node) == Some("receive") && f.len() == 5 {
                    branches.push(f[4]);
                }
                let mut first = true;
                for child in branches {
                    let names = self.source_export_names(child)?;
                    if first {
                        out = names;
                        first = false;
                    } else {
                        out = out.intersection(&names).cloned().collect();
                    }
                }
            }
            _ => (),
        }
        Ok(out)
    }
}
