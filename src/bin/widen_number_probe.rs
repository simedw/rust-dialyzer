use rust_dialyzer_poc::{lattice::Type, native_error::NativeTransferError};
use serde_json::{Value, json};
use std::io::{self, BufRead};
fn main() {
    for line in io::stdin().lock().lines() {
        let r: Value = serde_json::from_str(&line.unwrap()).unwrap();
        let out = match Type::from_value(&r["type"]) {
            Ok(t) => match t.widen_to_number() {
                Ok(value) => json!({"kind":"ok","value":value.to_value()}),
                Err(e) => NativeTransferError::from(e).report(),
            },
            Err(e) => {
                let _ = e;
                json!({"kind":"constructor_rejected"})
            }
        };
        println!("{}", json!({"id":r["id"],"native":out}));
    }
}
