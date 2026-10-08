use rust_dialyzer_poc::{
    core::ErlangTerms,
    declaration_key::{DeclarationKey, optional_callbacks},
    native_form_environment::RawFormTerm,
    term_order::compare_map_keys,
};
use serde_json::{Value, json};
use std::{
    io::{self, BufRead},
    sync::Arc,
};
fn raw(v: &Value) -> RawFormTerm {
    let a = Arc::new(ErlangTerms::parse(v).unwrap());
    RawFormTerm::new(a.clone(), a.root).unwrap()
}
fn eq(a: &RawFormTerm, b: &Value) -> bool {
    let b = raw(b);
    compare_map_keys(a.arena(), a.id(), b.arena(), b.id())
        .unwrap()
        .is_eq()
}
fn main() {
    for l in io::stdin().lock().lines() {
        let r: Value = serde_json::from_str(&l.unwrap()).unwrap();
        let opts = optional_callbacks("owner", &raw(&r["optional_callbacks"])).unwrap();
        let outcome = DeclarationKey::parse("owner", &raw(&r["identifier"]));
        let matched = match outcome {
            Ok(k) => {
                r["expected"]["status"] == "ok"
                    && eq(k.raw(), &r["expected"]["key"])
                    && (opts
                        .iter()
                        .any(|o| o.optional_membership_equals(&k).unwrap())
                        == r["expected"]["optional"].as_bool().unwrap())
            }
            Err(e) => {
                let error = e.report();
                r["expected"]["status"] == "error"
                    && error["kind"] == r["expected"]["class"]
                    && eq(&raw(&error["reason"]), &r["expected"]["reason"])
            }
        };
        let expected = raw(&r["expected_optional_keys"]);
        let ids = match expected.term() {
            rust_dialyzer_poc::core::ErlangTerm::Nil => Vec::new(),
            rust_dialyzer_poc::core::ErlangTerm::List { items, tail } => {
                assert!(matches!(
                    expected.arena().terms[tail.0],
                    rust_dialyzer_poc::core::ErlangTerm::Nil
                ));
                items.clone()
            }
            _ => panic!("expected keys list"),
        };
        let list_match = opts.len() == ids.len()
            && opts.iter().zip(ids).all(|(k, id)| {
                compare_map_keys(k.raw().arena(), k.raw().id(), expected.arena(), id)
                    .unwrap()
                    .is_eq()
            });
        println!(
            "{}",
            json!({"matched":matched,"optional_keys_matched":list_match,"optional_keys":opts.len()})
        );
    }
}
