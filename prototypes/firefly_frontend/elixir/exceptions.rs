// SPDX-License-Identifier: Apache-2.0
use super::*;
impl Lower {
    pub(super) fn op(&mut self, ann: Id, name: &str, args: Vec<Id>) -> Id {
        let name = self.atom(name);
        let mut fields = vec![name];
        fields.extend(args);
        self.ast("op", ann, fields)
    }
    pub(super) fn atom_expr(&mut self, ann: Id, name: &str) -> Id {
        let name = self.atom(name);
        self.ast("atom", ann, vec![name])
    }
    pub(super) fn rescue_guard(
        &mut self,
        ann: Id,
        var: Id,
        alias: &str,
        stack_only: bool,
    ) -> Option<Id> {
        let direct = match alias {
            "Elixir.UndefinedFunctionError" => Some("undef"),
            "Elixir.FunctionClauseError" => Some("function_clause"),
            "Elixir.SystemLimitError" => Some("system_limit"),
            "Elixir.ArithmeticError" => Some("badarith"),
            "Elixir.CondClauseError" => Some("cond_clause"),
            _ => None,
        };
        if let Some(tag) = direct {
            let tag = self.atom_expr(ann, tag);
            return Some(self.op(ann, "==", vec![var, tag]));
        }
        if alias == "Elixir.ErlangError" {
            let map = self.remote(ann, "erlang", "is_map", vec![var]);
            let key = self.atom_expr(ann, "__exception__");
            let key = self.remote(ann, "erlang", "is_map_key", vec![key, var]);
            let both = self.op(ann, "andalso", vec![map, key]);
            return Some(self.op(ann, "not", vec![both]));
        }
        let (tag, size) = match alias {
            "Elixir.BadArityError" => ("badarity", 2),
            "Elixir.BadFunctionError" => ("badfun", 2),
            "Elixir.MatchError" => ("badmatch", 2),
            "Elixir.CaseClauseError" => ("case_clause", 2),
            "Elixir.WithClauseError" => ("else_clause", 2),
            "Elixir.TryClauseError" => ("try_clause", 2),
            "Elixir.BadStructError" => ("badstruct", 3),
            "Elixir.BadMapError" => ("badmap", 2),
            "Elixir.BadBooleanError" => ("badbool", 3),
            "Elixir.KeyError" => ("badkey", 2),
            "Elixir.ArgumentError" => ("badarg", 2),
            _ => return None,
        };
        let tuple_size = self.remote(ann, "erlang", "tuple_size", vec![var]);
        let count = self.int(size);
        let count = self.ast("integer", ann, vec![count]);
        let mut is_size = self.op(ann, "==", vec![tuple_size, count]);
        if alias == "Elixir.KeyError" && !stack_only {
            let count = self.int(3);
            let count = self.ast("integer", ann, vec![count]);
            let other = self.op(ann, "==", vec![tuple_size, count]);
            is_size = self.op(ann, "orelse", vec![is_size, other])
        }
        let one = self.int(1);
        let one = self.ast("integer", ann, vec![one]);
        let field = self.remote(ann, "erlang", "element", vec![one, var]);
        let tag_expr = self.atom_expr(ann, tag);
        let tag_eq = self.op(ann, "==", vec![field, tag_expr]);
        let tuple = self.op(ann, "andalso", vec![is_size, tag_eq]);
        if alias == "Elixir.ArgumentError" {
            let tag = self.atom_expr(ann, "badarg");
            let direct = self.op(ann, "==", vec![var, tag]);
            Some(self.op(ann, "orelse", vec![direct, tuple]))
        } else {
            Some(tuple)
        }
    }
    pub(super) fn try_expr(&mut self, xs: &[Id], ann: Id, s: &mut Scope) -> Result<Id> {
        if xs.len() != 1 {
            return Err("invalid try".into());
        }
        let opts = xs[0];
        let nil = self.atom("nil");
        let rawdo = self.prop(opts, "do")?.unwrap_or(nil);
        let doexpr = self.expr(rawdo, ann, s, Context::Expr, 0)?;
        let doexpr = self.unblock(doexpr)?;
        let stack_count = s.counters.entry("__STACKTRACE__".into()).or_default();
        *stack_count += 1;
        let stack_name = format!("___STACKTRACE__@{stack_count}");
        let old = s.stacktrace.replace(stack_name.clone());
        let mut catch = vec![];
        if let Some(rescues) = self.prop(opts, "rescue")? {
            for arrow in self.items(rescues)? {
                let p = self.tup(arrow)?;
                let clauseann = self.ann(p[1])?;
                let pair = self.items(p[2])?;
                let args = self.items(pair[0])?;
                if args.len() != 1 {
                    return Err("invalid rescue pattern".into());
                }
                let (left, aliases) = if let T::Tuple(in_ast) = self.get(args[0]).clone() {
                    if in_ast.len() == 3 && self.name(in_ast[0]).ok().as_deref() == Some("in") {
                        let parts = self.items(in_ast[2])?;
                        (parts[0], self.items(parts[1])?)
                    } else {
                        (args[0], vec![])
                    }
                } else {
                    return Err("invalid rescue binding".into());
                };
                let temp = self.fresh(clauseann, s);
                let temp_fields = self.tup(temp)?;
                let temp_name = self.name(temp_fields[2])?;
                let version = -(s.versions.len() as i64 + 1);
                s.versions.insert(version.to_string(), temp_name);
                let mut guards = vec![];
                let mut erl_aliases = vec![];
                for alias in &aliases {
                    let alias = self.name(*alias)?;
                    if let Some(g) = self.rescue_guard(clauseann, temp, &alias, false) {
                        guards.insert(0, g);
                        erl_aliases.insert(0, alias)
                    }
                }
                let mut allguards = vec![];
                for alias in &aliases {
                    let field = self.atom_expr(clauseann, "__struct__");
                    let field = self.remote(clauseann, "erlang", "map_get", vec![field, temp]);
                    let alias = self.ast("atom", clauseann, vec![*alias]);
                    let compare = self.op(clauseann, "==", vec![field, alias]);
                    let exception = self.atom_expr(clauseann, "__exception__");
                    let exception =
                        self.remote(clauseann, "erlang", if self.compiler == CompilerVersion::V1_20 { "is_map_key" } else { "map_get" }, vec![exception, temp]);
                    allguards.push(self.op(clauseann, "andalso", vec![compare, exception]))
                }
                allguards.extend(guards);
                let mut body = vec![];
                let rawleft = self.tup(left)?;
                let ignored = self.name(rawleft[0])? == "_";
                if !ignored {
                    let value = if erl_aliases.is_empty() && !aliases.is_empty() {
                        temp
                    } else {
                        let needs = [
                            "Elixir.FunctionClauseError",
                            "Elixir.UndefinedFunctionError",
                            "Elixir.KeyError",
                            "Elixir.ArgumentError",
                            "Elixir.SystemLimitError",
                        ];
                        let has_erlang = erl_aliases.iter().any(|a| a == "Elixir.ErlangError")
                            || aliases.is_empty();
                        let needing: Vec<_> = if has_erlang {
                            needs.iter().copied().collect()
                        } else {
                            erl_aliases
                                .iter()
                                .filter(|a| needs.contains(&a.as_str()))
                                .map(|a| a.as_str())
                                .collect()
                        };
                        let stack = if needing.is_empty() {
                            self.list(vec![])
                        } else if !has_erlang && needing.len() == erl_aliases.len() {
                            let name = self.atom(&stack_name);
                            self.ast("var", clauseann, vec![name])
                        } else {
                            let generated = self.generated(clauseann);
                            let wild = self.atom_expr(generated, "_");
                            let wild_fields = self.tup(wild)?;
                            let wild_name = wild_fields[2];
                            let wild = self.ast("var", generated, vec![wild_name]);
                            let mut guards = vec![];
                            for alias in needing.into_iter().rev() {
                                let g = self
                                    .rescue_guard(generated, temp, alias, true)
                                    .ok_or("unknown rescue stack guard")?;
                                guards.push(self.list(vec![g]))
                            }
                            let name = self.atom(&stack_name);
                            let stack = self.ast("var", generated, vec![name]);
                            let first =
                                self.make_clause(generated, vec![wild], guards, vec![stack]);
                            let empty = self.ast("nil", generated, vec![]);
                            let second =
                                self.make_clause(generated, vec![wild], vec![], vec![empty]);
                            let clauses = self.list(vec![first, second]);
                            self.ast("case", generated, vec![temp, clauses])
                        };
                        let error = self.atom_expr(clauseann, "error");
                        self.remote(
                            clauseann,
                            "Elixir.Exception",
                            "normalize",
                            vec![error, temp, stack],
                        )
                    };
                    let target = self.expr(left, clauseann, s, Context::Match, 0)?;
                    body.push(self.ast("match", clauseann, vec![target, value]))
                }
                let translated = self.expr(pair[1], clauseann, s, Context::Expr, 0)?;
                body.extend(self.unblock(translated)?);
                let error = self.atom_expr(clauseann, "error");
                let stack = self.atom(&stack_name);
                let stack = self.ast("var", clauseann, vec![stack]);
                let pattern = self.list(vec![error, temp, stack]);
                let pattern = self.ast("tuple", clauseann, vec![pattern]);
                let gs = allguards.into_iter().map(|g| self.list(vec![g])).collect();
                catch.push(self.make_clause(clauseann, vec![pattern], gs, body))
            }
        }
        if let Some(catches) = self.prop(opts, "catch")? {
            for arrow in self.items(catches)? {
                let p = self.tup(arrow)?;
                let clauseann = self.ann(p[1])?;
                let pair = self.items(p[2])?;
                let mut args = self.items(pair[0])?;
                let mut raw_guards = vec![];
                if args.len() == 1 {
                    if let T::Tuple(when) = self.get(args[0]).clone() {
                        if when.len() == 3 && self.name(when[0]).ok().as_deref() == Some("when") {
                            args = self.items(when[2])?;
                            let guard = args.pop().ok_or("catch when lacks guard")?;
                            raw_guards = self.or_guards(guard)?;
                        }
                    }
                }
                if args.len() == 1 {
                    let throw = self.atom("throw");
                    args.insert(0, throw)
                }
                if args.len() != 2 {
                    return Err("catch requires one or two patterns".into());
                }
                let kind = self.expr(args[0], clauseann, s, Context::Match, 0)?;
                let reason = self.expr(args[1], clauseann, s, Context::Match, 0)?;
                let stack = self.atom(&stack_name);
                let stack = self.ast("var", clauseann, vec![stack]);
                let pattern = self.list(vec![kind, reason, stack]);
                let pattern = self.ast("tuple", clauseann, vec![pattern]);
                let guards = self.take_pattern_guards(s, raw_guards, clauseann)?;
                let body = self.expr(pair[1], clauseann, s, Context::Expr, 0)?;
                let body = self.unblock(body)?;
                catch.push(self.make_clause(clauseann, vec![pattern], guards, body))
            }
        }
        s.stacktrace = old;
        let after = if let Some(raw) = self.prop(opts, "after")? {
            let value = self.expr(raw, ann, s, Context::Expr, 0)?;
            self.unblock(value)?
        } else {
            vec![]
        };
        let otherwise = if let Some(raw) = self.prop(opts, "else")? {
            self.arrows(raw, s)?
        } else {
            vec![]
        };
        let body = self.list(doexpr);
        let otherwise = self.list(otherwise);
        let catch = self.list(catch);
        let after = self.list(after);
        Ok(self.ast("try", ann, vec![body, otherwise, catch, after]))
    }
}
