use rust_dialyzer_poc::{
    core::ErlangTerms,
    native_form_environment::RawFormTerm,
    native_form_io::{self, Encoding},
    type_exports,
};
use serde_json::{Value, json};
use std::{
    io::{self, BufRead},
    sync::Arc,
};
fn main() {
    for line in io::stdin().lock().lines() {
        let c: Value = serde_json::from_str(&line.unwrap()).unwrap();
        let a = Arc::new(ErlangTerms::parse(&c["arena"]).unwrap());
        let raw = RawFormTerm::new(a.clone(), a.root).unwrap();
        let encoding = if c["encoding"] == "unicode" {
            Encoding::Unicode
        } else {
            Encoding::Latin1
        };
        let orders = c["map_orders"]
            .as_array()
            .map(|xs| {
                xs.iter()
                    .map(|x| {
                        (
                            rust_dialyzer_poc::core::TermId(x["id"].as_u64().unwrap() as usize),
                            x["order"]
                                .as_array()
                                .unwrap()
                                .iter()
                                .map(|n| n.as_u64().unwrap() as usize)
                                .collect(),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        let r = if let Some(template) = c["format"].as_str() {
            let arguments = c["arguments"]
                .as_array()
                .unwrap()
                .iter()
                .map(|id| {
                    RawFormTerm::new(
                        a.clone(),
                        rust_dialyzer_poc::core::TermId(id.as_u64().unwrap() as usize),
                    )
                    .unwrap()
                })
                .collect::<Vec<_>>();
            match native_form_io::format(template, &arguments, &orders) {
                Ok(out) => json!({"ok":type_exports::arena_to_value(&out.arena().terms,out.id())}),
                Err(e) => json!({"error":e.report()}),
            }
        } else {
            match native_form_io::write_with_map_orders(&raw, encoding, &orders) {
                Ok(out) => json!({"ok":type_exports::arena_to_value(&out.arena().terms,out.id())}),
                Err(e) => json!({"unsupported":e}),
            }
        };
        println!("{r}");
    }
}
