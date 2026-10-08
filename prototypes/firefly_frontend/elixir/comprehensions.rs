// SPDX-License-Identifier: Apache-2.0
use super::*;
struct Generator {
    ann: Id,
    binary: bool,
    left: Id,
    right: Id,
    filters: Vec<(Option<Id>, Id)>,
}
impl Lower {
    pub(super) fn for_expr(&mut self, xs: &[Id], ann: Id, s: &mut Scope) -> Result<Id> {
        let (opts, steps) = xs.split_last().ok_or("invalid for")?;
        let initial_vars = s
            .versions
            .values()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>();
        let reducing = self.prop(*opts, "reduce")?;
        let uniq = self
            .prop(*opts, "uniq")?
            .is_some_and(|id| self.name(id).ok().as_deref() == Some("true"));
        let into = if let Some(raw) = reducing.or(self.prop(*opts, "into")?) {
            Some(self.expr(raw, ann, s, Context::Expr, 0)?)
        } else {
            None
        };
        let mut generators = vec![];
        let mut i = 0;
        while i < steps.len() {
            let (p, binary) = self
                .generator_parts(steps[i])?
                .ok_or("invalid comprehension generator")?;
            let generator_ann = self.ann(p[1])?;
            let parts = self.items(p[2])?;
            let right = self.expr(parts[1], generator_ann, s, Context::Expr, 0)?;
            let (left, guards) = if let T::Tuple(w) = self.get(parts[0]) {
                if w.len() == 3 && self.name(w[0]).ok().as_deref() == Some("when") {
                    let args = self.items(w[2])?;
                    (args[0], self.or_guards(args[1])?)
                } else {
                    (parts[0], vec![])
                }
            } else {
                (parts[0], vec![])
            };
            let previous_binding = std::mem::replace(
                &mut s.pattern_binding,
                PatternBinding::FreshParameter,
            );
            let left = self.expr(left, generator_ann, s, Context::Match, 0);
            s.pattern_binding = previous_binding;
            let left = left?;
            let extra_guards = std::mem::take(&mut s.extra_guards);
            i += 1;
            let mut filters = vec![];
            while i < steps.len() {
                if self.generator_parts(steps[i])?.is_some() {
                    break;
                }
                let boolean = self.boolean(steps[i]);
                let filter = self.expr(steps[i], generator_ann, s, Context::Expr, 0)?;
                let var = if boolean {
                    None
                } else {
                    let zero = self.int(0);
                    Some(self.fresh(zero, s))
                };
                filters.push((var, filter));
                i += 1
            }
            filters.reverse();
            let mut guard = None;
            for g in guards {
                let g = self.expr(g, generator_ann, s, Context::Guard, 0)?;
                guard = Some(if let Some(previous) = guard {
                    self.op(generator_ann, "orelse", vec![previous, g])
                } else {
                    g
                })
            }
            if let Some(guard) = guard {
                filters.push((None, guard))
            }
            filters.extend(extra_guards.into_iter().map(|guard| (None, guard)));
            generators.push(Generator {
                ann: generator_ann,
                binary,
                left,
                right,
                filters,
            })
        }
        let rawbody = self.prop(*opts, "do")?.ok_or("for lacks do")?;
        if reducing.is_some() {
            let clauses = self.arrows(rawbody, s)?;
            let clauses = self.list(clauses);
            let acc = self.fresh(ann, s);
            let body = self.ast("case", ann, vec![acc, clauses]);
            return self.reduce_generators(&generators, body, into.unwrap(), acc, s, &initial_vars);
        }
        let mut body = self.expr(rawbody, ann, s, Context::Expr, 0)?;
        if into.is_none() {
            let zero = self.int(0);
            let nil = self.atom_expr(zero, "nil");
            let xs = self.list(vec![body, nil]);
            body = self.ast("block", zero, vec![xs])
        }
        let into_kind = if let Some(into) = into {
            let p = self.tup(into)?;
            self.name(p[0])?
        } else {
            "unused".into()
        };
        let empty_map = into.is_some_and(|id| {
            self.tup(id).is_ok_and(|p| {
                p.len() == 3
                    && into_kind == "map"
                    && self.items(p[2]).is_ok_and(|fields| fields.is_empty())
            })
        });
        let empty_map_set = into.is_some_and(|id| self.empty_map_set(id));
        let empty_binary = into.is_some_and(|id| {
            self.tup(id).is_ok_and(|p| {
                p.len() == 3
                    && into_kind == "bin"
                    && self.items(p[2]).is_ok_and(|fields| fields.is_empty())
            })
        });
        if (!matches!(into_kind.as_str(), "nil" | "unused") && !empty_binary)
            && !empty_map
            && !empty_map_set
        {
            return self.collectable_into(
                &generators,
                body,
                into.unwrap(),
                ann,
                s,
                &initial_vars,
                uniq,
            );
        }
        let into_kind = if empty_map || empty_map_set {
            "nil"
        } else {
            &into_kind
        };
        if generators.iter().all(|g| g.binary) && !empty_map && !empty_map_set && !uniq {
            let mut clauses = vec![];
            for g in &generators {
                clauses.push(self.ast("b_generate", g.ann, vec![g.left, g.right]));
                for (var, filter) in g.filters.iter().rev() {
                    let yes = self.atom_expr(g.ann, "true");
                    let no = self.atom_expr(g.ann, "false");
                    clauses.push(self.join_filter(g.ann, *var, *filter, yes, no));
                }
            }
            let clauses = self.list(clauses);
            return Ok(self.ast(
                if into_kind == "bin" { "bc" } else { "lc" },
                ann,
                vec![body, clauses],
            ));
        }
        if !uniq
            && generators.len() == 1
            && into_kind == "nil"
            && !empty_map_set
            && generators[0].filters.is_empty()
            && self.name(self.tup(generators[0].left)?[0])? == "var"
        {
            let clause = self.make_clause(ann, vec![generators[0].left], vec![], vec![body]);
            let clauses = self.list(vec![clause]);
            let tag = self.atom("clauses");
            let inner = self.tuple(vec![tag, clauses]);
            let fun = self.ast("fun", ann, vec![inner]);
            let mapped = self.remote(ann, "Elixir.Enum", "map", vec![generators[0].right, fun]);
            return Ok(if empty_map {
                self.remote(ann, "maps", "from_list", vec![mapped])
            } else {
                mapped
            });
        }
        let inner_value = if into_kind == "bin" {
            Some(self.fresh(ann, s))
        } else {
            None
        };
        let acc = self.fresh(ann, s);
        let initial = self.ast("nil", ann, vec![]);
        let unique = self.unique_vars(uniq, ann, s);
        let (value_body, inner_acc) = unique
            .map(|(value, into_acc, _)| (value, into_acc))
            .unwrap_or((body, acc));
        let inner = if let Some(value) = inner_value {
            let generated = self.generated(ann);
            let bitstring = self.remote(ann, "erlang", "is_bitstring", vec![value]);
            let list = self.remote(ann, "erlang", "is_list", vec![inner_acc]);
            let guards = self.list(vec![bitstring, list]);
            let cons = self.ast("cons", generated, vec![inner_acc, value]);
            let first = self.make_clause(generated, vec![value], vec![guards], vec![cons]);
            let badarg = self.atom_expr(ann, "badarg");
            let reason = self.list(vec![badarg, value]);
            let reason = self.ast("tuple", ann, vec![reason]);
            let error = self.remote(ann, "erlang", "error", vec![reason]);
            let second = self.make_clause(generated, vec![value], vec![], vec![error]);
            let clauses = self.list(vec![first, second]);
            self.ast("case", ann, vec![value_body, clauses])
        } else if into_kind == "nil" {
            self.ast("cons", ann, vec![value_body, inner_acc])
        } else {
            value_body
        };
        let (inner, initial) = self.unique_body(unique, body, inner, initial, acc, ann);
        let reduce = self.reduce_generators(&generators, inner, initial, acc, s, &initial_vars)?;
        let reduce = self.unique_result(uniq, reduce, ann);
        Ok(if empty_map_set {
            self.remote(ann, "Elixir.MapSet", "new", vec![reduce])
        } else if empty_map {
            let reversed = self.remote(ann, "lists", "reverse", vec![reduce]);
            self.remote(ann, "maps", "from_list", vec![reversed])
        } else if into_kind == "bin" {
            self.remote(ann, "erlang", "list_to_bitstring", vec![reduce])
        } else if into_kind == "nil" {
            self.remote(ann, "lists", "reverse", vec![reduce])
        } else {
            reduce
        })
    }
    fn empty_map_set(&self, into: Id) -> bool {
        let Ok(p) = self.tup(into) else {
            return false;
        };
        if p.len() != 3 || self.name(p[0]).ok().as_deref() != Some("map") {
            return false;
        }
        let Ok(fields) = self.items(p[2]) else {
            return false;
        };
        if fields.len() != 2 {
            return false;
        }
        let Ok(structure) = self.tup(fields[0]) else {
            return false;
        };
        let Ok(map) = self.tup(fields[1]) else {
            return false;
        };
        let atom_is = |id, name: &str| {
            self.tup(id).is_ok_and(|v| {
                v.len() == 3
                    && self.name(v[0]).ok().as_deref() == Some("atom")
                    && self.name(v[2]).ok().as_deref() == Some(name)
            })
        };
        structure.len() == 4
            && map.len() == 4
            && self.name(structure[0]).ok().as_deref() == Some("map_field_assoc")
            && self.name(map[0]).ok().as_deref() == Some("map_field_assoc")
            && atom_is(structure[2], "__struct__")
            && atom_is(structure[3], "Elixir.MapSet")
            && atom_is(map[2], "map")
            && self.tup(map[3]).is_ok_and(|v| {
                v.len() == 3
                    && self.name(v[0]).ok().as_deref() == Some("map")
                    && self.items(v[2]).is_ok_and(|fields| fields.is_empty())
            })
    }
    fn collectable_into(
        &mut self,
        generators: &[Generator],
        body: Id,
        into: Id,
        ann: Id,
        s: &mut Scope,
        initial_vars: &std::collections::BTreeSet<String>,
        uniq: bool,
    ) -> Result<Id> {
        let fun = self.fresh(ann, s);
        let initial = self.fresh(ann, s);
        let kind = self.fresh(ann, s);
        let reason = self.fresh(ann, s);
        let stack = self.fresh(ann, s);
        let done = self.fresh(ann, s);
        let acc = self.fresh(ann, s);
        let unique = self.unique_vars(uniq, ann, s);
        let (value_body, inner_acc) = unique
            .map(|(value, into_acc, _)| (value, into_acc))
            .unwrap_or((body, acc));
        let cont = self.atom_expr(ann, "cont");
        let pair = self.list(vec![cont, value_body]);
        let pair = self.ast("tuple", ann, vec![pair]);
        let args = self.list(vec![inner_acc, pair]);
        let inner = self.ast("call", ann, vec![fun, args]);
        let (inner, reduce_initial) = self.unique_body(unique, body, inner, initial, acc, ann);
        let reduce =
            self.reduce_generators(generators, inner, reduce_initial, acc, s, initial_vars)?;
        let reduce = self.unique_result(uniq, reduce, ann);
        let pattern = self.list(vec![initial, fun]);
        let pattern = self.ast("tuple", ann, vec![pattern]);
        let setup = self.remote(ann, "Elixir.Collectable", "into", vec![into]);
        let setup = self.ast("match", ann, vec![pattern, setup]);
        let done_atom = self.atom_expr(ann, "done");
        let args = self.list(vec![done, done_atom]);
        let finish = self.ast("call", ann, vec![fun, args]);
        let success = self.make_clause(ann, vec![done], vec![], vec![finish]);
        let halt = self.atom_expr(ann, "halt");
        let args = self.list(vec![initial, halt]);
        let halt = self.ast("call", ann, vec![fun, args]);
        let raise = self.remote(ann, "erlang", "raise", vec![kind, reason, stack]);
        let pattern = self.list(vec![kind, reason, stack]);
        let pattern = self.ast("tuple", ann, vec![pattern]);
        let catch = self.make_clause(ann, vec![pattern], vec![], vec![halt, raise]);
        let body = self.list(vec![reduce]);
        let success = self.list(vec![success]);
        let catch = self.list(vec![catch]);
        let after = self.list(vec![]);
        let attempt = self.ast("try", ann, vec![body, success, catch, after]);
        let block = self.list(vec![setup, attempt]);
        Ok(self.ast("block", ann, vec![block]))
    }
    fn unique_vars(&mut self, uniq: bool, ann: Id, s: &mut Scope) -> Option<(Id, Id, Id)> {
        uniq.then(|| (self.fresh(ann, s), self.fresh(ann, s), self.fresh(ann, s)))
    }
    fn unique_body(
        &mut self,
        unique: Option<(Id, Id, Id)>,
        expr: Id,
        inner: Id,
        initial: Id,
        acc: Id,
        ann: Id,
    ) -> (Id, Id) {
        let Some((value, into_acc, unique_acc)) = unique else {
            return (inner, initial);
        };
        let empty = self.list(vec![]);
        let empty = self.ast("map", ann, vec![empty]);
        let tuple = self.list(vec![initial, empty]);
        let initial = self.ast("tuple", ann, vec![tuple]);
        let tuple = self.list(vec![into_acc, unique_acc]);
        let tuple = self.ast("tuple", ann, vec![tuple]);
        let unpack = self.ast("match", ann, vec![tuple, acc]);
        let evaluate = self.ast("match", ann, vec![value, expr]);
        let yes = self.atom_expr(ann, "true");
        let exact = self.ast("map_field_exact", ann, vec![value, yes]);
        let fields = self.list(vec![exact]);
        let seen = self.ast("map", ann, vec![fields]);
        let first = self.make_clause(ann, vec![seen], vec![], vec![tuple]);
        let assoc = self.ast("map_field_assoc", ann, vec![value, yes]);
        let fields = self.list(vec![assoc]);
        let put = self.ast("map", ann, vec![unique_acc, fields]);
        let result = self.list(vec![inner, put]);
        let result = self.ast("tuple", ann, vec![result]);
        let second = self.make_clause(ann, vec![empty], vec![], vec![result]);
        let clauses = self.list(vec![first, second]);
        let select = self.ast("case", ann, vec![unique_acc, clauses]);
        let body = self.list(vec![unpack, evaluate, select]);
        (self.ast("block", ann, vec![body]), initial)
    }
    fn unique_result(&mut self, uniq: bool, result: Id, ann: Id) -> Id {
        if !uniq {
            return result;
        }
        let one = self.int(1);
        let one = self.ast("integer", ann, vec![one]);
        self.remote(ann, "erlang", "element", vec![one, result])
    }
    fn generator_parts(&self, id: Id) -> Result<Option<(Vec<Id>, bool)>> {
        let Ok(p) = self.tup(id) else {
            return Ok(None);
        };
        if p.len() != 3 {
            return Ok(None);
        }
        if self.name(p[0]).ok().as_deref() == Some("<-") {
            return Ok(Some((p, false)));
        }
        if self.name(p[0]).ok().as_deref() == Some("<<>>") {
            let parts = self.items(p[2])?;
            if parts.len() == 1 {
                let inner = self.tup(parts[0])?;
                if inner.len() == 3 && self.name(inner[0]).ok().as_deref() == Some("<-") {
                    return Ok(Some((inner, true)));
                }
            }
        }
        Ok(None)
    }
    fn reduce_generators(
        &mut self,
        generators: &[Generator],
        body: Id,
        initial: Id,
        acc: Id,
        s: &Scope,
        initial_vars: &std::collections::BTreeSet<String>,
    ) -> Result<Id> {
        self.reduce_generators_inner(generators, body, initial, acc, &mut s.clone(), initial_vars)
    }
    fn reduce_generators_inner(
        &mut self,
        generators: &[Generator],
        body: Id,
        initial: Id,
        acc: Id,
        s: &mut Scope,
        initial_vars: &std::collections::BTreeSet<String>,
    ) -> Result<Id> {
        let Some((g, rest)) = generators.split_first() else {
            return Ok(body);
        };
        let generated = self.generated(g.ann);
        let tail = if g.binary {
            Some(self.fresh(g.ann, s))
        } else {
            None
        };
        let named = if g.binary {
            Some(self.fresh(g.ann, s))
        } else {
            None
        };
        let inner = self.reduce_generators_inner(rest, body, acc, acc, s, initial_vars)?;
        let mut joined = inner;
        for (var, filter) in &g.filters {
            joined = self.join_filter(generated, *var, *filter, joined, acc);
        }
        if let (Some(tail), Some(named)) = (tail, named) {
            let left = self.tup(g.left)?;
            if self.name(left[0])? != "bin" {
                return Err("binary generator requires binary pattern".into());
            }
            let elements = self.items(left[2])?;
            let size = self.atom("default");
            let bitstring = self.atom("bitstring");
            let types = self.list(vec![bitstring]);
            let tail_element = self.ast("bin_element", g.ann, vec![tail, size, types]);
            let tail_elements = self.list(vec![tail_element]);
            let tail_pattern = self.ast("bin", g.ann, vec![tail_elements]);
            let exhausted = self.make_clause(generated, vec![tail_pattern, acc], vec![], vec![acc]);
            let wild_name = self.atom("_");
            let wild = self.ast("var", g.ann, vec![wild_name]);
            let badarg = self.atom_expr(g.ann, "badarg");
            let pair = self.list(vec![badarg, tail]);
            let pair = self.ast("tuple", g.ann, vec![pair]);
            let error = self.remote(g.ann, "erlang", "error", vec![pair]);
            let invalid = self.make_clause(generated, vec![tail, wild], vec![], vec![error]);
            let mut clauses = vec![exhausted, invalid];
            let mut skipped = vec![];
            let mut valid = true;
            for element in &elements {
                let segment = self.tup(*element)?;
                if !self.valid_initial_size(segment[3], initial_vars)? {
                    valid = false;
                    break;
                }
                let expression = self.tup(segment[2])?;
                let count = if self.name(expression[0])? == "string" {
                    self.items(expression[2])?.len()
                } else {
                    1
                };
                for _ in 0..count {
                    let wildcard_name = self.atom("_");
                    let wildcard = self.ast("var", generated, vec![wildcard_name]);
                    skipped.push(self.ast(
                        "bin_element",
                        segment[1],
                        vec![wildcard, segment[3], segment[4]],
                    ));
                }
            }
            if valid {
                skipped.push(tail_element);
                let skipped = self.list(skipped);
                let skipped = self.ast("bin", g.ann, vec![skipped]);
                let args = self.list(vec![tail, acc]);
                let recur = self.ast("call", g.ann, vec![named, args]);
                clauses.insert(
                    0,
                    self.make_clause(generated, vec![skipped, acc], vec![], vec![recur]),
                );
            }
            let mut matched = elements;
            matched.push(tail_element);
            let matched = self.list(matched);
            let matched = self.ast("bin", g.ann, vec![matched]);
            let args = self.list(vec![tail, joined]);
            let recur = self.ast("call", g.ann, vec![named, args]);
            clauses.insert(
                0,
                self.make_clause(g.ann, vec![matched, acc], vec![], vec![recur]),
            );
            let clauses = self.list(clauses);
            let name = self.tup(named)?[2];
            let fun = self.ast("named_fun", g.ann, vec![name, clauses]);
            let args = self.list(vec![g.right, initial]);
            return Ok(self.ast("call", g.ann, vec![fun, args]));
        }
        let mut clauses = vec![self.make_clause(g.ann, vec![g.left, acc], vec![], vec![joined])];
        let left = self.tup(g.left)?;
        if self.name(left[0])? != "var" {
            let wild = self.atom("_");
            let wild = self.ast("var", g.ann, vec![wild]);
            clauses.push(self.make_clause(generated, vec![wild, acc], vec![], vec![acc]));
        }
        let clauses = self.list(clauses);
        let tag = self.atom("clauses");
        let fun = self.tuple(vec![tag, clauses]);
        let fun = self.ast("fun", g.ann, vec![fun]);
        Ok(self.remote(g.ann, "Elixir.Enum", "reduce", vec![g.right, initial, fun]))
    }
    fn valid_initial_size(
        &self,
        size: Id,
        initial_vars: &std::collections::BTreeSet<String>,
    ) -> Result<bool> {
        if self.compiler == CompilerVersion::V1_19 {
            return Ok(!self
                .tup(size)
                .is_ok_and(|p| self.name(p[0]).ok().as_deref() == Some("var")));
        }
        if matches!(self.get(size), T::Integer(_))
            || self.name(size).ok().as_deref() == Some("default")
        {
            return Ok(true);
        }
        let p = self.tup(size)?;
        Ok(match self.name(p[0])?.as_str() {
            "var" => initial_vars.contains(&self.name(p[2])?),
            "integer" => true,
            "op" if p.len() == 5 => {
                self.valid_initial_size(p[3], initial_vars)?
                    && self.valid_initial_size(p[4], initial_vars)?
            }
            _ => false,
        })
    }
    pub(super) fn join_filter(
        &mut self,
        ann: Id,
        var: Option<Id>,
        filter: Id,
        yes: Id,
        no: Id,
    ) -> Id {
        let (first, second) = if let Some(var) = var {
            let false_atom = self.atom_expr(ann, "false");
            let nil = self.atom_expr(ann, "nil");
            let left = self.op(ann, "==", vec![var, false_atom]);
            let right = self.op(ann, "==", vec![var, nil]);
            let guard = self.op(ann, "orelse", vec![left, right]);
            let guard = self.list(vec![guard]);
            let first = self.make_clause(ann, vec![var], vec![guard], vec![no]);
            let wild = self.atom("_");
            let wild = self.ast("var", ann, vec![wild]);
            let second = self.make_clause(ann, vec![wild], vec![], vec![yes]);
            (first, second)
        } else {
            let true_atom = self.atom_expr(ann, "true");
            let false_atom = self.atom_expr(ann, "false");
            let first = self.make_clause(ann, vec![true_atom], vec![], vec![yes]);
            let second = self.make_clause(ann, vec![false_atom], vec![], vec![no]);
            (first, second)
        };
        let clauses = self.list(vec![first, second]);
        self.ast("case", ann, vec![filter, clauses])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fallback_binary_pattern_only_uses_sizes_bound_before_comprehension() {
        let mut lower = Lower {
            compiler: CompilerVersion::V1_20,
            a: ErlangTerms {
                root: Id(0),
                terms: vec![T::Nil],
            },
        };
        let ann = lower.int(0);
        let name = lower.atom("external");
        let external = lower.ast("var", ann, vec![name]);
        let name = lower.atom("generated");
        let generated = lower.ast("var", ann, vec![name]);
        let initial = ["external".to_string()].into_iter().collect();
        assert!(lower.valid_initial_size(external, &initial).unwrap());
        assert!(!lower.valid_initial_size(generated, &initial).unwrap());
        let raw = lower.int(8);
        let size = lower.ast("integer", ann, vec![raw]);
        let op = lower.op(ann, "+", vec![external, size]);
        assert!(lower.valid_initial_size(op, &initial).unwrap());
        let op = lower.op(ann, "+", vec![generated, size]);
        assert!(!lower.valid_initial_size(op, &initial).unwrap());
    }
    #[test]
    fn collectable_calls_done_outside_reduce_and_halts_original_accumulator_on_failure() {
        let mut lower = Lower {
            compiler: CompilerVersion::V1_20,
            a: ErlangTerms {
                root: Id(0),
                terms: vec![T::Nil],
            },
        };
        let ann = lower.int(8);
        let into = lower.atom_expr(ann, "destination");
        let body = lower.int(42);
        let body = lower.ast("integer", ann, vec![body]);
        let result = lower
            .collectable_into(
                &[],
                body,
                into,
                ann,
                &mut Scope::default(),
                &Default::default(),
                false,
            )
            .unwrap();
        let block = lower.tup(result).unwrap();
        assert_eq!(lower.name(block[0]).unwrap(), "block");
        let expressions = lower.items(block[2]).unwrap();
        let setup = lower.tup(expressions[0]).unwrap();
        let pair = lower.tup(setup[2]).unwrap();
        let pair = lower.items(pair[2]).unwrap();
        let initial = pair[0];
        let fun = pair[1];
        let attempt = lower.tup(expressions[1]).unwrap();
        assert_eq!(lower.name(attempt[0]).unwrap(), "try");
        let successes = lower.items(attempt[3]).unwrap();
        let success = lower.tup(successes[0]).unwrap();
        let done = lower.items(success[2]).unwrap()[0];
        let finish = lower.items(success[4]).unwrap()[0];
        let finish = lower.tup(finish).unwrap();
        assert_eq!(finish[2], fun);
        assert_eq!(lower.items(finish[3]).unwrap()[0], done);
        let catches = lower.items(attempt[4]).unwrap();
        let catch = lower.tup(catches[0]).unwrap();
        let handlers = lower.items(catch[4]).unwrap();
        assert_eq!(handlers.len(), 2);
        let halt = lower.tup(handlers[0]).unwrap();
        assert_eq!(halt[2], fun);
        assert_eq!(lower.items(halt[3]).unwrap()[0], initial);
        let raise = lower.tup(handlers[1]).unwrap();
        let target = lower.tup(raise[2]).unwrap();
        let name = lower.tup(target[3]).unwrap();
        assert_eq!(lower.name(name[2]).unwrap(), "raise");
        assert!(lower.items(attempt[5]).unwrap().is_empty());
    }
}
