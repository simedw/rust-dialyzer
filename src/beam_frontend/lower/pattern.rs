//! Separate OTP pattern construction from upattern variable normalization.
use super::*;
impl Lower {
    pub(super) fn normalize_pattern(
        &mut self,
        node: TermId,
        bound: &mut HashSet<String>,
    ) -> Result<TermId> {
        let mut f = self.a.tuple_fields(node)?.to_vec();
        match self.a.tag(node) {
            Some("c_var") => {
                let name = self
                    .a
                    .as_atom(f[2])
                    .map(str::to_owned)
                    .or_else(|| self.a.as_integer(f[2]).map(|n| format!("#native:{n}")))
                    .ok_or("lower: pattern variable")?;
                if name != "_"
                    && (bound.contains(&name)
                        || self.case_patterns && self.known_vars.contains(&name))
                {
                    let empty = self.empty();
                    let variable = self.deferred_fresh(empty);
                    let erlang = self.atom_literal("erlang");
                    let equal = self.atom_literal("=:=");
                    let args = self.a.list(vec![variable, node]);
                    let test = self.a.tagged("c_call", vec![f[1], erlang, equal, args]);
                    self.pin_tests.push(test);
                    return Ok(variable);
                }
                if name != "_" {
                    bound.insert(name);
                }
                Ok(node)
            }
            Some("c_literal") => Ok(node),
            Some("c_tuple") => {
                let mut es = Vec::new();
                for e in self.a.list_items(f[2])? {
                    es.push(self.normalize_pattern(e, bound)?);
                }
                f[2] = self.a.list(es);
                Ok(self.a.tuple(f))
            }
            Some("c_cons" | "c_alias") => {
                f[2] = self.normalize_pattern(f[2], bound)?;
                f[3] = self.normalize_pattern(f[3], bound)?;
                Ok(self.a.tuple(f))
            }
            Some("c_map") => {
                let mut pairs = Vec::new();
                for pair in self.a.list_items(f[3])? {
                    let mut pf = self.a.tuple_fields(pair)?.to_vec();
                    pf[4] = self.normalize_pattern(pf[4], bound)?;
                    pairs.push(self.a.tuple(pf));
                }
                f[3] = self.a.list(pairs);
                Ok(self.a.tuple(f))
            }
            Some("c_binary") => {
                let mut segments = Vec::new();
                let mut locals = std::collections::BTreeMap::new();
                for segment in self.a.list_items(f[2])? {
                    let mut sf = self.a.tuple_fields(segment)?.to_vec();
                    let old = sf[2];
                    sf[2] = self.normalize_pattern(old, bound)?;
                    if self.a.tag(sf[3]) == Some("c_var") {
                        let size = self.a.tuple_fields(sf[3])?.to_vec();
                        if let Some(name) = self.a.as_atom(size[2]) {
                            if let Some(new) = locals.get(name) {
                                sf[3] = *new;
                            } else {
                                let empty = self.empty();
                                sf[3] = self.variable(empty, size[2]);
                            }
                        }
                    }
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
            _ => Err(self.err("Core pattern normalization")),
        }
    }
    pub(super) fn collect_raw_pattern_names(
        &self,
        node: TermId,
        out: &mut HashSet<String>,
    ) -> Result<()> {
        let f = self.a.tuple_fields(node)?;
        match self.a.tag(node) {
            Some("c_var") => {
                if let Some(name) = self.a.as_atom(f[2]) {
                    if name != "_" {
                        out.insert(name.to_owned());
                    }
                }
            }
            Some("c_tuple" | "c_binary") => {
                for child in self.a.list_items(f[2])? {
                    self.collect_raw_pattern_names(child, out)?;
                }
            }
            Some("c_cons" | "c_alias") => {
                self.collect_raw_pattern_names(f[2], out)?;
                self.collect_raw_pattern_names(f[3], out)?;
            }
            Some("c_map") => {
                for pair in self.a.list_items(f[3])? {
                    self.collect_raw_pattern_names(self.a.tuple_fields(pair)?[4], out)?;
                }
            }
            Some("c_bitstr") => self.collect_raw_pattern_names(f[2], out)?,
            _ => {}
        }
        Ok(())
    }
    pub(super) fn sort_pattern_map_pairs(&mut self, rows: Vec<TermId>) -> Result<Vec<TermId>> {
        let mut groups: Vec<(TermId, TermId)> = Vec::new();
        for pair in rows {
            let pf = self.a.tuple_fields(pair)?.to_vec();
            let key = if matches!(self.a.tag(pf[3]), Some("c_var" | "c_literal")) {
                let mut k = self.a.tuple_fields(pf[3])?.to_vec();
                k[1] = self.empty();
                let atomic = self.a.atom("atomic");
                let k = self.a.tuple(k);
                self.a.tuple(vec![atomic, k])
            } else {
                let expr = self.a.atom("expr");
                let n = self.a.integer(groups.len());
                self.a.tuple(vec![expr, n])
            };
            let mut existing = None;
            for (i, (old, _)) in groups.iter().enumerate() {
                let arena = self.a.snapshot(key);
                let order = crate::term_order::TermOrderArena::reachable(&arena, &[key, *old])?;
                if order.compare_map_keys(key, &order, *old)? == std::cmp::Ordering::Equal {
                    existing = Some(i);
                    break;
                }
            }
            if let Some(i) = existing {
                let previous = self.a.tuple_fields(groups[i].1)?.to_vec();
                let mut new = pf;
                new[4] = self.alias_patterns(previous[4], new[4])?;
                groups[i].1 = self.a.tuple(new);
            } else {
                groups.push((key, pair));
            }
        }
        let keys = groups.iter().map(|g| g.0).collect::<Vec<_>>();
        if let Some(root) = keys.first() {
            let arena = self.a.snapshot(*root);
            let order = crate::term_order::TermOrderArena::reachable(&arena, &keys)?;
            // Reachable term-order validation makes comparisons infallible for these same-arena keys.
            groups.sort_by(|a, b| {
                order
                    .compare(a.0, &order, b.0)
                    .expect("validated map pattern sort keys")
            });
        }
        Ok(groups.into_iter().map(|(_, pair)| pair).collect())
    }
    fn literal_pattern_data(&mut self, node: TermId) -> Result<TermId> {
        let f = self.a.tuple_fields(node)?.to_vec();
        if self.a.tag(node) != Some("c_literal") {
            return Ok(node);
        }
        let empty = self.empty();
        Ok(match self.a.get(f[2]).clone() {
            ErlangTerm::Tuple(es) => {
                let es = es.into_iter().map(|e| self.literal(empty, e)).collect();
                let es = self.a.list(es);
                self.a.tagged("c_tuple", vec![empty, es])
            }
            ErlangTerm::List { mut items, tail } if !items.is_empty() => {
                let head = items.remove(0);
                let tail = if items.is_empty() {
                    tail
                } else {
                    self.a.push(ErlangTerm::List { items, tail })
                };
                let head = self.literal(empty, head);
                let tail = self.literal(empty, tail);
                self.a.tagged("c_cons", vec![empty, head, tail])
            }
            _ => node,
        })
    }
    pub(super) fn alias_patterns(&mut self, left: TermId, right: TermId) -> Result<TermId> {
        let lf = self.a.tuple_fields(left)?.to_vec();
        let rf = self.a.tuple_fields(right)?.to_vec();
        let empty = self.empty();
        if self.a.tag(left) == Some("c_literal")
            && matches!(self.a.tag(right), Some("c_tuple" | "c_cons"))
        {
            let data = self.literal_pattern_data(left)?;
            if data != left {
                return self.alias_patterns(data, right);
            }
        }
        if self.a.tag(right) == Some("c_literal")
            && matches!(self.a.tag(left), Some("c_tuple" | "c_cons"))
        {
            let data = self.literal_pattern_data(right)?;
            if data != right {
                return self.alias_patterns(left, data);
            }
        }
        match (self.a.tag(left), self.a.tag(right)) {
            (Some("c_var"), Some("c_var")) if self.a.get(lf[2]) == self.a.get(rf[2]) => Ok(left),
            (Some("c_var"), Some("c_alias")) => {
                let aliasvar = self.a.tuple_fields(rf[2])?;
                if self.a.get(lf[2]) == self.a.get(aliasvar[2]) {
                    return Ok(right);
                }
                let mut alias = rf;
                alias[3] = self.alias_patterns(left, alias[3])?;
                Ok(self.a.tuple(alias))
            }
            (Some("c_var"), _) => Ok(self.a.tagged("c_alias", vec![empty, left, right])),
            (Some("c_alias"), Some("c_var"))
                if self.a.get(self.a.tuple_fields(lf[2])?[2]) == self.a.get(rf[2]) =>
            {
                Ok(left)
            }
            (Some("c_alias"), Some("c_alias")) => {
                let pat = self.alias_patterns(lf[3], rf[3])?;
                if self.a.get(self.a.tuple_fields(lf[2])?[2])
                    == self.a.get(self.a.tuple_fields(rf[2])?[2])
                {
                    Ok(self.a.tagged("c_alias", vec![empty, lf[2], pat]))
                } else {
                    let pat = self.alias_patterns(rf[2], pat)?;
                    self.alias_patterns(lf[2], pat)
                }
            }
            (Some("c_alias"), _) => {
                let pat = self.alias_patterns(lf[3], right)?;
                Ok(self.a.tagged("c_alias", vec![empty, lf[2], pat]))
            }
            (_, Some("c_var")) => Ok(self.a.tagged("c_alias", vec![empty, right, left])),
            (_, Some("c_alias")) => {
                let mut alias = rf;
                alias[3] = self.alias_patterns(left, alias[3])?;
                Ok(self.a.tuple(alias))
            }
            (Some("c_binary"), Some("c_binary")) => {
                let mut left_segments = self.a.list_items(lf[2])?;
                let mut right_segments = self.a.list_items(rf[2])?;
                if left_segments.is_empty() && right_segments.is_empty() {
                    return Ok(left);
                }
                if !left_segments.is_empty() && !right_segments.is_empty() {
                    let mut first = self.a.tuple_fields(right_segments[0])?.to_vec();
                    let marker = self.a.atom("sequential_match");
                    let mut ann = vec![marker];
                    ann.extend(self.a.list_items(first[1])?);
                    first[1] = self.a.list(ann);
                    right_segments[0] = self.a.tuple(first);
                    left_segments.extend(right_segments);
                } else if !left_segments.is_empty() {
                    let mut first = self.a.tuple_fields(left_segments[0])?.to_vec();
                    let marker = self.a.atom("sequential_match");
                    let mut ann = vec![marker];
                    ann.extend(self.a.list_items(first[1])?);
                    first[1] = self.a.list(ann);
                    left_segments[0] = self.a.tuple(first);
                } else {
                    return Err(self.err("impossible aliased binary pattern"));
                }
                let mut f = lf;
                f[2] = self.a.list(left_segments);
                Ok(self.a.tuple(f))
            }
            (Some("c_map"), Some("c_map")) => {
                let mut pairs = self.a.list_items(lf[3])?;
                pairs.extend(self.a.list_items(rf[3])?);
                let pairs = self.sort_pattern_map_pairs(pairs)?;
                let mut f = lf;
                f[3] = self.a.list(pairs);
                Ok(self.a.tuple(f))
            }
            (Some("c_tuple"), Some("c_tuple")) => {
                let ls = self.a.list_items(lf[2])?;
                let rs = self.a.list_items(rf[2])?;
                if ls.len() != rs.len() {
                    return Err(self.err("impossible aliased tuple pattern"));
                }
                let mut es = Vec::new();
                for (l, r) in ls.into_iter().zip(rs) {
                    es.push(self.alias_patterns(l, r)?);
                }
                if es.iter().all(|v| self.a.tag(*v) == Some("c_literal")) {
                    let values = es
                        .iter()
                        .map(|e| self.a.tuple_fields(*e).map(|f| f[2]))
                        .collect::<Result<Vec<_>>>()?;
                    let value = self.a.tuple(values);
                    Ok(self.literal(empty, value))
                } else {
                    let es = self.a.list(es);
                    Ok(self.a.tagged("c_tuple", vec![empty, es]))
                }
            }
            (Some("c_cons"), Some("c_cons")) => {
                let h = self.alias_patterns(lf[2], rf[2])?;
                let t = self.alias_patterns(lf[3], rf[3])?;
                if self.a.tag(h) == Some("c_literal") && self.a.tag(t) == Some("c_literal") {
                    let h = self.a.tuple_fields(h)?[2];
                    let t = self.a.tuple_fields(t)?[2];
                    let value = self.a.push(ErlangTerm::List {
                        items: vec![h],
                        tail: t,
                    });
                    Ok(self.literal(empty, value))
                } else {
                    Ok(self.a.tagged("c_cons", vec![empty, h, t]))
                }
            }
            (Some("c_literal"), Some("c_literal")) => {
                if self.literal_key(lf[2], 0)? != self.literal_key(rf[2], 0)? {
                    return Err(self.err("impossible aliased literal pattern"));
                }
                Ok(self.literal(empty, lf[2]))
            }
            _ => Err(self.err("structural pattern alias requires data compatibility")),
        }
    }
}
