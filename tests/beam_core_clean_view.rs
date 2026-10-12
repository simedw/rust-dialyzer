use rust_dialyzer_poc::{
    beam_frontend::{
        self,
        clean_view::{CleanView, ViewRef},
        term::Arena,
    },
    core::{ErlangTerm as T, ErlangTerms, TermId},
    external_term,
    fast_hash::FastHashSet,
};
#[path = "support/beam_core_oracle.rs"]
mod oracle;

// Test-only reconstruction checks the complete virtual Erlang term, including
// fields which the analyzer stores outside typed Core nodes.
fn materialize(view: &CleanView) -> ErlangTerms {
    fn node(view: &CleanView, reference: ViewRef, a: &mut Arena) -> TermId {
        let s = view.shape(reference).unwrap();
        let g = s
            .groups
            .into_iter()
            .map(|group| {
                group
                    .into_iter()
                    .map(|child| node(view, child, a))
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let one = |i: usize| g[i][0];
        let pairs = |i: usize, a: &mut Arena| {
            let pairs = g[i].chunks_exact(2).map(|p| a.tuple(p.to_vec())).collect();
            a.list(pairs)
        };
        let f = match s.kind {
            "literal" | "var" | "opaque" => vec![s.annotations, s.value.unwrap()],
            "binary" | "tuple" | "values" => vec![s.annotations, a.list(g[0].clone())],
            "alias" | "cons" | "seq" => vec![s.annotations, one(0), one(1)],
            "apply" | "primop" | "case" => vec![s.annotations, one(0), a.list(g[1].clone())],
            "bitstr" => vec![s.annotations, one(0), one(1), one(2), one(3), one(4)],
            "call" => vec![s.annotations, one(0), one(1), a.list(g[2].clone())],
            "clause" | "let" | "receive" => {
                vec![s.annotations, a.list(g[0].clone()), one(1), one(2)]
            }
            "fun" => vec![s.annotations, a.list(g[0].clone()), one(1)],
            "catch" => vec![s.annotations, one(0)],
            "try" => vec![
                s.annotations,
                one(0),
                a.list(g[1].clone()),
                one(2),
                a.list(g[3].clone()),
                one(4),
            ],
            "letrec" => vec![s.annotations, pairs(0, a), one(1)],
            "module" => vec![
                s.annotations,
                one(0),
                a.list(g[1].clone()),
                pairs(2, a),
                pairs(3, a),
            ],
            "map_pair" => vec![s.annotations, one(0), one(1), one(2)],
            "map" => {
                let arg = node(view, s.map_arg.unwrap(), a);
                let flag = a.atom(if s.pattern.unwrap() { "true" } else { "false" });
                vec![s.annotations, arg, a.list(g[0].clone()), flag]
            }
            other => panic!("unexpected {other}"),
        };
        a.tagged(&format!("c_{}", s.kind), f)
    }
    let mut a = Arena::from_terms(view.arena());
    let root = node(view, view.root(), &mut a);
    a.finish(root)
}
fn atoms(arena: &ErlangTerms) -> FastHashSet<String> {
    let mut names = FastHashSet::default();
    let mut visited = FastHashSet::default();
    let mut pending = vec![arena.root];
    while let Some(id) = pending.pop() {
        if !visited.insert(id) {
            continue;
        }
        match &arena.terms[id.0] {
            T::Atom(name) => {
                names.insert(name.clone());
            }
            T::ExternalFun {
                module, function, ..
            } => {
                names.insert(module.clone());
                names.insert(function.clone());
            }
            T::Tuple(items) => pending.extend(items.iter().copied()),
            T::List { items, tail } => {
                pending.extend(items.iter().copied());
                pending.push(*tail);
            }
            T::Map(entries) => {
                for &(key, value) in entries {
                    pending.extend([key, value]);
                }
            }
            _ => {}
        }
    }
    names
}
fn check(raw: ErlangTerms, label: &str) {
    let clean = beam_frontend::clean::clean(&raw).unwrap();
    let view = CleanView::new(raw).unwrap();
    assert_eq!(
        view.atoms().unwrap(),
        atoms(&clean),
        "{label}: atom namespace"
    );
    oracle::assert_exact(&materialize(&view), &clean, label);
}
#[test]
fn selected_cleanup_matches_complete_fixture_modules() {
    let root =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/beam_core_port");
    for name in [
        "control/native_control.beam",
        "control/native_control_more.beam",
        "control/native_named_pattern.beam",
        "scopes/Elixir.NativeAfterProtected.beam",
        "scopes/Elixir.NativeDependent.beam",
        "aliases/native_aliases.beam",
        "matches/native_matches.beam",
        "exports/native_export_patterns.beam",
        "comprehensions/native_comprehension.beam",
        "comprehensions/native_zip_patterns.beam",
        "Elixir.BinaryMapFn.beam",
        "core_lower_try.beam",
        "core_lower_receive.beam",
    ] {
        let bytes = std::fs::read(root.join(name)).unwrap();
        let debug =
            beam_frontend::debug::read_beam(&bytes, external_term::Limits::default()).unwrap();
        let forms = beam_frontend::forms(&debug).unwrap();
        let raw = beam_frontend::raw_core(&debug, &forms).unwrap();
        check(raw, name);
    }
}
fn literal(a: &mut Arena, name: &str) -> TermId {
    let ann = a.nil();
    let value = a.atom(name);
    a.tagged("c_literal", vec![ann, value])
}
fn goto(a: &mut Arena, body: TermId, fun_body: TermId, params: Vec<TermId>) -> TermId {
    let nil = a.nil();
    let name_value = a.atom("goto");
    let name = a.tagged("c_var", vec![nil, name_value]);
    let params = a.list(params);
    let fun = a.tagged("c_fun", vec![nil, params, fun_body]);
    let pair = a.tuple(vec![name, fun]);
    let defs = a.list(vec![pair]);
    let tag = a.atom("letrec_goto");
    let ann = a.list(vec![tag]);
    a.tagged("c_letrec", vec![ann, defs, body])
}
#[test]
fn synthesized_goto_clauses_are_not_filtered_a_second_time() {
    for params in [0, 1] {
        let mut a = Arena::default();
        let nil = a.nil();
        let name = literal(&mut a, "raise");
        let raising = a.tagged("c_primop", vec![nil, name, nil]);
        let body = literal(&mut a, "ok");
        let vars = (0..params)
            .map(|i| {
                let name = a.integer(i);
                a.tagged("c_var", vec![nil, name])
            })
            .collect();
        let root = goto(&mut a, body, raising, vars);
        check(a.finish(root), "goto raising synthetic clause");
    }
}
#[test]
fn nested_goto_resolves_transformed_root_before_ignore_check() {
    let mut a = Arena::default();
    let nil = a.nil();
    let ignored = a.atom("dialyzer_ignore");
    let ignored_ann = a.list(vec![ignored]);
    let value = a.atom("discard");
    let ignored_body = a.tagged("c_literal", vec![ignored_ann, value]);
    let replacement = literal(&mut a, "replacement");
    let inner = goto(&mut a, ignored_body, replacement, vec![]);
    // An ignored annotation on the original inner goto disappears when that
    // goto resolves to its ordinary replacement body.
    let f = a.tuple_fields(inner).unwrap().to_vec();
    let goto_tag = a.atom("letrec_goto");
    let ann = a.list(vec![goto_tag, ignored]);
    let inner = a.tagged("c_letrec", vec![ann, f[2], f[3]]);
    let outer_target = literal(&mut a, "outer_target");
    let outer = goto(&mut a, inner, outer_target, vec![]);
    let _ = nil;
    check(a.finish(outer), "nested goto transformed annotations");
}
