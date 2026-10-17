use rust_dialyzer_poc::{
    beam_frontend::{self, Compilation, adapter, clean_view::CleanView},
    core::{CoreModule, ErlangTerm, ErlangTerms},
    fast_hash::FastHashSet,
    native_runtime_hash::NativeAtomLayout,
};
use serde_json::{Value, json};
fn same(a: &CoreModule, b: &CoreModule) -> Result<(), String> {
    if a.name != b.name || a.core != b.core || a.source_forms != b.source_forms {
        return Err("roots differ".into());
    }
    if a.nodes.len() != b.nodes.len() || a.terms.len() != b.terms.len() {
        return Err("arena sizes differ".into());
    }
    if let Some(i) = a.nodes.iter().zip(b.nodes.iter()).position(|(a, b)| a != b) {
        return Err(format!("node {i} differs"));
    }
    if let Some(i) = a.terms.iter().zip(b.terms.iter()).position(|(a, b)| a != b) {
        return Err(format!("term {i} differs"));
    }
    if a.map_iterator_orders != b.map_iterator_orders
        || a.contract_variable_maps != b.contract_variable_maps
    {
        return Err("ordering metadata differs".into());
    }
    Ok(())
}
fn atoms(input: &ErlangTerms) -> FastHashSet<String> {
    let mut names = FastHashSet::default();
    let mut visited = vec![false; input.terms.len()];
    let mut pending = vec![input.root];
    while let Some(id) = pending.pop() {
        if std::mem::replace(&mut visited[id.0], true) {
            continue;
        }
        match &input.terms[id.0] {
            ErlangTerm::Atom(a) => {
                names.insert(a.clone());
            }
            ErlangTerm::ExternalFun(fun) => {
                names.insert(fun.module.clone());
                names.insert(fun.function.clone());
            }
            ErlangTerm::Tuple(items) => pending.extend(items.iter().copied()),
            ErlangTerm::List { items, tail } => {
                pending.extend(items.iter().copied());
                pending.push(*tail);
            }
            ErlangTerm::Map(entries) => {
                for &(k, v) in entries {
                    pending.extend([k, v]);
                }
            }
            _ => {}
        }
    }
    names
}
#[test]
#[ignore = "requires the saved whole-workload BEAM inventory and runtime layouts"]
fn full_corpus_emission_and_atom_scope_parity() {
    let plan: Value = serde_json::from_slice(
        &std::fs::read(std::env::var("BEAM_CORE_ADAPTER_PLAN").unwrap()).unwrap(),
    )
    .unwrap();
    let layouts = plan["layouts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| NativeAtomLayout::parse(x).unwrap())
        .collect::<Vec<_>>();
    let mut rows = vec![];
    for row in plan["modules"].as_array().unwrap() {
        let result = (|| -> Result<(), String> {
            let bytes = std::fs::read(row["beam"].as_str().unwrap()).map_err(|e| e.to_string())?;
            let raw = beam_frontend::compile_beam_raw(&bytes)?;
            let cleaned = beam_frontend::clean::clean(&raw.raw_core)?;
            let view = CleanView::new(raw.raw_core)?;
            if atoms(&cleaned) != view.atoms()? {
                return Err("cleaned atom set differs".into());
            }
            let layout = &layouts[row["layout"].as_u64().unwrap() as usize];
            let compilation = || Compilation {
                module: raw.module.clone(),
                source_forms: raw.source_forms.clone(),
                cleaned_core: cleaned.clone(),
            };
            let reference = adapter::module(compilation(), layout)?;
            same(&reference, &adapter::module_direct(compilation(), layout)?)?;
            same(
                &reference,
                &adapter::module_view(raw.module.clone(), raw.source_forms.clone(), &view, layout)?,
            )
        })();
        rows.push(match result {
            Ok(()) => json!({"beam":row["beam"],"equal":true}),
            Err(e) => json!({"beam":row["beam"],"equal":false,"error":e}),
        });
    }
    let failures = rows.iter().filter(|x| x["equal"] != true).count();
    std::fs::write(
        std::env::var("BEAM_CORE_FUSION_REPORT").unwrap(),
        serde_json::to_vec_pretty(&rows).unwrap(),
    )
    .unwrap();
    assert_eq!(failures, 0, "{failures}/{} modules differ", rows.len());
}
