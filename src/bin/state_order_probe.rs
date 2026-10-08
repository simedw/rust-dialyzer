use rust_dialyzer_poc::{
    native_dict::NativeDictOrder,
    worklist::{FunctionKey, Worklist},
};
use serde_json::{Value, json};
use std::io::{self, BufRead};
fn key(v: &Value) -> FunctionKey {
    if v == "top" {
        FunctionKey::Top
    } else {
        FunctionKey::Label(rust_dialyzer_poc::labels::CoreLabel(
            v.as_u64().expect("validated nonnegative label"),
        ))
    }
}
fn wire(k: FunctionKey) -> Value {
    match k {
        FunctionKey::Top => json!("top"),
        FunctionKey::Label(n) => json!(n.0),
    }
}
fn main() {
    for line in io::stdin().lock().lines() {
        let row: Value = serde_json::from_str(&line.unwrap()).unwrap();
        let mut dict = NativeDictOrder::default();
        for v in row["insert"].as_array().unwrap() {
            dict.store(key(v))
        }
        let mut work = Worklist::new(row["initial"].as_array().unwrap().iter().map(key).collect());
        let mut steps = Vec::new();
        for op in row["operations"].as_array().unwrap() {
            if op == "pop" {
                steps.push(json!({"result":work.pop().map(wire),"queue":work.ordered().into_iter().map(wire).collect::<Vec<_>>() }))
            } else {
                work.add(key(&op["add"]));
                steps.push(
                    json!({"queue":work.ordered().into_iter().map(wire).collect::<Vec<_>>() }),
                )
            }
        }
        println!(
            "{}",
            json!({"id":row["id"],"dict":dict.keys().into_iter().map(wire).collect::<Vec<_>>(),"slots":dict.slots(),"steps":steps})
        );
    }
}
