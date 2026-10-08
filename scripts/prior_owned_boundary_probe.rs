use rust_dialyzer_poc::{core::CoreProgram, dependencies::{ImportPlan, PriorPlt}};
use std::io::{self, BufRead};
fn main() {
    let path = std::env::args().nth(1).expect("core path");
    let core = CoreProgram::parse(&std::fs::read_to_string(path).unwrap()).unwrap();
    for line in io::stdin().lock().lines() {
        let value = serde_json::from_str(&line.unwrap()).unwrap();
        let result = PriorPlt::from_value(value, &core, &ImportPlan::cold_targets(&core))
            .and_then(|prior| prior.into_cold_store());
        let output = match result {
            Ok(store) => serde_json::json!({"ok":store.inspect()}),
            Err(error) => serde_json::json!({"error":error}),
        };
        println!("{output}");
    }
}
