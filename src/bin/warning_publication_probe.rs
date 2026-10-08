use rust_dialyzer_poc::{
    compiled_definitions::PreparedDefinitions,
    core::{CoreProgram, ErlangTerm, ErlangTerms},
    native_form_environment::RawFormTerm,
    program::Program,
    type_exports,
    warning_payload::NativeWarning,
    warning_policy::Policy,
    warning_publication::{PublicationError, publish},
    warning_render::LocationOption,
    warnings::Registry,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::{self, BufRead},
    sync::Arc,
};
fn warnings(v: &Value) -> Vec<NativeWarning> {
    let arena = Arc::new(ErlangTerms::parse(v).unwrap());
    let mut current = arena.root;
    let mut result = Vec::new();
    loop {
        match &arena.terms[current.0] {
            ErlangTerm::Nil => break,
            ErlangTerm::List { items, tail } => {
                result.extend(items.iter().map(|id| NativeWarning {
                    raw: RawFormTerm::new(arena.clone(), *id).unwrap(),
                }));
                current = *tail;
            }
            _ => panic!("warnings list"),
        }
    }
    result
}
fn main() {
    let library = std::fs::read_to_string(std::env::args().nth(1).unwrap()).unwrap();
    let registry = Registry::load(include_str!("../../corpus/warnings.json")).unwrap();
    let mut prepared = BTreeMap::new();
    for line in io::stdin().lock().lines() {
        let row: Value = serde_json::from_str(&line.unwrap()).unwrap();
        let core_key = row["core"].to_string();
        let (definitions, policy) = prepared
            .entry((
                core_key.clone(),
                row["options"].to_string(),
                row["missing_metadata"].to_string(),
            ))
            .or_insert_with(|| {
                let core = CoreProgram::parse(&core_key).unwrap();
                let program = Program::build(&core).unwrap();
                let store = rust_dialyzer_poc::dependencies::PriorPlt::parse(
                    &library,
                    &core,
                    &rust_dialyzer_poc::dependencies::ImportPlan::cold_targets(&core),
                )
                .and_then(|prior| prior.into_cold_store())
                .unwrap();
                let definitions = PreparedDefinitions::prepare(&core, &program, &store).unwrap();
                let options = row["options"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|x| x.as_str().unwrap().to_owned())
                    .collect::<Vec<_>>();
                let initial = registry
                    .apply(&registry.default_profile(), &options)
                    .unwrap();
                let mut policy = Policy::build(&core, &registry, &initial).unwrap();
                if row["missing_metadata"].as_bool() == Some(true) {
                    policy.modules.clear();
                }
                (definitions, policy)
            });
        let result = publish(
            &warnings(&row["analysis"]),
            &[],
            &warnings(&row["unknown"]),
            definitions,
            policy,
            if row["location"] == "line" {
                LocationOption::Line
            } else {
                LocationOption::Column
            },
        );
        let value = match result {
            Ok(p) => {
                json!({"status":"ok","warnings":p.raw.iter().map(|r|type_exports::arena_to_value(&r.arena().terms,r.id())).collect::<Vec<_>>()})
            }
            Err(PublicationError::Native(e)) => e.report(),
            Err(e) => json!({"boundary_error":format!("{e:?}")}),
        };
        println!("{}", json!({"id":row["id"],"native":value}));
    }
}
