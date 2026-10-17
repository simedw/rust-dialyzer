//! OTP 28.3 final Core pattern splitting, including dependent binary sizes.
use super::*;
struct Split {
    args: Vec<TermId>,
    pattern: TermId,
    nested: Option<Box<Split>>,
    wrap: SplitWrap,
}
enum SplitWrap {
    Identity,
    Size {
        var: TermId,
        arg: TermId,
        evars: Vec<TermId>,
    },
    MapSafe {
        key: TermId,
        arg: TermId,
    },
    MapUnsafe {
        key: TermId,
        success: TermId,
        arg: TermId,
        evars: Vec<TermId>,
    },
}
impl Lower {
    fn split_new_vars(&mut self, n: usize) -> Vec<TermId> {
        let empty = self.empty();
        let mut vars = (0..n).map(|_| self.fresh(empty)).collect::<Vec<_>>();
        vars.reverse();
        vars
    }
    fn split_values(&mut self, es: Vec<TermId>) -> TermId {
        if es.len() == 1 {
            return es[0];
        }
        let ann = es
            .first()
            .and_then(|n| self.a.tuple_fields(*n).ok())
            .map(|f| f[1])
            .unwrap_or_else(|| self.empty());
        let es = self.a.list(es);
        self.a.tagged("c_values", vec![ann, es])
    }
    fn split_goto(&mut self) -> TermId {
        let empty = self.empty();
        let name = self.a.atom(format!("label^{}", self.goto_counter));
        let arity = self.a.integer(0);
        let pair = self.a.tuple(vec![name, arity]);
        self.variable(empty, pair)
    }
    fn split_case_node(&mut self, ann: TermId, args: Vec<TermId>, clauses: Vec<TermId>) -> TermId {
        let arg = self.split_values(args);
        let clauses = self.a.list(clauses);
        self.a.tagged("c_case", vec![ann, arg, clauses])
    }
    fn split_fc(&mut self, args: &[TermId], ann: TermId) -> Result<TermId> {
        let generated = self.a.atom("compiler_generated");
        let mut as_ = vec![generated];
        as_.extend(self.a.list_items(ann)?);
        let ann = self.a.list(as_.clone());
        let vars = self.split_new_vars(args.len());
        let op = self.split_goto();
        let empty = self.empty();
        let apply = self.a.tagged("c_apply", vec![ann, op, empty]);
        let ignore = self.a.atom("dialyzer_ignore");
        as_.insert(0, ignore);
        let ann = self.a.list(as_);
        let pats = self.a.list(vars);
        let truth = self.atom_literal("true");
        Ok(self.a.tagged("c_clause", vec![ann, pats, truth, apply]))
    }
    pub(super) fn split_case(&mut self, node: TermId) -> Result<TermId> {
        let f = self.a.tuple_fields(node)?.to_vec();
        let args = if self.a.tag(f[2]) == Some("c_values") {
            self.a.list_items(self.a.tuple_fields(f[2])?[2])?
        } else {
            vec![f[2]]
        };
        let before = self.counter;
        let empty = self.empty();
        let mut vars = Vec::new();
        for arg in &args {
            vars.push(if matches!(self.a.tag(*arg), Some("c_var" | "c_literal")) {
                *arg
            } else {
                self.fresh(empty)
            });
        }
        let source = self.a.list_items(f[3])?;
        let mut found = None;
        for (i, clause) in source.iter().enumerate().rev() {
            let cf = self.a.tuple_fields(*clause)?.to_vec();
            let pats = self.a.list_items(cf[2])?;
            if let Some((pats, split)) = self.split_patterns(&pats)? {
                let pre = self.split_reconstruct(vars.clone(), pats, Some(split), *clause, f[1])?;
                found = Some((i, pre));
                break;
            }
        }
        let Some((i, pre)) = found else {
            self.counter = before;
            return Ok(node);
        };
        let mut pf = self.a.tuple_fields(pre)?.to_vec();
        let mut clauses = source[..i].to_vec();
        clauses.extend(self.a.list_items(pf[3])?);
        pf[3] = self.a.list(clauses);
        let pre = self.a.tuple(pf);
        let after = self.split_case_node(f[1], vars.clone(), source[i + 1..].to_vec());
        let compiler = self.a.atom("compiler_generated");
        let funann = self.a.list(vec![compiler]);
        let no_vars = self.empty();
        let fun = self.a.tagged("c_fun", vec![funann, no_vars, after]);
        let goto = self.split_goto();
        let def = self.a.tuple(vec![goto, fun]);
        let defs = self.a.list(vec![def]);
        let gotoann = self.a.atom("letrec_goto");
        let noinline = self.a.atom("no_inline");
        let ann = self.a.list(vec![gotoann, noinline]);
        let tree = self.a.tagged("c_letrec", vec![ann, defs, pre]);
        self.goto_counter += 1;
        let tree = self.lower_final(tree)?;
        let mut changed_vars = Vec::new();
        let mut changed_args = Vec::new();
        for (var, arg) in vars.into_iter().zip(args) {
            if var != arg {
                changed_vars.push(var);
                changed_args.push(arg);
            }
        }
        if changed_vars.is_empty() {
            Ok(tree)
        } else {
            let args = self.split_values(changed_args);
            let vars = self.a.list(changed_vars);
            let ann = self.empty();
            Ok(self.a.tagged("c_let", vec![ann, vars, args, tree]))
        }
    }
    fn split_reconstruct(
        &mut self,
        args: Vec<TermId>,
        pats: Vec<TermId>,
        nested: Option<Split>,
        clause: TermId,
        ann: TermId,
    ) -> Result<TermId> {
        let mut cf = self.a.tuple_fields(clause)?.to_vec();
        if let Some(split) = nested {
            let inner = self.split_reconstruct(
                split.args,
                vec![split.pattern],
                split.nested.map(|n| *n),
                clause,
                ann,
            )?;
            let fc = self.split_fc(&args, cf[1])?;
            let body = self.split_wrap(split.wrap, inner)?;
            cf[2] = self.a.list(pats);
            cf[3] = self.atom_literal("true");
            cf[4] = body;
            let success = self.a.tuple(cf);
            Ok(self.split_case_node(ann, args, vec![success, fc]))
        } else {
            let fc = self.split_fc(&pats, cf[1])?;
            cf[2] = self.a.list(pats);
            let success = self.a.tuple(cf);
            Ok(self.split_case_node(ann, args, vec![success, fc]))
        }
    }
    fn split_wrap(&mut self, wrap: SplitWrap, body: TermId) -> Result<TermId> {
        let empty = self.empty();
        Ok(match wrap {
            SplitWrap::Identity => body,
            SplitWrap::Size { var, arg, evars } => {
                let vars = self.a.list(vec![var]);
                let evars = self.a.list(evars);
                let failed = self.atom_literal("bad_size");
                let try_ = self
                    .a
                    .tagged("c_try", vec![empty, arg, vars, var, evars, failed]);
                self.a.tagged("c_let", vec![empty, vars, try_, body])
            }
            SplitWrap::MapSafe { key, arg } => {
                let vars = self.a.list(vec![key]);
                self.a.tagged("c_let", vec![empty, vars, arg, body])
            }
            SplitWrap::MapUnsafe {
                key,
                success,
                arg,
                evars,
            } => {
                let truth = self.atom_literal("true");
                let falsity = self.atom_literal("false");
                let returns = self.a.list(vec![truth, key]);
                let returns = self.a.tagged("c_values", vec![empty, returns]);
                let fails = self.a.list(vec![falsity, falsity]);
                let fails = self.a.tagged("c_values", vec![empty, fails]);
                let vars = self.a.list(vec![key]);
                let evars = self.a.list(evars);
                let try_ = self
                    .a
                    .tagged("c_try", vec![empty, arg, vars, returns, evars, fails]);
                let vars = self.a.list(vec![success, key]);
                self.a.tagged("c_let", vec![empty, vars, try_, body])
            }
        })
    }
    fn split_patterns(&mut self, patterns: &[TermId]) -> Result<Option<(Vec<TermId>, Split)>> {
        for i in (0..patterns.len()).rev() {
            if let Some((replacement, split)) = self.split_pattern(patterns[i])? {
                let mut out = patterns.to_vec();
                out[i] = replacement;
                return Ok(Some((out, split)));
            }
        }
        Ok(None)
    }
    fn split_pattern(&mut self, node: TermId) -> Result<Option<(TermId, Split)>> {
        let f = self.a.tuple_fields(node)?.to_vec();
        let empty = self.empty();
        match self.a.tag(node) {
            Some("c_binary") => self.split_binary(node),
            Some("c_map") => self.split_map(node),
            #[cfg(otp_domain = "29")]
            Some("c_record") => {
                let mut pairs = self.a.list_items(f[4])?;
                for (index, pair) in pairs.clone().into_iter().enumerate() {
                    let mut p = self.a.tuple_fields(pair)?.to_vec();
                    if let Some((pattern, nested)) = self.split_pattern(p[3])? {
                        let variable = self.fresh(empty);
                        p[3] = variable;
                        pairs[index] = self.a.tuple(p);
                        let mut record = f;
                        record[4] = self.a.list(pairs);
                        return Ok(Some((self.a.tuple(record), Split { args: vec![variable], pattern, nested: Some(Box::new(nested)), wrap: SplitWrap::Identity })));
                    }
                }
                Ok(None)
            }
            Some("c_var" | "c_literal") => Ok(None),
            Some("c_alias") => {
                if let Some((pat, nested)) = self.split_pattern(f[3])? {
                    let var = self.fresh(empty);
                    let mut alias = f;
                    alias[3] = var;
                    let alias = self.a.tuple(alias);
                    Ok(Some((
                        alias,
                        Split {
                            args: vec![var],
                            pattern: pat,
                            nested: Some(Box::new(nested)),
                            wrap: SplitWrap::Identity,
                        },
                    )))
                } else {
                    Ok(None)
                }
            }
            Some("c_tuple" | "c_cons") => {
                let tuple = self.a.tag(node) == Some("c_tuple");
                let es = if tuple {
                    self.a.list_items(f[2])?
                } else {
                    vec![f[2], f[3]]
                };
                for (i, e) in es.iter().enumerate() {
                    if let Some((pat, nested)) = self.split_pattern(*e)? {
                        let var = self.fresh(empty);
                        let mut es = es.clone();
                        es[i] = var;
                        let replacement = if tuple {
                            let es = self.a.list(es);
                            self.a.tagged("c_tuple", vec![empty, es])
                        } else {
                            self.a.tagged("c_cons", vec![empty, es[0], es[1]])
                        };
                        return Ok(Some((
                            replacement,
                            Split {
                                args: vec![var],
                                pattern: pat,
                                nested: Some(Box::new(nested)),
                                wrap: SplitWrap::Identity,
                            },
                        )));
                    }
                }
                Ok(None)
            }
            _ => Err(self.err("final pattern splitting tag")),
        }
    }
    fn split_map(&mut self, node: TermId) -> Result<Option<(TermId, Split)>> {
        let f = self.a.tuple_fields(node)?.to_vec();
        let pairs = self.a.list_items(f[3])?;
        let empty = self.empty();
        for (i, pair) in pairs.iter().enumerate() {
            let mut pf = self.a.tuple_fields(*pair)?.to_vec();
            if !matches!(self.a.tag(pf[3]), Some("c_var" | "c_literal")) {
                let mut vars = self.split_new_vars(2);
                let keyvar = vars.remove(0);
                let mapvar = vars.remove(0);
                let key = pf[3];
                pf[3] = keyvar;
                let newpair = self.a.tuple(pf);
                let mut after = f.clone();
                let mut rest = vec![newpair];
                rest.extend(&pairs[i + 1..]);
                after[3] = self.a.list(rest);
                let mut after = self.a.tuple(after);
                let safe = matches!(self.a.tag(key), Some("c_tuple" | "c_cons"));
                let (wrap, arg) = if safe {
                    (
                        SplitWrap::MapSafe {
                            key: keyvar,
                            arg: key,
                        },
                        mapvar,
                    )
                } else {
                    let mut vars = self.split_new_vars(4);
                    let success = vars.remove(0);
                    let truth = self.atom_literal("true");
                    let es = self.a.list(vec![truth, after]);
                    after = self.a.tagged("c_tuple", vec![empty, es]);
                    let es = self.a.list(vec![success, mapvar]);
                    let arg = self.a.tagged("c_tuple", vec![empty, es]);
                    (
                        SplitWrap::MapUnsafe {
                            key: keyvar,
                            success,
                            arg: key,
                            evars: vars,
                        },
                        arg,
                    )
                };
                let mut before = f;
                before[3] = self.a.list(pairs[..i].to_vec());
                let before = self.a.tuple(before);
                let before = self.a.tagged("c_alias", vec![empty, mapvar, before]);
                return Ok(Some((
                    before,
                    Split {
                        args: vec![arg],
                        pattern: after,
                        nested: None,
                        wrap,
                    },
                )));
            }
            if let Some((pat, nested)) = self.split_pattern(pf[4])? {
                let var = self.fresh(empty);
                pf[4] = var;
                let pair = self.a.tuple(pf);
                let mut ps = pairs.clone();
                ps[i] = pair;
                let mut map = f;
                map[3] = self.a.list(ps);
                let map = self.a.tuple(map);
                return Ok(Some((
                    map,
                    Split {
                        args: vec![var],
                        pattern: pat,
                        nested: Some(Box::new(nested)),
                        wrap: SplitWrap::Identity,
                    },
                )));
            }
        }
        Ok(None)
    }
    fn split_binary(&mut self, node: TermId) -> Result<Option<(TermId, Split)>> {
        let f = self.a.tuple_fields(node)?.to_vec();
        let segments = self.a.list_items(f[2])?;
        let mut bound: Vec<TermId> = Vec::new();
        let empty = self.empty();
        for (i, segment) in segments.iter().enumerate() {
            let mut sf = self.a.tuple_fields(*segment)?.to_vec();
            let ann = self.a.list_items(sf[1])?;
            if ann
                .iter()
                .any(|a| self.a.as_atom(*a) == Some("sequential_match"))
            {
                sf[1] = self.a.list(
                    ann.into_iter()
                        .filter(|a| self.a.as_atom(*a) != Some("sequential_match"))
                        .collect(),
                );
                let mut anns = self.a.list_items(f[1])?;
                let prior = anns
                    .iter()
                    .find(|a| self.a.tag(**a) == Some("binary_var"))
                    .map(|a| self.a.tuple_fields(*a).map(|f| f[1]))
                    .transpose()?;
                anns.retain(|a| self.a.tag(*a) != Some("binary_var"));
                let afterann = self.a.list(anns);
                let mut before = f.clone();
                before[2] = self.a.list(segments[..i].to_vec());
                let binvar = if let Some(var) = prior {
                    before[1] = afterann;
                    var
                } else {
                    self.fresh(empty)
                };
                let before = self.a.tuple(before);
                let before = if prior.is_none() {
                    self.a.tagged("c_alias", vec![empty, binvar, before])
                } else {
                    before
                };
                let mut after = f;
                after[1] = afterann;
                let mut rest = vec![self.a.tuple(sf)];
                rest.extend(&segments[i + 1..]);
                after[2] = self.a.list(rest);
                let after = self.a.tuple(after);
                return Ok(Some((
                    before,
                    Split {
                        args: vec![binvar],
                        pattern: after,
                        nested: None,
                        wrap: SplitWrap::Identity,
                    },
                )));
            }
            let size = sf[3];
            let dependent = if self.a.tag(size) == Some("c_var") {
                {
                    let name = self.a.tuple_fields(size)?[2];
                    bound
                        .iter()
                        .any(|prior| self.a.get(*prior) == self.a.get(name))
                }
            } else {
                self.a.tag(size) != Some("c_literal")
            };
            if dependent {
                let tail = self.fresh(empty);
                let unit = self.split_binary_unit(&segments[i..])?;
                let all = self.atom_literal("all");
                let unit = self.a.integer(unit);
                let unit = self.literal(empty, unit);
                let ty = self.atom_literal("binary");
                let unsigned = self.a.atom("unsigned");
                let big = self.a.atom("big");
                let flags = self.a.list(vec![unsigned, big]);
                let flags = self.literal(empty, flags);
                let tailseg = self
                    .a
                    .tagged("c_bitstr", vec![sf[1], tail, all, unit, ty, flags]);
                let wrap = if self.a.tag(size) == Some("c_var") {
                    SplitWrap::Identity
                } else {
                    let var = self.fresh(empty);
                    sf[3] = var;
                    let evars = self.split_new_vars(3);
                    SplitWrap::Size {
                        var,
                        arg: size,
                        evars,
                    }
                };
                let mut before = f.clone();
                let mut segs = segments[..i].to_vec();
                segs.push(tailseg);
                before[2] = self.a.list(segs);
                let mut after = f;
                let has_sequential = segments.iter().any(|s| {
                    self.a
                        .tuple_fields(*s)
                        .ok()
                        .and_then(|f| self.a.list_items(f[1]).ok())
                        .is_some_and(|a| {
                            a.iter()
                                .any(|a| self.a.as_atom(*a) == Some("sequential_match"))
                        })
                });
                let mut before = self.a.tuple(before);
                if has_sequential {
                    let mut ann = self.a.list_items(after[1])?;
                    if !ann.iter().any(|a| self.a.tag(*a) == Some("binary_var")) {
                        let var = self.fresh(empty);
                        before = self.a.tagged("c_alias", vec![empty, var, before]);
                        let key = self.a.atom("binary_var");
                        ann.insert(0, self.a.tuple(vec![key, var]));
                        after[1] = self.a.list(ann);
                    } else {
                        let mut bf = self.a.tuple_fields(before)?.to_vec();
                        ann.retain(|a| self.a.tag(*a) != Some("binary_var"));
                        bf[1] = self.a.list(ann);
                        before = self.a.tuple(bf);
                    }
                }
                let mut rest = vec![self.a.tuple(sf)];
                rest.extend(&segments[i + 1..]);
                after[2] = self.a.list(rest);
                let after = self.a.tuple(after);
                return Ok(Some((
                    before,
                    Split {
                        args: vec![tail],
                        pattern: after,
                        nested: None,
                        wrap,
                    },
                )));
            }
            if self.a.tag(sf[2]) == Some("c_var") {
                bound.push(self.a.tuple_fields(sf[2])?[2]);
            }
        }
        Ok(None)
    }
    fn split_binary_unit(&self, segments: &[TermId]) -> Result<i64> {
        let mut gcd = 0i64;
        for segment in segments {
            let f = self.a.tuple_fields(*segment)?;
            let ty = self.a.as_atom(self.a.tuple_fields(f[5])?[2]);
            let unit = self
                .a
                .as_integer(self.a.tuple_fields(f[4])?[2])
                .ok_or("lower: binary split unit")?;
            let bits = match ty {
                Some("utf8") => 8,
                Some("utf16") => 16,
                Some("utf32") => 32,
                _ => {
                    if self.a.tag(f[3]) == Some("c_literal") {
                        match self.a.as_integer(self.a.tuple_fields(f[3])?[2]) {
                            Some(0) => 1,
                            Some(n) => n
                                .checked_mul(unit)
                                .ok_or("lower: binary split size overflow")?,
                            None => unit,
                        }
                    } else {
                        unit
                    }
                }
            };
            let mut a = gcd;
            let mut b = bits;
            while b != 0 {
                let r = a % b;
                a = b;
                b = r;
            }
            gcd = a;
        }
        Ok(gcd)
    }
}
