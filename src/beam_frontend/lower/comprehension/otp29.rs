//! OTP29 multiple comprehension heads and maps:from_keys selection, following
//! v3_core lc_tq2/mc_pairs/mc_tuples. State is local to one nested comprehension.
use super::*;
pub(in super::super) struct MapHead {
    source: TermId,
    bound: HashSet<String>,
    result: Option<Option<TermId>>,
}
impl Lower {
    pub(in super::super) fn comp29_begin_map(&mut self, source: TermId, qualifiers: &[Qualifier]) -> Result<()> {
        let mut bound = HashSet::new();
        for qualifier in qualifiers {
            match qualifier {
                Qualifier::Gen(generator) => self.collect_raw_pattern_names(generator.accept, &mut bound)?,
                Qualifier::Zip { gens, .. } => for generator in gens { self.collect_raw_pattern_names(generator.accept, &mut bound)?; },
                Qualifier::Filter { arg, .. } => for instruction in &arg.pre {
                    match instruction {
                        Pre::Set(variable, _) => self.collect_raw_pattern_names(*variable, &mut bound)?,
                        Pre::Match { pattern, .. } => self.collect_raw_pattern_names(*pattern, &mut bound)?,
                        Pre::Effect(_) => (),
                    }
                },
                Qualifier::Guard { .. } => (),
            }
        }
        self.map_comprehensions.push(MapHead { source, bound, result: None });
        Ok(())
    }
    pub(in super::super) fn comp29_end_map(&mut self) -> Result<(&'static str, Vec<TermId>)> {
        let head = self.map_comprehensions.pop().ok_or("missing map comprehension context")?;
        Ok(match head.result.ok_or("map comprehension did not compile a head")? {
            Some(value) => ("from_keys", vec![value]),
            None => ("from_list", vec![]),
        })
    }
    fn comp29_head_list(&self, source: TermId) -> Result<Vec<TermId>> {
        if matches!(self.a.get(source), ErlangTerm::List { .. } | ErlangTerm::Nil) { Ok(self.a.list_items(source)?) }
        else { Ok(vec![source]) }
    }
    fn comp29_safe(&self, value: TermId) -> Result<bool> {
        Ok(match self.a.tag(value) {
            Some("c_literal" | "c_cons" | "c_tuple") => true,
            Some("c_var") => !matches!(self.a.get(self.a.tuple_fields(value)?[2]), ErlangTerm::Tuple(_)),
            _ => false,
        })
    }
    fn comp29_uses_bound(&self, value: TermId, bound: &HashSet<String>) -> Result<bool> {
        let f = self.a.tuple_fields(value)?;
        Ok(match self.a.tag(value) {
            Some("c_var") => self.a.as_atom(f[2]).is_some_and(|name| bound.contains(name)),
            Some("c_cons") => self.comp29_uses_bound(f[2], bound)? || self.comp29_uses_bound(f[3], bound)?,
            Some("c_tuple") => {
                let mut used = false;
                for value in self.a.list_items(f[2])? { used |= self.comp29_uses_bound(value, bound)?; }
                used
            }
            _ => false,
        })
    }
    fn comp29_same(&self, left: TermId, right: TermId) -> Result<bool> {
        let a = self.a.tuple_fields(left)?;
        let b = self.a.tuple_fields(right)?;
        Ok(match (self.a.tag(left), self.a.tag(right)) {
            (Some("c_literal"), Some("c_literal")) => self.literal_key(a[2], 0)? == self.literal_key(b[2], 0)?,
            (Some("c_var"), Some("c_var")) => self.a.get(a[2]) == self.a.get(b[2]),
            (Some("c_cons"), Some("c_cons")) => self.comp29_same(a[2], b[2])? && self.comp29_same(a[3], b[3])?,
            (Some("c_tuple"), Some("c_tuple")) => {
                let a = self.a.list_items(a[2])?;
                let b = self.a.list_items(b[2])?;
                let mut same = a.len() == b.len();
                for (a, b) in a.into_iter().zip(b) { same &= self.comp29_same(a, b)?; }
                same
            }
            _ => false,
        })
    }
    pub(in super::super) fn comp29_values(&mut self, source: TermId) -> Result<(Vec<TermId>, Vec<Pre>)> {
        let Some(index) = self.map_comprehensions.len().checked_sub(1).filter(|&i| self.map_comprehensions[i].source == source) else {
            let heads = self.comp29_head_list(source)?;
            return self.safe_list(heads);
        };
        let bound = self.map_comprehensions[index].bound.clone();
        let pairs = self.comp29_head_list(source)?;
        let mut keys = Vec::new();
        let mut pre = Vec::new();
        let mut previous = None;
        for (position, &pair) in pairs.iter().enumerate() {
            let f = self.a.tuple_fields(pair)?.to_vec();
            if self.a.tag(pair) != Some("map_field_assoc") || f.len() != 4 { return Err(self.err("map comprehension head")); }
            let value = self.expr(f[3])?;
            let key = self.source_safe(f[2])?;
            let same = if let Some(previous) = previous { self.comp29_same(value.value, previous)? } else { true };
            if self.comp29_safe(value.value)? && value.pre.is_empty() && !self.comp29_uses_bound(value.value, &bound)? && same {
                keys.insert(0, key.value);
                let mut next = key.pre;
                next.extend(pre);
                pre = next;
                previous = Some(value.value);
                continue;
            }
            let forced = self.force_safe_expression(Expr { value: value.value, pre: vec![] })?;
            let ann = self.annotation(f[1])?;
            let mut tuples = vec![self.comp_tuple(ann, vec![key.value, forced.value])];
            if let Some(previous) = previous {
                for key in keys { tuples.push(self.comp_tuple(ann, vec![key, previous])); }
            }
            let mut prefix = value.pre;
            prefix.extend(forced.pre);
            prefix.extend(key.pre);
            prefix.extend(pre);
            for &pair in &pairs[position + 1..] {
                let f = self.a.tuple_fields(pair)?.to_vec();
                if self.a.tag(pair) != Some("map_field_assoc") || f.len() != 4 { return Err(self.err("map comprehension head")); }
                let es = self.a.list(vec![f[2], f[3]]);
                let tuple = self.a.tagged("tuple", vec![f[1], es]);
                let tuple = self.source_safe(tuple)?;
                tuples.insert(0, tuple.value);
                prefix.extend(tuple.pre);
            }
            tuples.reverse();
            self.map_comprehensions[index].result = Some(None);
            return Ok((tuples, prefix));
        }
        keys.reverse();
        self.map_comprehensions[index].result = Some(Some(previous.ok_or("empty map comprehension head")?));
        Ok((keys, pre))
    }
}
