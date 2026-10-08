use rust_dialyzer_poc::{bifs::type_ranks, lattice::Type};
use serde_json::{Value, json};
use std::io::{self, BufRead};
fn main() {
    for line in io::stdin().lock().lines() {
        let v: Value = serde_json::from_str(&line.expect("line")).expect("json");
        let t = Type::from_value(&v["type"]).expect("type constructor");
        let result = match type_ranks(&t) {
            Ok((min, max)) => json!({"ok":[min,max]}),
            Err(error) => json!({"error":error.report()}),
        };
        let expected = if let Some(v) = v.get("ok") {
            json!({"ok":v})
        } else {
            json!({"error":v["error"]})
        };
        println!(
            "{}",
            json!({"id":v["id"],"matches":result==expected,"output":result,"expected":if result==expected{Value::Null}else{expected}})
        );
    }
}
