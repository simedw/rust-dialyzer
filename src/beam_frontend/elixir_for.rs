//! Adapted from Elixir 1.19 elixir_erl_for (Elixir contributors, Apache-2.0).
use super::*;
#[derive(Clone)]
struct Generator {
    binary: bool,
    ann: TermId,
    left: TermId,
    right: TermId,
    filters: Vec<(Option<TermId>, TermId)>,
}
#[derive(Clone, Copy)]
enum Inner {
    Discard,
    Cons,
    Case,
    Collect(TermId),
    Binary(TermId),
}
fn var(l: &mut Lift, ann: TermId, s: &mut Scope) -> TermId {
    let name = l.build(s, "_");
    l.var_node(ann, &name)
}
fn tuple(l: &mut Lift, ann: TermId, items: Vec<TermId>) -> TermId {
    let items = l.a.list(items);
    l.node("tuple", ann, vec![items])
}
fn pair(l: &mut Lift, ann: TermId, tag: &str, value: TermId) -> TermId {
    let tag = l.atom_node(ann, tag);
    tuple(l, ann, vec![tag, value])
}
fn clause(
    l: &mut Lift,
    ann: TermId,
    args: Vec<TermId>,
    guards: Vec<Vec<TermId>>,
    body: Vec<TermId>,
) -> TermId {
    let args = l.a.list(args);
    let guards: Vec<_> = guards.into_iter().map(|g| l.a.list(g)).collect();
    let guards = l.a.list(guards);
    let body = l.a.list(body);
    l.node("clause", ann, vec![args, guards, body])
}
fn function(l: &mut Lift, ann: TermId, clauses: Vec<TermId>) -> TermId {
    let clauses = l.a.list(clauses);
    let clauses = l.a.tagged("clauses", vec![clauses]);
    l.node("fun", ann, vec![clauses])
}
fn op(l: &mut Lift, ann: TermId, name: &str, args: Vec<TermId>) -> TermId {
    let name = l.a.atom(name);
    let mut fields = vec![name];
    fields.extend(args);
    l.node("op", ann, fields)
}
fn join(
    l: &mut Lift,
    ann: TermId,
    filter: (Option<TermId>, TermId),
    yes: TermId,
    no: TermId,
) -> TermId {
    let (var, expr) = filter;
    let (first, second) = if let Some(var) = var {
        let false_ = l.atom_node(ann, "false");
        let nil = l.atom_node(ann, "nil");
        let false_ = op(l, ann, "==", vec![var, false_]);
        let nil = op(l, ann, "==", vec![var, nil]);
        let guard = op(l, ann, "orelse", vec![false_, nil]);
        let wildcard = l.var_node(ann, "_");
        (
            clause(l, ann, vec![var], vec![vec![guard]], vec![no]),
            clause(l, ann, vec![wildcard], vec![], vec![yes]),
        )
    } else {
        let true_ = l.atom_node(ann, "true");
        let false_ = l.atom_node(ann, "false");
        (
            clause(l, ann, vec![true_], vec![], vec![yes]),
            clause(l, ann, vec![false_], vec![], vec![no]),
        )
    };
    let clauses = l.a.list(vec![first, second]);
    l.node("case", ann, vec![expr, clauses])
}
fn joins(
    l: &mut Lift,
    ann: TermId,
    filters: &[(Option<TermId>, TermId)],
    yes: TermId,
    no: TermId,
) -> TermId {
    filters
        .iter()
        .fold(yes, |acc, &filter| join(l, ann, filter, acc, no))
}
fn is_generator(l: &Lift, id: TermId) -> bool {
    l.a.tag(id) == Some("<-")
        || (l.a.tag(id) == Some("<<>>")
            && l.fields(id, 3).is_ok_and(|fields| {
                l.a.list_items(fields[2])
                    .is_ok_and(|args| args.len() == 1 && l.a.tag(args[0]) == Some("<-"))
            }))
}
fn generators(l: &mut Lift, args: &[TermId], s: &mut Scope) -> Result<Vec<Generator>> {
    let mut out = Vec::new();
    let mut index = 0;
    while index < args.len() {
        let mut generator = args[index];
        let binary = l.a.tag(generator) == Some("<<>>");
        if binary {
            let f = l.fields(generator, 3)?;
            let a = l.a.list_items(f[2])?;
            if a.len() != 1 {
                return Err(l.error("binary comprehension generator"));
            }
            generator = a[0];
        }
        if l.a.tag(generator) != Some("<-") {
            return Err(l.error("comprehension expected generator"));
        }
        let f = l.fields(generator, 3)?;
        let a = l.a.list_items(f[2])?;
        if a.len() != 2 {
            return Err(l.error("comprehension generator arguments"));
        }
        let ann = l.annotation(f[1])?;
        let right = l.expr(a[1], ann, s)?;
        let (pattern, guards) = l.extract_guards(a[0])?;
        let old_match = s.matching;
        let old_pin = s.pin_guard;
        s.matching = true;
        s.pin_guard = true;
        let left = l.expr(pattern, ann, s);
        s.matching = old_match;
        s.pin_guard = old_pin;
        let left = left?;
        let extra = std::mem::take(&mut s.extra_guards);
        let mut guard_exprs = Vec::new();
        for guard in guards {
            let mut gs = s.clone();
            gs.guarding = true;
            guard_exprs.push(l.expr(guard, ann, &mut gs)?);
        }
        let guard = guard_exprs.into_iter().reduce(|left, right| {
            let fields = l.a.tuple_fields(left).unwrap_or(&[]).to_vec();
            let ann = fields.get(1).copied().unwrap_or(ann);
            op(l, ann, "orelse", vec![left, right])
        });
        index += 1;
        let mut filters = Vec::new();
        while index < args.len() && !is_generator(l, args[index]) {
            let filter = args[index];
            let expr = l.expr(filter, ann, s)?;
            let temp = if l.boolean(filter)? {
                None
            } else {
                let zero = l.a.integer(0);
                Some(var(l, zero, s))
            };
            filters.insert(0, (temp, expr));
            index += 1;
        }
        if let Some(guard) = guard {
            filters.push((None, guard));
        }
        filters.extend(extra.into_iter().map(|g| (None, g)));
        out.push(Generator {
            binary,
            ann,
            left,
            right,
            filters,
        });
    }
    Ok(out)
}
fn inner(l: &mut Lift, ann: TermId, kind: Inner, expr: TermId, acc: TermId) -> Result<TermId> {
    Ok(match kind {
        Inner::Discard => expr,
        Inner::Cons => l.node("cons", ann, vec![expr, acc]),
        Inner::Case => {
            let mut f = l.fields(expr, 4)?;
            if l.a.as_atom(f[0]) != Some("case") {
                return Err(l.error("for reduce case"));
            }
            f[2] = acc;
            l.a.tuple(f)
        }
        Inner::Collect(fun) => {
            let cont = pair(l, ann, "cont", expr);
            let args = l.a.list(vec![acc, cont]);
            l.node("call", ann, vec![fun, args])
        }
        Inner::Binary(value) => {
            let generated = l.generated(l.location(ann));
            let bit = l.remote_call(ann, "erlang", "is_bitstring", vec![value]);
            let list = l.remote_call(ann, "erlang", "is_list", vec![acc]);
            let cons = l.node("cons", generated, vec![acc, value]);
            let first = clause(l, generated, vec![value], vec![vec![bit, list]], vec![cons]);
            let reason = pair(l, ann, "badarg", value);
            let error = l.remote_call(ann, "erlang", "error", vec![reason]);
            let second = clause(l, generated, vec![value], vec![], vec![error]);
            let clauses = l.a.list(vec![first, second]);
            l.node("case", ann, vec![expr, clauses])
        }
    })
}
fn reduce_each(
    l: &mut Lift,
    gens: &[Generator],
    expr: TermId,
    arg: TermId,
    acc: TermId,
    s: &Scope,
) -> Result<TermId> {
    let Some((g, rest)) = gens.split_first() else {
        return Ok(expr);
    };
    let ann = g.ann;
    let generated = l.generated(l.location(ann));
    if !g.binary {
        let yes = reduce_each(l, rest, expr, acc, acc, s)?;
        let joined = joins(l, generated, &g.filters, yes, acc);
        let first = clause(l, ann, vec![g.left, acc], vec![], vec![joined]);
        let mut clauses = vec![first];
        if l.a.tag(g.left) != Some("var") {
            let wildcard = l.var_node(ann, "_");
            clauses.push(clause(l, generated, vec![wildcard, acc], vec![], vec![acc]));
        }
        let fun = function(l, ann, clauses);
        return Ok(l.remote_call(ann, "Elixir.Enum", "reduce", vec![g.right, arg, fun]));
    }
    let mut local = s.clone();
    let tail = var(l, ann, &mut local);
    let fun = var(l, ann, &mut local);
    let yes = reduce_each(l, rest, expr, acc, acc, &local)?;
    let bin = l.fields(g.left, 3)?;
    if l.a.as_atom(bin[0]) != Some("bin") {
        return Err(l.error("binary comprehension pattern"));
    }
    let elements = l.a.list_items(bin[2])?;
    let default = l.a.atom("default");
    let bitstring = l.a.atom("bitstring");
    let types = l.a.list(vec![bitstring]);
    let tail_element = l.node("bin_element", ann, vec![tail, default, types]);
    let elements_tail = l.a.list(vec![tail_element]);
    let tail_bin = l.node("bin", ann, vec![elements_tail]);
    let first_tail = clause(l, generated, vec![tail_bin, acc], vec![], vec![acc]);
    let wildcard = l.var_node(ann, "_");
    let bad = pair(l, ann, "badarg", tail);
    let error = l.remote_call(ann, "erlang", "error", vec![bad]);
    let error_clause = clause(l, generated, vec![tail, wildcard], vec![], vec![error]);
    let mut endings = vec![first_tail, error_clause];
    let mut no_vars = Vec::new();
    let mut unbound = false;
    for element in &elements {
        let f = l.fields(*element, 5)?;
        if l.a.tag(f[3]) == Some("var") {
            unbound = true;
            break;
        }
        let repeats = if l.a.tag(f[2]) == Some("string") {
            let string = l.fields(f[2], 3)?;
            l.a.list_items(string[2])?.len()
        } else {
            1
        };
        for _ in 0..repeats {
            let wildcard = l.var_node(generated, "_");
            no_vars.push(l.node("bin_element", f[1], vec![wildcard, f[3], f[4]]));
        }
    }
    if !unbound {
        no_vars.push(tail_element);
        let fields = l.a.list(no_vars);
        let pattern = l.node("bin", ann, vec![fields]);
        let args = l.a.list(vec![tail, acc]);
        let call = l.node("call", ann, vec![fun, args]);
        endings.insert(
            0,
            clause(l, generated, vec![pattern, acc], vec![], vec![call]),
        );
    }
    let mut pattern = elements;
    pattern.push(tail_element);
    let pattern = l.a.list(pattern);
    let pattern = l.node("bin", ann, vec![pattern]);
    let value = joins(l, generated, &g.filters, yes, acc);
    let args = l.a.list(vec![tail, value]);
    let recur = l.node("call", ann, vec![fun, args]);
    let first = clause(l, ann, vec![pattern, acc], vec![], vec![recur]);
    endings.insert(0, first);
    let fun_fields = l.fields(fun, 3)?;
    let clauses = l.a.list(endings);
    let named = l.node("named_fun", ann, vec![fun_fields[2], clauses]);
    let args = l.a.list(vec![g.right, arg]);
    Ok(l.node("call", ann, vec![named, args]))
}
fn reduce(
    l: &mut Lift,
    ann: TermId,
    gens: &[Generator],
    kind: Inner,
    expr: TermId,
    into: TermId,
    uniq: bool,
    s: &mut Scope,
) -> Result<TermId> {
    let acc = var(l, ann, s);
    if !uniq {
        let expr = inner(l, ann, kind, expr, acc)?;
        return reduce_each(l, gens, expr, into, acc, s);
    }
    let value = var(l, ann, s);
    let into_acc = var(l, ann, s);
    let uniq_acc = var(l, ann, s);
    let empty = l.a.nil();
    let map = l.node("map", ann, vec![empty]);
    let initial = tuple(l, ann, vec![into, map]);
    let acc_tuple = tuple(l, ann, vec![into_acc, uniq_acc]);
    let yes = l.atom_node(ann, "true");
    let field = l.node("map_field_assoc", ann, vec![value, yes]);
    let fields = l.a.list(vec![field]);
    let put = l.node("map", ann, vec![uniq_acc, fields]);
    let field = l.node("map_field_exact", ann, vec![value, yes]);
    let fields = l.a.list(vec![field]);
    let pattern = l.node("map", ann, vec![fields]);
    let first = clause(l, ann, vec![pattern], vec![], vec![acc_tuple]);
    let empty = l.a.nil();
    let map = l.node("map", ann, vec![empty]);
    let result = inner(l, ann, kind, value, into_acc)?;
    let result = tuple(l, ann, vec![result, put]);
    let second = clause(l, ann, vec![map], vec![], vec![result]);
    let clauses = l.a.list(vec![first, second]);
    let case = l.node("case", ann, vec![uniq_acc, clauses]);
    let unpack = l.node("match", ann, vec![acc_tuple, acc]);
    let evaluate = l.node("match", ann, vec![value, expr]);
    let body = l.a.list(vec![unpack, evaluate, case]);
    let body = l.node("block", ann, vec![body]);
    let reduced = reduce_each(l, gens, body, initial, acc, s)?;
    let one = l.a.integer(1);
    let one = l.node("integer", ann, vec![one]);
    Ok(l.remote_call(ann, "erlang", "element", vec![one, reduced]))
}
fn inline(
    l: &mut Lift,
    ann: TermId,
    gens: &[Generator],
    expr: TermId,
    into: Option<TermId>,
    uniq: bool,
    s: &mut Scope,
) -> Result<TermId> {
    let kind = into.map(|x| l.a.tag(x).unwrap_or("").to_owned());
    if !uniq && gens.iter().all(|g| g.binary) {
        let mut qualifiers = Vec::new();
        for g in gens {
            qualifiers.push(l.node("b_generate", g.ann, vec![g.left, g.right]));
            for &filter in g.filters.iter().rev() {
                let yes = l.atom_node(g.ann, "true");
                let no = l.atom_node(g.ann, "false");
                qualifiers.push(join(l, g.ann, filter, yes, no));
            }
        }
        let qualifiers = l.a.list(qualifiers);
        return Ok(l.node(
            if kind.as_deref() == Some("bin") {
                "bc"
            } else {
                "lc"
            },
            ann,
            vec![expr, qualifiers],
        ));
    }
    if let Some(into) = into {
        if gens.len() == 1
            && !gens[0].binary
            && l.a.tag(gens[0].left) == Some("var")
            && gens[0].filters.is_empty()
            && !uniq
            && matches!(kind.as_deref(), Some("nil" | "map"))
        {
            let g = &gens[0];
            let clause = clause(l, ann, vec![g.left], vec![], vec![expr]);
            let fun = function(l, ann, vec![clause]);
            let mapped = l.remote_call(ann, "Elixir.Enum", "map", vec![g.right, fun]);
            return Ok(if kind.as_deref() == Some("map") {
                l.remote_call(ann, "maps", "from_list", vec![mapped])
            } else {
                mapped
            });
        }
        if kind.as_deref() == Some("nil") {
            let result = reduce(l, ann, gens, Inner::Cons, expr, into, uniq, s)?;
            return Ok(l.remote_call(ann, "lists", "reverse", vec![result]));
        }
        if kind.as_deref() == Some("bin") {
            let value = var(l, ann, s);
            let nil = l.node("nil", ann, vec![]);
            let result = reduce(l, ann, gens, Inner::Binary(value), expr, nil, uniq, s)?;
            return Ok(l.remote_call(ann, "erlang", "list_to_bitstring", vec![result]));
        }
    }
    let nil = l.node("nil", ann, vec![]);
    reduce(l, ann, gens, Inner::Discard, expr, nil, uniq, s)
}
pub(super) fn translate(
    l: &mut Lift,
    meta: TermId,
    mut args: Vec<TermId>,
    s: &mut Scope,
) -> Result<TermId> {
    let opts = args.pop().ok_or_else(|| l.error("for options"))?;
    let body = l.a.keyword(opts, "do").ok_or_else(|| l.error("for do"))?;
    let ann = l.annotation(meta)?;
    if let Some(initial) = l.a.keyword(opts, "reduce") {
        let initial = l.expr(initial, ann, s)?;
        let gens = generators(l, &args, s)?;
        let ok = l.a.atom("ok");
        let do_ = l.a.tagged("do", vec![body]);
        let opts = l.a.list(vec![do_]);
        let case = l.expanded("case", meta, vec![ok, opts]);
        let expr = l.expr(case, ann, s)?;
        return reduce(l, ann, &gens, Inner::Case, expr, initial, false, s);
    }
    let into = if let Some(into) = l.a.keyword(opts, "into") {
        Some(l.expr(into, ann, s)?)
    } else {
        None
    };
    let uniq = l.a.keyword(opts, "uniq").and_then(|x| l.a.as_atom(x)) == Some("true");
    let gens = generators(l, &args, s)?;
    let body = if into.is_none() {
        let empty = l.a.nil();
        let nil = l.a.atom("nil");
        l.expanded("__block__", empty, vec![body, nil])
    } else {
        body
    };
    let expr = l.expr(body, ann, s)?;
    let literal_into = into.is_none()
        || into.is_some_and(|x| {
            l.a.tag(x) == Some("nil")
                || l.fields(x, 3).is_ok_and(|f| {
                    matches!(l.a.as_atom(f[0]), Some("bin"))
                        && l.a.list_items(f[2]).is_ok_and(|items| items.is_empty())
                })
        });
    if literal_into {
        return inline(l, ann, &gens, expr, into, uniq, s);
    }
    let into = into.unwrap();
    let empty_map = l.fields(into, 3).is_ok_and(|f| {
        l.a.as_atom(f[0]) == Some("map") && l.a.list_items(f[2]).is_ok_and(|items| items.is_empty())
    });
    if empty_map {
        let nil = l.node("nil", ann, vec![]);
        let reduced = if gens.iter().all(|g| g.binary) {
            let reduced = reduce(l, ann, &gens, Inner::Cons, expr, nil, uniq, s)?;
            l.remote_call(ann, "lists", "reverse", vec![reduced])
        } else {
            inline(l, ann, &gens, expr, Some(nil), uniq, s)?
        };
        return Ok(l.remote_call(ann, "maps", "from_list", vec![reduced]));
    }
    let fun = var(l, ann, s);
    let acc = var(l, ann, s);
    let kind = var(l, ann, s);
    let reason = var(l, ann, s);
    let stack = var(l, ann, s);
    let done = var(l, ann, s);
    let pattern = tuple(l, ann, vec![acc, fun]);
    let call = l.remote_call(ann, "Elixir.Collectable", "into", vec![into]);
    let assign = l.node("match", ann, vec![pattern, call]);
    let reduced = reduce(l, ann, &gens, Inner::Collect(fun), expr, acc, uniq, s)?;
    let done_tag = l.atom_node(ann, "done");
    let args = l.a.list(vec![done, done_tag]);
    let finished = l.node("call", ann, vec![fun, args]);
    let finished = clause(l, ann, vec![done], vec![], vec![finished]);
    let else_ = l.a.list(vec![finished]);
    let pattern = tuple(l, ann, vec![kind, reason, stack]);
    let halt = l.atom_node(ann, "halt");
    let args = l.a.list(vec![acc, halt]);
    let halt = l.node("call", ann, vec![fun, args]);
    let raise = l.remote_call(ann, "erlang", "raise", vec![kind, reason, stack]);
    let catch = clause(l, ann, vec![pattern], vec![], vec![halt, raise]);
    let catches = l.a.list(vec![catch]);
    let after = l.a.nil();
    let body = l.a.list(vec![reduced]);
    let try_ = l.node("try", ann, vec![body, else_, catches, after]);
    let body = l.a.list(vec![assign, try_]);
    Ok(l.node("block", ann, vec![body]))
}
