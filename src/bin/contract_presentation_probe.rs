use rust_dialyzer_poc::{
    contract_presentation, core::ErlangTerms, native_form_environment::RawFormTerm,
};
use serde_json::{Value, json};
use std::{
    io::{self, BufRead},
    sync::Arc,
};
fn main() {
    for line in io::stdin().lock().lines() {
        let r: Value = serde_json::from_str(&line.unwrap()).unwrap();
        if let Some(c) = r.get("dependency") {
            let c: rust_dialyzer_poc::dependencies::Contract =
                serde_json::from_value(c.clone()).unwrap();
            let result = if r["op"] == "is_overloaded" {
                contract_presentation::dependency_is_overloaded(&c).map(|v| json!(v))
            } else {
                contract_presentation::dependency_to_string(&c).map(|v| json!(v))
            };
            println!(
                "{}",
                match result {
                    Ok(v) => json!({"ok":v}),
                    Err(e) => json!({"error":e.report()}),
                }
            );
            continue;
        }
        let arena = Arc::new(ErlangTerms::parse(&r["arena"]).unwrap());
        let raw = RawFormTerm::new(arena.clone(), arena.root).unwrap();
        let result = if r["op"] == "is_overloaded" {
            contract_presentation::is_overloaded(&raw).map(|v| json!(v))
        } else {
            contract_presentation::contract_to_string(&raw).map(|v| json!(v))
        };
        println!(
            "{}",
            match result {
                Ok(v) => json!({"ok":v}),
                Err(e) => json!({"error":e.report()}),
            }
        );
    }
}
