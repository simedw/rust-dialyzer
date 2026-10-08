use rust_dialyzer_poc::{
    core::ErlangTerms, native_form_environment::RawFormTerm,
    native_type_operand::NativeTypeOperand, type_exports,
};
use serde_json::{Value, json};
use std::{
    io::{self, BufRead},
    sync::Arc,
};
fn operand(v: &Value) -> Result<NativeTypeOperand, String> {
    let a = Arc::new(ErlangTerms::parse(v)?);
    Ok(NativeTypeOperand::from_raw(RawFormTerm::new(
        a.clone(),
        a.root,
    )?))
}
fn run(v: &Value) -> Result<Value, String> {
    let left = operand(&v["left"])?;
    if v["op"] == "impossible" {
        return Ok(json!({"ok":left.is_impossible()}));
    }
    let right = operand(&v["right"])?;
    Ok(match left.inf(&right) {
        Ok(value) => {
            let raw = value.to_raw();
            json!({"ok":type_exports::arena_to_value(&raw.arena().terms,raw.id()),"canonical":matches!(value,NativeTypeOperand::Canonical(_))})
        }
        Err(error) => json!({"error":error.report()}),
    })
}
fn main() {
    for line in io::stdin().lock().lines() {
        let result = line
            .map_err(|e| e.to_string())
            .and_then(|s| serde_json::from_str(&s).map_err(|e| e.to_string()))
            .and_then(|v| run(&v));
        println!(
            "{}",
            match result {
                Ok(v) => v,
                Err(e) => json!({"transport":e}),
            }
        );
    }
}
