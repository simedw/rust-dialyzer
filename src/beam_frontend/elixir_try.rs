//! Adapted from Elixir 1.19 elixir_erl_try (Elixir contributors, Apache-2.0).
use super::*;

fn op(l: &mut Lift, meta: TermId, name: &str, args: Vec<TermId>) -> TermId {
    l.expanded_remote(meta, name, args)
}
fn compare(l: &mut Lift, meta: TermId, var: TermId, value: &str) -> TermId {
    let value = l.a.atom(value);
    op(l, meta, "==", vec![var, value])
}
fn tuple_size(l: &mut Lift, meta: TermId, var: TermId, size: i64) -> TermId {
    let value = op(l, meta, "tuple_size", vec![var]);
    let size = l.a.integer(size);
    op(l, meta, "==", vec![value, size])
}
fn record(l: &mut Lift, meta: TermId, var: TermId, tag: &str) -> TermId {
    let one = l.a.integer(1);
    let value = op(l, meta, "element", vec![one, var]);
    compare(l, meta, value, tag)
}
fn guard_for(l: &mut Lift, meta: TermId, var: TermId, alias: &str) -> Option<TermId> {
    let atom = match alias {
        "Elixir.UndefinedFunctionError" => Some("undef"),
        "Elixir.FunctionClauseError" => Some("function_clause"),
        "Elixir.SystemLimitError" => Some("system_limit"),
        "Elixir.ArithmeticError" => Some("badarith"),
        "Elixir.CondClauseError" => Some("cond_clause"),
        _ => None,
    };
    if let Some(atom) = atom {
        return Some(compare(l, meta, var, atom));
    }
    let tuple = match alias {
        "Elixir.BadArityError" => Some((2, "badarity")),
        "Elixir.BadFunctionError" => Some((2, "badfun")),
        "Elixir.MatchError" => Some((2, "badmatch")),
        "Elixir.CaseClauseError" => Some((2, "case_clause")),
        "Elixir.WithClauseError" => Some((2, "else_clause")),
        "Elixir.TryClauseError" => Some((2, "try_clause")),
        "Elixir.BadStructError" => Some((3, "badstruct")),
        "Elixir.BadMapError" => Some((2, "badmap")),
        "Elixir.BadBooleanError" => Some((3, "badbool")),
        _ => None,
    };
    if let Some((size, tag)) = tuple {
        let size = tuple_size(l, meta, var, size);
        let tag = record(l, meta, var, tag);
        return Some(op(l, meta, "andalso", vec![size, tag]));
    }
    match alias {
        "Elixir.KeyError" => {
            let a = tuple_size(l, meta, var, 2);
            let b = tuple_size(l, meta, var, 3);
            let either = op(l, meta, "orelse", vec![a, b]);
            let tag = record(l, meta, var, "badkey");
            Some(op(l, meta, "andalso", vec![either, tag]))
        }
        "Elixir.ArgumentError" => {
            let a = compare(l, meta, var, "badarg");
            let size = tuple_size(l, meta, var, 2);
            let tag = record(l, meta, var, "badarg");
            let b = op(l, meta, "andalso", vec![size, tag]);
            Some(op(l, meta, "orelse", vec![a, b]))
        }
        "Elixir.ErlangError" => {
            let map = op(l, meta, "is_map", vec![var]);
            let exception = l.a.atom("__exception__");
            let key = op(l, meta, "is_map_key", vec![exception, var]);
            let condition = op(l, meta, "andalso", vec![map, key]);
            Some(op(l, meta, "not", vec![condition]))
        }
        _ => None,
    }
}
fn dynamic_normalize(
    l: &mut Lift,
    meta: TermId,
    var: TermId,
    aliases: &[String],
) -> Result<TermId> {
    let generated = l.generated_meta(meta)?;
    let mut iter = aliases.iter();
    let first = iter
        .next()
        .ok_or_else(|| l.error("rescue stacktrace aliases"))?;
    let mut guard = if first == "Elixir.KeyError" {
        let size = tuple_size(l, generated, var, 2);
        let tag = record(l, generated, var, "badkey");
        op(l, generated, "andalso", vec![size, tag])
    } else {
        guard_for(l, generated, var, first).ok_or_else(|| l.error("rescue stacktrace alias"))?
    };
    for alias in iter {
        let next = if alias == "Elixir.KeyError" {
            let size = tuple_size(l, generated, var, 2);
            let tag = record(l, generated, var, "badkey");
            op(l, generated, "andalso", vec![size, tag])
        } else {
            guard_for(l, generated, var, alias).ok_or_else(|| l.error("rescue stacktrace alias"))?
        };
        guard = l.expanded("when", generated, vec![next, guard]);
    }
    let underscore = l.a.atom("_");
    let nil = l.a.atom("nil");
    let wildcard = l.a.tuple(vec![underscore, generated, nil]);
    let pattern = l.expanded("when", generated, vec![wildcard, guard]);
    let patterns = l.a.list(vec![pattern]);
    let stack = l.a.atom("__STACKTRACE__");
    let stack = l.a.tuple(vec![stack, generated, nil]);
    let first = l.expanded("->", generated, vec![patterns, stack]);
    let patterns = l.a.list(vec![wildcard]);
    let empty = l.a.nil();
    let second = l.expanded("->", generated, vec![patterns, empty]);
    let clauses = l.a.list(vec![first, second]);
    let do_entry = l.a.tagged("do", vec![clauses]);
    let opts = l.a.list(vec![do_entry]);
    Ok(l.expanded("case", generated, vec![var, opts]))
}
fn normalize(
    l: &mut Lift,
    meta: TermId,
    var: TermId,
    pattern: TermId,
    body: TermId,
    aliases: &[String],
) -> Result<TermId> {
    let fields = l.fields(pattern, 3)?;
    if l.a.as_atom(fields[0]) == Some("_") && l.a.as_atom(fields[2]).is_some() {
        return Ok(body);
    }
    let normalized = if aliases.is_empty() {
        var
    } else {
        let stack = if aliases.iter().any(|x| x == "Elixir.ErlangError") {
            dynamic_normalize(
                l,
                meta,
                var,
                &[
                    "Elixir.FunctionClauseError",
                    "Elixir.UndefinedFunctionError",
                    "Elixir.KeyError",
                    "Elixir.ArgumentError",
                    "Elixir.SystemLimitError",
                ]
                .map(str::to_owned),
            )?
        } else {
            let with_stack: Vec<_> = aliases
                .iter()
                .filter(|x| {
                    matches!(
                        x.as_str(),
                        "Elixir.FunctionClauseError"
                            | "Elixir.UndefinedFunctionError"
                            | "Elixir.KeyError"
                            | "Elixir.ArgumentError"
                            | "Elixir.SystemLimitError"
                    )
                })
                .cloned()
                .collect();
            if with_stack.is_empty() {
                l.a.nil()
            } else if with_stack.len() == aliases.len() {
                let name = l.a.atom("__STACKTRACE__");
                let nil = l.a.atom("nil");
                l.a.tuple(vec![name, meta, nil])
            } else {
                dynamic_normalize(l, meta, var, &with_stack)?
            }
        };
        let module = l.a.atom("Elixir.Exception");
        let name = l.a.atom("normalize");
        let dot = l.expanded(".", meta, vec![module, name]);
        let error = l.a.atom("error");
        let args = l.a.list(vec![error, var, stack]);
        l.a.tuple(vec![dot, meta, args])
    };
    let assign = l.expanded("=", meta, vec![pattern, normalized]);
    if l.a.tag(body) == Some("__block__") {
        let mut f = l.fields(body, 3)?;
        let mut items = vec![assign];
        items.extend(l.a.list_items(f[2])?);
        f[2] = l.a.list(items);
        Ok(l.a.tuple(f))
    } else {
        Ok(l.expanded("__block__", meta, vec![assign, body]))
    }
}
fn pack(l: &mut Lift, clause: TermId, stack: &str) -> Result<TermId> {
    let mut fields = l.fields(clause, 5)?;
    let mut patterns = l.a.list_items(fields[2])?;
    if patterns.len() != 2 {
        return Err(l.error("exception clause patterns"));
    }
    patterns.push(l.var_node(fields[1], stack));
    let patterns = l.a.list(patterns);
    let tuple = l.node("tuple", fields[1], vec![patterns]);
    fields[2] = l.a.list(vec![tuple]);
    Ok(l.a.tuple(fields))
}
pub(super) fn clauses(l: &mut Lift, opts: TermId, scope: &mut Scope) -> Result<TermId> {
    let stack = l.build(scope, "__STACKTRACE__");
    let old = scope.stacktrace.replace(stack.clone());
    let mut clauses = Vec::new();
    if let Some(rescue) = l.a.keyword(opts, "rescue") {
        for item in l.a.list_items(rescue)? {
            let f = l.fields(item, 3)?;
            let args = l.a.list_items(f[2])?;
            if args.len() != 2 {
                return Err(l.error("rescue arrow"));
            }
            let patterns = l.a.list_items(args[0])?;
            if patterns.len() != 1 {
                return Err(l.error("rescue pattern"));
            }
            let meta = f[1];
            let (var, _) = l.assign(meta, scope)?;
            let mut guards = Vec::new();
            let mut aliases = Vec::<String>::new();
            let pattern = if l.a.tag(patterns[0]) == Some("in") {
                let f = l.fields(patterns[0], 3)?;
                let pair = l.a.list_items(f[2])?;
                if pair.len() != 2 {
                    return Err(l.error("rescue in"));
                }
                let selected = l.a.list_items(pair[1])?;
                let mut erlang_guards = Vec::new();
                for alias in &selected {
                    let name = l.atom(*alias)?;
                    if let Some(guard) = guard_for(l, meta, var, &name) {
                        erlang_guards.insert(0, guard);
                        aliases.insert(0, name);
                    }
                }
                for alias in selected {
                    let key = l.a.atom("__struct__");
                    let name = op(l, meta, "map_get", vec![key, var]);
                    let same = op(l, meta, "==", vec![name, alias]);
                    let key = l.a.atom("__exception__");
                    let exception = op(l, meta, "map_get", vec![key, var]);
                    guards.push(op(l, meta, "andalso", vec![same, exception]));
                }
                guards.extend(erlang_guards);
                pair[0]
            } else {
                aliases.push("Elixir.ErlangError".into());
                patterns[0]
            };
            let body = normalize(l, meta, var, pattern, args[1], &aliases)?;
            let ann = l.annotation(meta)?;
            let error = l.a.atom("error");
            let clause = l.clause(ann, vec![error, var], guards, body, scope, false)?;
            clauses.push(pack(l, clause, &stack)?);
        }
    }
    if let Some(catch) = l.a.keyword(opts, "catch") {
        for item in l.a.list_items(catch)? {
            let f = l.fields(item, 3)?;
            let args = l.a.list_items(f[2])?;
            if args.len() != 2 {
                return Err(l.error("catch arrow"));
            }
            let mut patterns = l.a.list_items(args[0])?;
            let mut guards = Vec::new();
            if patterns.len() == 1 && l.a.tag(patterns[0]) == Some("when") {
                let f = l.fields(patterns[0], 3)?;
                let mut args = l.a.list_items(f[2])?;
                let mut guard = args.pop().ok_or_else(|| l.error("catch guard"))?;
                while l.a.tag(guard) == Some("when") {
                    let f = l.fields(guard, 3)?;
                    let a = l.a.list_items(f[2])?;
                    if a.len() != 2 {
                        return Err(l.error("catch alternative guard"));
                    }
                    guards.push(a[0]);
                    guard = a[1];
                }
                guards.push(guard);
                patterns = args;
            }
            if patterns.len() == 1 {
                patterns.insert(0, l.a.atom("throw"));
            }
            if patterns.len() != 2 {
                return Err(l.error("catch patterns"));
            }
            let ann = l.annotation(f[1])?;
            let clause = l.clause(ann, patterns, guards, args[1], scope, false)?;
            clauses.push(pack(l, clause, &stack)?);
        }
    }
    scope.stacktrace = old;
    Ok(l.a.list(clauses))
}
