use rust_dialyzer_poc::{
    core::ErlangTerms, form_error::FormError, native_form_environment::RawFormTerm,
    native_type_operand::NativeTypeOperand, term_order::compare_map_keys,
};
use serde_json::{Value, json};
use std::{
    cmp::Ordering,
    io::{self, BufRead},
    sync::Arc,
};
fn main() {
    for line in io::stdin().lock().lines() {
        let v: Value = serde_json::from_str(&line.expect("line")).expect("request");
        let arena = Arc::new(ErlangTerms::parse(&v["raw"]).expect("validated native term arena"));
        let raw = RawFormTerm::new(arena.clone(), arena.root).expect("validated root");
        let (output, reason_matches) = match NativeTypeOperand::Raw(raw)
            .is_opaque(v["module"].as_str().expect("module"))
        {
            Ok(value) => (json!({"ok":value}), true),
            Err(FormError::Native(error)) => (json!({"error":error.report()}), true),
            Err(FormError::ErrorReason(reason)) => {
                let expected = ErlangTerms::parse(&v["reason"]).expect("native reason");
                let equal = compare_map_keys(reason.arena(), reason.id(), &expected, expected.root)
                    .expect("finite term comparator")
                    == Ordering::Equal;
                (
                    json!({"error":{"kind":"error","class":"case_clause"}}),
                    equal,
                )
            }
            Err(error) => panic!("unexpected boundary error {error:?}"),
        };
        let matches = reason_matches
            && if v.get("ok").is_some() {
                output["ok"] == v["ok"]
            } else {
                output["error"] == v["error"]
            };
        println!(
            "{}",
            json!({"id":v["id"],"matches":matches,"output":output})
        );
    }
}
