//! Native port of OTP 28.3 dialyzer_clean_core.erl (Apache-2.0,
//! Copyright Ericsson AB and the original Dialyzer authors).
//! Cleanup is semantic: ignored/raising clauses disappear and goto letrecs split.
use super::term::Arena;
use crate::core::{ErlangTerms, TermId};

pub fn clean(core: &ErlangTerms) -> Result<ErlangTerms, String> {
    let mut cleaner = Cleaner {
        arena: Arena::from_terms(core),
        depth: 0,
    };
    let root = cleaner.node(core.root)?;
    Ok(cleaner.arena.finish(root))
}
struct Cleaner {
    arena: Arena,
    depth: usize,
}
impl Cleaner {
    fn ignored(&self, node: TermId) -> Result<bool, String> {
        let fields = self.arena.tuple_fields(node)?;
        let ann = *fields.get(1).ok_or("clean: Core node lacks annotations")?;
        Ok(self
            .arena
            .list_items(ann)?
            .iter()
            .any(|&id| self.arena.as_atom(id) == Some("dialyzer_ignore")))
    }
    fn literal(&mut self, atom: &str) -> TermId {
        let ann = self.arena.nil();
        let value = self.arena.atom(atom);
        self.arena.tagged("c_literal", vec![ann, value])
    }
    fn list(&mut self, list: TermId) -> Result<TermId, String> {
        let mut out = Vec::new();
        for item in self.arena.list_items(list)? {
            out.push(self.node(item)?);
        }
        Ok(self.arena.list(out))
    }
    fn defs(&mut self, list: TermId) -> Result<TermId, String> {
        let mut out = Vec::new();
        for pair in self.arena.list_items(list)? {
            let fields = self.arena.tuple_fields(pair)?.to_vec();
            if fields.len() != 2 {
                return Err("clean: malformed definition pair".into());
            }
            let fun = self.node(fields[1])?;
            out.push(self.arena.tuple(vec![fields[0], fun]));
        }
        Ok(self.arena.list(out))
    }
    fn clauses(&mut self, list: TermId) -> Result<TermId, String> {
        let mut out = Vec::new();
        for clause in self.arena.list_items(list)? {
            let mut f = self.arena.tuple_fields(clause)?.to_vec();
            if self.arena.tag(clause) != Some("c_clause") || f.len() != 5 {
                return Err("clean: malformed clause".into());
            }
            let raising = if self.arena.tag(f[4]) == Some("c_primop") {
                let body = self.arena.tuple_fields(f[4])?;
                let name = self.arena.tuple_fields(body[2])?;
                matches!(self.arena.as_atom(name[2]), Some("match_fail" | "raise"))
            } else {
                false
            };
            if self.ignored(clause)? || raising {
                continue;
            }
            f[3] = self.node(f[3])?;
            f[4] = self.node(f[4])?;
            out.push(self.arena.tuple(f));
        }
        Ok(self.arena.list(out))
    }
    fn node(&mut self, node: TermId) -> Result<TermId, String> {
        if self.depth >= 4096 {
            return Err("clean: Core nesting exceeds 4096".into());
        }
        self.depth += 1;
        let result = self.node_inner(node);
        self.depth -= 1;
        result
    }
    fn node_inner(&mut self, node: TermId) -> Result<TermId, String> {
        let tag = self
            .arena
            .tag(node)
            .ok_or("clean: expected Core record")?
            .to_owned();
        let mut f = self.arena.tuple_fields(node)?.to_vec();
        let (nodes, lists): (&[usize], &[usize]) = match tag.as_str() {
            "c_literal" | "c_var" => return Ok(node),
            "c_apply" => (&[2], &[3]),
            "c_binary" => (&[], &[2]),
            "c_bitstr" => (&[2, 3], &[]),
            "c_call" => (&[2, 3], &[4]),
            "c_catch" => (&[2], &[]),
            "c_cons" => (&[2, 3], &[]),
            "c_fun" => (&[3], &[]),
            "c_let" => (&[3, 4], &[]),
            "c_primop" => (&[], &[3]),
            "c_seq" => (&[2, 3], &[]),
            "c_try" => (&[2, 4, 6], &[]),
            "c_tuple" | "c_values" => (&[], &[2]),
            "c_map_pair" => (&[3, 4], &[]),
            "c_case" => {
                f[2] = self.node(f[2])?;
                f[3] = self.clauses(f[3])?;
                return Ok(self.arena.tuple(f));
            }
            "c_map" => {
                f[2] = self.node(f[2])?;
                f[3] = self.list(f[3])?;
                return Ok(self.arena.tuple(f));
            }
            "c_module" => {
                f[5] = self.defs(f[5])?;
                return Ok(self.arena.tuple(f));
            }
            "c_letrec" => return self.letrec(f),
            other => return Err(format!("clean: unsupported Core record {other}")),
        };
        for &index in nodes {
            let child = *f
                .get(index)
                .ok_or_else(|| format!("clean: malformed {tag}"))?;
            f[index] = self.node(child)?;
        }
        for &index in lists {
            let child = *f
                .get(index)
                .ok_or_else(|| format!("clean: malformed {tag}"))?;
            f[index] = self.list(child)?;
        }
        Ok(self.arena.tuple(f))
    }
    fn letrec(&mut self, mut f: Vec<TermId>) -> Result<TermId, String> {
        if !self
            .arena
            .list_items(f[1])?
            .iter()
            .any(|&id| self.arena.as_atom(id) == Some("letrec_goto"))
        {
            f[2] = self.defs(f[2])?;
            f[3] = self.node(f[3])?;
            return Ok(self.arena.tuple(f));
        }
        let defs = self.arena.list_items(f[2])?;
        if defs.len() != 1 {
            return Err("clean: goto letrec must have one definition".into());
        }
        let pair = self.arena.tuple_fields(defs[0])?.to_vec();
        let fun = self.arena.tuple_fields(pair[1])?.to_vec();
        if self.arena.tag(pair[1]) != Some("c_fun") || fun.len() != 4 {
            return Err("clean: goto target must be fun".into());
        }
        let fun_body = self.node(fun[3])?;
        let body = self.node(f[3])?;
        if self.ignored(body)? {
            return Ok(fun_body);
        }
        let ann = self.arena.nil();
        let name = self.literal("dialyzer_unknown");
        let args = self.arena.nil();
        let unknown = self.arena.tagged("c_primop", vec![ann, name, args]);
        let a = self.literal("a");
        let b = self.literal("b");
        let mut b_elements = vec![b];
        b_elements.extend(self.arena.list_items(fun[2])?);
        let ann = self.arena.nil();
        let pattern_b = if b_elements.len() == 1 {
            let b_atom = self.arena.tuple_fields(b)?[2];
            let tuple = self.arena.tuple(vec![b_atom]);
            self.arena.tagged("c_literal", vec![ann, tuple])
        } else {
            let b_elements = self.arena.list(b_elements);
            self.arena.tagged("c_tuple", vec![ann, b_elements])
        };
        let mut clauses = Vec::new();
        for (pattern, result) in [(a, body), (pattern_b, fun_body)] {
            let ann = self.arena.nil();
            let pats = self.arena.list(vec![pattern]);
            let guard = self.literal("true");
            clauses.push(
                self.arena
                    .tagged("c_clause", vec![ann, pats, guard, result]),
            );
        }
        let ann = self.arena.nil();
        let clauses = self.arena.list(clauses);
        Ok(self.arena.tagged("c_case", vec![ann, unknown, clauses]))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cleaner_drops_raise_and_preserves_literal_annotations() {
        let mut a = Arena::default();
        let ann = a.list(vec![]);
        let value = a.atom("ok");
        let literal = a.tagged("c_literal", vec![ann, value]);
        let raise_name = a.atom("raise");
        let name = a.tagged("c_literal", vec![ann, raise_name]);
        let args = a.nil();
        let raising = a.tagged("c_primop", vec![ann, name, args]);
        let pats = a.nil();
        let good = a.tagged("c_clause", vec![ann, pats, literal, literal]);
        let bad = a.tagged("c_clause", vec![ann, pats, literal, raising]);
        let clauses = a.list(vec![good, bad]);
        let root = a.tagged("c_case", vec![ann, literal, clauses]);
        let result = clean(&a.finish(root)).unwrap();
        let a = Arena::from_terms(&result);
        let f = a.tuple_fields(result.root).unwrap();
        assert_eq!(a.list_items(f[3]).unwrap().len(), 1);
        assert_eq!(a.tuple_fields(f[2]).unwrap()[1], ann);
    }
}
