use rust_dialyzer_poc::{
    core::{ErlangTerms, TermId},
    type_exports,
};
use serde_json::{Value, json};
use std::io::{self, BufRead};
fn named(values: Vec<rust_dialyzer_poc::raw_literal::RawLiteral>) -> Vec<String> {
    values
        .iter()
        .map(|v| v.as_atom().expect("named metadata fixture").to_owned())
        .collect()
}
fn run(v: &Value) -> Result<Value, type_exports::ExportError> {
    if v["op"] == "cache_new" {
        return Ok(rust_dialyzer_poc::form_cache_wire::cache_to_value(
            &rust_dialyzer_poc::form_exports::cache_new(),
        )
        .unwrap());
    }
    if v["op"] == "var_table_new" {
        let table = rust_dialyzer_poc::form_exports::var_table_new();
        assert!(table.is_empty());
        return Ok(type_exports::arena_to_value(
            &[rust_dialyzer_poc::core::ErlangTerm::Map(vec![])],
            TermId(0),
        ));
    }
    let a = std::sync::Arc::new(ErlangTerms::parse(&v["arena"]).unwrap());
    let field = |n: &str| TermId(v[n].as_u64().unwrap() as usize);
    Ok(match v["op"].as_str().unwrap() {
        "constraints" => json!(named(type_exports::constraint_form_to_remote_modules(
            &a, a.root
        )?)),
        "deps" => json!(named(type_exports::type_form_to_remote_modules(
            &a, a.root
        )?)),
        "table_deps" => json!(named(type_exports::module_type_deps_of_type_defs(
            &a, a.root
        )?)),
        "vars" => json!(
            type_exports::t_var_names(&a.terms, a.root)?
                .iter()
                .map(|x| x.0)
                .collect::<Vec<_>>()
        ),
        "lookup" => json!(
            type_exports::lookup_record(
                &a.terms,
                v["tag"].as_str().unwrap(),
                field("arity"),
                a.root
            )?
            .map(|x| x.0)
        ),
        "defined" => json!(type_exports::type_is_defined(
            &a.terms,
            field("kind"),
            field("name"),
            field("arity"),
            a.root
        )?),
        _ => panic!("unsupported boundary command"),
    })
}
fn main() {
    for line in io::stdin().lock().lines() {
        let line = line.unwrap();
        let v: Value = serde_json::from_str(&line).unwrap();
        println!(
            "{}",
            match run(&v) {
                Ok(x) => json!({"ok":x}),
                Err(type_exports::ExportError::BadMatchModules(v)) =>
                    json!({"error":"BadMatchModules","payload":named(v)}),
                Err(e) => json!({"error":format!("{e:?}")}),
            }
        );
    }
}
