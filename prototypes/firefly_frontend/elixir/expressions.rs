// SPDX-License-Identifier: Apache-2.0
use super::*;
impl Lower {
    pub(super) fn args(&mut self, id: Id, ann: Id, s: &mut Scope, c: Context) -> Result<Vec<Id>> {
        let mut out = vec![];
        for x in self.items(id)? {
            out.push(self.expr(x, ann, s, c, 0)?)
        }
        Ok(out)
    }
    pub(super) fn expr(
        &mut self,
        id: Id,
        parent_ann: Id,
        s: &mut Scope,
        c: Context,
        depth: usize,
    ) -> Result<Id> {
        if depth > 256 || self.a.terms.len() > 2_000_000 {
            return Err("Elixir translation resource limit".into());
        }
        let raw = self.get(id).clone();
        if let T::List { items, tail } = raw {
            let mut t = self.expr(tail, parent_ann, s, c, depth + 1)?;
            for x in items.into_iter().rev() {
                if let T::Tuple(v) = self.get(x).clone() {
                    if v.len() == 3 && self.name(v[0]).ok().as_deref() == Some("|") {
                        let pair = self.items(v[2])?;
                        if pair.len() != 2 {
                            return Err("invalid list tail".into());
                        }
                        let h = self.expr(pair[0], parent_ann, s, c, depth + 1)?;
                        let end = self.expr(pair[1], parent_ann, s, c, depth + 1)?;
                        t = self.ast("cons", parent_ann, vec![h, end]);
                        continue;
                    }
                }
                let h = self.expr(x, parent_ann, s, c, depth + 1)?;
                t = self.ast("cons", parent_ann, vec![h, t])
            }
            return Ok(t);
        }
        let T::Tuple(v) = raw else {
            return self.lit(id, parent_ann);
        };
        if v.len() == 2 {
            let x = self.expr(v[0], parent_ann, s, c, depth + 1)?;
            let y = self.expr(v[1], parent_ann, s, c, depth + 1)?;
            let l = self.list(vec![x, y]);
            return Ok(self.ast("tuple", parent_ann, vec![l]));
        }
        if v.len() != 3 {
            return self.lit(id, parent_ann);
        }
        let ann = self.ann(v[1])?;
        if let T::Tuple(dot) = self.get(v[0]).clone() {
            if dot.len() != 3 || self.name(dot[0])? != "." {
                return Err("unsupported expression callee".into());
            }
            let parts = self.items(dot[2])?;
            if parts.len() == 2 {
                if let (Ok(module), Ok(name)) = (self.name(parts[0]), self.name(parts[1])) {
                    if let Some(result) = self.try_reordered_remote(ann, &module, &name, v[2], s, c)? {
                        return Ok(result);
                    }
                }
            }
            if parts.len() == 2
                && self.name(parts[0]).ok().as_deref() == Some("Elixir.MapSet")
                && self.name(parts[1]).ok().as_deref() == Some("new")
            {
                if let Some(value) = self.mapset_constant(v[2])? {
                    let generated = self.generated(ann);
                    return self.lit(value, generated);
                }
            }
            let args = self.args(v[2], ann, s, c)?;
            let target = if parts.len() == 1 {
                self.expr(parts[0], ann, s, c, depth + 1)?
            } else if parts.len() == 2 {
                let right = self.name(parts[1])?;
                let left_name = self.name(parts[0]).ok();
                if left_name.as_deref() == Some("Elixir.String.Chars")
                    && right == "to_string"
                    && args.len() == 1
                {
                    return Ok(self.string_chars(args[0], ann, s));
                }
                if left_name.as_deref() == Some("erlang") && is_op(&right, args.len()) {
                    let op = self.atom(&right);
                    let mut x = vec![op];
                    x.extend(args);
                    return Ok(self.ast("op", ann, x));
                }
                if args.is_empty() && left_name.is_none() {
                    return self.dynamic_access(parts[0], parts[1], v[1], ann, s, c);
                }
                let l = self.expr(parts[0], ann, s, c, depth + 1)?;
                let r = self.ast("atom", ann, vec![parts[1]]);
                self.ast("remote", ann, vec![l, r])
            } else {
                return Err("invalid remote target".into());
            };
            let args = self.list(args);
            return Ok(self.ast("call", ann, vec![target, args]));
        }
        let name = self.name(v[0])?;
        if name == "__STACKTRACE__" {
            let name = s.stacktrace.clone().ok_or("stacktrace outside try")?;
            let name = self.atom(&name);
            return Ok(self.ast("var", ann, vec![name]));
        }
        if name == "__CALLER__" {
            s.caller = true;
            let n = self.atom("__CALLER__");
            return Ok(self.ast("var", ann, vec![n]));
        }
        if matches!(self.get(v[2]), T::Atom(_)) {
            let kind = self.name(v[2])?;
            let n = if name == "_" {
                name
            } else {
                let version = self
                    .prop(v[1], "version")?
                    .ok_or("variable has no version")?;
                let version = self.integer(version)?.to_string();
                if let Some(n) = s.versions.get(&version) {
                    n.clone()
                } else {
                    let key = if kind == "nil" { name.as_str() } else { "_" };
                    let count = s.counters.entry(key.into()).or_default();
                    *count += 1;
                    let n = if key == "_" {
                        self.compiler.variable_name(*count)
                    } else {
                        format!("_{key}@{count}")
                    };
                    s.versions.insert(version, n.clone());
                    n
                }
            };
            let n = self.atom(&n);
            return Ok(self.ast("var", ann, vec![n]));
        }
        let xs = self.items(v[2])?;
        match name.as_str() {
            "{}" => {
                let a = self.args(v[2], ann, s, c)?;
                let l = self.list(a);
                Ok(self.ast("tuple", ann, vec![l]))
            }
            "__block__" => {
                let a = self.args(v[2], ann, s, c)?;
                let l = self.list(a);
                Ok(self.ast("block", ann, vec![l]))
            }
            "=" => {
                if xs.len() != 2 {
                    return Err("invalid match".into());
                }
                let r = self.expr(xs[1], ann, s, c, depth + 1)?;
                let wildcard = matches!(self.get(xs[0]), T::Tuple(fields)
                    if fields.len() == 3
                    && self.name(fields[0]).ok().as_deref() == Some("_")
                    && matches!(self.get(fields[2]), T::Atom(_)));
                let l = if wildcard {
                    let name = self.atom("_");
                    self.ast("var", ann, vec![name])
                } else {
                    self.expr(xs[0], ann, s, Context::Match, depth + 1)?
                };
                let matched = self.ast("match", ann, vec![l, r]);
                if c == Context::Match || s.extra_guards.is_empty() {
                    return Ok(matched);
                }
                let extra = std::mem::take(&mut s.extra_guards);
                let generated = self.generated(ann);
                let result = self.fresh(generated, s);
                let assigned = self.ast("match", generated, vec![result, matched]);
                let guards = self.list(extra);
                let success = self.make_clause(generated, vec![], vec![guards], vec![result]);
                let badmatch = self.atom_expr(generated, "badmatch");
                let reason = self.list(vec![badmatch, result]);
                let reason = self.ast("tuple", generated, vec![reason]);
                let raise = self.remote(generated, "erlang", "error", vec![reason]);
                let yes = self.atom_expr(generated, "true");
                let yes = self.list(vec![yes]);
                let failure = self.make_clause(generated, vec![], vec![yes], vec![raise]);
                let clauses = self.list(vec![success, failure]);
                let check = self.ast("if", generated, vec![clauses]);
                let body = self.list(vec![assigned, check]);
                Ok(self.ast("block", generated, vec![body]))
            }
            "^" => {
                if xs.len() != 1 {
                    return Err("invalid pin".into());
                }
                let captured = self.expr(xs[0], ann, s, c, depth + 1)?;
                if c == Context::Match && s.pattern_binding == PatternBinding::FreshParameter {
                    let generated = self.generated(ann);
                    let parameter = self.fresh(generated, s);
                    let equality = self.op(generated, "=:=", vec![captured, parameter]);
                    s.extra_guards.push(equality);
                    Ok(parameter)
                } else {
                    Ok(captured)
                }
            }
            "%{}" => {
                let mut fs = vec![];
                let mut map_ann = ann;
                let mut update = None;
                let mut pairs = xs;
                if pairs.len() == 1 {
                    if let T::Tuple(p) = self.get(pairs[0]).clone() {
                        if p.len() == 3 && self.name(p[0]).ok().as_deref() == Some("|") {
                            let a = self.items(p[2])?;
                            if a.len() != 2 {
                                return Err("invalid map update".into());
                            }
                            update = Some(self.expr(a[0], ann, s, c, depth + 1)?);
                            map_ann = self.ann(p[1])?;
                            pairs = self.items(a[1])?
                        }
                    }
                }
                for p in pairs {
                    let p = self.tup(p)?;
                    if p.len() != 2 {
                        return Err("invalid map pair".into());
                    }
                    let key_context = if c == Context::Match { Context::Guard } else { c };
                    let k = self.expr(p[0], map_ann, s, key_context, depth + 1)?;
                    let val = self.expr(p[1], map_ann, s, c, depth + 1)?;
                    fs.push(self.ast(
                        if c == Context::Match || update.is_some() {
                            "map_field_exact"
                        } else {
                            "map_field_assoc"
                        },
                        map_ann,
                        vec![k, val],
                    ))
                }
                let fs = self.list(fs);
                Ok(self.ast(
                    "map",
                    map_ann,
                    if let Some(u) = update {
                        vec![u, fs]
                    } else {
                        vec![fs]
                    },
                ))
            }
            "case" => {
                if xs.len() != 2 {
                    return Err("invalid case".into());
                }
                let val = self.expr(xs[0], ann, s, c, depth + 1)?;
                let clauses = self.prop(xs[1], "do")?.ok_or("case lacks do")?;
                let clauses = if self
                    .prop(v[1], "optimize_boolean")?
                    .is_some_and(|id| self.name(id).ok().as_deref() == Some("true"))
                {
                    self.generated_arrows(clauses)?
                } else {
                    clauses
                };
                let cs = self.arrows(clauses, s)?;
                let cs = self.list(cs);
                Ok(self.ast("case", ann, vec![val, cs]))
            }
            "fn" => {
                let cs = self.arrows_in(v[2], s, PatternBinding::FreshParameter)?;
                let cs = self.list(cs);
                let tag = self.atom("clauses");
                let inner = self.tuple(vec![tag, cs]);
                Ok(self.ast("fun", ann, vec![inner]))
            }
            "&" => self.capture(&xs, ann, s),
            "<<>>" => self.binary(&xs, ann, s, c),
            "%" => self.structure(&xs, ann, s, c),
            "with" => self.with_expr(&xs, ann, s),
            "cond" => self.cond_expr(&xs, ann, s),
            "try" => self.try_expr(&xs, ann, s),
            "for" => self.for_expr(&xs, ann, s),
            "super" => {
                let identity = self.prop(v[1], "super")?.ok_or("super lacks target metadata")?;
                let identity = self.tup(identity)?;
                if identity.len() != 2 {
                    return Err("invalid super target metadata".into());
                }
                let kind = self.name(identity[0])?;
                let mut args = self.args(v[2], ann, s, c)?;
                let name = match kind.as_str() {
                    "def" | "defp" => identity[1],
                    "defmacro" | "defmacrop" => {
                        let name = self.name(identity[1])?;
                        let caller = self.atom("__CALLER__");
                        let caller = self.ast("var", ann, vec![caller]);
                        args.insert(0, caller);
                        s.caller = true;
                        self.atom(&format!("MACRO-{name}"))
                    }
                    _ => return Err(format!("invalid super target kind {kind}")),
                };
                let target = self.ast("atom", ann, vec![name]);
                let args = self.list(args);
                Ok(self.ast("call", ann, vec![target, args]))
            }
            "receive" => {
                if xs.len() != 1 {
                    return Err("invalid receive options".into());
                }
                let clauses = if let Some(raw) = self.prop(xs[0], "do")? {
                    if matches!(self.get(raw), T::List { .. } | T::Nil) {
                        self.arrows(raw, s)?
                    } else {
                        vec![]
                    }
                } else {
                    vec![]
                };
                let clauses = self.list(clauses);
                if let Some(after) = self.prop(xs[0], "after")? {
                    let arrows = self.items(after)?;
                    if arrows.len() != 1 {
                        return Err("receive requires exactly one after clause".into());
                    }
                    let arrow = self.tup(arrows[0])?;
                    if arrow.len() != 3 || self.name(arrow[0])? != "->" {
                        return Err("invalid receive after clause".into());
                    }
                    let parts = self.items(arrow[2])?;
                    if parts.len() != 2 {
                        return Err("invalid receive after body".into());
                    }
                    let args = self.items(parts[0])?;
                    if args.len() != 1 {
                        return Err("receive after requires one timeout expression".into());
                    }
                    let after_ann = self.ann(arrow[1])?;
                    let timeout = self.expr(args[0], after_ann, s, Context::Expr, 0)?;
                    let body = self.body(parts[1], after_ann, s)?;
                    Ok(self.ast("receive", ann, vec![clauses, timeout, body]))
                } else {
                    Ok(self.ast("receive", ann, vec![clauses]))
                }
            }
            _ => {
                let args = self.args(v[2], ann, s, c)?;
                let target = self.ast("atom", ann, vec![v[0]]);
                let args = self.list(args);
                Ok(self.ast("call", ann, vec![target, args]))
            }
        }
    }
    pub(super) fn body(&mut self, id: Id, ann: Id, s: &mut Scope) -> Result<Id> {
        let x = self.expr(id, ann, s, Context::Expr, 0)?;
        let t = self.tup(x)?;
        if self.name(t[0])? == "block" {
            Ok(t[2])
        } else {
            Ok(self.list(vec![x]))
        }
    }
    pub(super) fn clause(
        &mut self,
        meta: Id,
        args: Id,
        guards: Id,
        body: Id,
        s: &mut Scope,
    ) -> Result<Id> {
        self.clause_in(meta, args, guards, body, s, PatternBinding::Ordinary)
    }
    fn clause_in(
        &mut self,
        meta: Id,
        args: Id,
        guards: Id,
        body: Id,
        s: &mut Scope,
        binding: PatternBinding,
    ) -> Result<Id> {
        let ann = self.ann(meta)?;
        let previous_binding = std::mem::replace(&mut s.pattern_binding, binding);
        let args = self.args(args, ann, s, Context::Match);
        s.pattern_binding = previous_binding;
        let args = args?;
        let args = self.list(args);
        let guards = self.items(guards)?;
        let gs = self.take_pattern_guards(s, guards, ann)?;
        let gs = self.list(gs);
        let body = self.body(body, ann, s)?;
        Ok(self.ast("clause", ann, vec![args, gs, body]))
    }
    pub(super) fn arrows(&mut self, id: Id, s: &mut Scope) -> Result<Vec<Id>> {
        self.arrows_in(id, s, PatternBinding::Ordinary)
    }
    fn arrows_in(&mut self, id: Id, s: &mut Scope, binding: PatternBinding) -> Result<Vec<Id>> {
        let mut out = vec![];
        for x in self.items(id)? {
            let x = self.tup(x)?;
            if x.len() != 3 || self.name(x[0])? != "->" {
                return Err("invalid arrow".into());
            }
            let pair = self.items(x[2])?;
            if pair.len() != 2 {
                return Err("invalid arrow pair".into());
            }
            let mut args = self.items(pair[0])?;
            let mut guards = vec![];
            if args.len() == 1 {
                if let T::Tuple(w) = self.get(args[0]).clone() {
                    if w.len() == 3 && self.name(w[0]).ok().as_deref() == Some("when") {
                        args = self.items(w[2])?;
                        let guard = args.pop().ok_or("invalid when")?;
                        guards.extend(self.or_guards(guard)?)
                    }
                }
            }
            let args = self.list(args);
            let gs = self.list(guards);
            out.push(self.clause_in(x[1], args, gs, pair[1], s, binding)?)
        }
        Ok(out)
    }
    pub(super) fn capture(&mut self, xs: &[Id], ann: Id, s: &mut Scope) -> Result<Id> {
        if xs.len() != 1 {
            return Err("invalid capture".into());
        }
        let slash = self.tup(xs[0])?;
        if slash.len() != 3 || self.name(slash[0])? != "/" {
            return Err("unsupported capture expansion".into());
        }
        let p = self.items(slash[2])?;
        if p.len() != 2 {
            return Err("invalid capture target".into());
        }
        let f = self.tup(p[0])?;
        if f.len() != 3 {
            return Err("invalid capture function".into());
        }
        let tag = self.atom("function");
        let inner = if matches!(self.get(f[0]), T::Atom(_)) {
            self.tuple(vec![tag, f[0], p[1]])
        } else {
            let dot = self.tup(f[0])?;
            let target = self.items(dot[2])?;
            if target.len() != 2 {
                return Err("invalid remote capture".into());
            }
            let m = self.expr(target[0], ann, s, Context::Expr, 0)?;
            let f = self.ast("atom", ann, vec![target[1]]);
            let a = self.ast("integer", ann, vec![p[1]]);
            self.tuple(vec![tag, m, f, a])
        };
        Ok(self.ast("fun", ann, vec![inner]))
    }
    pub(super) fn binary(&mut self, xs: &[Id], ann: Id, s: &mut Scope, c: Context) -> Result<Id> {
        let mut segments = vec![];
        for x in xs {
            let seg = self.tup(*x)?;
            if seg.len() != 3 || self.name(seg[0])? != "::" {
                return Err("invalid expanded binary segment".into());
            }
            let p = self.items(seg[2])?;
            if p.len() != 2 {
                return Err("invalid binary specification".into());
            }
            let segment_ann = self.ann(seg[1])?;
            let (size, mut types) = self.bit_info(p[1], segment_ann, s)?;
            let value = if matches!(self.get(p[0]), T::Binary(_))
                && self.name(size).ok().as_deref() == Some("default")
            {
                let T::Binary(bytes) = self.get(p[0]).clone() else {
                    unreachable!()
                };
                let raw_types = self.items(types)?;
                let bytes_only = raw_types.first().is_some_and(|id| {
                    matches!(self.name(*id).ok().as_deref(), Some("binary" | "bitstring"))
                });
                let xs = if bytes_only {
                    types = self.atom("default");
                    bytes.into_iter().map(|b| self.int(b as i64)).collect()
                } else {
                    String::from_utf8(bytes)
                        .map_err(|_| "invalid UTF binary segment")?
                        .chars()
                        .map(|c| self.int(c as i64))
                        .collect()
                };
                let chars = self.list(xs);
                let zero = self.int(0);
                self.ast("string", zero, vec![chars])
            } else {
                self.expr(p[0], ann, s, c, 0)?
            };
            segments.push(self.ast("bin_element", ann, vec![value, size, types]))
        }
        let segs = self.list(segments);
        Ok(self.ast("bin", ann, vec![segs]))
    }
    pub(super) fn bit_info(&mut self, id: Id, ann: Id, s: &mut Scope) -> Result<(Id, Id)> {
        let mut size = self.atom("default");
        let mut types = vec![];
        let mut pending = vec![id];
        while let Some(id) = pending.pop() {
            let p = self.tup(id)?;
            if p.len() != 3 {
                return Err("invalid bit specifier".into());
            }
            let name = self.name(p[0])?;
            if name == "-" {
                let xs = self.items(p[2])?;
                pending.extend(xs.into_iter().rev())
            } else if name == "size" {
                let xs = self.items(p[2])?;
                size = self.expr(xs[0], ann, s, Context::Guard, 0)?
            } else if name == "unit" {
                let xs = self.items(p[2])?;
                types.push(self.tuple(vec![p[0], xs[0]]))
            } else {
                types.push(p[0])
            }
        }
        Ok((size, self.list(types)))
    }
    pub(super) fn remote(&mut self, ann: Id, module: &str, name: &str, args: Vec<Id>) -> Id {
        let m = self.atom(module);
        let m = self.ast("atom", ann, vec![m]);
        let n = self.atom(name);
        let n = self.ast("atom", ann, vec![n]);
        let target = self.ast("remote", ann, vec![m, n]);
        let args = self.list(args);
        self.ast("call", ann, vec![target, args])
    }
    pub(super) fn structure(
        &mut self,
        xs: &[Id],
        ann: Id,
        s: &mut Scope,
        c: Context,
    ) -> Result<Id> {
        if xs.len() != 2 {
            return Err("invalid structure".into());
        }
        let map = self.tup(xs[1])?;
        if map.len() != 3 || self.name(map[0])? != "%{}" {
            return Err("invalid structure fields".into());
        }
        let fields = self.items(map[2])?;
        if fields.len() == 1 {
            if let T::Tuple(u) = self.get(fields[0]).clone() {
                if u.len() == 3 && self.name(u[0]).ok().as_deref() == Some("|") {
                    return self.expr(xs[1], ann, s, c, 0);
                }
            }
        }
        let key = self.atom("__struct__");
        let pair = self.tuple(vec![key, xs[0]]);
        let mut fields = fields;
        fields.insert(0, pair);
        let fields = self.list(fields);
        let map_tag = self.atom("%{}");
        let meta = self.meta_from_ann(ann)?;
        let fake = self.tuple(vec![map_tag, meta, fields]);
        let result = self.expr(fake, ann, s, c, 0)?;
        let mut p = self.tup(result)?;
        p[1] = ann;
        if !matches!(self.get(xs[0]), T::Atom(_)) {
            if c != Context::Match {
                return Err("dynamic struct name outside a pattern".into());
            }
            let mut fields = self.items(p[2])?;
            let mut field = self.tup(fields[0])?;
            let generated = self.generated(ann);
            let module = self.fresh(generated, s);
            field[3] = self.ast("match", generated, vec![module, field[3]]);
            fields[0] = self.tuple(field);
            p[2] = self.list(fields);
            let guard = self.remote(generated, "erlang", "is_atom", vec![module]);
            s.extra_guards.insert(0, guard);
        }
        Ok(self.tuple(p))
    }
    pub(super) fn take_pattern_guards(
        &mut self,
        s: &mut Scope,
        guards: Vec<Id>,
        ann: Id,
    ) -> Result<Vec<Id>> {
        let extra = std::mem::take(&mut s.extra_guards);
        if guards.is_empty() {
            return Ok(if extra.is_empty() { vec![] } else { vec![self.list(extra)] });
        }
        let mut groups = vec![];
        for guard in guards {
            let mut scope = s.clone();
            let guard = self.expr(guard, ann, &mut scope, Context::Guard, 0)?;
            let mut group = vec![guard];
            group.extend(extra.iter().copied());
            groups.push(self.list(group));
        }
        Ok(groups)
    }
    pub(super) fn dynamic_access(
        &mut self,
        left: Id,
        right: Id,
        meta: Id,
        ann: Id,
        s: &mut Scope,
        c: Context,
    ) -> Result<Id> {
        let left = self.expr(left, ann, s, c, 0)?;
        let right = self.ast("atom", ann, vec![right]);
        if c == Context::Guard {
            return Ok(self.remote(ann, "erlang", "map_get", vec![right, left]));
        }
        let generated = self.generated(ann);
        let inner = self.fresh(generated, s);
        let var = self.fresh(generated, s);
        let pair = self.ast("map_field_exact", ann, vec![right, var]);
        let pairs = self.list(vec![pair]);
        let map = self.ast("map", ann, vec![pairs]);
        let noparens = self
            .prop(meta, "no_parens")?
            .is_some_and(|id| self.name(id).ok().as_deref() == Some("true"));
        let (first_body, second_body) = if noparens {
            let call = self.remote(
                generated,
                "elixir_erl_pass",
                "no_parens_remote",
                vec![var, right],
            );
            let ok = self.atom("ok");
            let ok = self.ast("atom", generated, vec![ok]);
            let error = self.atom("error");
            let error = self.ast("atom", generated, vec![error]);
            let ok_pattern = self.list(vec![ok, inner]);
            let ok_pattern = self.ast("tuple", generated, vec![ok_pattern]);
            let error_pattern = self.list(vec![error, inner]);
            let error_pattern = self.ast("tuple", generated, vec![error_pattern]);
            let raise = self.remote(ann, "erlang", "error", vec![inner]);
            let ok_clause = self.make_clause(generated, vec![ok_pattern], vec![], vec![inner]);
            let err_clause = self.make_clause(generated, vec![error_pattern], vec![], vec![raise]);
            let clauses = self.list(vec![ok_clause, err_clause]);
            let case = self.ast("case", generated, vec![call, clauses]);
            (var, case)
        } else {
            let checked = self.remote(
                generated,
                "elixir_erl_pass",
                "parens_map_field",
                vec![right, var],
            );
            let remote = self.ast("remote", generated, vec![var, right]);
            let empty = self.list(vec![]);
            let call = self.ast("call", generated, vec![remote, empty]);
            (checked, call)
        };
        let first = self.make_clause(generated, vec![map], vec![], vec![first_body]);
        let second = self.make_clause(generated, vec![var], vec![], vec![second_body]);
        let clauses = self.list(vec![first, second]);
        Ok(self.ast("case", generated, vec![left, clauses]))
    }
    pub(super) fn string_chars(&mut self, arg: Id, ann: Id, s: &mut Scope) -> Id {
        let generated = self.generated(ann);
        let var = self.fresh(generated, s);
        let guard = self.remote(generated, "erlang", "is_binary", vec![var]);
        let guard = self.list(vec![guard]);
        let slow = self.remote(generated, "Elixir.String.Chars", "to_string", vec![var]);
        let first = self.make_clause(generated, vec![var], vec![guard], vec![var]);
        let second = self.make_clause(generated, vec![var], vec![], vec![slow]);
        let clauses = self.list(vec![first, second]);
        self.ast("case", generated, vec![arg, clauses])
    }
}
fn is_op(s: &str, n: usize) -> bool {
    match n {
        1 => matches!(s, "+" | "-" | "bnot" | "not"),
        2 => matches!(
            s,
            "+" | "-"
                | "*"
                | "/"
                | "div"
                | "rem"
                | "band"
                | "bor"
                | "bxor"
                | "bsl"
                | "bsr"
                | "=="
                | "/="
                | "=<"
                | "<"
                | ">="
                | ">"
                | "=:="
                | "=/="
                | "and"
                | "or"
                | "xor"
                | "andalso"
                | "orelse"
        ),
        _ => false,
    }
}
