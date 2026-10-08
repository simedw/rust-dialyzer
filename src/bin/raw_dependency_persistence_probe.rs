use rust_dialyzer_poc::{
    core::{ErlangTerm, ErlangTerms, TermId},
    external_term,
    module_dependencies::ModuleDependencies,
    native_form_environment::RawFormTerm,
    raw_literal::RawLiteralArena,
};
use std::{
    io::{self, BufRead},
    sync::Arc,
};
fn list(a: &ErlangTerms, mut id: TermId) -> Vec<TermId> {
    let mut out = Vec::new();
    loop {
        match &a.terms[id.0] {
            ErlangTerm::Nil => return out,
            ErlangTerm::List { items, tail } => {
                out.extend(items);
                id = *tail
            }
            _ => panic!("proper source list required"),
        }
    }
}
fn main() {
    for line in io::stdin().lock().lines() {
        let r: serde_json::Value = serde_json::from_str(&line.unwrap()).unwrap();
        let deps = if let Some(input) = r.get("dependency_dict") {
            let a = Arc::new(ErlangTerms::parse(input).unwrap());
            let root = a.root;
            ModuleDependencies::from_raw_dict(&RawFormTerm::new(a, root).unwrap()).unwrap()
        } else if let Some(input) = r.get("dependency_map") {
            let a = Arc::new(ErlangTerms::parse(input).unwrap());
            let root = a.root;
            let raw = RawFormTerm::new(a, root).unwrap();
            ModuleDependencies::from_raw_map(&raw).unwrap()
        } else {
            let a = Arc::new(ErlangTerms::parse(&r["source"]).unwrap());
            let literals = RawLiteralArena::new(a.clone()).unwrap();
            let mut rows = Vec::new();
            for id in list(&a, a.root) {
                let ErlangTerm::Tuple(fields) = &a.terms[id.0] else {
                    panic!()
                };
                let values = list(&a, fields[1])
                    .into_iter()
                    .map(|id| literals.at(id).unwrap())
                    .collect();
                rows.push((literals.at(fields[0]).unwrap(), values));
            }
            if r["use_contracts"] == false && r["kind"] == "spec" {
                rows.clear();
            }
            ModuleDependencies::collect_type_rows(rows)
        };
        let deps = if let Some(input) = r.get("strip_values") {
            let a = Arc::new(ErlangTerms::parse(input).unwrap());
            let literals = RawLiteralArena::new(a.clone()).unwrap();
            let seeds = list(&a, a.root)
                .into_iter()
                .map(|id| literals.at(id).unwrap())
                .collect::<std::collections::BTreeSet<_>>();
            deps.retaining_values(|v| !seeds.contains(v)).unwrap()
        } else {
            deps
        };
        let deps = if let Some(other) = r.get("incoming_dependency_dict") {
            let a = Arc::new(ErlangTerms::parse(other).unwrap());
            let root = a.root;
            deps.union(
                &ModuleDependencies::from_raw_dict(&RawFormTerm::new(a, root).unwrap()).unwrap(),
            )
            .unwrap()
        } else if let Some(other) = r.get("incoming_dependency_map") {
            let a = Arc::new(ErlangTerms::parse(other).unwrap());
            let root = a.root;
            let raw = RawFormTerm::new(a, root).unwrap();
            deps.union(&ModuleDependencies::from_raw_map(&raw).unwrap())
                .unwrap()
        } else {
            deps
        };
        let raw = deps.to_raw_dict();
        let mut arena = (**raw.arena()).clone();
        arena.root = raw.id();
        let bytes = external_term::encode(&arena, external_term::Limits::default(), false).unwrap();
        let hex = bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
        println!("{}", serde_json::json!({"id":r["id"],"dictionary_etf":hex}));
    }
}
