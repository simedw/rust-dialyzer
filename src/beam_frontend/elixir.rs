//! Elixir 1.19 expanded debug-info lifting to Erlang abstract forms.
//!
//! Adapted from Elixir's elixir_erl, elixir_erl_var and elixir_erl_pass
//! (Elixir contributors, Apache-2.0). No parser, macro evaluation or VM calls.
use super::term::Arena;
#[path = "elixir_for.rs"]
mod comprehensions;
#[path = "elixir_try.rs"]
mod exceptions;
use crate::core::{ErlangTerm, ErlangTerms, TermId};
use std::collections::BTreeMap;
type Result<T> = std::result::Result<T, String>;
#[derive(Clone, Default)]
struct Scope {
    names: BTreeMap<i64, String>,
    counters: BTreeMap<String, usize>,
    matching: bool,
    guarding: bool,
    caller: bool,
    stacktrace: Option<String>,
    map_key: bool,
    pin_guard: bool,
    extra_guards: Vec<TermId>,
}
struct Lift {
    a: Arena,
    function: String,
    module: String,
    depth: usize,
}
impl Lift {
    fn error(&self, tag: &str) -> String {
        format!(
            "Elixir lifting {}:{}: unsupported or malformed {tag}",
            self.module, self.function
        )
    }
    fn fields(&self, id: TermId, n: usize) -> Result<Vec<TermId>> {
        let fields = self.a.tuple_fields(id)?.to_vec();
        if fields.len() != n {
            return Err(self.error(&format!("tuple arity {n} at term {}", id.0)));
        }
        Ok(fields)
    }
    fn atom(&self, id: TermId) -> Result<String> {
        self.a
            .as_atom(id)
            .map(str::to_owned)
            .ok_or_else(|| self.error("atom"))
    }
    fn required(&self, id: TermId, key: &str) -> Result<TermId> {
        self.a
            .map_get(id, key)
            .ok_or_else(|| self.error(&format!("metadata field {key}")))
    }
    fn node(&mut self, kind: &str, ann: TermId, fields: Vec<TermId>) -> TermId {
        let mut all = vec![ann];
        all.extend(fields);
        self.a.tagged(kind, all)
    }
    fn atom_node(&mut self, ann: TermId, name: &str) -> TermId {
        let value = self.a.atom(name);
        self.node("atom", ann, vec![value])
    }
    fn var_node(&mut self, ann: TermId, name: &str) -> TermId {
        let value = self.a.atom(name);
        self.node("var", ann, vec![value])
    }
    fn generated(&mut self, location: TermId) -> TermId {
        let yes = self.a.atom("true");
        let generated = self.a.tagged("generated", vec![yes]);
        let location = self.a.tagged("location", vec![location]);
        self.a.list(vec![generated, location])
    }
    fn annotation(&mut self, meta: TermId) -> Result<TermId> {
        let line = self
            .a
            .keyword(meta, "line")
            .and_then(|x| self.a.as_integer(x))
            .unwrap_or(0);
        let line = self.a.integer(line);
        let location = match self.a.keyword(meta, "column") {
            Some(column) => self.a.tuple(vec![line, column]),
            None => line,
        };
        Ok(
            if self
                .a
                .keyword(meta, "generated")
                .and_then(|x| self.a.as_atom(x))
                == Some("true")
            {
                self.generated(location)
            } else {
                location
            },
        )
    }
    fn location(&self, ann: TermId) -> TermId {
        self.a.keyword(ann, "location").unwrap_or(ann)
    }
    fn chars(&mut self, id: TermId) -> Result<TermId> {
        match self.a.get(id).clone() {
            ErlangTerm::Binary(bytes) => {
                let text = std::str::from_utf8(&bytes).map_err(|_| self.error("UTF-8 filename"))?;
                let items = text.chars().map(|c| self.a.integer(c as u32)).collect();
                Ok(self.a.list(items))
            }
            ErlangTerm::Nil | ErlangTerm::List { .. } => Ok(id),
            _ => Err(self.error("filename")),
        }
    }
    fn build(&mut self, scope: &mut Scope, name: &str) -> String {
        let count = scope.counters.entry(name.to_owned()).or_default();
        *count += 1;
        if name == "_" {
            format!("_@{count}")
        } else {
            format!("_{name}@{count}")
        }
    }
    fn variable(
        &mut self,
        meta: TermId,
        name: &str,
        kind: &str,
        scope: &mut Scope,
    ) -> Result<TermId> {
        let ann = self.annotation(meta)?;
        if name == "_" {
            return Ok(self.var_node(ann, "_"));
        }
        let version = self
            .a
            .keyword(meta, "version")
            .and_then(|x| self.a.as_integer(x))
            .ok_or_else(|| self.error("variable version"))?;
        let erl = match scope.names.get(&version) {
            Some(name) => name.clone(),
            None => {
                let built = self.build(scope, if kind == "nil" { name } else { "_" });
                scope.names.insert(version, built.clone());
                built
            }
        };
        Ok(self.var_node(ann, &erl))
    }
    fn unblock(&self, expr: TermId) -> Result<Vec<TermId>> {
        if self.a.tag(expr) == Some("block") {
            let t = self.fields(expr, 3)?;
            self.a.list_items(t[2])
        } else {
            Ok(vec![expr])
        }
    }
    fn literals_list(&mut self, ann: TermId, values: Vec<TermId>, tail: TermId) -> Result<TermId> {
        let mut out = self.literal(tail, ann)?;
        for value in values.into_iter().rev() {
            let value = self.literal(value, ann)?;
            out = self.node("cons", ann, vec![value, out]);
        }
        Ok(out)
    }
    fn literal(&mut self, id: TermId, ann: TermId) -> Result<TermId> {
        let scalar = match self.a.get(id) {
            ErlangTerm::Atom(_) => Some("atom"),
            ErlangTerm::Integer(_) => Some("integer"),
            ErlangTerm::Float(bits) if f64::from_bits(*bits) != 0.0 => Some("float"),
            ErlangTerm::Nil => Some("nil"),
            _ => None,
        };
        if let Some(tag) = scalar {
            return Ok(self.node(tag, ann, if tag == "nil" { vec![] } else { vec![id] }));
        }
        Ok(match self.a.get(id).clone() {
            ErlangTerm::Atom(_) => self.node("atom", ann, vec![id]),
            ErlangTerm::Integer(_) => self.node("integer", ann, vec![id]),
            ErlangTerm::Float(bits) if f64::from_bits(bits) == 0.0 => {
                let value = self.a.push(ErlangTerm::Float(0));
                let float = self.node("float", ann, vec![value]);
                let sign = self.a.atom(if bits >> 63 == 1 { "-" } else { "+" });
                self.node("op", ann, vec![sign, float])
            }
            ErlangTerm::Float(_) => self.node("float", ann, vec![id]),
            ErlangTerm::Nil => self.node("nil", ann, vec![]),
            ErlangTerm::Tuple(items) => {
                let mut values = Vec::new();
                for item in items {
                    values.push(self.literal(item, ann)?);
                }
                let values = self.a.list(values);
                self.node("tuple", ann, vec![values])
            }
            ErlangTerm::List { items, tail } => self.literals_list(ann, items, tail)?,
            ErlangTerm::Binary(bytes) => {
                let mut elements = Vec::new();
                if !bytes.is_empty() {
                    let values = bytes.into_iter().map(|b| self.a.integer(b)).collect();
                    let values = self.a.list(values);
                    let string = self.node("string", ann, vec![values]);
                    let default = self.a.atom("default");
                    elements.push(self.node("bin_element", ann, vec![string, default, default]));
                }
                let elements = self.a.list(elements);
                self.node("bin", ann, vec![elements])
            }
            ErlangTerm::Map(pairs) => {
                // OTP sorts the complete {key,value} pairs in ordinary term order.
                let mut pairs = pairs;
                if pairs.len() > 1 {
                    let mut sorted = Vec::with_capacity(pairs.len());
                    for (key, value) in pairs {
                        sorted.push((self.a.tuple(vec![key, value]), key, value));
                    }
                    let roots: Vec<_> = sorted.iter().map(|pair| pair.0).collect();
                    let snapshot = self.a.snapshot(id);
                    let order = crate::term_order::TermOrderArena::reachable(&snapshot, &roots)?;
                    for i in 1..sorted.len() {
                        let mut j = i;
                        while j > 0
                            && order.compare(sorted[j - 1].0, &order, sorted[j].0)?
                                == std::cmp::Ordering::Greater
                        {
                            sorted.swap(j - 1, j);
                            j -= 1;
                        }
                    }
                    pairs = sorted
                        .into_iter()
                        .map(|(_, key, value)| (key, value))
                        .collect();
                }
                let mut fields = Vec::with_capacity(pairs.len());
                for (key, value) in pairs {
                    let key = self.literal(key, ann)?;
                    let value = self.literal(value, ann)?;
                    fields.push(self.node("map_field_assoc", ann, vec![key, value]));
                }
                let fields = self.a.list(fields);
                self.node("map", ann, vec![fields])
            }
            ErlangTerm::Bitstring { .. } => return Err(self.error("non-byte literal bitstring")),
            ErlangTerm::ExternalFun {
                module,
                function,
                arity,
            } => {
                let module = self.atom_node(ann, &module);
                let function = self.atom_node(ann, &function);
                let arity = self.a.integer(arity);
                let arity = self.node("integer", ann, vec![arity]);
                let function = self.a.tagged("function", vec![module, function, arity]);
                self.node("fun", ann, vec![function])
            }
        })
    }
    fn expr(&mut self, id: TermId, inherited: TermId, scope: &mut Scope) -> Result<TermId> {
        if self.depth >= 512 {
            return Err(self.error("expression nesting resource limit"));
        }
        self.depth += 1;
        let result = self.expr_inner(id, inherited, scope);
        self.depth -= 1;
        result
    }
    fn block_items(
        &mut self,
        args: Vec<TermId>,
        ann: TermId,
        scope: &mut Scope,
    ) -> Result<Vec<TermId>> {
        let mut out = Vec::new();
        let count = args.len();
        for (index, arg) in args.into_iter().enumerate() {
            if index + 1 < count && self.a.tag(arg) == Some("__block__") {
                let fields = self.fields(arg, 3)?;
                let nested = self.a.list_items(fields[2])?;
                let ann = self.annotation(fields[1])?;
                out.extend(self.block_items(nested, ann, scope)?);
            } else {
                out.push(self.expr(arg, ann, scope)?);
            }
        }
        Ok(out)
    }
    fn arguments(&mut self, args: TermId, ann: TermId, scope: &mut Scope) -> Result<TermId> {
        let mut out = Vec::new();
        for arg in self.a.list_items(args)? {
            out.push(self.expr(arg, ann, scope)?);
        }
        Ok(self.a.list(out))
    }
    fn expr_inner(&mut self, id: TermId, inherited: TermId, scope: &mut Scope) -> Result<TermId> {
        let t = if let ErlangTerm::Tuple(items) = self.a.get(id) {
            items.clone()
        } else {
            if let ErlangTerm::List { items, tail } = self.a.get(id) {
                let (items, tail) = (items.clone(), *tail);
                let mut values = Vec::new();
                let mut final_tail = tail;
                for (index, item) in items.iter().copied().enumerate() {
                    if index + 1 == items.len() && self.a.tag(item) == Some("|") {
                        let fields = self.fields(item, 3)?;
                        let args = self.a.list_items(fields[2])?;
                        if args.len() != 2 || !matches!(self.a.get(tail), ErlangTerm::Nil) {
                            return Err(self.error("list tail"));
                        }
                        values.push(self.expr(args[0], inherited, scope)?);
                        final_tail = args[1];
                    } else {
                        values.push(self.expr(item, inherited, scope)?);
                    }
                }
                let mut out = self.expr(final_tail, inherited, scope)?;
                for value in values.into_iter().rev() {
                    out = self.node("cons", inherited, vec![value, out]);
                }
                return Ok(out);
            }
            return self.literal(id, inherited);
        };
        if t.len() == 2 {
            let left = self.expr(t[0], inherited, scope)?;
            let right = self.expr(t[1], inherited, scope)?;
            let values = self.a.list(vec![left, right]);
            return Ok(self.node("tuple", inherited, vec![values]));
        }
        if t.len() != 3 {
            return Err(self.error("expanded expression tuple"));
        }
        let ann = self.annotation(t[1])?;
        if let Some(name) = self.a.as_atom(t[0]).map(str::to_owned) {
            if let Some(kind) = self.a.as_atom(t[2]).map(str::to_owned) {
                if name == "__CALLER__" {
                    scope.caller = true;
                    return Ok(self.var_node(ann, "__CALLER__"));
                }
                if name == "__STACKTRACE__" {
                    let stack = scope
                        .stacktrace
                        .clone()
                        .ok_or_else(|| self.error("stacktrace outside try"))?;
                    return Ok(self.var_node(ann, &stack));
                }
                return self.variable(t[1], &name, &kind, scope);
            }
            let args = self.a.list_items(t[2])?;
            match name.as_str() {
                "=" if args.len() == 2 => {
                    let right = self.expr(args[1], ann, scope)?;
                    if self.fields(args[0], 3).is_ok_and(|fields| {
                        self.a.as_atom(fields[0]) == Some("_")
                            && self.a.as_atom(fields[2]).is_some()
                    }) {
                        let wildcard = self.var_node(ann, "_");
                        return Ok(self.node("match", ann, vec![wildcard, right]));
                    }
                    let saved = scope.matching;
                    scope.matching = true;
                    let left = self.expr(args[0], ann, scope);
                    scope.matching = saved;
                    let matched = self.node("match", ann, vec![left?, right]);
                    if !saved && !scope.extra_guards.is_empty() {
                        let extra = std::mem::take(&mut scope.extra_guards);
                        let generated = self.generated(self.location(ann));
                        let name = self.build(scope, "_");
                        let result = self.var_node(generated, &name);
                        let assign = self.node("match", generated, vec![result, matched]);
                        let yes = self.atom_node(generated, "true");
                        let bad = self.atom_node(generated, "badmatch");
                        let values = self.a.list(vec![bad, result]);
                        let reason = self.node("tuple", generated, vec![values]);
                        let error = self.remote_call(generated, "erlang", "error", vec![reason]);
                        let args = self.a.nil();
                        let extra = self.a.list(extra);
                        let guards = self.a.list(vec![extra]);
                        let body = self.a.list(vec![result]);
                        let success = self.node("clause", generated, vec![args, guards, body]);
                        let args = self.a.nil();
                        let guard = self.a.list(vec![yes]);
                        let guards = self.a.list(vec![guard]);
                        let body = self.a.list(vec![error]);
                        let failure = self.node("clause", generated, vec![args, guards, body]);
                        let branches = self.a.list(vec![success, failure]);
                        let check = self.node("if", generated, vec![branches]);
                        let body = self.a.list(vec![assign, check]);
                        return Ok(self.node("block", generated, vec![body]));
                    }
                    return Ok(matched);
                }
                "^" if args.len() == 1 => {
                    let var = self.expr(args[0], inherited, scope)?;
                    if scope.pin_guard && !scope.map_key {
                        let fields = self.fields(args[0], 3)?;
                        let variable_ann = self.annotation(fields[1])?;
                        let generated = self.generated(self.location(variable_ann));
                        let name = self.build(scope, "_");
                        let pin = self.var_node(generated, &name);
                        let equal = self.a.atom("=:=");
                        let guard = self.node("op", generated, vec![equal, var, pin]);
                        scope.extra_guards.insert(0, guard);
                        return Ok(pin);
                    }
                    return Ok(var);
                }
                "{}" => {
                    let args = self.arguments(t[2], ann, scope)?;
                    return Ok(self.node("tuple", ann, vec![args]));
                }
                "__block__" => {
                    let out = self.block_items(args, ann, scope)?;
                    let out = self.a.list(out);
                    return Ok(self.node("block", ann, vec![out]));
                }
                "case" if args.len() == 2 => {
                    let value = self.expr(args[0], ann, scope)?;
                    let clauses = self
                        .a
                        .keyword(args[1], "do")
                        .ok_or_else(|| self.error("case clauses"))?;
                    let generated = self
                        .a
                        .keyword(t[1], "optimize_boolean")
                        .and_then(|x| self.a.as_atom(x))
                        == Some("true");
                    let clauses = self.arrow_clauses(clauses, scope, generated, false)?;
                    return Ok(self.node("case", ann, vec![value, clauses]));
                }
                "fn" => {
                    let clauses = self.arrow_clauses(t[2], scope, false, true)?;
                    let clauses = self.a.tagged("clauses", vec![clauses]);
                    return Ok(self.node("fun", ann, vec![clauses]));
                }
                "%{}" => return self.map_expr(ann, args, scope),
                "%" if args.len() == 2 => return self.struct_expr(ann, args[0], args[1], scope),
                "super" => {
                    let entry = self
                        .a
                        .keyword(t[1], "super")
                        .ok_or_else(|| self.error("super metadata"))?;
                    let pair = self.fields(entry, 2)?;
                    let kind = self.atom(pair[0])?;
                    let name = self.atom(pair[1])?;
                    let args = self.arguments(t[2], ann, scope)?;
                    let macro_call = matches!(kind.as_str(), "defmacro" | "defmacrop");
                    let name = self.atom_node(
                        ann,
                        &if macro_call {
                            format!("MACRO-{name}")
                        } else {
                            name
                        },
                    );
                    let args = if macro_call {
                        scope.caller = true;
                        let caller = self.var_node(ann, "__CALLER__");
                        let mut all = vec![caller];
                        all.extend(self.a.list_items(args)?);
                        self.a.list(all)
                    } else {
                        args
                    };
                    return Ok(self.node("call", ann, vec![name, args]));
                }
                "&" if args.len() == 1 => return self.capture(args[0], ann, scope),
                "receive" if args.len() == 1 => {
                    let clauses = self
                        .a
                        .keyword(args[0], "do")
                        .unwrap_or_else(|| self.a.nil());
                    let clauses = self.arrow_clauses(clauses, scope, false, false)?;
                    if let Some(after) = self.a.keyword(args[0], "after") {
                        let after = self.a.list_items(after)?;
                        if after.len() != 1 {
                            return Err(self.error("receive after clauses"));
                        }
                        let fields = self.fields(after[0], 3)?;
                        let args = self.a.list_items(fields[2])?;
                        if args.len() != 2 {
                            return Err(self.error("receive after arrow"));
                        }
                        let condition = self.a.list_items(args[0])?;
                        if condition.len() != 1 {
                            return Err(self.error("receive timeout"));
                        }
                        let after_ann = self.annotation(fields[1])?;
                        let timeout = self.expr(condition[0], after_ann, scope)?;
                        let body = self.expr(args[1], after_ann, scope)?;
                        let body = self.unblock(body)?;
                        let body = self.a.list(body);
                        return Ok(self.node("receive", ann, vec![clauses, timeout, body]));
                    }
                    return Ok(self.node("receive", ann, vec![clauses]));
                }
                "<<>>" => return self.bitstring(ann, args, scope),
                "cond" if args.len() == 1 => {
                    return self.cond_expr(t[1], args[0], inherited, scope);
                }
                "with" if !args.is_empty() => return self.with_expr(t[1], args, scope),
                "try" if args.len() == 1 => return self.try_expr(ann, args[0], scope),
                "for" if !args.is_empty() => {
                    return comprehensions::translate(self, t[1], args, scope);
                }
                "for" | "when" | "|" => {
                    return Err(self.error(&name));
                }
                _ => {
                    let args = self.arguments(t[2], ann, scope)?;
                    let name = self.atom_node(ann, &name);
                    return Ok(self.node("call", ann, vec![name, args]));
                }
            }
        }
        let dot = self.fields(t[0], 3)?;
        if self.a.as_atom(dot[0]) != Some(".") {
            return Err(self.error("call operator"));
        }
        let parts = self.a.list_items(dot[2])?;
        if parts.len() == 1 {
            let callee = self.expr(parts[0], ann, scope)?;
            let args = self.arguments(t[2], ann, scope)?;
            return Ok(self.node("call", ann, vec![callee, args]));
        }
        if parts.len() != 2 {
            return Err(self.error("remote call"));
        }
        let module = self.a.as_atom(parts[0]).map(str::to_owned);
        let name = self.atom(parts[1])?;
        let args_len = self.a.list_items(t[2])?.len();
        if module.as_deref() == Some("erlang") && is_operator(&name, args_len) {
            let args = self.a.list_items(t[2])?;
            let mut values = vec![self.a.atom(&name)];
            for arg in args {
                values.push(self.expr(arg, ann, scope)?);
            }
            return Ok(self.node("op", ann, values));
        }
        if args_len == 0
            && (matches!(self.a.get(parts[0]), ErlangTerm::Tuple(_))
                || matches!(module.as_deref(), Some("nil" | "true" | "false")))
        {
            return self.dynamic_zero(parts[0], &name, t[1], ann, scope);
        }
        if module.as_deref() == Some("Elixir.String.Chars") && name == "to_string" && args_len == 1
        {
            let args = self.a.list_items(t[2])?;
            let value = self.expr(args[0], ann, scope)?;
            let name = self.build(scope, "_");
            let generated = self.generated(self.location(ann));
            let var = self.var_node(generated, &name);
            let guard = self.remote_call(generated, "erlang", "is_binary", vec![var]);
            let slow = self.remote_call(generated, "Elixir.String.Chars", "to_string", vec![var]);
            let pattern = self.a.list(vec![var]);
            let guard = self.a.list(vec![guard]);
            let guards = self.a.list(vec![guard]);
            let body = self.a.list(vec![var]);
            let first = self.node("clause", generated, vec![pattern, guards, body]);
            let second = self.simple_clause(generated, var, slow);
            let clauses = self.a.list(vec![first, second]);
            return Ok(self.node("case", generated, vec![value, clauses]));
        }
        if module.as_deref() == Some("maps")
            && ((name == "put" && args_len == 3) || (name == "merge" && args_len == 2))
        {
            let args = self.arguments(t[2], ann, scope)?;
            let args = self.a.list_items(args)?;
            if name == "put" {
                let field = self.node("map_field_assoc", ann, vec![args[0], args[1]]);
                return self.append_map(ann, args[2], vec![field]);
            }
            if self.a.tag(args[1]) == Some("map") {
                let fields = self.a.tuple_fields(args[1])?.to_vec();
                if fields.len() == 3 {
                    let pairs = self.a.list_items(fields[2])?;
                    return self.append_map(ann, args[0], pairs);
                }
            }
            return Ok(self.remote_call(ann, "maps", "merge", args));
        }
        if module
            .as_deref()
            .is_some_and(|module| inline_pure(module, &name))
            && self.basic_args(t[2])?
        {
            let args = self.a.list_items(t[2])?;
            if let Some(value) = self.pure(module.as_deref().unwrap(), &name, args)? {
                let generated = self.generated(self.location(ann));
                return self.expr(value, generated, scope);
            }
        }
        if matches!(
            module.as_deref(),
            Some("Elixir.Date" | "Elixir.DateTime" | "Elixir.NaiveDateTime" | "Elixir.Time")
        ) && name == "shift"
        {
            let args = self.a.list_items(t[2])?;
            if args.len() >= 2 && self.basic(args[1])? {
                return Err(self.error("compiler inline duration options"));
            }
        }
        let left = self.expr(parts[0], ann, scope)?;
        let right = self.atom_node(ann, &name);
        let remote = self.node("remote", ann, vec![left, right]);
        let args = self.arguments(t[2], ann, scope)?;
        Ok(self.node("call", ann, vec![remote, args]))
    }
    fn string_units(&mut self, bytes: &[u8], graphemes: bool) -> Vec<TermId> {
        use unicode_segmentation::UnicodeSegmentation;
        let mut remaining = bytes;
        let mut units = Vec::new();
        while !remaining.is_empty() {
            let (valid, rest) = match std::str::from_utf8(remaining) {
                Ok(valid) => (valid, &[][..]),
                Err(error) => (
                    std::str::from_utf8(&remaining[..error.valid_up_to()])
                        .expect("validated UTF-8 prefix"),
                    &remaining[error.valid_up_to()..],
                ),
            };
            if graphemes {
                for part in valid.graphemes(true) {
                    units.push(self.a.push(ErlangTerm::Binary(part.as_bytes().to_vec())));
                }
            } else {
                for ch in valid.chars() {
                    let mut encoded = [0; 4];
                    units.push(self.a.push(ErlangTerm::Binary(
                        ch.encode_utf8(&mut encoded).as_bytes().to_vec(),
                    )));
                }
            }
            if let Some((invalid, tail)) = rest.split_first() {
                units.push(self.a.push(ErlangTerm::Binary(vec![*invalid])));
                remaining = tail;
            } else {
                break;
            }
        }
        units
    }
    fn pure(&mut self, module: &str, name: &str, args: Vec<TermId>) -> Result<Option<TermId>> {
        if module == "Elixir.MapSet" && name == "new" {
            let keys = match args.as_slice() {
                [] => vec![],
                [list] => match self.a.list_items(*list) {
                    Ok(list) => list,
                    Err(_) => return Ok(None),
                },
                _ => return Ok(None),
            };
            let unique = if keys.len() < 2 {
                keys
            } else {
                let snapshot = self.a.snapshot(keys[0]);
                let order = crate::term_order::TermOrderArena::reachable(&snapshot, &keys)?;
                let mut unique = Vec::with_capacity(keys.len());
                for key in keys {
                    let mut found = false;
                    for &other in &unique {
                        if order.compare_map_keys(key, &order, other)? == std::cmp::Ordering::Equal
                        {
                            found = true;
                            break;
                        }
                    }
                    if !found {
                        unique.push(key);
                    }
                }
                unique
            };
            let empty = self.a.nil();
            let entries = unique.into_iter().map(|key| (key, empty)).collect();
            let map = self.a.push(ErlangTerm::Map(entries));
            let struct_key = self.a.atom("__struct__");
            let struct_name = self.a.atom("Elixir.MapSet");
            let map_key = self.a.atom("map");
            let value = self.a.push(ErlangTerm::Map(vec![
                (struct_key, struct_name),
                (map_key, map),
            ]));
            return Ok(Some(value));
        }
        if module == "Elixir.String" && matches!(name, "length" | "graphemes" | "codepoints") {
            let [arg] = args.as_slice() else {
                return Ok(None);
            };
            let ErlangTerm::Binary(bytes) = self.a.get(*arg).clone() else {
                return Ok(None);
            };
            let units = self.string_units(&bytes, name != "codepoints");
            let value = if name == "length" {
                self.a.integer(units.len())
            } else {
                self.a.list(units)
            };
            return Ok(Some(value));
        }
        Err(self.error(&format!(
            "compiler inline-pure {module}.{name}/{}",
            args.len()
        )))
    }
    fn append_map(&mut self, ann: TermId, map: TermId, pairs: Vec<TermId>) -> Result<TermId> {
        if self.a.tag(map) == Some("map") {
            let fields = self.a.tuple_fields(map)?.to_vec();
            if fields.len() == 3 || fields.len() == 4 {
                let mut all = self.a.list_items(*fields.last().unwrap())?;
                all.extend(pairs);
                let all = self.a.list(all);
                return Ok(self.node(
                    "map",
                    ann,
                    if fields.len() == 3 {
                        vec![all]
                    } else {
                        vec![fields[2], all]
                    },
                ));
            }
        }
        let pairs = self.a.list(pairs);
        Ok(self.node("map", ann, vec![map, pairs]))
    }
    fn generated_meta(&mut self, meta: TermId) -> Result<TermId> {
        let yes = self.a.atom("true");
        let pair = self.a.tagged("generated", vec![yes]);
        let mut items = vec![pair];
        items.extend(self.a.list_items(meta)?);
        Ok(self.a.list(items))
    }
    fn assign(&mut self, meta: TermId, scope: &mut Scope) -> Result<(TermId, TermId)> {
        let version = -(scope.names.len() as i64 + 1);
        let ann = self.annotation(meta)?;
        let name = self.build(scope, "_");
        scope.names.insert(version, name.clone());
        let erl = self.var_node(ann, &name);
        let version = self.a.integer(version);
        let pair = self.a.tagged("version", vec![version]);
        let mut items = vec![pair];
        items.extend(self.a.list_items(meta)?);
        let meta = self.a.list(items);
        let var = self.a.atom("var");
        let context = self.a.atom("elixir_erl_var");
        Ok((self.a.tuple(vec![var, meta, context]), erl))
    }
    fn with_expr(
        &mut self,
        meta: TermId,
        mut args: Vec<TermId>,
        scope: &mut Scope,
    ) -> Result<TermId> {
        let opts = args.pop().ok_or_else(|| self.error("with options"))?;
        let body = self
            .a
            .keyword(opts, "do")
            .ok_or_else(|| self.error("with do"))?;
        let ann = self.annotation(meta)?;
        let mut closure = None;
        let else_clause = if let Some(else_clauses) = self.a.keyword(opts, "else") {
            let items = self.a.list_items(else_clauses)?;
            let mut single = None;
            if items.len() == 1 {
                let f = self.fields(items[0], 3)?;
                let a = self.a.list_items(f[2])?;
                if a.len() == 2 {
                    let patterns = self.a.list_items(a[0])?;
                    if patterns.len() == 1 {
                        if let Ok(var) = self.fields(patterns[0], 3) {
                            if self.a.as_atom(var[0]).is_some() && self.a.as_atom(var[2]).is_some()
                            {
                                single = Some((patterns[0], a[1]));
                            }
                        }
                    }
                }
            }
            let (clauses, closure_meta) = if let Some((pattern, body)) = single {
                let pattern = self.expr(pattern, ann, scope)?;
                let body = self.expr(body, ann, scope)?;
                {
                    let clause = self.simple_clause(ann, pattern, body);
                    (self.a.list(vec![clause]), meta)
                }
            } else {
                let generated = self.generated_meta(meta)?;
                let (raise, _) = self.assign(generated, scope)?;
                let reason = self.a.atom("else_clause");
                let reason = self.a.tuple(vec![reason, raise]);
                let expr = self.expanded_remote(generated, "error", vec![reason]);
                let pattern = self.a.list(vec![raise]);
                let catchall = self.expanded("->", generated, vec![pattern, expr]);
                let mut items = items;
                items.push(catchall);
                let items = self.a.list(items);
                let old = scope.pin_guard;
                scope.pin_guard = true;
                let translated = self.arrow_clauses(items, scope, false, false);
                scope.pin_guard = old;
                (translated?, generated)
            };
            let closure_ann = self.annotation(closure_meta)?;
            let (_, fun) = self.assign(closure_meta, scope)?;
            let (_, arg) = self.assign(closure_meta, scope)?;
            let generated = self.generated(self.location(closure_ann));
            let wrapper = self.a.tagged("clauses", vec![clauses]);
            let function = self.node("fun", generated, vec![wrapper]);
            closure = Some(self.node("match", closure_ann, vec![fun, function]));
            let args = self.a.list(vec![arg]);
            let call = self.node("call", closure_ann, vec![fun, args]);
            self.simple_clause(generated, arg, call)
        } else {
            let name = self.build(scope, "_");
            let var = self.var_node(ann, &name);
            let generated = self.generated(self.location(ann));
            self.simple_clause(generated, var, var)
        };
        let result = self.with_do(&args, ann, body, else_clause, scope)?;
        if let Some(closure) = closure {
            let body = self.a.list(vec![closure, result]);
            Ok(self.node("block", ann, vec![body]))
        } else {
            Ok(result)
        }
    }
    fn with_do(
        &mut self,
        args: &[TermId],
        ann: TermId,
        body: TermId,
        otherwise: TermId,
        scope: &mut Scope,
    ) -> Result<TermId> {
        let Some((first, rest)) = args.split_first() else {
            return self.expr(body, ann, scope);
        };
        if self.a.tag(*first) == Some("<-") {
            let fields = self.fields(*first, 3)?;
            let pair = self.a.list_items(fields[2])?;
            if pair.len() != 2 {
                return Err(self.error("with <-"));
            }
            if self
                .fields(pair[0], 3)
                .is_ok_and(|f| self.a.as_atom(f[0]).is_some() && self.a.as_atom(f[2]).is_some())
            {
                let assign = self.expanded("=", fields[1], pair);
                let mut all = vec![assign];
                all.extend(rest.iter().copied());
                return self.with_do(&all, ann, body, otherwise, scope);
            }
            let ann = self.annotation(fields[1])?;
            let (pattern, guards) = self.extract_guards(pair[0])?;
            let value = self.expr(pair[1], ann, scope)?;
            let old = scope.matching;
            scope.matching = true;
            let pattern = self.expr(pattern, ann, scope);
            scope.matching = old;
            let pattern = pattern?;
            let extra = std::mem::take(&mut scope.extra_guards);
            let mut translated = Vec::new();
            for guard in guards {
                let mut guard_scope = scope.clone();
                guard_scope.guarding = true;
                let guard = self.expr(guard, ann, &mut guard_scope)?;
                let mut all = vec![guard];
                all.extend(extra.iter().copied());
                translated.push(self.a.list(all));
            }
            if translated.is_empty() && !extra.is_empty() {
                translated.push(self.a.list(extra));
            }
            let guards = self.a.list(translated);
            let body = self.with_do(rest, ann, body, otherwise, scope)?;
            let body = self.unblock(body)?;
            let body = self.a.list(body);
            let pattern = self.a.list(vec![pattern]);
            let success = self.node("clause", ann, vec![pattern, guards, body]);
            let clauses = self.a.list(vec![success, otherwise]);
            return Ok(self.node("case", ann, vec![value, clauses]));
        }
        let value = self.expr(*first, ann, scope)?;
        let rest = self.with_do(rest, ann, body, otherwise, scope)?;
        let mut body = vec![value];
        body.extend(self.unblock(rest)?);
        let body = self.a.list(body);
        Ok(self.node("block", ann, vec![body]))
    }
    fn try_expr(&mut self, ann: TermId, opts: TermId, scope: &mut Scope) -> Result<TermId> {
        let body = self
            .a
            .keyword(opts, "do")
            .unwrap_or_else(|| self.a.atom("nil"));
        let body = self.expr(body, ann, scope)?;
        let body = self.unblock(body)?;
        let body = self.a.list(body);
        let catches = exceptions::clauses(self, opts, scope)?;
        let after = if let Some(after) = self.a.keyword(opts, "after") {
            let after = self.expr(after, ann, scope)?;
            let after = self.unblock(after)?;
            self.a.list(after)
        } else {
            self.a.nil()
        };
        let else_clauses = self.a.keyword(opts, "else").unwrap_or_else(|| self.a.nil());
        let else_clauses = self.arrow_clauses(else_clauses, scope, false, false)?;
        Ok(self.node("try", ann, vec![body, else_clauses, catches, after]))
    }
    fn expanded(&mut self, tag: &str, meta: TermId, args: Vec<TermId>) -> TermId {
        let tag = self.a.atom(tag);
        let args = self.a.list(args);
        self.a.tuple(vec![tag, meta, args])
    }
    fn expanded_remote(&mut self, meta: TermId, name: &str, args: Vec<TermId>) -> TermId {
        let module = self.a.atom("erlang");
        let name = self.a.atom(name);
        let dot = self.expanded(".", meta, vec![module, name]);
        let args = self.a.list(args);
        self.a.tuple(vec![dot, meta, args])
    }
    fn boolean(&self, id: TermId) -> Result<bool> {
        if matches!(self.a.as_atom(id), Some("true" | "false")) {
            return Ok(true);
        }
        let Ok(fields) = self.fields(id, 3) else {
            return Ok(false);
        };
        if let Some(tag) = self.a.as_atom(fields[0]) {
            if tag == "__block__" {
                return self
                    .a
                    .list_items(fields[2])?
                    .last()
                    .copied()
                    .map_or(Ok(false), |x| self.boolean(x));
            }
            let args = self.a.list_items(fields[2]).unwrap_or_default();
            let opts = match tag {
                "case" if args.len() == 2 => Some(args[1]),
                "cond" if args.len() == 1 => Some(args[0]),
                _ => None,
            };
            if let Some(opts) = opts {
                let clauses = self
                    .a
                    .keyword(opts, "do")
                    .ok_or_else(|| self.error("boolean clauses"))?;
                for clause in self.a.list_items(clauses)? {
                    let f = self.fields(clause, 3)?;
                    let pair = self.a.list_items(f[2])?;
                    if pair.len() != 2 || !self.boolean(pair[1])? {
                        return Ok(false);
                    }
                }
                return Ok(true);
            }
            return Ok(false);
        }
        let dot = self.fields(fields[0], 3)?;
        if self.a.as_atom(dot[0]) != Some(".") {
            return Ok(false);
        }
        let names = self.a.list_items(dot[2])?;
        let args = self.a.list_items(fields[2])?;
        if names.len() != 2 || self.a.as_atom(names[0]) != Some("erlang") {
            return Ok(false);
        }
        let Some(name) = self.a.as_atom(names[1]) else {
            return Ok(false);
        };
        Ok(match args.len() {
            1 => matches!(
                name,
                "not"
                    | "is_atom"
                    | "is_binary"
                    | "is_bitstring"
                    | "is_boolean"
                    | "is_float"
                    | "is_function"
                    | "is_integer"
                    | "is_list"
                    | "is_number"
                    | "is_pid"
                    | "is_port"
                    | "is_reference"
                    | "is_tuple"
                    | "is_map"
                    | "is_process_alive"
            ),
            2 if matches!(name, "andalso" | "orelse") => self.boolean(args[1])?,
            2 => matches!(
                name,
                "and"
                    | "or"
                    | "xor"
                    | "=="
                    | "/="
                    | "=<"
                    | ">="
                    | "<"
                    | ">"
                    | "=:="
                    | "=/="
                    | "is_map_key"
                    | "is_function"
                    | "is_record"
            ),
            3 => matches!(name, "function_exported" | "is_record"),
            _ => false,
        })
    }
    fn cond_expr(
        &mut self,
        meta: TermId,
        opts: TermId,
        inherited: TermId,
        scope: &mut Scope,
    ) -> Result<TermId> {
        let clauses = self
            .a
            .keyword(opts, "do")
            .ok_or_else(|| self.error("cond clauses"))?;
        let clauses = self.a.list_items(clauses)?;
        let last = clauses
            .last()
            .copied()
            .ok_or_else(|| self.error("empty cond"))?;
        let fields = self.fields(last, 3)?;
        let args = self.a.list_items(fields[2])?;
        if args.len() != 2 {
            return Err(self.error("cond arrow"));
        }
        let conditions = self.a.list_items(args[0])?;
        if conditions.len() != 1 {
            return Err(self.error("cond condition"));
        }
        let mut old_meta = fields[1];
        if self
            .a
            .as_atom(conditions[0])
            .is_some_and(|name| name != "false" && name != "nil")
        {
            let yes = self.a.atom("true");
            let generated = self.a.tagged("generated", vec![yes]);
            let mut items = vec![generated];
            items.extend(self.a.list_items(old_meta)?);
            old_meta = self.a.list(items);
        }
        let reason = self.a.atom("cond_clause");
        let mut acc = self.expanded_remote(fields[1], "error", vec![reason]);
        for clause in clauses.into_iter().rev() {
            let fields = self.fields(clause, 3)?;
            let args = self.a.list_items(fields[2])?;
            if args.len() != 2 {
                return Err(self.error("cond arrow"));
            }
            let conditions = self.a.list_items(args[0])?;
            if conditions.len() != 1 {
                return Err(self.error("cond condition"));
            }
            let mut condition = conditions[0];
            let mut body = args[1];
            if self.a.tag(condition) == Some("=") {
                let f = self.fields(condition, 3)?;
                let pair = self.a.list_items(f[2])?;
                if pair.len() == 2 {
                    if let (Ok(var), Ok(returned)) = (self.fields(pair[0], 3), self.fields(body, 3))
                    {
                        if self.a.as_atom(var[0]).is_some()
                            && self.a.as_atom(var[2]).is_some()
                            && self.a.as_atom(var[0]) == self.a.as_atom(returned[0])
                            && self.a.as_atom(var[2]) == self.a.as_atom(returned[2])
                            && self.boolean(pair[1])?
                        {
                            condition = pair[1];
                            body = self.a.atom("true");
                        }
                    }
                }
            }
            let (truthy, other) = if self.boolean(condition)? {
                (self.a.atom("true"), self.a.atom("false"))
            } else {
                let version = -(scope.names.len() as i64 + 1);
                let name = self.build(scope, "_");
                scope.names.insert(version, name);
                let version = self.a.integer(version);
                let pair = self.a.tagged("version", vec![version]);
                let mut items = vec![pair];
                items.extend(self.a.list_items(fields[1])?);
                let vmeta = self.a.list(items);
                let vname = self.a.atom("var");
                let context = self.a.atom("elixir_erl_var");
                let var = self.a.tuple(vec![vname, vmeta, context]);
                let empty = self.a.nil();
                let nil = self.a.atom("nil");
                let no = self.a.atom("false");
                let nonnil = self.expanded_remote(empty, "/=", vec![var, nil]);
                let nonfalse = self.expanded_remote(empty, "/=", vec![var, no]);
                let guard = self.expanded_remote(empty, "andalso", vec![nonnil, nonfalse]);
                let pattern = self.expanded("when", empty, vec![var, guard]);
                let underscore = self.a.atom("_");
                let wildcard = self.a.tuple(vec![underscore, empty, nil]);
                (pattern, wildcard)
            };
            let truthy = self.a.list(vec![truthy]);
            let first = self.expanded("->", fields[1], vec![truthy, body]);
            let other = self.a.list(vec![other]);
            let second = self.expanded("->", old_meta, vec![other, acc]);
            let clauses = self.a.list(vec![first, second]);
            let entry = self.a.tagged("do", vec![clauses]);
            let opts = self.a.list(vec![entry]);
            acc = self.expanded("case", fields[1], vec![condition, opts]);
            old_meta = fields[1];
        }
        let mut fields = self.fields(acc, 3)?;
        fields[1] = meta;
        let case = self.a.tuple(fields);
        self.expr(case, inherited, scope)
    }
    fn remote_call(&mut self, ann: TermId, module: &str, name: &str, args: Vec<TermId>) -> TermId {
        let module = self.atom_node(ann, module);
        let name = self.atom_node(ann, name);
        let remote = self.node("remote", ann, vec![module, name]);
        let args = self.a.list(args);
        self.node("call", ann, vec![remote, args])
    }
    fn simple_clause(&mut self, ann: TermId, pattern: TermId, body: TermId) -> TermId {
        let args = self.a.list(vec![pattern]);
        let guards = self.a.nil();
        let body = self.a.list(vec![body]);
        self.node("clause", ann, vec![args, guards, body])
    }
    fn dynamic_zero(
        &mut self,
        left: TermId,
        name: &str,
        meta: TermId,
        ann: TermId,
        scope: &mut Scope,
    ) -> Result<TermId> {
        let translated = self.expr(left, ann, scope)?;
        let key = self.atom_node(ann, name);
        if scope.guarding && matches!(self.a.get(left), ErlangTerm::Tuple(_)) {
            return Ok(self.remote_call(ann, "erlang", "map_get", vec![key, translated]));
        }
        let generated = self.generated(self.location(ann));
        let inner_name = self.build(scope, "_");
        let inner = self.var_node(generated, &inner_name);
        let name = self.build(scope, "_");
        let var = self.var_node(generated, &name);
        let field = self.node("map_field_exact", ann, vec![key, var]);
        let fields = self.a.list(vec![field]);
        let map = self.node("map", ann, vec![fields]);
        let no_parens = self
            .a
            .keyword(meta, "no_parens")
            .and_then(|x| self.a.as_atom(x))
            == Some("true");
        let (first, second) = if no_parens {
            let call = self.remote_call(
                generated,
                "elixir_erl_pass",
                "no_parens_remote",
                vec![var, key],
            );
            let mut branches = Vec::new();
            for tag in ["ok", "error"] {
                let tag_atom = self.atom_node(generated, tag);
                let fields = self.a.list(vec![tag_atom, inner]);
                let tuple = self.node("tuple", generated, vec![fields]);
                let body = if tag == "ok" {
                    inner
                } else {
                    self.remote_call(ann, "erlang", "error", vec![inner])
                };
                branches.push(self.simple_clause(generated, tuple, body));
            }
            let branches = self.a.list(branches);
            let body = self.node("case", generated, vec![call, branches]);
            (
                self.simple_clause(generated, map, var),
                self.simple_clause(generated, var, body),
            )
        } else {
            let first = self.remote_call(
                generated,
                "elixir_erl_pass",
                "parens_map_field",
                vec![key, var],
            );
            let remote = self.node("remote", generated, vec![var, key]);
            let args = self.a.nil();
            let second = self.node("call", generated, vec![remote, args]);
            (
                self.simple_clause(generated, map, first),
                self.simple_clause(generated, var, second),
            )
        };
        let clauses = self.a.list(vec![first, second]);
        Ok(self.node("case", generated, vec![translated, clauses]))
    }
    fn struct_expr(
        &mut self,
        ann: TermId,
        name: TermId,
        map: TermId,
        scope: &mut Scope,
    ) -> Result<TermId> {
        let fields = self.fields(map, 3)?;
        if self.a.as_atom(fields[0]) != Some("%{}") {
            return Err(self.error("struct map"));
        }
        let mut pairs = self.a.list_items(fields[2])?;
        let static_name = self.a.as_atom(name).is_some();
        if pairs.len() == 1 && self.a.tag(pairs[0]) == Some("|") {
            if !static_name {
                return Err(self.error("dynamic struct update"));
            }
            let pipe = self.fields(pairs[0], 3)?;
            let args = self.a.list_items(pipe[2])?;
            if args.len() != 2 {
                return Err(self.error("struct update"));
            }
            let generated = self.generated(self.location(ann));
            let var_name = self.build(scope, "_");
            let var = self.var_node(ann, &var_name);
            let key = self.atom_node(ann, "__struct__");
            let name_expr = self.literal(name, ann)?;
            let field = self.node("map_field_exact", ann, vec![key, name_expr]);
            let fields = self.a.list(vec![field]);
            let map = self.node("map", ann, vec![fields]);
            let pattern = self.node("match", ann, vec![var, map]);
            let bad = self.atom_node(ann, "badstruct");
            let reason = self.a.list(vec![bad, name_expr, var]);
            let reason = self.node("tuple", ann, vec![reason]);
            let value = self.expr(args[0], ann, scope)?;
            let mut translated = Vec::new();
            for pair in self.a.list_items(args[1])? {
                let pair = self.fields(pair, 2)?;
                let key = self.expr(pair[0], ann, scope)?;
                let val = self.expr(pair[1], ann, scope)?;
                translated.push(self.node("map_field_exact", ann, vec![key, val]));
            }
            let fields = self.a.list(translated);
            let update = self.node("map", ann, vec![var, fields]);
            let first = self.simple_clause(ann, pattern, update);
            let error = self.remote_call(ann, "erlang", "error", vec![reason]);
            let second = self.simple_clause(generated, var, error);
            let clauses = self.a.list(vec![first, second]);
            return Ok(self.node("case", generated, vec![value, clauses]));
        }
        let key = self.a.atom("__struct__");
        let pair = self.a.tuple(vec![key, name]);
        pairs.insert(0, pair);
        let result = self.map_expr(ann, pairs, scope)?;
        if !static_name {
            // Expanded variable struct names occur only in patterns. OTP binds an
            // extra anonymous variable and adds is_atom to the clause guards.
            if !scope.matching {
                return Err(self.error("dynamic struct outside pattern"));
            }
            let mut map = self.fields(result, 3)?;
            let mut fields = self.a.list_items(map[2])?;
            let mut field = self.fields(fields[0], 4)?;
            if self.a.as_atom(field[0]) != Some("map_field_exact") {
                return Err(self.error("dynamic struct map-key pattern"));
            }
            let generated = self.generated(self.location(ann));
            let var_name = self.build(scope, "_");
            let var = self.var_node(generated, &var_name);
            let matched = self.node("match", generated, vec![var, field[3]]);
            field[3] = matched;
            fields[0] = self.a.tuple(field);
            let guard = self.remote_call(generated, "erlang", "is_atom", vec![var]);
            scope.extra_guards.insert(0, guard);
            map[2] = self.a.list(fields);
            return Ok(self.a.tuple(map));
        }
        Ok(result)
    }
    fn basic(&self, id: TermId) -> Result<bool> {
        Ok(match self.a.get(id) {
            ErlangTerm::Atom(_)
            | ErlangTerm::Integer(_)
            | ErlangTerm::Float(_)
            | ErlangTerm::Binary(_)
            | ErlangTerm::Nil => true,
            ErlangTerm::List { .. } => self.basic_args(id)?,
            ErlangTerm::Tuple(x) if x.len() == 2 => self.basic(x[0])? && self.basic(x[1])?,
            _ => false,
        })
    }
    fn basic_args(&self, id: TermId) -> Result<bool> {
        for arg in self.a.list_items(id)? {
            if !self.basic(arg)? {
                return Ok(false);
            }
        }
        Ok(true)
    }
    fn bit_types(&mut self, id: TermId, out: &mut Vec<TermId>) -> Result<()> {
        let fields = self.fields(id, 3)?;
        match self.a.as_atom(fields[0]) {
            Some("-") => {
                let args = self.a.list_items(fields[2])?;
                if args.len() != 2 {
                    return Err(self.error("binary type combination"));
                }
                self.bit_types(args[0], out)?;
                self.bit_types(args[1], out)?;
            }
            Some("unit") => {
                let args = self.a.list_items(fields[2])?;
                if args.len() != 1 {
                    return Err(self.error("binary unit"));
                }
                out.push(self.a.tagged("unit", vec![args[0]]));
            }
            Some(_name) if self.a.as_atom(fields[2]) == Some("nil") => out.push(fields[0]),
            _ => return Err(self.error("binary type specifier")),
        }
        Ok(())
    }
    fn bitstring(&mut self, ann: TermId, args: Vec<TermId>, scope: &mut Scope) -> Result<TermId> {
        let mut elements = Vec::new();
        for arg in args {
            if self.a.tag(arg) != Some("::") {
                return Err(self.error("expanded binary segment without ::"));
            }
            let fields = self.fields(arg, 3)?;
            let args = self.a.list_items(fields[2])?;
            if args.len() != 2 {
                return Err(self.error("binary segment"));
            }
            let mut spec = args[1];
            let mut size = None;
            if self.a.tag(spec) == Some("-") {
                let f = self.fields(spec, 3)?;
                let pair = self.a.list_items(f[2])?;
                if pair.len() == 2 && self.a.tag(pair[1]) == Some("size") {
                    spec = pair[0];
                    size = Some(pair[1]);
                }
            } else if self.a.tag(spec) == Some("size") {
                size = Some(spec);
            }
            let mut types = Vec::new();
            if size != Some(spec) {
                self.bit_types(spec, &mut types)?;
            }
            let size = if let Some(size) = size {
                let f = self.fields(size, 3)?;
                let args = self.a.list_items(f[2])?;
                if args.len() != 1 {
                    return Err(self.error("binary size"));
                }
                let meta_ann = self.annotation(fields[1])?;
                let mut size_scope = scope.clone();
                size_scope.guarding = true;
                size_scope.matching = false;
                size_scope.map_key = false;
                size_scope.pin_guard = false;
                self.expr(args[0], meta_ann, &mut size_scope)?
            } else {
                self.a.atom("default")
            };
            let mut default_types = false;
            let value = if let ErlangTerm::Binary(bytes) = self.a.get(args[0]).clone() {
                if self.a.as_atom(size) == Some("default") {
                    let utf = !matches!(
                        types.first().and_then(|x| self.a.as_atom(*x)),
                        Some("binary" | "bitstring")
                    );
                    let chars = if utf {
                        std::str::from_utf8(&bytes)
                            .map_err(|_| self.error("UTF-8 binary segment"))?
                            .chars()
                            .map(|c| self.a.integer(c as u32))
                            .collect()
                    } else {
                        bytes.into_iter().map(|c| self.a.integer(c)).collect()
                    };
                    if !utf {
                        types.clear();
                        default_types = true;
                    }
                    let chars = self.a.list(chars);
                    let zero = self.a.integer(0);
                    self.node("string", zero, vec![chars])
                } else {
                    self.expr(args[0], ann, scope)?
                }
            } else {
                self.expr(args[0], ann, scope)?
            };
            let types = if default_types {
                self.a.atom("default")
            } else {
                self.a.list(types)
            };
            elements.push(self.node("bin_element", ann, vec![value, size, types]));
        }
        let elements = self.a.list(elements);
        Ok(self.node("bin", ann, vec![elements]))
    }
    fn map_expr(
        &mut self,
        mut ann: TermId,
        mut args: Vec<TermId>,
        scope: &mut Scope,
    ) -> Result<TermId> {
        let mut base = None;
        if args.len() == 1 && self.a.tag(args[0]) == Some("|") {
            let t = self.fields(args[0], 3)?;
            let update = self.a.list_items(t[2])?;
            if update.len() != 2 {
                return Err(self.error("map update"));
            }
            base = Some(self.expr(update[0], ann, scope)?);
            ann = self.annotation(t[1])?;
            args = self.a.list_items(update[1])?;
        }
        let mut pairs = Vec::new();
        for arg in args {
            let pair = self.fields(arg, 2)?;
            let saved = scope.map_key;
            scope.map_key = true;
            let key = self.expr(pair[0], ann, scope);
            scope.map_key = saved;
            let key = key?;
            let value = self.expr(pair[1], ann, scope)?;
            pairs.push(self.node(
                if !scope.map_key && (base.is_some() || scope.matching) {
                    "map_field_exact"
                } else {
                    "map_field_assoc"
                },
                ann,
                vec![key, value],
            ));
        }
        let pairs = self.a.list(pairs);
        Ok(self.node(
            "map",
            ann,
            match base {
                Some(base) => vec![base, pairs],
                None => vec![pairs],
            },
        ))
    }
    fn capture(&mut self, arg: TermId, ann: TermId, scope: &mut Scope) -> Result<TermId> {
        let t = self.fields(arg, 3)?;
        if self.a.as_atom(t[0]) != Some("/") {
            return Err(self.error("capture slash"));
        }
        let args = self.a.list_items(t[2])?;
        if args.len() != 2 {
            return Err(self.error("capture arity"));
        }
        let target = self.fields(args[0], 3)?;
        let function = if let Some(name) = self.a.as_atom(target[0]).map(str::to_owned) {
            let name = self.a.atom(name);
            self.a.tagged("function", vec![name, args[1]])
        } else {
            let dot = self.fields(target[0], 3)?;
            if self.a.as_atom(dot[0]) != Some(".") {
                return Err(self.error("remote capture"));
            }
            let names = self.a.list_items(dot[2])?;
            if names.len() != 2 {
                return Err(self.error("remote capture target"));
            }
            let module = self.expr(names[0], ann, scope)?;
            let name = self.literal(names[1], ann)?;
            let arity = self.literal(args[1], ann)?;
            self.a.tagged("function", vec![module, name, arity])
        };
        Ok(self.node("fun", ann, vec![function]))
    }
    fn extract_guards(&self, condition: TermId) -> Result<(TermId, Vec<TermId>)> {
        if self.a.tag(condition) != Some("when") {
            return Ok((condition, vec![]));
        }
        let t = self.fields(condition, 3)?;
        let args = self.a.list_items(t[2])?;
        if args.len() != 2 {
            return Err(self.error("guard when"));
        }
        let mut guards = Vec::new();
        let mut current = args[1];
        while self.a.tag(current) == Some("when") {
            let t = self.fields(current, 3)?;
            let args = self.a.list_items(t[2])?;
            if args.len() != 2 {
                return Err(self.error("guard alternatives"));
            }
            guards.push(args[0]);
            current = args[1];
        }
        guards.push(current);
        Ok((args[0], guards))
    }
    fn clause(
        &mut self,
        ann: TermId,
        args: Vec<TermId>,
        guards: Vec<TermId>,
        body: TermId,
        scope: &mut Scope,
        fn_match: bool,
    ) -> Result<TermId> {
        let old = scope.matching;
        let old_pin = scope.pin_guard;
        if fn_match {
            scope.pin_guard = true;
        }
        scope.matching = true;
        let mut patterns = Vec::new();
        for arg in args {
            patterns.push(self.expr(arg, ann, scope)?);
        }
        scope.matching = old;
        scope.pin_guard = old_pin;
        let patterns = self.a.list(patterns);
        let extra = std::mem::take(&mut scope.extra_guards);
        let mut translated = Vec::new();
        for guard in guards {
            let mut guard_scope = scope.clone();
            guard_scope.guarding = true;
            let guard = self.expr(guard, ann, &mut guard_scope)?;
            let mut all = vec![guard];
            all.extend(extra.iter().copied());
            translated.push(self.a.list(all));
        }
        if translated.is_empty() && !extra.is_empty() {
            translated.push(self.a.list(extra));
        }
        let guards = self.a.list(translated);
        let body = self.expr(body, ann, scope)?;
        let body = self.unblock(body)?;
        let body = self.a.list(body);
        Ok(self.node("clause", ann, vec![patterns, guards, body]))
    }
    fn arrow_clauses(
        &mut self,
        id: TermId,
        scope: &mut Scope,
        generated: bool,
        splat: bool,
    ) -> Result<TermId> {
        let mut clauses = Vec::new();
        for item in self.a.list_items(id)? {
            let t = self.fields(item, 3)?;
            if self.a.as_atom(t[0]) != Some("->") {
                return Err(self.error("arrow clause"));
            }
            let args = self.a.list_items(t[2])?;
            if args.len() != 2 {
                return Err(self.error("arrow arguments"));
            }
            let mut patterns = self.a.list_items(args[0])?;
            let mut guards = Vec::new();
            if patterns.len() == 1 {
                if splat && self.a.tag(patterns[0]) == Some("when") {
                    let when = self.fields(patterns[0], 3)?;
                    let mut all = self.a.list_items(when[2])?;
                    let mut guard = all.pop().ok_or_else(|| self.error("fn guard"))?;
                    while self.a.tag(guard) == Some("when") {
                        let fields = self.fields(guard, 3)?;
                        let pair = self.a.list_items(fields[2])?;
                        if pair.len() != 2 {
                            return Err(self.error("fn alternative guard"));
                        }
                        guards.push(pair[0]);
                        guard = pair[1];
                    }
                    guards.push(guard);
                    patterns = all;
                } else {
                    let (pattern, found) = self.extract_guards(patterns[0])?;
                    patterns = vec![pattern];
                    guards = found;
                }
            }
            let mut ann = self.annotation(t[1])?;
            if generated {
                ann = self.generated(self.location(ann));
            }
            clauses.push(self.clause(ann, patterns, guards, args[1], scope, splat)?);
        }
        Ok(self.a.list(clauses))
    }
    fn attr(&mut self, ann: TermId, name: &str, value: TermId) -> TermId {
        let name = self.a.atom(name);
        self.node("attribute", ann, vec![name, value])
    }
    fn reflection(
        &mut self,
        line: TermId,
        module: TermId,
        defs: &[TermId],
        macros: &[TermId],
        deprecated: TermId,
        structure: TermId,
        digest: Vec<u8>,
    ) -> Result<(TermId, TermId)> {
        let info = self.a.atom("__info__");
        let one = self.a.integer(1);
        let name_arity = self.a.tuple(vec![info, one]);
        let allowed = [
            "attributes",
            "compile",
            "functions",
            "macros",
            "md5",
            "exports_md5",
            "module",
            "deprecated",
            "struct",
        ];
        let allowed = allowed
            .iter()
            .map(|name| self.atom_node(line, name))
            .collect();
        let allowed = self.a.list(allowed);
        let union = self.a.atom("union");
        let union = self.node("type", line, vec![union, allowed]);
        let args = self.a.list(vec![union]);
        let product = self.a.atom("product");
        let product = self.node("type", line, vec![product, args]);
        let any = self.a.atom("any");
        let nil = self.a.nil();
        let any = self.node("type", line, vec![any, nil]);
        let values = self.a.list(vec![product, any]);
        let fun = self.a.atom("fun");
        let fun = self.node("type", line, vec![fun, values]);
        let funs = self.a.list(vec![fun]);
        let spec = self.a.tuple(vec![name_arity, funs]);
        let spec = self.attr(line, "spec", spec);
        let zero = self.a.integer(0);
        let mut clauses = Vec::new();
        let functions = self.a.list(defs.to_vec());
        let macros = self.a.list(macros.to_vec());
        let digest = self.a.push(ErlangTerm::Binary(digest));
        for (name, value) in [
            ("module", module),
            ("functions", functions),
            ("macros", macros),
            ("struct", structure),
            ("exports_md5", digest),
        ] {
            let arg = self.atom_node(zero, name);
            let args = self.a.list(vec![arg]);
            let guards = self.a.nil();
            let body = self.literal(value, zero)?;
            let body = self.a.list(vec![body]);
            clauses.push(self.node("clause", zero, vec![args, guards, body]));
        }
        for name in ["attributes", "compile", "md5"] {
            let key = self.var_node(zero, "Key");
            let literal = self.atom_node(zero, name);
            let pattern = self.node("match", zero, vec![key, literal]);
            let args = self.a.list(vec![pattern]);
            let guards = self.a.nil();
            let erlang = self.atom_node(zero, "erlang");
            let get = self.atom_node(zero, "get_module_info");
            let remote = self.node("remote", zero, vec![erlang, get]);
            let module_expr = self.literal(module, zero)?;
            let call_args = self.a.list(vec![module_expr, key]);
            let call = self.node("call", zero, vec![remote, call_args]);
            let body = self.a.list(vec![call]);
            clauses.push(self.node("clause", zero, vec![args, guards, body]));
        }
        let arg = self.atom_node(zero, "deprecated");
        let args = self.a.list(vec![arg]);
        let guards = self.a.nil();
        let body = self.literal(deprecated, zero)?;
        let body = self.a.list(vec![body]);
        clauses.push(self.node("clause", zero, vec![args, guards, body]));
        let clauses = self.a.list(clauses);
        let function = self.node("function", zero, vec![info, one, clauses]);
        Ok((spec, function))
    }
}
fn inline_pure(module: &str, name: &str) -> bool {
    match module {
        "Elixir.Duration" => name == "new!",
        "Elixir.MapSet" => name == "new",
        "Elixir.String" => matches!(name, "length" | "graphemes" | "codepoints" | "split"),
        "Elixir.Kernel" => name == "to_timeout",
        "Elixir.URI" => matches!(
            name,
            "new"
                | "new!"
                | "parse"
                | "encode_query"
                | "encode_www_form"
                | "decode"
                | "decode_www_for"
        ),
        "Elixir.Version" => matches!(
            name,
            "parse" | "parse!" | "parse_requirement" | "parse_requirement!"
        ),
        _ => false,
    }
}

fn is_operator(name: &str, arity: usize) -> bool {
    match arity {
        1 => matches!(name, "+" | "-" | "bnot" | "not"),
        2 => matches!(
            name,
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
                | "and"
                | "or"
                | "xor"
                | "andalso"
                | "orelse"
                | "=="
                | "/="
                | "=<"
                | "<"
                | ">="
                | ">"
                | "=:="
                | "=/="
        ),
        _ => false,
    }
}

pub fn lift(debug: &ErlangTerms) -> Result<ErlangTerms> {
    let mut l = Lift {
        a: Arena::from_terms(debug),
        module: "?".into(),
        function: "metadata".into(),
        depth: 0,
    };
    let wrapper = l.fields(debug.root, 3)?;
    if l.a.as_atom(wrapper[0]) != Some("debug_info_v1")
        || l.a.as_atom(wrapper[1]) != Some("elixir_erl")
    {
        return Err(l.error("debug_info_v1/elixir_erl"));
    }
    let payload = l.fields(wrapper[2], 3)?;
    if l.a.as_atom(payload[0]) != Some("elixir_v1") {
        return Err(l.error("elixir_v1"));
    }
    let map = payload[1];
    let module = l.required(map, "module")?;
    l.module = l.atom(module)?;
    let line = match l.a.map_get(map, "anno") {
        Some(ann) => {
            let loc = l.location(ann);
            match l.a.get(loc) {
                ErlangTerm::Tuple(parts) => parts[0],
                _ => loc,
            }
        }
        None => l.required(map, "line")?,
    };
    let module_line = l.a.as_integer(line).ok_or_else(|| l.error("module line"))?;
    let relative = l.required(map, "relative_file")?;
    let relative = l.chars(relative)?;
    let location = l.a.tuple(vec![relative, line]);
    let mut forms = vec![
        l.attr(line, "file", location),
        l.attr(line, "module", module),
    ];
    let compile = l.required(map, "compile_opts")?;
    let mut options = vec![l.a.atom("no_auto_import")];
    for option in l.a.list_items(compile)? {
        let name = l.a.as_atom(option).or_else(|| {
            l.a.tuple_fields(option)
                .ok()
                .and_then(|x| x.first())
                .and_then(|x| l.a.as_atom(*x))
        });
        if !matches!(name, Some("debug_info" | "no_warn_undefined")) {
            options.push(option);
        }
    }
    let options = l.a.list(options);
    forms.push(l.attr(line, "compile", options));
    forms.extend(l.a.list_items(payload[2])?);
    let definitions = l.required(map, "definitions")?;
    let unreachable = l.required(map, "unreachable")?;
    let unreachable = l.a.list_items(unreachable)?;
    let mut head = Vec::new();
    let mut tail = Vec::<Vec<TermId>>::new();
    let mut defs = Vec::<(String, i64, TermId)>::new();
    let mut macros = Vec::<(String, i64, TermId)>::new();
    let mut exports = Vec::<(String, i64, TermId)>::new();
    for definition in l.a.list_items(definitions)? {
        let definition = l.fields(definition, 4)?;
        let na = l.fields(definition[0], 2)?;
        let name = l.atom(na[0])?;
        let arity =
            l.a.as_integer(na[1])
                .ok_or_else(|| l.error("function arity"))?;
        if unreachable.iter().any(|id| {
            let Ok(t) = l.fields(*id, 2) else {
                return false;
            };
            l.a.as_atom(t[0]) == Some(name.as_str()) && l.a.as_integer(t[1]) == Some(arity)
        }) {
            continue;
        }
        let kind = l.atom(definition[1])?;
        if !matches!(kind.as_str(), "def" | "defp" | "defmacro" | "defmacrop") {
            return Err(l.error("definition kind"));
        }
        l.function = format!("{name}/{arity}");
        let macro_kind = kind == "defmacro" || kind == "defmacrop";
        let mut clauses = Vec::new();
        for clause in l.a.list_items(definition[3])? {
            let clause = l.fields(clause, 4)?;
            let clause_line =
                l.a.keyword(clause[0], "line")
                    .and_then(|id| l.a.as_integer(id))
                    .unwrap_or(0);
            let ann = if clause_line == module_line {
                let zero = l.a.integer(0);
                l.generated(zero)
            } else {
                l.annotation(clause[0])?
            };
            let mut scope = Scope::default();
            let args = l.a.list_items(clause[1])?;
            let guards = l.a.list_items(clause[2])?;
            let mut result = l.clause(ann, args, guards, clause[3], &mut scope, false)?;
            if macro_kind {
                let mut fields = l.fields(result, 5)?;
                let caller = l.var_node(ann, "_@CALLER");
                let mut args = vec![caller];
                args.extend(l.a.list_items(fields[2])?);
                fields[2] = l.a.list(args);
                if scope.caller {
                    let target = l.var_node(ann, "__CALLER__");
                    let env = l.atom_node(ann, "elixir_env");
                    let name = l.atom_node(ann, "to_caller");
                    let remote = l.node("remote", ann, vec![env, name]);
                    let args = l.a.list(vec![caller]);
                    let call = l.node("call", ann, vec![remote, args]);
                    let assign = l.node("match", ann, vec![target, call]);
                    let mut body = vec![assign];
                    body.extend(l.a.list_items(fields[4])?);
                    fields[4] = l.a.list(body);
                }
                result = l.a.tuple(fields);
            }
            clauses.push(result);
        }
        let ann = l.annotation(definition[2])?;
        let erl_name = if macro_kind {
            format!("MACRO-{name}")
        } else {
            name.clone()
        };
        let erl_name_term = l.a.atom(&erl_name);
        let erl_arity = arity + i64::from(macro_kind);
        let erl_arity_term = l.a.integer(erl_arity);
        let clauses = l.a.list(clauses);
        let body = l.node(
            "function",
            ann,
            vec![erl_name_term, erl_arity_term, clauses],
        );
        if kind == "def" {
            defs.push((name.clone(), arity, definition[0]));
        } else if kind == "defmacro" {
            macros.push((name.clone(), arity, definition[0]));
        }
        if kind == "def" || kind == "defmacro" {
            let na = l.a.tuple(vec![erl_name_term, erl_arity_term]);
            exports.push((erl_name, erl_arity, na));
        }
        if let Some(file) = l.a.keyword(definition[2], "file") {
            let file = l.fields(file, 2)?;
            let chars = l.chars(file[0])?;
            let value = l.a.tuple(vec![chars, file[1]]);
            let location = l.location(ann);
            tail.push(vec![l.attr(location, "file", value), body]);
        } else {
            head.push(body);
        }
    }
    defs.sort_by(|a, b| (&a.0, a.1).cmp(&(&b.0, b.1)));
    macros.sort_by(|a, b| (&a.0, a.1).cmp(&(&b.0, b.1)));
    let defs: Vec<_> = defs.into_iter().map(|x| x.2).collect();
    let macros: Vec<_> = macros.into_iter().map(|x| x.2).collect();
    let deprecated = l.required(map, "deprecated")?;
    let structure =
        l.a.map_get(map, "struct")
            .unwrap_or_else(|| l.a.atom("nil"));
    let defs_term = l.a.list(defs.clone());
    let macros_term = l.a.list(macros.clone());
    // Pinned elixir_module:exports_md5 compatibility; verified by stage oracle.
    let digest_input = l.a.tuple(vec![defs_term, macros_term, structure]);
    let encoded = crate::external_term::encode(
        &l.a.snapshot(digest_input),
        crate::external_term::Limits::default(),
        false,
    )
    .map_err(|e| e.to_string())?;
    let digest = crate::fingerprint::md5(&encoded).to_vec();
    let (spec, info) = l.reflection(line, module, &defs, &macros, deprecated, structure, digest)?;
    let info_name = l.a.atom("__info__");
    let one = l.a.integer(1);
    let info_na = l.a.tuple(vec![info_name, one]);
    exports.push(("__info__".into(), 1, info_na));
    exports.sort_by(|a, b| (&a.0, a.1).cmp(&(&b.0, b.1)));
    exports.dedup_by(|a, b| a.0 == b.0 && a.1 == b.1);
    let exports = l.a.list(exports.into_iter().map(|x| x.2).collect());
    let attributes = l.required(map, "attributes")?;
    for attribute in l.a.list_items(attributes)? {
        let pair = l.fields(attribute, 2)?;
        let name = l.atom(pair[0])?;
        forms.push(l.attr(line, &name, pair[1]));
    }
    forms.push(l.attr(line, "export", exports));
    forms.extend([spec, info]);
    head.reverse();
    forms.extend(head);
    for block in tail.into_iter().rev() {
        forms.extend(block);
    }
    let root = l.a.list(forms);
    Ok(l.a.finish(root))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hygienic_versions_are_stable_and_distinct() {
        let mut l = Lift {
            a: Arena::default(),
            module: "M".into(),
            function: "f/0".into(),
            depth: 0,
        };
        let v0 = l.a.integer(0);
        let v1 = l.a.integer(1);
        let meta0 = l.a.tagged("version", vec![v0]);
        let meta0 = l.a.list(vec![meta0]);
        let meta1 = l.a.tagged("version", vec![v1]);
        let meta1 = l.a.list(vec![meta1]);
        let mut scope = Scope::default();
        let first = l.variable(meta0, "x", "nil", &mut scope).unwrap();
        let again = l.variable(meta0, "other", "nil", &mut scope).unwrap();
        let second = l.variable(meta1, "x", "nil", &mut scope).unwrap();
        assert_eq!(l.a.as_atom(l.fields(first, 3).unwrap()[2]), Some("_x@1"));
        assert_eq!(l.a.as_atom(l.fields(again, 3).unwrap()[2]), Some("_x@1"));
        assert_eq!(l.a.as_atom(l.fields(second, 3).unwrap()[2]), Some("_x@2"));
    }
    #[test]
    fn negative_zero_is_an_explicit_unary_minus() {
        let mut l = Lift {
            a: Arena::default(),
            module: "M".into(),
            function: "f/0".into(),
            depth: 0,
        };
        let value = l.a.push(ErlangTerm::Float(1 << 63));
        let zero = l.a.integer(0);
        let result = l.literal(value, zero).unwrap();
        let fields = l.fields(result, 4).unwrap();
        assert_eq!(l.a.tag(result), Some("op"));
        assert_eq!(l.a.as_atom(fields[2]), Some("-"));
    }
    fn translator() -> Lift {
        Lift {
            a: Arena::default(),
            module: "M".into(),
            function: "f/1".into(),
            depth: 0,
        }
    }
    #[test]
    fn literal_map_pairs_follow_erlang_term_order() {
        let mut l = translator();
        let zero = l.a.integer(0);
        let atom = l.a.atom("key");
        let number = l.a.integer(2);
        let value = l.a.atom("value");
        let map =
            l.a.push(ErlangTerm::Map(vec![(atom, value), (number, value)]));
        let translated = l.literal(map, zero).unwrap();
        let fields = l.fields(translated, 3).unwrap();
        let pairs = l.a.list_items(fields[2]).unwrap();
        let first = l.fields(pairs[0], 4).unwrap();
        let second = l.fields(pairs[1], 4).unwrap();
        assert_eq!(l.a.tag(first[2]), Some("integer"));
        assert_eq!(l.a.tag(second[2]), Some("atom"));
    }
    #[test]
    fn fn_pin_adds_guard_and_restores_body_context() {
        let mut l = translator();
        let zero = l.a.integer(0);
        let nil = l.a.nil();
        let version = l.a.tagged("version", vec![zero]);
        let meta = l.a.list(vec![version]);
        let name = l.a.atom("x");
        let kind = l.a.atom("nil");
        let variable = l.a.tuple(vec![name, meta, kind]);
        let pin = l.expanded("^", nil, vec![variable]);
        let mut scope = Scope::default();
        l.variable(meta, "x", "nil", &mut scope).unwrap();
        let clause = l
            .clause(zero, vec![pin], vec![], variable, &mut scope, true)
            .unwrap();
        let fields = l.fields(clause, 5).unwrap();
        let args = l.a.list_items(fields[2]).unwrap();
        let guards = l.a.list_items(fields[3]).unwrap();
        let pattern = l.fields(args[0], 3).unwrap();
        assert_eq!(l.a.as_atom(pattern[2]), Some("_@1"));
        let guard = l.a.list_items(guards[0]).unwrap();
        let guard = l.fields(guard[0], 5).unwrap();
        assert_eq!(l.a.as_atom(guard[2]), Some("=:="));
        assert!(!scope.pin_guard);
        assert!(scope.extra_guards.is_empty());
        let body = l.a.list_items(fields[4]).unwrap();
        let body = l.fields(body[0], 3).unwrap();
        assert_eq!(l.a.as_atom(body[2]), Some("_x@1"));
    }
    #[test]
    fn unsupported_construct_has_module_function_and_tag() {
        let mut l = translator();
        let nil = l.a.nil();
        let unsupported = l.expanded("for", nil, vec![]);
        let error = l.expr(unsupported, nil, &mut Scope::default()).unwrap_err();
        assert!(error.contains("M:f/1"));
        assert!(error.contains("for"));
    }
    #[test]
    fn underscore_match_uses_operator_annotation() {
        let mut l = translator();
        let line = l.a.integer(12);
        let five = l.a.integer(5);
        let seven = l.a.integer(7);
        let line_meta = l.a.tagged("line", vec![line]);
        let left_column = l.a.tagged("column", vec![five]);
        let op_column = l.a.tagged("column", vec![seven]);
        let left_meta = l.a.list(vec![line_meta, left_column]);
        let op_meta = l.a.list(vec![line_meta, op_column]);
        let name = l.a.atom("_");
        let nil = l.a.atom("nil");
        let pattern = l.a.tuple(vec![name, left_meta, nil]);
        let right = l.a.atom("ok");
        let expr = l.expanded("=", op_meta, vec![pattern, right]);
        let ann = l.a.integer(0);
        let result = l.expr(expr, ann, &mut Scope::default()).unwrap();
        let fields = l.fields(result, 4).unwrap();
        let var = l.fields(fields[2], 3).unwrap();
        let location = l.fields(var[1], 2).unwrap();
        assert_eq!(l.a.as_integer(location[1]), Some(7));
    }
    #[test]
    fn final_nested_block_remains_nested() {
        let mut l = translator();
        let nil = l.a.nil();
        let first = l.a.atom("first");
        let last = l.a.atom("last");
        let nested = l.expanded("__block__", nil, vec![first, last]);
        let outer = l.expanded("__block__", nil, vec![first, nested]);
        let zero = l.a.integer(0);
        let result = l.expr(outer, zero, &mut Scope::default()).unwrap();
        let fields = l.fields(result, 3).unwrap();
        let body = l.a.list_items(fields[2]).unwrap();
        assert_eq!(body.len(), 2);
        assert_eq!(l.a.tag(body[1]), Some("block"));
    }
}
